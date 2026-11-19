//! Entity state: classes, per-entity property storage and `svc_PacketEntities` decoding.

pub mod field_path;
mod value;

pub use field_path::FieldPath;
pub use value::{FromValue, Transform, Value};

use crate::bitreader::{BitReader, CoordParams};
use crate::error::{Error, Result};
use crate::sendtables::Serializer;
use field_path::read_field_paths;
use rustc_hash::FxHashMap;
use std::sync::{Arc, Mutex};

/// CS2 networks at most 2^14 entities; handles carry the index in the low 14 bits.
pub const MAX_ENTITIES: usize = 1 << 14;
pub const INDEX_BITS: u32 = 14;
pub const INDEX_MASK: u32 = (1 << INDEX_BITS) - 1;
const SERIAL_BITS: u32 = 17;
/// Value of an unset `CHandle`.
pub const INVALID_HANDLE: u32 = 0xFF_FFFF;

/// Converts an entity handle (`CHandle`) to an entity index.
pub fn handle_to_index(handle: u32) -> Option<usize> {
    if handle == INVALID_HANDLE || handle & INDEX_MASK == INDEX_MASK {
        None
    } else {
        Some((handle & INDEX_MASK) as usize)
    }
}

/// A networked class (e.g. `CCSPlayerPawn`) and its serializer.
#[derive(Debug)]
pub struct Class {
    pub id: u32,
    pub name: Arc<str>,
    pub serializer: Arc<Serializer>,
    name_cache: Mutex<FxHashMap<Box<str>, Option<FieldPath>>>,
}

impl Class {
    pub fn new(id: u32, name: Arc<str>, serializer: Arc<Serializer>) -> Self {
        Class {
            id,
            name,
            serializer,
            name_cache: Mutex::new(FxHashMap::default()),
        }
    }

    /// Resolves a dotted property name to its field path (cached per class).
    /// Pre-resolving with this and reading through [`Entity::value_at`] is the
    /// fastest way to read a property repeatedly.
    pub fn prop_path(&self, name: &str) -> Option<FieldPath> {
        let mut cache = self.name_cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(k) = cache.get(name) {
            return *k;
        }
        let path = self
            .serializer
            .path_for(name)
            .and_then(|p| FieldPath::from_slice(&p));
        cache.insert(name.into(), path);
        path
    }
}

/// Property storage: a trie indexed directly by field-path components, so a
/// write is a few array indexings (no hashing).
#[derive(Debug, Clone, Default)]
pub(crate) struct PropNode {
    value: Option<Value>,
    children: Vec<PropNode>,
}

impl PropNode {
    #[inline]
    fn set(&mut self, path: &[i32], v: Value) {
        let mut node = self;
        for &c in path {
            // Components are non-negative: the decoder lookup already validated them.
            let c = c as usize;
            if node.children.len() <= c {
                node.children.resize_with(c + 1, PropNode::default);
            }
            node = &mut node.children[c];
        }
        node.value = Some(v);
    }

    #[inline]
    fn get(&self, path: &[i32]) -> Option<&Value> {
        let mut node = self;
        for &c in path {
            node = node.children.get(usize::try_from(c).ok()?)?;
        }
        node.value.as_ref()
    }

    fn walk<'a>(&'a self, prefix: &mut Vec<i32>, f: &mut dyn FnMut(&[i32], &'a Value)) {
        if let Some(v) = &self.value {
            f(prefix, v);
        }
        for (i, c) in self.children.iter().enumerate() {
            prefix.push(i as i32);
            c.walk(prefix, f);
            prefix.pop();
        }
    }

    fn count(&self) -> usize {
        self.value.is_some() as usize + self.children.iter().map(PropNode::count).sum::<usize>()
    }
}

/// What happened to an entity in a packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityOp {
    Created,
    Updated,
    /// Left the PVS (still exists, no longer updated).
    Left,
    /// About to be removed (the entity is still readable in the callback).
    Deleted,
}

#[derive(Debug, Clone)]
pub struct Entity {
    index: u32,
    serial: u32,
    class: Arc<Class>,
    props: PropNode,
    active: bool,
}

impl Entity {
    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn serial(&self) -> u32 {
        self.serial
    }

    /// Handle referencing this entity (index | serial << 14).
    pub fn handle(&self) -> u32 {
        self.index | (self.serial << INDEX_BITS)
    }

    pub fn class(&self) -> &Arc<Class> {
        &self.class
    }

    pub fn class_name(&self) -> &str {
        &self.class.name
    }

    /// False after the entity left the PVS.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Raw value by dotted property name, e.g. `m_iHealth` or `CBodyComponent.m_cellX`.
    pub fn value(&self, name: &str) -> Option<&Value> {
        self.props.get(self.class.prop_path(name)?.as_slice())
    }

    /// Typed value by dotted property name.
    pub fn get<T: FromValue>(&self, name: &str) -> Option<T> {
        self.value(name).and_then(T::from_value)
    }

    /// Value at a pre-resolved field path (fastest path, see [`Class::prop_path`]).
    pub fn value_at(&self, path: &FieldPath) -> Option<&Value> {
        self.props.get(path.as_slice())
    }

    /// Elements of a dynamic vector property (`CUtlVector`, `CNetworkUtlVectorBase`),
    /// honouring its current length.
    pub fn vector<T: FromValue>(&self, name: &str) -> Vec<T> {
        let Some(path) = self.class.serializer.path_for(name) else {
            return Vec::new();
        };
        let Some(len) = self.props.get(&path).and_then(Value::as_u32) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(len as usize);
        let mut p = path;
        p.push(0);
        for i in 0..len {
            *p.last_mut().unwrap() = i as i32;
            if let Some(v) = self.props.get(&p).and_then(T::from_value) {
                out.push(v);
            }
        }
        out
    }

    /// All properties as `(name, value)`, ordered by field path.
    pub fn props(&self) -> Vec<(String, &Value)> {
        let mut out = Vec::new();
        let ser = &self.class.serializer;
        self.props.walk(&mut Vec::new(), &mut |path, v| {
            let name = ser.name_for(path).unwrap_or_else(|| format!("{path:?}"));
            out.push((name, v));
        });
        out
    }

    pub fn prop_count(&self) -> usize {
        self.props.count()
    }
}

/// Fixed-size entity table indexed by entity index.
#[derive(Debug, Clone)]
pub struct EntityStore {
    slots: Vec<Option<Entity>>,
    count: usize,
}

impl Default for EntityStore {
    fn default() -> Self {
        EntityStore {
            slots: vec![None; MAX_ENTITIES],
            count: 0,
        }
    }
}

impl EntityStore {
    pub fn get(&self, index: usize) -> Option<&Entity> {
        self.slots.get(index).and_then(Option::as_ref)
    }

    /// Entity referenced by a `CHandle` value (index part only).
    pub fn by_handle(&self, handle: u32) -> Option<&Entity> {
        self.get(handle_to_index(handle)?)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Entity> {
        self.slots.iter().filter_map(Option::as_ref)
    }

    pub fn by_class<'a>(&'a self, class: &'a str) -> impl Iterator<Item = &'a Entity> + 'a {
        self.iter().filter(move |e| &*e.class.name == class)
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn clear(&mut self) {
        self.slots.iter_mut().for_each(|s| *s = None);
        self.count = 0;
    }
}

fn trace_enabled() -> bool {
    static T: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *T.get_or_init(|| std::env::var_os("CS2_DEMO_TRACE").is_some())
}

/// Reusable decoding state for packet entities.
#[derive(Default)]
pub(crate) struct EntityDecoder {
    paths: Vec<FieldPath>,
    values: Vec<(FieldPath, Value)>,
    /// Decoded instance baselines per class id.
    baselines: FxHashMap<u32, Arc<Vec<(FieldPath, Value)>>>,
    pub diagnostics: EntityDiagnostics,
    /// Scratch buffer for `serialized_entities` sizes.
    pub sizes: Vec<u32>,
}

impl EntityDecoder {
    pub fn invalidate_baselines(&mut self) {
        self.baselines.clear();
    }

    /// Field paths written by the last Created/Updated step.
    pub fn paths(&self) -> &[FieldPath] {
        &self.paths
    }
}

/// Reads the changed field paths, then their values, appending `(path, value)`
/// to `out`. `existing` (the entity's current state) and values decoded earlier
/// in this batch resolve the concrete subtype behind polymorphic pointers.
fn read_fields(
    r: &mut BitReader,
    serializer: &Serializer,
    paths: &mut Vec<FieldPath>,
    cp: &CoordParams,
    out: &mut Vec<(FieldPath, Value)>,
    existing: Option<&PropNode>,
) -> Result<()> {
    read_field_paths(r, paths)?;
    let trace = trace_enabled();
    let batch_start = out.len();
    for fp in paths.iter() {
        let decoder = {
            let decoded = &out[batch_start..];
            let subtype = |p: &[i32]| -> Option<i32> {
                decoded
                    .iter()
                    .rev()
                    .find(|(q, _)| q.as_slice() == p)
                    .map(|(_, v)| v)
                    .or_else(|| existing?.get(p))
                    .and_then(Value::as_i32)
            };
            serializer.decoder_for_with(fp.as_slice(), &subtype)
        }
        .ok_or_else(|| {
            Error::malformed(format!(
                "no field at path {fp:?} in serializer {}",
                serializer.name
            ))
        })?;
        let start = r.position();
        let v = decoder.decode(r, cp);
        if trace {
            eprintln!(
                "[trace]     {:?} {} = {} ({:?}, {} bits)",
                fp,
                serializer.name_for(fp.as_slice()).unwrap_or_default(),
                v,
                decoder,
                r.position() - start
            );
        }
        out.push((*fp, v));
    }
    r.check_overflow("entity fields")
}

/// Everything needed to decode entities that lives in the parser context.
pub(crate) struct DecodeEnv<'a> {
    pub class_id_bits: u32,
    pub classes: &'a [Option<Arc<Class>>],
    pub coord_params: CoordParams,
    /// The `instancebaseline` string table (key = class id).
    pub baselines: Option<&'a crate::stringtables::StringTable>,
}

/// Counters describing entity data that could not be decoded exactly.
#[derive(Debug, Clone, Default)]
pub struct EntityDiagnostics {
    /// Entity updates whose decoded bit length disagreed with the length
    /// announced by the server; those updates were skipped.
    pub size_mismatches: u64,
    /// Entity creates/updates whose decoded length was checked against the
    /// server-announced length (0 for demos from builds without that data).
    pub verified_updates: u64,
    pub baseline_errors: u64,
    /// The first few problems, for debugging.
    pub samples: Vec<String>,
}

impl EntityDiagnostics {
    fn record(&mut self, msg: impl FnOnce() -> String) {
        if self.samples.len() < 32 {
            self.samples.push(msg());
        }
    }
}

/// Decodes one entity's field data into `out`. With a known `size`, the reader is
/// moved exactly past the data and `false` is returned if the decode disagreed.
fn decode_entity_fields(
    r: &mut BitReader,
    serializer: &Serializer,
    paths: &mut Vec<FieldPath>,
    cp: &CoordParams,
    out: &mut Vec<(FieldPath, Value)>,
    size: Option<usize>,
    existing: Option<&PropNode>,
) -> Result<bool> {
    out.clear();
    let start = r.position();
    let res = read_fields(r, serializer, paths, cp, out, existing);
    let Some(size) = size else {
        res?;
        return Ok(true);
    };
    let consumed = r.position() - start;
    if trace_enabled() {
        eprintln!("[trace]   field data: expected {size} bits, consumed {consumed} ({res:?})");
    }
    r.seek(start + size);
    Ok(res.is_ok() && consumed == size)
}

/// Walks one `svc_PacketEntities` message, applying one entity change per
/// [`PacketEntitiesCursor::next`] call so the caller can inspect the store in between.
pub(crate) struct PacketEntitiesCursor<'a> {
    r: BitReader<'a>,
    index: i64,
    remaining: i32,
    has_pvs_vis_bits: bool,
    /// Bit length of each create/update's field data, in order
    /// (`serialized_entities`, recent builds only).
    sizes: Option<Vec<u32>>,
    size_pos: usize,
    /// Entity reported as Deleted by the previous step, removed on the next one.
    pending_delete: Option<usize>,
}

impl<'a> PacketEntitiesCursor<'a> {
    pub fn new(
        data: &'a [u8],
        updated_entries: i32,
        has_pvs_vis_bits: bool,
        sizes: Option<Vec<u32>>,
    ) -> Self {
        PacketEntitiesCursor {
            r: BitReader::new(data),
            index: -1,
            remaining: updated_entries,
            has_pvs_vis_bits,
            sizes,
            size_pos: 0,
            pending_delete: None,
        }
    }

    fn flush_delete(&mut self, store: &mut EntityStore) {
        if let Some(idx) = self.pending_delete.take()
            && store.slots[idx].take().is_some()
        {
            store.count -= 1;
        }
    }

    /// Finishes the message; returns the size buffer for reuse.
    pub fn finish(mut self, store: &mut EntityStore) -> Result<Option<Vec<u32>>> {
        self.flush_delete(store);
        self.r.check_overflow("packet entities")?;
        Ok(self.sizes)
    }

    /// Applies the next entity change. For Created/Updated, the changed field
    /// paths are in `dec.paths()`. Deleted entities stay readable until the next call.
    pub fn next(
        &mut self,
        store: &mut EntityStore,
        dec: &mut EntityDecoder,
        env: &DecodeEnv,
    ) -> Result<Option<(usize, EntityOp)>> {
        self.flush_delete(store);
        while self.remaining > 0 {
            self.remaining -= 1;
            let r = &mut self.r;
            self.index += r.read_ubitvar() as i64 + 1;
            let idx = self.index as usize;
            if idx >= MAX_ENTITIES {
                return Err(Error::malformed(format!("entity index {idx} out of range")));
            }
            let cmd = r.read_bits(2);
            if trace_enabled() {
                eprintln!("[trace] bit {} idx {idx} cmd {cmd}", r.position());
            }

            if cmd & 1 == 1 {
                if cmd & 2 != 0 {
                    // delete
                    if store.slots[idx].is_some() {
                        self.pending_delete = Some(idx);
                        dec.paths.clear();
                        return Ok(Some((idx, EntityOp::Deleted)));
                    }
                } else if let Some(e) = store.slots[idx].as_mut() {
                    // leave PVS
                    e.active = false;
                    dec.paths.clear();
                    return Ok(Some((idx, EntityOp::Left)));
                }
                continue;
            }

            if cmd & 2 == 0 && self.has_pvs_vis_bits && r.read_bits(2) & 1 == 1 {
                continue;
            }
            let size = match &self.sizes {
                Some(s) => {
                    let v = *s
                        .get(self.size_pos)
                        .ok_or_else(|| Error::malformed("fewer entity sizes than entity updates"))?;
                    self.size_pos += 1;
                    Some(v as usize)
                }
                None => None,
            };
            dec.diagnostics.verified_updates += size.is_some() as u64;

            if cmd & 2 != 0 {
                self.create(idx, size, store, dec, env)?;
                return Ok(Some((idx, EntityOp::Created)));
            }

            // update
            let Some(entity) = store.slots[idx].as_mut() else {
                return Err(Error::malformed(format!("update for missing entity {idx}")));
            };
            let Entity {
                class, props, active, ..
            } = entity;
            *active = true;
            let mut values = std::mem::take(&mut dec.values);
            let ok = decode_entity_fields(
                &mut self.r,
                &class.serializer,
                &mut dec.paths,
                &env.coord_params,
                &mut values,
                size,
                Some(props),
            )?;
            if ok {
                for (path, v) in values.drain(..) {
                    props.set(path.as_slice(), v);
                }
                dec.values = values;
                return Ok(Some((idx, EntityOp::Updated)));
            }
            dec.values = values;
            dec.diagnostics.size_mismatches += 1;
            let name = class.name.clone();
            dec.diagnostics
                .record(|| format!("update of {name} #{idx}: field data size mismatch"));
        }
        Ok(None)
    }

    fn create(
        &mut self,
        idx: usize,
        size: Option<usize>,
        store: &mut EntityStore,
        dec: &mut EntityDecoder,
        env: &DecodeEnv,
    ) -> Result<()> {
        let r = &mut self.r;
        let class_id = r.read_bits(env.class_id_bits);
        let serial = r.read_bits(SERIAL_BITS);
        let _unknown = r.read_varu32();
        let class = env
            .classes
            .get(class_id as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| Error::malformed(format!("unknown class id {class_id} for entity {idx}")))?
            .clone();

        let baseline = match dec.baselines.get(&class_id) {
            Some(b) => b.clone(),
            None => {
                let mut decoded = Vec::new();
                let key = class_id.to_string();
                let raw = env
                    .baselines
                    .and_then(|t| t.find(&key))
                    .and_then(|(_, e)| e.value.clone());
                if let Some(raw) = raw {
                    let mut br = BitReader::new(&raw);
                    let res = read_fields(
                        &mut br,
                        &class.serializer,
                        &mut dec.paths,
                        &env.coord_params,
                        &mut decoded,
                        None,
                    );
                    // A correct decode ends inside the final (padding) byte.
                    let res = res.and_then(|()| match br.remaining_bits() {
                        0..8 => Ok(()),
                        n => Err(Error::malformed(format!("{n} trailing bits"))),
                    });
                    if let Err(e) = res {
                        dec.diagnostics.baseline_errors += 1;
                        dec.diagnostics
                            .record(|| format!("baseline of {} failed: {e}", class.name));
                    }
                }
                let b = Arc::new(decoded);
                dec.baselines.insert(class_id, b.clone());
                b
            }
        };

        if trace_enabled() {
            eprintln!(
                "[trace]   create class {class_id} {} serial {serial} baseline {} props",
                class.name,
                baseline.len()
            );
        }
        let mut props = PropNode::default();
        for (path, v) in baseline.iter() {
            props.set(path.as_slice(), v.clone());
        }
        let mut values = std::mem::take(&mut dec.values);
        let ok = decode_entity_fields(
            &mut self.r,
            &class.serializer,
            &mut dec.paths,
            &env.coord_params,
            &mut values,
            size,
            Some(&props),
        )?;
        if ok {
            for (path, v) in values.drain(..) {
                props.set(path.as_slice(), v);
            }
        } else {
            dec.diagnostics.size_mismatches += 1;
            dec.diagnostics
                .record(|| format!("create of {} #{idx}: field data size mismatch", class.name));
        }
        dec.values = values;
        if store.slots[idx].is_none() {
            store.count += 1;
        }
        store.slots[idx] = Some(Entity {
            index: idx as u32,
            serial,
            class,
            props,
            active: true,
        });
        Ok(())
    }
}
