//! Field value decoders. Which decoder a field uses is derived from its type
//! name, its encoder name (`var_encoder`), bit count, range and flags.

use crate::bitreader::{BitReader, CoordParams};
use crate::entities::Value;

const QFF_ROUNDDOWN: u32 = 1 << 0;
const QFF_ROUNDUP: u32 = 1 << 1;
const QFF_ENCODE_ZERO_EXACTLY: u32 = 1 << 2;
const QFF_ENCODE_INTEGERS_EXACTLY: u32 = 1 << 3;

/// Valve's `CNetworkedQuantizedFloat` / quantized float32 encoding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuantizedFloat {
    pub low: f32,
    pub high: f32,
    pub bit_count: u32,
    pub flags: u32,
    high_low_mul: f32,
    dec_mul: f32,
    no_scale: bool,
}

impl QuantizedFloat {
    pub fn new(bit_count: i32, flags: i32, low: f32, high: f32) -> QuantizedFloat {
        let mut q = QuantizedFloat {
            low,
            high,
            bit_count: bit_count.max(0) as u32,
            flags: flags.max(0) as u32,
            high_low_mul: 0.0,
            dec_mul: 0.0,
            no_scale: false,
        };
        if q.bit_count == 0 || q.bit_count >= 32 {
            q.no_scale = true;
            q.bit_count = 32;
            return q;
        }
        q.validate_flags();

        let mut steps = 1u64 << q.bit_count;
        if q.flags & QFF_ROUNDDOWN != 0 {
            let range = q.high - q.low;
            q.high -= range / steps as f32;
        } else if q.flags & QFF_ROUNDUP != 0 {
            let range = q.high - q.low;
            q.low += range / steps as f32;
        }

        if q.flags & QFF_ENCODE_INTEGERS_EXACTLY != 0 {
            let delta = (q.high - q.low).max(1.0);
            let delta_log2 = (delta as f64).log2().ceil() as u32;
            let range2 = 1u64 << delta_log2;
            let mut bc = q.bit_count;
            while (1u64 << bc) < range2 {
                bc += 1;
            }
            if bc > q.bit_count {
                q.bit_count = bc;
                steps = 1u64 << bc;
            }
            let offset = range2 as f32 / steps as f32;
            q.high = q.low + range2 as f32 - offset;
        }

        q.assign_multipliers(steps);

        if q.flags & QFF_ROUNDDOWN != 0 && q.quantize(q.low) == q.low {
            q.flags &= !QFF_ROUNDDOWN;
        }
        if q.flags & QFF_ROUNDUP != 0 && q.quantize(q.high) == q.high {
            q.flags &= !QFF_ROUNDUP;
        }
        if q.flags & QFF_ENCODE_ZERO_EXACTLY != 0 && q.quantize(0.0) == 0.0 {
            q.flags &= !QFF_ENCODE_ZERO_EXACTLY;
        }
        q
    }

    fn validate_flags(&mut self) {
        if self.flags == 0 {
            return;
        }
        if (self.low == 0.0 && self.flags & QFF_ROUNDDOWN != 0)
            || (self.high == 0.0 && self.flags & QFF_ROUNDUP != 0)
        {
            self.flags &= !QFF_ENCODE_ZERO_EXACTLY;
        }
        if self.low == 0.0 && self.flags & QFF_ENCODE_ZERO_EXACTLY != 0 {
            self.flags |= QFF_ROUNDDOWN;
            self.flags &= !QFF_ENCODE_ZERO_EXACTLY;
        }
        if self.high == 0.0 && self.flags & QFF_ENCODE_ZERO_EXACTLY != 0 {
            self.flags |= QFF_ROUNDUP;
            self.flags &= !QFF_ENCODE_ZERO_EXACTLY;
        }
        if self.low > 0.0 || self.high < 0.0 {
            self.flags &= !QFF_ENCODE_ZERO_EXACTLY;
        }
        if self.flags & QFF_ENCODE_INTEGERS_EXACTLY != 0 {
            self.flags &= !(QFF_ROUNDUP | QFF_ROUNDDOWN | QFF_ENCODE_ZERO_EXACTLY);
        }
    }

    fn assign_multipliers(&mut self, steps: u64) {
        let range = self.high - self.low;
        let high: u32 = if self.bit_count == 32 {
            0xFFFF_FFFE
        } else {
            (1u32 << self.bit_count) - 1
        };
        let mut high_mul = if range.abs() <= 0.0 {
            high as f32
        } else {
            high as f32 / range
        };
        if high_mul * range > high as f32 || (high_mul * range) as f64 > high as f64 {
            for mult in [0.9999f32, 0.99, 0.9, 0.8, 0.7] {
                high_mul = high as f32 / range * mult;
                if high_mul * range > high as f32 || (high_mul * range) as f64 > high as f64 {
                    continue;
                }
                break;
            }
        }
        self.high_low_mul = high_mul;
        self.dec_mul = 1.0 / (steps - 1) as f32;
    }

    fn quantize(&self, val: f32) -> f32 {
        if val < self.low {
            return self.low;
        }
        if val > self.high {
            return self.high;
        }
        let i = ((val - self.low) * self.high_low_mul) as u32;
        self.low + (self.high - self.low) * (i as f32 * self.dec_mul)
    }

    #[inline]
    pub fn decode(&self, r: &mut BitReader) -> f32 {
        if self.no_scale {
            return r.read_f32();
        }
        if self.flags & QFF_ROUNDDOWN != 0 && r.read_bool() {
            return self.low;
        }
        if self.flags & QFF_ROUNDUP != 0 && r.read_bool() {
            return self.high;
        }
        if self.flags & QFF_ENCODE_ZERO_EXACTLY != 0 && r.read_bool() {
            return 0.0;
        }
        self.low + (self.high - self.low) * r.read_bits(self.bit_count) as f32 * self.dec_mul
    }
}

/// CS2 servers simulate at 64 ticks per second.
pub const SIM_TIME_INTERVAL: f32 = 1.0 / 64.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FloatDecoder {
    /// Raw IEEE-754 32 bits.
    NoScale,
    Coord,
    /// Simulation time sent as a varint tick count; decoded to seconds at 64 tick.
    SimTime,
    RuneTime,
    Quantized(QuantizedFloat),
}

impl FloatDecoder {
    #[inline]
    pub fn decode(&self, r: &mut BitReader, cp: &CoordParams) -> f32 {
        match self {
            FloatDecoder::NoScale => r.read_f32(),
            FloatDecoder::Coord => r.read_coord(cp),
            FloatDecoder::SimTime => r.read_varu32() as f32 * SIM_TIME_INTERVAL,
            FloatDecoder::RuneTime => f32::from_bits(r.read_bits(4)),
            FloatDecoder::Quantized(q) => q.decode(r),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Decoder {
    Bool,
    VarU32,
    VarI32,
    VarU64,
    VarI64,
    Fixed64,
    String,
    /// Body/physics/render component presence bit.
    Component,
    /// `m_iClip1`: varint encoded as ammo + 1.
    Ammo,
    Float(FloatDecoder),
    Vector2(FloatDecoder),
    Vector3(FloatDecoder),
    Vector4(FloatDecoder),
    VectorNormal,
    QAnglePitchYaw(u32),
    QAngleBits(u32),
    QAngleCoord,
    QAnglePrecise,
    /// `CTransform`: position (3 floats) + orientation quaternion (4 floats).
    Transform(FloatDecoder),
    /// `CUtlBinaryBlock`: varint byte length + raw bytes.
    Bytes,
    /// Raw fixed-width integer (`fixed8` / `fixed16` / `fixed32` encoders):
    /// unsigned, or two's-complement when `signed`.
    FixedInt {
        bits: u8,
        signed: bool,
    },
    /// Polymorphic pointer (e.g. `CCSGameModeRules*`): presence bit, then the
    /// index of the concrete subtype in the field's polymorphic type list.
    /// Decodes to `I32(index)`, or `I32(-1)` when null.
    PolymorphicPointer,
}

impl Decoder {
    #[inline]
    pub fn decode(&self, r: &mut BitReader, cp: &CoordParams) -> Value {
        match *self {
            Decoder::Bool | Decoder::Component => Value::Bool(r.read_bool()),
            Decoder::VarU32 => Value::U32(r.read_varu32()),
            Decoder::VarI32 => Value::I32(r.read_vari32()),
            Decoder::VarU64 => Value::U64(r.read_varu64()),
            Decoder::VarI64 => Value::I64(r.read_vari64()),
            Decoder::Fixed64 => Value::U64(r.read_bits64(64)),
            Decoder::String => Value::String(r.read_cstring().into()),
            Decoder::Ammo => Value::I32(r.read_varu32() as i32 - 1),
            Decoder::Float(f) => Value::F32(f.decode(r, cp)),
            Decoder::Vector2(f) => Value::Vec2([f.decode(r, cp), f.decode(r, cp)]),
            Decoder::Vector3(f) => Value::Vec3([f.decode(r, cp), f.decode(r, cp), f.decode(r, cp)]),
            Decoder::Vector4(f) => {
                Value::Vec4([f.decode(r, cp), f.decode(r, cp), f.decode(r, cp), f.decode(r, cp)])
            }
            Decoder::VectorNormal => Value::Vec3(r.read_3bit_normal(cp)),
            Decoder::QAnglePitchYaw(n) => Value::Vec3([r.read_angle(n), r.read_angle(n), 0.0]),
            Decoder::QAngleBits(n) => Value::Vec3([r.read_angle(n), r.read_angle(n), r.read_angle(n)]),
            Decoder::QAngleCoord => {
                let (hx, hy, hz) = (r.read_bool(), r.read_bool(), r.read_bool());
                let x = if hx { r.read_coord(cp) } else { 0.0 };
                let y = if hy { r.read_coord(cp) } else { 0.0 };
                let z = if hz { r.read_coord(cp) } else { 0.0 };
                Value::Vec3([x, y, z])
            }
            Decoder::QAnglePrecise => {
                let (hx, hy, hz) = (r.read_bool(), r.read_bool(), r.read_bool());
                let x = if hx { r.read_coord_precise() } else { 0.0 };
                let y = if hy { r.read_coord_precise() } else { 0.0 };
                let z = if hz { r.read_coord_precise() } else { 0.0 };
                Value::Vec3([x, y, z])
            }
            Decoder::Transform(f) => {
                let pos = [f.decode(r, cp), f.decode(r, cp), f.decode(r, cp)];
                let rot = [r.read_f32(), r.read_f32(), r.read_f32(), r.read_f32()];
                Value::Transform(Box::new(crate::entities::Transform { pos, rot }))
            }
            Decoder::Bytes => {
                let len = r.read_varu32() as usize;
                Value::Bytes(r.read_bytes(len).into())
            }
            Decoder::FixedInt { bits, signed } => {
                let raw = r.read_bits(bits as u32);
                if signed {
                    // sign-extend from `bits`
                    let shift = 32 - bits as u32;
                    Value::I32(((raw << shift) as i32) >> shift)
                } else {
                    Value::U32(raw)
                }
            }
            Decoder::PolymorphicPointer => {
                if r.read_bool() {
                    Value::I32(r.read_ubitvar() as i32)
                } else {
                    Value::I32(-1)
                }
            }
        }
    }
}

/// Field metadata needed to choose a decoder.
pub struct DecoderInput<'a> {
    pub var_name: &'a str,
    pub base_type: &'a str,
    pub encoder: Option<&'a str>,
    pub bit_count: i32,
    pub low: f32,
    pub high: f32,
    pub flags: i32,
    pub signed_enum: bool,
}

fn float_decoder(f: &DecoderInput) -> FloatDecoder {
    match f.encoder {
        Some("coord") => return FloatDecoder::Coord,
        Some("simtime") => return FloatDecoder::SimTime,
        Some("runetime") => return FloatDecoder::RuneTime,
        _ => {}
    }
    if f.bit_count <= 0 || f.bit_count >= 32 {
        return FloatDecoder::NoScale;
    }
    FloatDecoder::Quantized(QuantizedFloat::new(f.bit_count, f.flags, f.low, f.high))
}

fn qangle_decoder(f: &DecoderInput) -> Decoder {
    match f.encoder {
        Some("qangle_pitch_yaw") if f.bit_count > 0 => return Decoder::QAnglePitchYaw(f.bit_count as u32),
        Some("qangle_precise") => return Decoder::QAnglePrecise,
        _ => {}
    }
    if f.bit_count > 0 && f.bit_count <= 32 {
        Decoder::QAngleBits(f.bit_count as u32)
    } else {
        Decoder::QAngleCoord
    }
}

/// Picks the decoder for a value of type `base_type`.
pub fn find_decoder(f: &DecoderInput) -> Decoder {
    if f.var_name == "m_iClip1" {
        return Decoder::Ammo;
    }
    // Patch 14197 added raw fixed-width encodings for small integer types and
    // byte-sized enums (previously varints).
    let fixed_bits = match f.encoder {
        Some("fixed8") => Some(8),
        Some("fixed16") => Some(16),
        Some("fixed32") => Some(32),
        _ => None,
    };
    if let Some(bits) = fixed_bits {
        let signed = f.signed_enum || matches!(f.base_type, "int8" | "int16" | "int32");
        return Decoder::FixedInt { bits, signed };
    }
    match f.base_type {
        "bool" => Decoder::Bool,
        "char" | "CUtlString" | "CUtlSymbolLarge" | "CGlobalSymbol" => Decoder::String,
        "int8" | "int16" | "int32" => Decoder::VarI32,
        "int64" => Decoder::VarI64,
        "uint8"
        | "uint16"
        | "uint32"
        | "color32"
        | "Color"
        | "CUtlStringToken"
        | "CGameSceneNodeHandle"
        | "CHandle"
        | "CEntityHandle"
        | "CEntityIndex" => Decoder::VarU32,
        "uint64" | "CStrongHandle" => {
            if f.encoder == Some("fixed64") {
                Decoder::Fixed64
            } else {
                Decoder::VarU64
            }
        }
        "CBodyComponent" | "CPhysicsComponent" | "CRenderComponent" => Decoder::Component,
        // Observed in build 10955 / protocol 14196: plain float32 fields without
        // bit count or encoder (m_flSimulationTime, m_flAnimTime) are tick varints,
        // while raw floats are declared with bit_count = 32.
        "float32" if f.bit_count == 0 && f.encoder.is_none() => Decoder::Float(FloatDecoder::SimTime),
        "float32" | "float64" | "CNetworkedQuantizedFloat" | "GameTime_t" => Decoder::Float(float_decoder(f)),
        "CTransform" => Decoder::Transform(float_decoder(f)),
        "CUtlBinaryBlock" => Decoder::Bytes,
        "Vector" | "VectorWS" => {
            if f.encoder == Some("normal") {
                Decoder::VectorNormal
            } else {
                Decoder::Vector3(float_decoder(f))
            }
        }
        "Vector2D" => Decoder::Vector2(float_decoder(f)),
        "Vector4D" | "Quaternion" => Decoder::Vector4(float_decoder(f)),
        "QAngle" => qangle_decoder(f),
        _ => {
            if f.signed_enum {
                Decoder::VarI32
            } else {
                Decoder::VarU32
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantized_basic_range() {
        // 8 bits over [0, 255]: raw value maps linearly
        let q = QuantizedFloat::new(8, 0, 0.0, 255.0);
        let data = [200u8];
        let mut r = BitReader::new(&data);
        assert!((q.decode(&mut r) - 200.0).abs() < 1e-3);
    }

    #[test]
    fn fixed8_signed_and_unsigned() {
        let cp = CoordParams::default();
        let data = [0xFEu8, 0xC8];
        let mut r = BitReader::new(&data);
        let signed = Decoder::FixedInt {
            bits: 8,
            signed: true,
        };
        let unsigned = Decoder::FixedInt {
            bits: 8,
            signed: false,
        };
        assert_eq!(signed.decode(&mut r, &cp), Value::I32(-2));
        assert_eq!(unsigned.decode(&mut r, &cp), Value::U32(200));
        assert_eq!(r.position(), 16);
    }

    #[test]
    fn quantized_noscale() {
        let q = QuantizedFloat::new(0, 0, 0.0, 1.0);
        let data = 1.5f32.to_bits().to_le_bytes();
        let mut r = BitReader::new(&data);
        assert_eq!(q.decode(&mut r), 1.5);
    }

    #[test]
    fn quantized_rounddown_shrinks_high_and_is_stripped() {
        // ROUNDDOWN lowers `high` by one step; since low quantizes exactly the flag
        // is then dropped, so no prefix bit is read.
        let q = QuantizedFloat::new(10, QFF_ROUNDDOWN as i32, -10.0, 10.0);
        assert_eq!(q.flags & QFF_ROUNDDOWN, 0);
        assert!((q.high - (10.0 - 20.0 / 1024.0)).abs() < 1e-5);
        let data = [0u8, 0];
        let mut r = BitReader::new(&data);
        assert_eq!(q.decode(&mut r), -10.0);
        assert_eq!(r.position(), 10);
    }
}
