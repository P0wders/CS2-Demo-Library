//! Little-endian (LSB-first) bit reader used by every Source 2 bitstream:
//! packet message framing, string tables and entity deltas.
//!
//! Reads never fail individually. Reading past the end yields zero bits and
//! sets an overflow flag, which callers check at message boundaries
//! ([`BitReader::check_overflow`]). This keeps the hot decode paths branch-light.

use crate::error::{Error, Result};

/// Source 2 coordinate / angle encoding parameters. Recent builds send these in
/// `CSVCMsg_FlattenedSerializer.coord_size_params`; the defaults match older builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordParams {
    pub coord_integer_bits: u32,
    pub coord_fractional_bits: u32,
    pub normal_fractional_bits: u32,
}

impl Default for CoordParams {
    fn default() -> Self {
        CoordParams {
            coord_integer_bits: 14,
            coord_fractional_bits: 5,
            normal_fractional_bits: 11,
        }
    }
}

#[derive(Clone)]
pub struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    len_bits: usize,
    overflow: bool,
}

impl<'a> BitReader<'a> {
    #[inline]
    pub fn new(data: &'a [u8]) -> Self {
        BitReader {
            data,
            pos: 0,
            len_bits: data.len() * 8,
            overflow: false,
        }
    }

    /// Current position in bits.
    #[inline]
    pub fn position(&self) -> usize {
        self.pos
    }

    #[inline]
    pub fn len_bits(&self) -> usize {
        self.len_bits
    }

    #[inline]
    pub fn remaining_bits(&self) -> usize {
        self.len_bits.saturating_sub(self.pos)
    }

    #[inline]
    pub fn overflowed(&self) -> bool {
        self.overflow || self.pos > self.len_bits
    }

    pub fn check_overflow(&self, what: &'static str) -> Result<()> {
        if self.overflowed() {
            Err(Error::UnexpectedEof(what))
        } else {
            Ok(())
        }
    }

    /// Moves to an absolute bit position.
    #[inline]
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos;
        self.overflow = pos > self.len_bits;
    }

    #[inline]
    pub fn skip_bits(&mut self, n: usize) {
        self.pos += n;
        if self.pos > self.len_bits {
            self.overflow = true;
        }
    }

    /// Returns the next (up to) 57 bits starting at the current position
    /// without consuming them. Bits past the end of the buffer read as zero.
    #[inline(always)]
    fn peek_word(&self) -> u64 {
        let byte = self.pos >> 3;
        let shift = (self.pos & 7) as u32;
        let word = if let Some(chunk) = self.data.get(byte..byte + 8) {
            u64::from_le_bytes(chunk.try_into().unwrap())
        } else {
            let mut buf = [0u8; 8];
            if byte < self.data.len() {
                let tail = &self.data[byte..];
                buf[..tail.len()].copy_from_slice(tail);
            }
            u64::from_le_bytes(buf)
        };
        word >> shift
    }

    /// Reads `n` bits (0..=32) as an unsigned integer.
    #[inline(always)]
    pub fn read_bits(&mut self, n: u32) -> u32 {
        debug_assert!(n <= 32);
        if n == 0 {
            return 0;
        }
        let v = (self.peek_word() & ((1u64 << n) - 1)) as u32;
        self.pos += n as usize;
        if self.pos > self.len_bits {
            self.overflow = true;
        }
        v
    }

    /// Reads `n` bits (0..=64) as an unsigned integer.
    #[inline]
    pub fn read_bits64(&mut self, n: u32) -> u64 {
        debug_assert!(n <= 64);
        if n <= 32 {
            return self.read_bits(n) as u64;
        }
        let lo = self.read_bits(32) as u64;
        let hi = self.read_bits(n - 32) as u64;
        lo | (hi << 32)
    }

    #[inline(always)]
    pub fn read_bool(&mut self) -> bool {
        self.read_bits(1) == 1
    }

    #[inline]
    pub fn read_u8(&mut self) -> u8 {
        self.read_bits(8) as u8
    }

    #[inline]
    pub fn read_f32(&mut self) -> f32 {
        f32::from_bits(self.read_bits(32))
    }

    /// Exposes the next (up to 57) bits without consuming them.
    #[inline(always)]
    pub fn peek(&self) -> u64 {
        self.peek_word()
    }

    #[inline(always)]
    fn advance(&mut self, n: usize) {
        self.pos += n;
        if self.pos > self.len_bits {
            self.overflow = true;
        }
    }

    /// Protobuf-style unsigned varint, at most 5 bytes.
    /// Decoded from a single 64-bit peek (all 5 bytes fit in the 57 valid bits).
    #[inline]
    pub fn read_varu32(&mut self) -> u32 {
        let w = self.peek_word();
        let mut result = 0u32;
        for i in 0..5u32 {
            let b = (w >> (8 * i)) as u32 & 0xff;
            result |= (b & 0x7f) << (7 * i);
            if b & 0x80 == 0 || i == 4 {
                self.advance(8 * (i as usize + 1));
                return result;
            }
        }
        unreachable!()
    }

    /// Zig-zag encoded signed varint (32-bit).
    #[inline]
    pub fn read_vari32(&mut self) -> i32 {
        let v = self.read_varu32();
        ((v >> 1) as i32) ^ -((v & 1) as i32)
    }

    /// Protobuf-style unsigned varint, at most 10 bytes.
    #[inline]
    pub fn read_varu64(&mut self) -> u64 {
        // Fast path: the first 7 bytes come from one peek.
        let w = self.peek_word();
        let mut result = 0u64;
        for i in 0..7u32 {
            let b = (w >> (8 * i)) & 0xff;
            result |= (b & 0x7f) << (7 * i);
            if b & 0x80 == 0 {
                self.advance(8 * (i as usize + 1));
                return result;
            }
        }
        self.advance(56);
        let mut shift = 49u32;
        loop {
            let b = self.read_u8() as u64;
            result |= (b & 0x7f) << shift;
            shift += 7;
            if b & 0x80 == 0 || shift >= 70 {
                return result;
            }
        }
    }

    /// Zig-zag encoded signed varint (64-bit).
    #[inline]
    pub fn read_vari64(&mut self) -> i64 {
        let v = self.read_varu64();
        ((v >> 1) as i64) ^ -((v & 1) as i64)
    }

    /// Valve "UBitVar": 6-bit prefix whose top two bits select 0/4/8/28 extra bits.
    #[inline]
    pub fn read_ubitvar(&mut self) -> u32 {
        let ret = self.read_bits(6);
        match ret & 0x30 {
            16 => (ret & 15) | (self.read_bits(4) << 4),
            32 => (ret & 15) | (self.read_bits(8) << 4),
            48 => (ret & 15) | (self.read_bits(28) << 4),
            _ => ret,
        }
    }

    /// Variable-length integer used by field-path operations: up to four
    /// selector bits choose a 2/4/10/17/31-bit payload.
    #[inline]
    pub fn read_ubitvar_fp(&mut self) -> u32 {
        let w = self.peek_word();
        // (selector bits consumed, payload bits)
        let (sel, bits) = if w & 1 != 0 {
            (1, 2)
        } else if w & 2 != 0 {
            (2, 4)
        } else if w & 4 != 0 {
            (3, 10)
        } else if w & 8 != 0 {
            (4, 17)
        } else {
            (4, 31)
        };
        // sel + bits <= 35 < 57 valid bits
        let v = ((w >> sel) & ((1u64 << bits) - 1)) as u32;
        self.advance(sel + bits as usize);
        v
    }

    /// Source "bit coord": optional integer and fractional parts plus a sign bit.
    pub fn read_coord(&mut self, p: &CoordParams) -> f32 {
        let has_int = self.read_bool();
        let has_frac = self.read_bool();
        if !has_int && !has_frac {
            return 0.0;
        }
        let negative = self.read_bool();
        let int_val = if has_int {
            self.read_bits(p.coord_integer_bits) + 1
        } else {
            0
        };
        let frac_val = if has_frac {
            self.read_bits(p.coord_fractional_bits)
        } else {
            0
        };
        let v = int_val as f32 + frac_val as f32 * (1.0 / (1u32 << p.coord_fractional_bits) as f32);
        if negative { -v } else { v }
    }

    /// Angle quantized to `n` bits over [0, 360).
    #[inline]
    pub fn read_angle(&mut self, n: u32) -> f32 {
        self.read_bits(n) as f32 * 360.0 / (1u64 << n) as f32
    }

    /// 20-bit precise angle in [-180, 180).
    #[inline]
    pub fn read_coord_precise(&mut self) -> f32 {
        self.read_bits(20) as f32 * 360.0 / (1u32 << 20) as f32 - 180.0
    }

    /// Sign bit plus fractional magnitude in [0, 1].
    pub fn read_normal(&mut self, p: &CoordParams) -> f32 {
        let negative = self.read_bool();
        let len = self.read_bits(p.normal_fractional_bits);
        let v = len as f32 * (1.0 / ((1u32 << p.normal_fractional_bits) - 1) as f32);
        if negative { -v } else { v }
    }

    /// Unit vector with optional X/Y and a Z reconstructed from them.
    pub fn read_3bit_normal(&mut self, p: &CoordParams) -> [f32; 3] {
        let has_x = self.read_bool();
        let has_y = self.read_bool();
        let x = if has_x { self.read_normal(p) } else { 0.0 };
        let y = if has_y { self.read_normal(p) } else { 0.0 };
        let neg_z = self.read_bool();
        let sum = x * x + y * y;
        let mut z = if sum < 1.0 { (1.0 - sum).sqrt() } else { 0.0 };
        if neg_z {
            z = -z;
        }
        [x, y, z]
    }

    /// Null-terminated string (lossy UTF-8).
    pub fn read_cstring(&mut self) -> String {
        let mut bytes = Vec::new();
        loop {
            if self.pos + 8 > self.len_bits {
                self.overflow = true;
                break;
            }
            let b = self.read_u8();
            if b == 0 {
                break;
            }
            bytes.push(b);
        }
        match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        }
    }

    /// Reads `n` whole bytes (not necessarily byte-aligned).
    pub fn read_bytes(&mut self, n: usize) -> Vec<u8> {
        if self.pos + n * 8 > self.len_bits {
            self.overflow = true;
            self.pos = self.len_bits + 1;
            return Vec::new();
        }
        if self.pos & 7 == 0 {
            let start = self.pos >> 3;
            self.pos += n * 8;
            return self.data[start..start + n].to_vec();
        }
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.read_u8());
        }
        out
    }

    /// Appends `n` whole bytes (not necessarily byte-aligned) to `out`.
    pub fn read_bytes_into(&mut self, out: &mut Vec<u8>, n: usize) {
        if self.pos + n * 8 > self.len_bits {
            self.overflow = true;
            self.pos = self.len_bits + 1;
            return;
        }
        let start = self.pos >> 3;
        let shift = (self.pos & 7) as u32;
        if shift == 0 {
            out.extend_from_slice(&self.data[start..start + n]);
        } else if n > 0 {
            // The last needed bit lives in byte start + n, which exists because
            // pos + 8n <= len_bits and shift > 0.
            let src = &self.data[start..=start + n];
            out.extend(src.windows(2).map(|w| (w[0] >> shift) | (w[1] << (8 - shift))));
        }
        self.pos += n * 8;
    }

    /// Returns a byte-aligned sub-slice of `n` bytes if the reader is aligned.
    pub fn read_bytes_aligned(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.pos & 7 != 0 || self.pos + n * 8 > self.len_bits {
            return None;
        }
        let start = self.pos >> 3;
        self.pos += n * 8;
        Some(&self.data[start..start + n])
    }

    /// Reads `bits` bits into a byte vector (last byte zero-padded).
    pub fn read_bits_as_bytes(&mut self, bits: usize) -> Vec<u8> {
        let mut out = self.read_bytes(bits / 8);
        let rem = (bits % 8) as u32;
        if rem > 0 {
            out.push(self.read_bits(rem) as u8);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_are_lsb_first() {
        let data = [0b1010_1100u8, 0xff];
        let mut r = BitReader::new(&data);
        assert_eq!(r.read_bits(2), 0b00);
        assert_eq!(r.read_bits(3), 0b011);
        assert_eq!(r.read_bits(3), 0b101);
        assert_eq!(r.read_bits(8), 0xff);
        assert!(!r.overflowed());
        r.read_bits(1);
        assert!(r.overflowed());
    }

    #[test]
    fn varints() {
        // 300 = 0b1_0010_1100 -> [0xAC, 0x02]
        let data = [0xAC, 0x02, 0x03];
        let mut r = BitReader::new(&data);
        assert_eq!(r.read_varu32(), 300);
        assert_eq!(r.read_vari32(), -2);
    }

    #[test]
    fn unaligned_varint_and_bytes() {
        // one leading 1-bit, then 0x96 0x01 (=150) and "hi\0", shifted left by one bit
        let raw: [u8; 5] = [0x96, 0x01, b'h', b'i', 0];
        let mut shifted = vec![0u8; raw.len() + 1];
        shifted[0] = 1;
        for (i, b) in raw.iter().enumerate() {
            shifted[i] |= b << 1;
            shifted[i + 1] |= b >> 7;
        }
        let mut r = BitReader::new(&shifted);
        assert!(r.read_bool());
        assert_eq!(r.read_varu32(), 150);
        assert_eq!(r.read_cstring(), "hi");
    }

    #[test]
    fn ubitvar() {
        // 6 bits = 0b01_0101 -> selector 16 -> low nibble 5 | next 4 bits << 4
        let data = [0b1101_0101u8, 0b0000_0010];
        let mut r = BitReader::new(&data);
        // bits: 0b010101 (=21), then 4 bits: 0b1011 -> 11
        assert_eq!(r.read_ubitvar(), 5 | (0b1011 << 4));
    }

    #[test]
    fn read_bits64_spans_words() {
        let data = 0x0123_4567_89ab_cdefu64.to_le_bytes();
        let mut r = BitReader::new(&data);
        r.read_bits(4);
        assert_eq!(r.read_bits64(60), 0x0012_3456_789a_bcde);
    }
}
