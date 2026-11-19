//! Legacy game events (`GE_Source1LegacyGameEventList` / `GE_Source1LegacyGameEvent`):
//! `player_death`, `player_hurt`, `round_end`, `bomb_planted`, ...
//!
//! Player references in CS2 events are player *slots* (`userid`, `attacker`,
//! `assister`, ...); the matching `CCSPlayerController` entity index is `slot + 1`.
//! Use [`crate::Context::player_by_slot`] to resolve them.

use crate::proto::{CMsgSource1LegacyGameEvent, CMsgSource1LegacyGameEventList};
use rustc_hash::FxHashMap;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum EventValue {
    String(String),
    Float(f32),
    Int(i32),
    Bool(bool),
    U64(u64),
}

impl EventValue {
    pub fn as_i32(&self) -> Option<i32> {
        match *self {
            EventValue::Int(v) => Some(v),
            EventValue::Bool(b) => Some(b as i32),
            EventValue::U64(v) => i32::try_from(v).ok(),
            EventValue::Float(f) => Some(f as i32),
            EventValue::String(_) => None,
        }
    }

    pub fn as_f32(&self) -> Option<f32> {
        match *self {
            EventValue::Float(v) => Some(v),
            EventValue::Int(v) => Some(v as f32),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            EventValue::Bool(b) => Some(b),
            EventValue::Int(v) => Some(v != 0),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match *self {
            EventValue::U64(v) => Some(v),
            EventValue::Int(v) => u64::try_from(v).ok(),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            EventValue::String(s) => Some(s),
            _ => None,
        }
    }
}

impl std::fmt::Display for EventValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventValue::String(s) => write!(f, "{s:?}"),
            EventValue::Float(v) => write!(f, "{v}"),
            EventValue::Int(v) => write!(f, "{v}"),
            EventValue::Bool(v) => write!(f, "{v}"),
            EventValue::U64(v) => write!(f, "{v}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EventDescriptor {
    pub id: i32,
    pub name: Arc<str>,
    pub keys: Vec<(Arc<str>, i32)>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct GameEvent {
    pub tick: i32,
    pub name: Arc<str>,
    pub fields: Vec<(Arc<str>, EventValue)>,
}

impl GameEvent {
    pub fn get(&self, key: &str) -> Option<&EventValue> {
        self.fields.iter().find(|(k, _)| &**k == key).map(|(_, v)| v)
    }

    pub fn i32(&self, key: &str) -> Option<i32> {
        self.get(key)?.as_i32()
    }

    pub fn f32(&self, key: &str) -> Option<f32> {
        self.get(key)?.as_f32()
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        self.get(key)?.as_bool()
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_str()
    }

    /// Player slot stored in `key` (e.g. `userid`, `attacker`), filtering the
    /// "no player" sentinels (-1 / 65535).
    pub fn player_slot(&self, key: &str) -> Option<i32> {
        let v = self.i32(key)?;
        (0..0xFFFF).contains(&v).then_some(v & 0xFF)
    }
}

#[derive(Debug, Clone, Default)]
pub struct EventDescriptors {
    by_id: FxHashMap<i32, EventDescriptor>,
}

impl EventDescriptors {
    pub(crate) fn load(&mut self, list: &CMsgSource1LegacyGameEventList) {
        for d in &list.descriptors {
            let id = d.eventid.unwrap_or(-1);
            self.by_id.insert(
                id,
                EventDescriptor {
                    id,
                    name: Arc::from(d.name.as_deref().unwrap_or("")),
                    keys: d
                        .keys
                        .iter()
                        .map(|k| (Arc::from(k.name.as_deref().unwrap_or("")), k.r#type.unwrap_or(0)))
                        .collect(),
                },
            );
        }
    }

    pub fn get(&self, id: i32) -> Option<&EventDescriptor> {
        self.by_id.get(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &EventDescriptor> {
        self.by_id.values()
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    pub(crate) fn decode(&self, msg: CMsgSource1LegacyGameEvent, tick: i32) -> Option<GameEvent> {
        let id = msg.eventid?;
        let desc = self.by_id.get(&id)?;
        let mut fields = Vec::with_capacity(msg.keys.len());
        for (i, k) in msg.keys.into_iter().enumerate() {
            let Some((name, _)) = desc.keys.get(i) else { break };
            let value = if let Some(s) = k.val_string {
                EventValue::String(s)
            } else if let Some(f) = k.val_float {
                EventValue::Float(f)
            } else if let Some(v) = k.val_long {
                EventValue::Int(v)
            } else if let Some(v) = k.val_short {
                EventValue::Int(v)
            } else if let Some(v) = k.val_byte {
                EventValue::Int(v)
            } else if let Some(v) = k.val_bool {
                EventValue::Bool(v)
            } else if let Some(v) = k.val_uint64 {
                EventValue::U64(v)
            } else {
                // Unset value: use the type's zero.
                match k.r#type.unwrap_or(0) {
                    1 => EventValue::String(String::new()),
                    2 => EventValue::Float(0.0),
                    6 => EventValue::Bool(false),
                    7 => EventValue::U64(0),
                    _ => EventValue::Int(0),
                }
            };
            fields.push((name.clone(), value));
        }
        Some(GameEvent {
            tick,
            name: desc.name.clone(),
            fields,
        })
    }
}
