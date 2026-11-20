use std::fmt;

/// A decoded entity property value.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum Value {
    Bool(bool),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    F32(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    String(Box<str>),
    /// `CTransform`: position + orientation quaternion (boxed: rare, keeps `Value` small).
    Transform(Box<Transform>),
    Bytes(Box<[u8]>),
}

impl Value {
    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Value::Bool(b) => Some(b),
            Value::I32(v) => Some(v != 0),
            Value::U32(v) => Some(v != 0),
            Value::U64(v) => Some(v != 0),
            Value::I64(v) => Some(v != 0),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Value::Bool(b) => Some(b as i64),
            Value::I32(v) => Some(v as i64),
            Value::U32(v) => Some(v as i64),
            Value::I64(v) => Some(v),
            Value::U64(v) => i64::try_from(v).ok(),
            Value::F32(v) => Some(v as i64),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match *self {
            Value::U32(v) => Some(v as i32),
            _ => self.as_i64().and_then(|v| i32::try_from(v).ok()),
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        match *self {
            Value::I32(v) => Some(v as u32),
            _ => self.as_i64().and_then(|v| u32::try_from(v).ok()),
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match *self {
            Value::U64(v) => Some(v),
            Value::I64(v) => Some(v as u64),
            _ => self.as_i64().map(|v| v as u64),
        }
    }

    pub fn as_f32(&self) -> Option<f32> {
        match *self {
            Value::F32(v) => Some(v),
            Value::I32(v) => Some(v as f32),
            Value::U32(v) => Some(v as f32),
            _ => None,
        }
    }

    pub fn as_vec3(&self) -> Option<[f32; 3]> {
        match *self {
            Value::Vec3(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Bool(v) => write!(f, "{v}"),
            Value::I32(v) => write!(f, "{v}"),
            Value::U32(v) => write!(f, "{v}"),
            Value::I64(v) => write!(f, "{v}"),
            Value::U64(v) => write!(f, "{v}"),
            Value::F32(v) => write!(f, "{v}"),
            Value::Vec2(v) => write!(f, "[{}, {}]", v[0], v[1]),
            Value::Vec3(v) => write!(f, "[{}, {}, {}]", v[0], v[1], v[2]),
            Value::Vec4(v) => write!(f, "[{}, {}, {}, {}]", v[0], v[1], v[2], v[3]),
            Value::String(s) => write!(f, "{s:?}"),
            Value::Transform(t) => write!(f, "pos {:?} rot {:?}", t.pos, t.rot),
            Value::Bytes(b) => write!(f, "<{} bytes>", b.len()),
        }
    }
}

/// Conversion used by the typed `Entity::get::<T>()` accessor.
pub trait FromValue: Sized {
    fn from_value(v: &Value) -> Option<Self>;
}

macro_rules! from_value {
    ($t:ty, $m:ident) => {
        impl FromValue for $t {
            fn from_value(v: &Value) -> Option<Self> {
                v.$m()
            }
        }
    };
}
from_value!(bool, as_bool);
from_value!(i32, as_i32);
from_value!(u32, as_u32);
from_value!(i64, as_i64);
from_value!(u64, as_u64);
from_value!(f32, as_f32);
from_value!([f32; 3], as_vec3);

impl FromValue for String {
    fn from_value(v: &Value) -> Option<Self> {
        v.as_str().map(str::to_owned)
    }
}

impl FromValue for Value {
    fn from_value(v: &Value) -> Option<Self> {
        Some(v.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Transform {
    pub pos: [f32; 3],
    pub rot: [f32; 4],
}

#[cfg(test)]
mod tests {
    #[test]
    fn value_stays_small() {
        assert!(std::mem::size_of::<super::Value>() <= 24);
    }

    #[test]
    fn field_path_stays_small() {
        // Decoded fields cross threads as (path, value) pairs.
        assert!(std::mem::size_of::<crate::entities::FieldPath>() <= 32);
    }
}
