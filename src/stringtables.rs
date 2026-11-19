//! Network string tables (`svc_CreateStringTable` / `svc_UpdateStringTable`).
//!
//! Two tables matter for demo analysis:
//! * `instancebaseline`: key = class id, value = the default field state
//!   applied when an entity of that class is created.
//! * `userinfo`: entry index = player slot, value = `CMsgPlayerInfo`.

use crate::bitreader::BitReader;
use crate::error::{Result, decode_proto};
use crate::proto::{CMsgPlayerInfo, CsvcMsgCreateStringTable, CsvcMsgUpdateStringTable};
use rustc_hash::FxHashMap;
use std::sync::Arc;

const KEY_HISTORY_SIZE: usize = 32;

#[derive(Debug, Clone, Default)]
pub struct StringTableEntry {
    pub key: String,
    pub value: Option<Arc<[u8]>>,
}

#[derive(Debug, Clone)]
pub struct StringTable {
    pub name: String,
    pub user_data_fixed_size: bool,
    pub user_data_size_bits: u32,
    pub flags: i32,
    pub using_varint_bitcounts: bool,
    /// Entries indexed by their table index; gaps are `None`.
    pub entries: Vec<Option<StringTableEntry>>,
}

impl StringTable {
    pub fn get(&self, index: usize) -> Option<&StringTableEntry> {
        self.entries.get(index).and_then(Option::as_ref)
    }

    pub fn find(&self, key: &str) -> Option<(usize, &StringTableEntry)> {
        self.entries
            .iter()
            .enumerate()
            .find_map(|(i, e)| e.as_ref().filter(|e| e.key == key).map(|e| (i, e)))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, &StringTableEntry)> {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.as_ref().map(|e| (i, e)))
    }

    /// Applies an encoded batch of `num_entries` changes.
    /// Returns the indices that were touched.
    fn apply(&mut self, data: &[u8], num_entries: i32) -> Result<Vec<usize>> {
        let mut touched = Vec::with_capacity(num_entries.max(0) as usize);
        if data.is_empty() {
            return Ok(touched);
        }
        let mut r = BitReader::new(data);
        let mut index: i64 = -1;
        let mut history: Vec<String> = Vec::with_capacity(KEY_HISTORY_SIZE);

        for _ in 0..num_entries {
            if r.read_bool() {
                index += 1;
            } else {
                index = r.read_varu32() as i64 + 1;
            }

            let mut key: Option<String> = None;
            if r.read_bool() {
                let k = if r.read_bool() {
                    let pos = r.read_bits(5) as usize;
                    let size = r.read_bits(5) as usize;
                    let suffix = r.read_cstring();
                    match history.get(pos) {
                        Some(prev) if size <= prev.len() => {
                            let mut s = String::with_capacity(size + suffix.len());
                            // keys are ASCII in practice; fall back to bytes if not on a char boundary
                            match prev.get(..size) {
                                Some(p) => s.push_str(p),
                                None => s.push_str(&String::from_utf8_lossy(&prev.as_bytes()[..size])),
                            }
                            s.push_str(&suffix);
                            s
                        }
                        Some(prev) => format!("{prev}{suffix}"),
                        None => suffix,
                    }
                } else {
                    r.read_cstring()
                };
                if history.len() >= KEY_HISTORY_SIZE {
                    history.remove(0);
                }
                history.push(k.clone());
                key = Some(k);
            }

            let mut value: Option<Arc<[u8]>> = None;
            if r.read_bool() {
                let mut compressed = false;
                let bits = if self.user_data_fixed_size {
                    self.user_data_size_bits as usize
                } else {
                    if self.flags & 0x1 != 0 {
                        compressed = r.read_bool();
                    }
                    if self.using_varint_bitcounts {
                        r.read_ubitvar() as usize * 8
                    } else {
                        r.read_bits(17) as usize * 8
                    }
                };
                let mut bytes = r.read_bits_as_bytes(bits);
                if compressed {
                    bytes = snap::raw::Decoder::new().decompress_vec(&bytes)?;
                }
                value = Some(bytes.into());
            }
            r.check_overflow("string table entry")?;

            let idx = index as usize;
            if idx >= self.entries.len() {
                self.entries.resize_with(idx + 1, || None);
            }
            let slot = &mut self.entries[idx];
            match slot {
                Some(entry) => {
                    if let Some(k) = key
                        && !k.is_empty()
                    {
                        entry.key = k;
                    }
                    if value.is_some() {
                        entry.value = value;
                    }
                }
                None => {
                    *slot = Some(StringTableEntry {
                        key: key.unwrap_or_default(),
                        value,
                    });
                }
            }
            touched.push(idx);
        }
        Ok(touched)
    }
}

/// All string tables, indexed by creation order (that is the `table_id` used by updates).
#[derive(Debug, Clone, Default)]
pub struct StringTables {
    tables: Vec<StringTable>,
    by_name: FxHashMap<String, usize>,
}

impl StringTables {
    pub fn get(&self, name: &str) -> Option<&StringTable> {
        self.by_name.get(name).map(|&i| &self.tables[i])
    }

    pub fn by_id(&self, id: usize) -> Option<&StringTable> {
        self.tables.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &StringTable> {
        self.tables.iter()
    }

    pub fn clear(&mut self) {
        self.tables.clear();
        self.by_name.clear();
    }

    /// Handles `svc_CreateStringTable`. Returns (table id, touched entries).
    pub(crate) fn create(&mut self, msg: &CsvcMsgCreateStringTable) -> Result<(usize, Vec<usize>)> {
        let mut table = StringTable {
            name: msg.name.clone().unwrap_or_default(),
            user_data_fixed_size: msg.user_data_fixed_size.unwrap_or(false),
            user_data_size_bits: msg.user_data_size_bits.unwrap_or(0).max(0) as u32,
            flags: msg.flags.unwrap_or(0),
            using_varint_bitcounts: msg.using_varint_bitcounts.unwrap_or(false),
            entries: Vec::new(),
        };
        let raw = msg.string_data.as_deref().unwrap_or(&[]);
        let touched = if msg.data_compressed.unwrap_or(false) {
            let data = snap::raw::Decoder::new().decompress_vec(raw)?;
            table.apply(&data, msg.num_entries.unwrap_or(0))?
        } else {
            table.apply(raw, msg.num_entries.unwrap_or(0))?
        };
        let id = self.tables.len();
        self.by_name.insert(table.name.clone(), id);
        self.tables.push(table);
        Ok((id, touched))
    }

    /// Handles `svc_UpdateStringTable`. Returns touched entries.
    pub(crate) fn update(&mut self, msg: &CsvcMsgUpdateStringTable) -> Result<(usize, Vec<usize>)> {
        let id = msg.table_id.unwrap_or(-1);
        let Some(table) = usize::try_from(id).ok().and_then(|i| self.tables.get_mut(i)) else {
            return Err(crate::Error::malformed(format!(
                "update for unknown string table {id}"
            )));
        };
        let touched = table.apply(
            msg.string_data.as_deref().unwrap_or(&[]),
            msg.num_changed_entries.unwrap_or(0),
        )?;
        Ok((id as usize, touched))
    }
}

/// Decodes a `userinfo` entry value.
pub fn decode_player_info(value: &[u8]) -> Result<CMsgPlayerInfo> {
    decode_proto("CMsgPlayerInfo", value)
}
