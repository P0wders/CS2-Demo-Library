//! Field paths: the address of a (possibly nested) property inside an entity,
//! and the Huffman-coded operation stream used to enumerate changed paths.

use crate::bitreader::BitReader;
use crate::error::{Error, Result};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::OnceLock;

pub const MAX_DEPTH: usize = 7;

/// Path through the serializer tree, e.g. `[12, 3, 0]`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FieldPath {
    path: [i32; MAX_DEPTH],
    last: usize,
}

impl std::fmt::Debug for FieldPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}

impl FieldPath {
    fn new() -> Self {
        let mut path = [0; MAX_DEPTH];
        path[0] = -1;
        FieldPath { path, last: 0 }
    }

    pub fn from_slice(s: &[i32]) -> Option<Self> {
        if s.is_empty() || s.len() > MAX_DEPTH {
            return None;
        }
        let mut fp = FieldPath {
            path: [0; MAX_DEPTH],
            last: s.len() - 1,
        };
        fp.path[..s.len()].copy_from_slice(s);
        Some(fp)
    }

    #[inline]
    pub fn as_slice(&self) -> &[i32] {
        &self.path[..=self.last]
    }

    #[inline]
    fn pop(&mut self, n: usize) -> bool {
        for _ in 0..n {
            if self.last == 0 {
                return false;
            }
            self.path[self.last] = 0;
            self.last -= 1;
        }
        true
    }

    #[inline]
    fn push(&mut self, v: i32) -> bool {
        if self.last + 1 >= MAX_DEPTH {
            return false;
        }
        self.last += 1;
        self.path[self.last] = v;
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Op {
    PlusOne,
    PlusTwo,
    PlusThree,
    PlusFour,
    PlusN,
    PushOneLeftDeltaZeroRightZero,
    PushOneLeftDeltaZeroRightNonZero,
    PushOneLeftDeltaOneRightZero,
    PushOneLeftDeltaOneRightNonZero,
    PushOneLeftDeltaNRightZero,
    PushOneLeftDeltaNRightNonZero,
    PushOneLeftDeltaNRightNonZeroPack6Bits,
    PushOneLeftDeltaNRightNonZeroPack8Bits,
    PushTwoLeftDeltaZero,
    PushTwoPack5LeftDeltaZero,
    PushThreeLeftDeltaZero,
    PushThreePack5LeftDeltaZero,
    PushTwoLeftDeltaOne,
    PushTwoPack5LeftDeltaOne,
    PushThreeLeftDeltaOne,
    PushThreePack5LeftDeltaOne,
    PushTwoLeftDeltaN,
    PushTwoPack5LeftDeltaN,
    PushThreeLeftDeltaN,
    PushThreePack5LeftDeltaN,
    PushN,
    PushNAndNonTopological,
    PopOnePlusOne,
    PopOnePlusN,
    PopAllButOnePlusOne,
    PopAllButOnePlusN,
    PopAllButOnePlusNPack3Bits,
    PopAllButOnePlusNPack6Bits,
    PopNPlusOne,
    PopNPlusN,
    PopNAndNonTopographical,
    NonTopoComplex,
    NonTopoPenultimatePlusOne,
    NonTopoComplexPack4Bits,
    FieldPathEncodeFinish,
}

/// (op, weight) in the engine's table order. Weights build the Huffman tree.
const OPS: [(Op, u32); 40] = [
    (Op::PlusOne, 36271),
    (Op::PlusTwo, 10334),
    (Op::PlusThree, 1375),
    (Op::PlusFour, 646),
    (Op::PlusN, 4128),
    (Op::PushOneLeftDeltaZeroRightZero, 35),
    (Op::PushOneLeftDeltaZeroRightNonZero, 3),
    (Op::PushOneLeftDeltaOneRightZero, 521),
    (Op::PushOneLeftDeltaOneRightNonZero, 2942),
    (Op::PushOneLeftDeltaNRightZero, 560),
    (Op::PushOneLeftDeltaNRightNonZero, 471),
    (Op::PushOneLeftDeltaNRightNonZeroPack6Bits, 10530),
    (Op::PushOneLeftDeltaNRightNonZeroPack8Bits, 251),
    (Op::PushTwoLeftDeltaZero, 0),
    (Op::PushTwoPack5LeftDeltaZero, 0),
    (Op::PushThreeLeftDeltaZero, 0),
    (Op::PushThreePack5LeftDeltaZero, 0),
    (Op::PushTwoLeftDeltaOne, 0),
    (Op::PushTwoPack5LeftDeltaOne, 0),
    (Op::PushThreeLeftDeltaOne, 0),
    (Op::PushThreePack5LeftDeltaOne, 0),
    (Op::PushTwoLeftDeltaN, 0),
    (Op::PushTwoPack5LeftDeltaN, 0),
    (Op::PushThreeLeftDeltaN, 0),
    (Op::PushThreePack5LeftDeltaN, 0),
    (Op::PushN, 0),
    (Op::PushNAndNonTopological, 310),
    (Op::PopOnePlusOne, 2),
    (Op::PopOnePlusN, 0),
    (Op::PopAllButOnePlusOne, 1837),
    (Op::PopAllButOnePlusN, 149),
    (Op::PopAllButOnePlusNPack3Bits, 300),
    (Op::PopAllButOnePlusNPack6Bits, 634),
    (Op::PopNPlusOne, 0),
    (Op::PopNPlusN, 0),
    (Op::PopNAndNonTopographical, 1),
    (Op::NonTopoComplex, 76),
    (Op::NonTopoPenultimatePlusOne, 271),
    (Op::NonTopoComplexPack4Bits, 99),
    (Op::FieldPathEncodeFinish, 25474),
];

/// Max Huffman code length is 17 bits for this table; a 2^LOOKUP_BITS table
/// resolves most codes in one step and falls back to walking the tree.
const LOOKUP_BITS: u32 = 12;

struct Huffman {
    /// Tree nodes: (left, right). Leaf indices are encoded as `!op_index` (negative).
    nodes: Vec<(i32, i32)>,
    root: i32,
    /// For each LOOKUP_BITS-bit prefix: (op index or -1, code length or node to continue from).
    lookup: Vec<(i16, u8, i32)>,
}

fn huffman() -> &'static Huffman {
    static H: OnceLock<Huffman> = OnceLock::new();
    H.get_or_init(build_huffman)
}

fn build_huffman() -> Huffman {
    // Heap items: (weight, value, node ref). Pop order: lowest weight first;
    // ties broken by *higher* value first (matches the engine / reference parsers).
    #[derive(PartialEq, Eq)]
    struct Item {
        weight: u32,
        value: u32,
        node: i32,
    }
    impl Ord for Item {
        fn cmp(&self, o: &Self) -> Ordering {
            // BinaryHeap is a max-heap: "greater" pops first.
            o.weight.cmp(&self.weight).then(self.value.cmp(&o.value))
        }
    }
    impl PartialOrd for Item {
        fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
            Some(self.cmp(o))
        }
    }

    let mut heap = BinaryHeap::new();
    for (i, &(_, w)) in OPS.iter().enumerate() {
        heap.push(Item {
            weight: w.max(1),
            value: i as u32,
            node: !(i as i32),
        });
    }
    let mut nodes = Vec::new();
    let mut next_value = OPS.len() as u32;
    while heap.len() > 1 {
        let a = heap.pop().unwrap();
        let b = heap.pop().unwrap();
        nodes.push((a.node, b.node));
        heap.push(Item {
            weight: a.weight + b.weight,
            value: next_value,
            node: (nodes.len() - 1) as i32,
        });
        next_value += 1;
    }
    let root = heap.pop().unwrap().node;

    // Build the prefix lookup table.
    let size = 1usize << LOOKUP_BITS;
    let mut lookup = vec![(-1i16, 0u8, root); size];
    for (prefix, slot) in lookup.iter_mut().enumerate() {
        let mut node = root;
        for depth in 0..LOOKUP_BITS {
            let bit = (prefix >> depth) & 1;
            let (l, r) = nodes[node as usize];
            let next = if bit == 1 { r } else { l };
            if next < 0 {
                *slot = ((!next) as i16, (depth + 1) as u8, 0);
                break;
            }
            node = next;
            *slot = (-1, LOOKUP_BITS as u8, node);
        }
    }
    Huffman { nodes, root, lookup }
}

#[inline]
fn read_op(r: &mut BitReader, h: &Huffman) -> Op {
    let prefix_bits = (LOOKUP_BITS as usize).min(r.remaining_bits().max(1)) as u32;
    let prefix = (r.peek() & ((1u64 << prefix_bits) - 1)) as usize;
    let (op, len, node) = h.lookup[prefix];
    if op >= 0 && (len as u32) <= prefix_bits {
        r.skip_bits(len as usize);
        return OPS[op as usize].0;
    }
    // Long code (or near the end of the buffer): walk the tree bit by bit.
    let mut node = if prefix_bits == LOOKUP_BITS {
        r.skip_bits(LOOKUP_BITS as usize);
        node
    } else {
        h.root
    };
    loop {
        let (l, rr) = h.nodes[node as usize];
        let next = if r.read_bool() { rr } else { l };
        if next < 0 {
            return OPS[(!next) as usize].0;
        }
        node = next;
        if r.overflowed() {
            return Op::FieldPathEncodeFinish;
        }
    }
}

/// Reads the list of changed field paths for one entity update into `out`.
pub fn read_field_paths(r: &mut BitReader, out: &mut Vec<FieldPath>) -> Result<()> {
    out.clear();
    let h = huffman();
    let mut fp = FieldPath::new();
    loop {
        if r.overflowed() {
            return Err(Error::UnexpectedEof("field path"));
        }
        let op = read_op(r, h);
        let ok = match op {
            Op::FieldPathEncodeFinish => return Ok(()),
            Op::PlusOne => {
                fp.path[fp.last] += 1;
                true
            }
            Op::PlusTwo => {
                fp.path[fp.last] += 2;
                true
            }
            Op::PlusThree => {
                fp.path[fp.last] += 3;
                true
            }
            Op::PlusFour => {
                fp.path[fp.last] += 4;
                true
            }
            Op::PlusN => {
                fp.path[fp.last] += r.read_ubitvar_fp() as i32 + 5;
                true
            }
            Op::PushOneLeftDeltaZeroRightZero => fp.push(0),
            Op::PushOneLeftDeltaZeroRightNonZero => fp.push(r.read_ubitvar_fp() as i32),
            Op::PushOneLeftDeltaOneRightZero => {
                fp.path[fp.last] += 1;
                fp.push(0)
            }
            Op::PushOneLeftDeltaOneRightNonZero => {
                fp.path[fp.last] += 1;
                fp.push(r.read_ubitvar_fp() as i32)
            }
            Op::PushOneLeftDeltaNRightZero => {
                fp.path[fp.last] += r.read_ubitvar_fp() as i32;
                fp.push(0)
            }
            Op::PushOneLeftDeltaNRightNonZero => {
                fp.path[fp.last] += r.read_ubitvar_fp() as i32 + 2;
                fp.push(r.read_ubitvar_fp() as i32 + 1)
            }
            Op::PushOneLeftDeltaNRightNonZeroPack6Bits => {
                fp.path[fp.last] += r.read_bits(3) as i32 + 2;
                fp.push(r.read_bits(3) as i32 + 1)
            }
            Op::PushOneLeftDeltaNRightNonZeroPack8Bits => {
                fp.path[fp.last] += r.read_bits(4) as i32 + 2;
                fp.push(r.read_bits(4) as i32 + 1)
            }
            Op::PushTwoLeftDeltaZero => push_n_fp(r, &mut fp, 2),
            Op::PushTwoPack5LeftDeltaZero => push_n_bits5(r, &mut fp, 2),
            Op::PushThreeLeftDeltaZero => push_n_fp(r, &mut fp, 3),
            Op::PushThreePack5LeftDeltaZero => push_n_bits5(r, &mut fp, 3),
            Op::PushTwoLeftDeltaOne => {
                fp.path[fp.last] += 1;
                push_n_fp(r, &mut fp, 2)
            }
            Op::PushTwoPack5LeftDeltaOne => {
                fp.path[fp.last] += 1;
                push_n_bits5(r, &mut fp, 2)
            }
            Op::PushThreeLeftDeltaOne => {
                fp.path[fp.last] += 1;
                push_n_fp(r, &mut fp, 3)
            }
            Op::PushThreePack5LeftDeltaOne => {
                fp.path[fp.last] += 1;
                push_n_bits5(r, &mut fp, 3)
            }
            Op::PushTwoLeftDeltaN => {
                fp.path[fp.last] += r.read_ubitvar() as i32 + 2;
                push_n_fp(r, &mut fp, 2)
            }
            Op::PushTwoPack5LeftDeltaN => {
                fp.path[fp.last] += r.read_ubitvar() as i32 + 2;
                push_n_bits5(r, &mut fp, 2)
            }
            Op::PushThreeLeftDeltaN => {
                fp.path[fp.last] += r.read_ubitvar() as i32 + 2;
                push_n_fp(r, &mut fp, 3)
            }
            Op::PushThreePack5LeftDeltaN => {
                fp.path[fp.last] += r.read_ubitvar() as i32 + 2;
                push_n_bits5(r, &mut fp, 3)
            }
            Op::PushN => {
                let n = r.read_ubitvar() as usize;
                fp.path[fp.last] += r.read_ubitvar() as i32;
                push_n_fp(r, &mut fp, n)
            }
            Op::PushNAndNonTopological => {
                for i in 0..=fp.last {
                    if r.read_bool() {
                        fp.path[i] += r.read_vari32() + 1;
                    }
                }
                let n = r.read_ubitvar() as usize;
                let mut ok = true;
                for _ in 0..n {
                    ok &= fp.push(r.read_ubitvar_fp() as i32);
                }
                ok
            }
            Op::PopOnePlusOne => {
                let ok = fp.pop(1);
                fp.path[fp.last] += 1;
                ok
            }
            Op::PopOnePlusN => {
                let ok = fp.pop(1);
                fp.path[fp.last] += r.read_ubitvar_fp() as i32 + 1;
                ok
            }
            Op::PopAllButOnePlusOne => {
                fp.pop(fp.last);
                fp.path[0] += 1;
                true
            }
            Op::PopAllButOnePlusN => {
                fp.pop(fp.last);
                fp.path[0] += r.read_ubitvar_fp() as i32 + 1;
                true
            }
            Op::PopAllButOnePlusNPack3Bits => {
                fp.pop(fp.last);
                fp.path[0] += r.read_bits(3) as i32 + 1;
                true
            }
            Op::PopAllButOnePlusNPack6Bits => {
                fp.pop(fp.last);
                fp.path[0] += r.read_bits(6) as i32 + 1;
                true
            }
            Op::PopNPlusOne => {
                let ok = fp.pop(r.read_ubitvar_fp() as usize);
                fp.path[fp.last] += 1;
                ok
            }
            Op::PopNPlusN => {
                let ok = fp.pop(r.read_ubitvar_fp() as usize);
                fp.path[fp.last] += r.read_vari32();
                ok
            }
            Op::PopNAndNonTopographical => {
                let ok = fp.pop(r.read_ubitvar_fp() as usize);
                for i in 0..=fp.last {
                    if r.read_bool() {
                        fp.path[i] += r.read_vari32();
                    }
                }
                ok
            }
            Op::NonTopoComplex => {
                for i in 0..=fp.last {
                    if r.read_bool() {
                        fp.path[i] += r.read_vari32();
                    }
                }
                true
            }
            Op::NonTopoPenultimatePlusOne => {
                if fp.last == 0 {
                    false
                } else {
                    fp.path[fp.last - 1] += 1;
                    true
                }
            }
            Op::NonTopoComplexPack4Bits => {
                for i in 0..=fp.last {
                    if r.read_bool() {
                        fp.path[i] += r.read_bits(4) as i32 - 7;
                    }
                }
                true
            }
        };
        if !ok {
            return Err(Error::malformed(format!(
                "invalid field path operation {op:?} at {fp:?}"
            )));
        }
        out.push(fp);
    }
}

#[inline]
fn push_n_fp(r: &mut BitReader, fp: &mut FieldPath, n: usize) -> bool {
    let mut ok = true;
    for _ in 0..n {
        ok &= fp.push(r.read_ubitvar_fp() as i32);
    }
    ok
}

#[inline]
fn push_n_bits5(r: &mut BitReader, fp: &mut FieldPath, n: usize) -> bool {
    let mut ok = true;
    for _ in 0..n {
        ok &= fp.push(r.read_bits(5) as i32);
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns the code (bits in read order) for an op by walking the tree.
    fn code_of(target: usize) -> Vec<u8> {
        fn rec(h: &Huffman, node: i32, target: usize, acc: &mut Vec<u8>) -> bool {
            if node < 0 {
                return (!node) as usize == target;
            }
            let (l, r) = h.nodes[node as usize];
            acc.push(0);
            if rec(h, l, target, acc) {
                return true;
            }
            acc.pop();
            acc.push(1);
            if rec(h, r, target, acc) {
                return true;
            }
            acc.pop();
            false
        }
        let h = huffman();
        let mut acc = Vec::new();
        assert!(rec(h, h.root, target, &mut acc));
        acc
    }

    fn pack(bits: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; bits.len().div_ceil(8) + 8];
        for (i, &b) in bits.iter().enumerate() {
            out[i / 8] |= b << (i % 8);
        }
        out
    }

    #[test]
    fn well_known_codes() {
        // The two most frequent ops get the shortest codes.
        assert_eq!(code_of(Op::PlusOne as usize), vec![0]);
        assert_eq!(code_of(Op::FieldPathEncodeFinish as usize), vec![1, 0]);
        // Every op is reachable and the code is prefix-free (tree property).
        let max = (0..40).map(|i| code_of(i).len()).max().unwrap();
        assert!(max <= 17, "max code length {max}");
    }

    #[test]
    fn lookup_matches_tree_walk() {
        for i in 0..40 {
            let code = code_of(i);
            let data = pack(&code);
            let mut r = BitReader::new(&data);
            assert_eq!(read_op(&mut r, huffman()) as usize, i);
            assert_eq!(r.position(), code.len());
        }
    }

    #[test]
    fn decode_simple_sequence() {
        // PlusOne, PlusOne, PushOneLeftDeltaZeroRightZero, Finish  =>  [0], [1], [1, 0]
        let mut bits = Vec::new();
        bits.extend(code_of(Op::PlusOne as usize));
        bits.extend(code_of(Op::PlusOne as usize));
        bits.extend(code_of(Op::PushOneLeftDeltaZeroRightZero as usize));
        bits.extend(code_of(Op::FieldPathEncodeFinish as usize));
        let data = pack(&bits);
        let mut r = BitReader::new(&data);
        let mut out = Vec::new();
        read_field_paths(&mut r, &mut out).unwrap();
        let got: Vec<Vec<i32>> = out.iter().map(|p| p.as_slice().to_vec()).collect();
        assert_eq!(got, vec![vec![0], vec![1], vec![1, 0]]);
    }

    #[test]
    fn from_slice_roundtrip() {
        let fp = FieldPath::from_slice(&[5, 0, 1023, 2]).unwrap();
        assert_eq!(fp.as_slice(), &[5, 0, 1023, 2]);
        assert!(FieldPath::from_slice(&[]).is_none());
        assert!(FieldPath::from_slice(&[0; MAX_DEPTH + 1]).is_none());
    }
}
