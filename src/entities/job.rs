//! Entity decoding in three steps: scan, decode, apply.
//!
//! Recent builds give the bit length of every entity's field data
//! (`serialized_entities`), so the entity headers of a message can be read
//! without decoding any field: that is the *scan*. Decoding an entity's fields
//! needs only its class (except for classes with polymorphic pointers, see
//! [`Class::needs_state`]), so it can run ahead of the game state, on other
//! threads. *Applying* the decoded fields to the entities, in order, is what
//! the visitor sees, and is the same whichever thread decoded them.

use super::{
    Class, ClassSkip, DecodeEnv, Entity, EntityDecoder, EntityOp, EntityStore, FieldPath, MAX_ENTITIES,
    PropNode, SERIAL_BITS, Value, baseline_props, decode_entity_fields, decode_entity_fields_append,
    trace_enabled,
};
use crate::bitreader::{BitReader, CoordParams};
use crate::error::{Error, Result};
use std::ops::Range;
use std::sync::Arc;

/// Class id of an entity index without an entity.
pub(crate) const NO_CLASS: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecordKind {
    Create,
    Update,
    Leave,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decoded {
    /// Not decoded yet: decoded when applied.
    Pending,
    /// Decoded to exactly the announced length; values are in the job.
    Exact,
    /// Decoded to a different length than announced; no values.
    Mismatch,
    /// An update of a skipped class, or one that changed only skipped
    /// properties: applied without being reported.
    Skipped,
}

/// One entry of a scanned `svc_PacketEntities` message.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EntityRecord {
    idx: u32,
    kind: RecordKind,
    /// Create: the class in the header. Update: the class the entity had
    /// when the message was scanned (`NO_CLASS` if there was none).
    class_id: u32,
    serial: u32,
    /// Field data: bit offset in the message and bit length.
    bit_start: u32,
    size: u32,
    status: Decoded,
    /// Paths of the decoded fields, in `EntityJob::paths`.
    fields_start: u32,
    fields_len: u32,
}

impl EntityRecord {
    pub fn index(&self) -> usize {
        self.idx as usize
    }

    pub fn kind(&self) -> RecordKind {
        self.kind
    }

    pub fn class_id(&self) -> u32 {
        self.class_id
    }
}

/// A scanned `svc_PacketEntities` message and, once decoded, its fields.
/// Buffers are kept between messages.
#[derive(Default)]
pub(crate) struct EntityJob {
    data: Vec<u8>,
    records: Vec<EntityRecord>,
    /// Raised by the scan after `records` (bad header, overrun).
    error: Option<Error>,
    paths: Vec<FieldPath>,
    /// Values of the `Exact` records, in record order.
    values: Vec<Value>,
    sizes: Vec<u32>,
}

impl EntityJob {
    pub fn records(&self) -> &[EntityRecord] {
        &self.records
    }

    pub fn has_error(&self) -> bool {
        self.error.is_some()
    }

    /// Bytes of entity data, as a measure of the work to decode it.
    pub fn data_len(&self) -> usize {
        self.data.len()
    }

    fn clear(&mut self) {
        // Messages have about 200 fields; give back the memory of the large
        // ones (players respawning, a round restart), which are rare.
        const KEEP: usize = 1024;
        self.records.clear();
        self.error = None;
        self.paths.clear();
        self.values.clear();
        if self.paths.capacity() > 2 * KEEP {
            self.paths.shrink_to(KEEP);
        }
        if self.values.capacity() > 2 * KEEP {
            self.values.shrink_to(KEEP);
        }
        if self.records.capacity() > KEEP / 2 {
            self.records.shrink_to(KEEP / 8);
        }
    }

    /// Reads the entity headers of a message (`data` is its `entity_data`,
    /// `sizes` its `serialized_entities`) without decoding any field.
    /// `class_of` gives the class id of the entity at an index before this
    /// message, or [`NO_CLASS`]. Returns false if the message can't be
    /// scanned (it doesn't give field lengths); the job must then be ignored.
    /// On success, hand the same `data` to [`EntityJob::set_data`].
    #[allow(clippy::too_many_arguments)]
    pub fn scan(
        &mut self,
        data: &[u8],
        sizes: Option<&[u8]>,
        updated_entries: i32,
        has_pvs_vis_bits: bool,
        class_id_bits: u32,
        classes: &[Option<Arc<Class>>],
        class_of: &dyn Fn(usize) -> u32,
    ) -> bool {
        self.clear();
        if trace_enabled() {
            // Only the one-step decoder traces every field.
            return false;
        }
        let raw = sizes.unwrap_or(&[]);
        let have_sizes = !raw.is_empty();
        self.sizes.clear();
        let mut pos = 0;
        while pos < raw.len() {
            match crate::parser::read_varint(raw, &mut pos) {
                Ok(v) => self.sizes.push(v as u32),
                Err(e) => {
                    self.error = Some(e);
                    return true;
                }
            }
        }
        let mut r = BitReader::new(data);
        let mut index: i64 = -1;
        let mut size_pos = 0;
        let record = |idx: usize, kind, class_id, serial, bit_start: usize, size| EntityRecord {
            idx: idx as u32,
            kind,
            class_id,
            serial,
            bit_start: bit_start as u32,
            size,
            status: Decoded::Pending,
            fields_start: 0,
            fields_len: 0,
        };
        for _ in 0..updated_entries.max(0) {
            index += r.read_ubitvar() as i64 + 1;
            let idx = index as usize;
            if idx >= MAX_ENTITIES {
                self.error = Some(Error::malformed(format!("entity index {idx} out of range")));
                return true;
            }
            let cmd = r.read_bits(2);
            if cmd & 1 == 1 {
                let kind = if cmd & 2 != 0 {
                    RecordKind::Delete
                } else {
                    RecordKind::Leave
                };
                self.records.push(record(idx, kind, NO_CLASS, 0, 0, 0));
                continue;
            }
            if cmd & 2 == 0 && has_pvs_vis_bits && r.read_bits(2) & 1 == 1 {
                continue;
            }
            if !have_sizes {
                return false;
            }
            let Some(&size) = self.sizes.get(size_pos) else {
                self.error = Some(Error::malformed("fewer entity sizes than entity updates"));
                return true;
            };
            size_pos += 1;
            if cmd & 2 != 0 {
                let class_id = r.read_bits(class_id_bits);
                let serial = r.read_bits(SERIAL_BITS);
                let _unknown = r.read_varu32();
                if classes.get(class_id as usize).and_then(Option::as_ref).is_none() {
                    self.error = Some(Error::malformed(format!(
                        "unknown class id {class_id} for entity {idx}"
                    )));
                    return true;
                }
                let start = r.position();
                self.records
                    .push(record(idx, RecordKind::Create, class_id, serial, start, size));
            } else {
                let start = r.position();
                self.records
                    .push(record(idx, RecordKind::Update, class_of(idx), 0, start, size));
            }
            r.seek(r.position() + size as usize);
        }
        if r.overflowed() {
            self.error = Some(Error::UnexpectedEof("packet entities"));
        }
        true
    }

    /// Takes the message data the job was scanned from.
    pub fn set_data(&mut self, data: Vec<u8>) {
        self.data = data;
    }

    /// Decodes the field data of every record whose class doesn't need the
    /// entity's state. `classes` must be the ones the job was scanned with.
    pub fn decode(&mut self, classes: &[Option<Arc<Class>>], cp: &CoordParams) {
        let EntityJob {
            data,
            records,
            paths,
            values,
            ..
        } = self;
        for rec in records.iter_mut() {
            if !matches!(rec.kind, RecordKind::Create | RecordKind::Update) {
                continue;
            }
            let Some(class) = classes.get(rec.class_id as usize).and_then(Option::as_ref) else {
                continue;
            };
            if class.skip.all {
                rec.status = Decoded::Skipped;
                continue;
            }
            if class.needs_state {
                continue;
            }
            let mut r = BitReader::new(data);
            r.seek(rec.bit_start as usize);
            let (paths_base, values_base) = (paths.len(), values.len());
            let exact = decode_entity_fields_append(
                &mut r,
                &class.serializer,
                paths,
                cp,
                values,
                Some(rec.size as usize),
                None,
            )
            .unwrap_or(false);
            if !exact {
                values.truncate(values_base);
            } else if !class.skip.props.is_empty() && paths.len() > paths_base {
                skip_props(&class.skip, paths, paths_base, values, values_base);
                if paths.len() == paths_base && rec.kind == RecordKind::Update {
                    rec.status = Decoded::Skipped;
                    continue;
                }
            }
            rec.fields_start = paths_base as u32;
            rec.fields_len = (paths.len() - paths_base) as u32;
            rec.status = if exact { Decoded::Exact } else { Decoded::Mismatch };
        }
    }
}

/// A record's fields, once applied.
enum Fields {
    /// Decoded exactly and applied; the changed paths.
    Exact(Range<usize>),
    /// Not decoded to the announced length, not applied; the paths decoded.
    Mismatch(Range<usize>),
    /// Only skipped properties changed (see `ParseOptions::skip`).
    OnlySkipped,
}

/// Removes the skipped properties from the fields decoded last: the paths
/// from `paths_base` on, with their values from `values_base` on.
fn skip_props(
    skip: &ClassSkip,
    paths: &mut Vec<FieldPath>,
    paths_base: usize,
    values: &mut Vec<Value>,
    values_base: usize,
) {
    let mut kept = 0;
    for i in 0..paths.len() - paths_base {
        if skip.keeps(&paths[paths_base + i]) {
            paths.swap(paths_base + kept, paths_base + i);
            values.swap(values_base + kept, values_base + i);
            kept += 1;
        }
    }
    paths.truncate(paths_base + kept);
    values.truncate(values_base + kept);
}

/// Applies the records of a scanned job to the entities one at a time, like
/// [`super::PacketEntitiesCursor`], so the caller can inspect the store in between.
pub(crate) struct JobCursor<'j> {
    data: &'j [u8],
    records: std::slice::Iter<'j, EntityRecord>,
    paths: &'j mut Vec<FieldPath>,
    /// Values of the `Exact` records, moved out in order.
    values: std::vec::Drain<'j, Value>,
    error: &'j mut Option<Error>,
    pending_delete: Option<usize>,
}

impl EntityJob {
    /// Starts applying the job's records.
    pub fn cursor(&mut self) -> JobCursor<'_> {
        let EntityJob {
            data,
            records,
            error,
            paths,
            values,
            ..
        } = self;
        JobCursor {
            data,
            records: records.iter(),
            paths,
            values: values.drain(..),
            error,
            pending_delete: None,
        }
    }
}

impl JobCursor<'_> {
    fn flush_delete(&mut self, store: &mut EntityStore) {
        if let Some(idx) = self.pending_delete.take()
            && store.slots[idx].take().is_some()
        {
            store.count -= 1;
        }
    }

    /// Paths written by a record, as returned by [`JobCursor::next`].
    pub fn paths(&self, range: Range<usize>) -> &[FieldPath] {
        &self.paths[range]
    }

    /// Applies the next record. Returns the entity, what happened to it and
    /// the range of its changed paths (see [`JobCursor::paths`]).
    pub fn next(
        &mut self,
        store: &mut EntityStore,
        dec: &mut EntityDecoder,
        env: &DecodeEnv,
    ) -> Result<Option<(usize, EntityOp, Range<usize>)>> {
        self.flush_delete(store);
        while let Some(&rec) = self.records.next() {
            let idx = rec.idx as usize;
            match rec.kind {
                RecordKind::Delete => {
                    if store.slots[idx].is_some() {
                        self.pending_delete = Some(idx);
                        return Ok(Some((idx, EntityOp::Deleted, 0..0)));
                    }
                }
                RecordKind::Leave => {
                    if let Some(e) = store.slots[idx].as_mut() {
                        e.active = false;
                        return Ok(Some((idx, EntityOp::Left, 0..0)));
                    }
                }
                RecordKind::Create => {
                    dec.diagnostics.verified_updates += 1;
                    let class = env
                        .classes
                        .get(rec.class_id as usize)
                        .and_then(Option::as_ref)
                        .ok_or_else(|| {
                            Error::malformed(format!("unknown class id {} for entity {idx}", rec.class_id))
                        })?
                        .clone();
                    let mut props = baseline_props(dec, rec.class_id, &class, env);
                    let (exact, range) = if class.skip.all {
                        (true, 0..0)
                    } else {
                        match self.fields(&rec, &class, &mut props, dec, env) {
                            Fields::Exact(range) => (true, range),
                            Fields::Mismatch(range) => (false, range),
                            Fields::OnlySkipped => (true, 0..0),
                        }
                    };
                    if !exact {
                        dec.diagnostics.size_mismatches += 1;
                        dec.diagnostics
                            .record(|| format!("create of {} #{idx}: field data size mismatch", class.name));
                    }
                    if store.slots[idx].is_none() {
                        store.count += 1;
                    }
                    store.slots[idx] = Some(Entity {
                        index: idx as u32,
                        serial: rec.serial,
                        class,
                        props,
                        active: true,
                    });
                    return Ok(Some((idx, EntityOp::Created, range)));
                }
                RecordKind::Update => {
                    dec.diagnostics.verified_updates += 1;
                    let Some(entity) = store.slots[idx].as_mut() else {
                        return Err(Error::malformed(format!("update for missing entity {idx}")));
                    };
                    let Entity {
                        class, props, active, ..
                    } = entity;
                    *active = true;
                    if class.skip.all || rec.status == Decoded::Skipped {
                        continue;
                    }
                    match self.fields(&rec, class, props, dec, env) {
                        Fields::Exact(range) => return Ok(Some((idx, EntityOp::Updated, range))),
                        Fields::OnlySkipped => continue,
                        Fields::Mismatch(_) => {}
                    }
                    dec.diagnostics.size_mismatches += 1;
                    let name = class.name.clone();
                    dec.diagnostics
                        .record(|| format!("update of {name} #{idx}: field data size mismatch"));
                }
            }
        }
        Ok(None)
    }

    /// Writes a record's field values into `props`, decoding them first if
    /// that wasn't done ahead (or was done with another serializer than the
    /// entity's).
    fn fields(
        &mut self,
        rec: &EntityRecord,
        class: &Arc<Class>,
        props: &mut PropNode,
        dec: &mut EntityDecoder,
        env: &DecodeEnv,
    ) -> Fields {
        let start = rec.fields_start as usize;
        let range = start..start + rec.fields_len as usize;
        if rec.status != Decoded::Pending {
            let same_serializer = env
                .classes
                .get(rec.class_id as usize)
                .and_then(Option::as_ref)
                .is_some_and(|c| Arc::ptr_eq(&c.serializer, &class.serializer));
            if same_serializer {
                match rec.status {
                    Decoded::Mismatch => return Fields::Mismatch(range),
                    Decoded::Skipped => return Fields::OnlySkipped,
                    Decoded::Exact | Decoded::Pending => {}
                }
                for (path, value) in self.paths[range.clone()].iter().zip(&mut self.values) {
                    props.set(path.as_slice(), value);
                }
                return Fields::Exact(range);
            }
            if rec.status == Decoded::Exact {
                self.values.by_ref().take(range.len()).for_each(drop);
            }
        }
        let mut r = BitReader::new(self.data);
        r.seek(rec.bit_start as usize);
        let mut values = std::mem::take(&mut dec.values);
        let exact = decode_entity_fields(
            &mut r,
            &class.serializer,
            &mut dec.paths,
            &env.coord_params,
            &mut values,
            Some(rec.size as usize),
            Some(props),
        )
        .unwrap_or(false);
        let had_fields = !dec.paths.is_empty();
        if exact && !class.skip.props.is_empty() {
            skip_props(&class.skip, &mut dec.paths, 0, &mut values, 0);
        }
        if exact && had_fields && dec.paths.is_empty() {
            dec.values = values;
            return Fields::OnlySkipped;
        }
        let start = self.paths.len();
        self.paths.extend_from_slice(&dec.paths);
        if exact {
            for (path, v) in dec.paths.iter().zip(values.drain(..)) {
                props.set(path.as_slice(), v);
            }
        }
        values.clear();
        dec.values = values;
        let range = start..self.paths.len();
        if exact {
            Fields::Exact(range)
        } else {
            Fields::Mismatch(range)
        }
    }

    /// Finishes the message: removes an entity deleted by the last record,
    /// and reports a scan error.
    pub fn finish(mut self, store: &mut EntityStore) -> Result<()> {
        self.flush_delete(store);
        match self.error.take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}
