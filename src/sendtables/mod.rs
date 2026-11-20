//! Flattened serializers (`CSVCMsg_FlattenedSerializer`, sent in DEM_SendTables):
//! the schema describing every networked entity class and how each of its
//! fields is encoded.

pub mod decoder;
pub mod field_type;

use crate::bitreader::CoordParams;
use crate::error::{Error, Result, decode_proto};
use crate::proto::{CDemoSendTables, CsvcMsgFlattenedSerializer};
use decoder::{Decoder, DecoderInput, find_decoder};
use field_type::FieldType;
use rustc_hash::FxHashMap;
use std::fmt::Write as _;
use std::sync::Arc;

/// Field types that are serialized as an optional embedded object
/// (presence bit + child serializer) even without a `*` in the type name.
const POINTER_TYPES: &[&str] = &[
    "CBodyComponent",
    "CBodyComponentBaseAnimGraph",
    "CBodyComponentBaseModelEntity",
    "CBodyComponentDCGBaseAnimating",
    "CBodyComponentPoint",
    "CBodyComponentSkeletonInstance",
    "CLightComponent",
    "CPhysicsComponent",
    "CRenderComponent",
    "C_BodyComponentBaseAnimGraph",
    "C_BodyComponentBaseModelEntity",
    "C_BodyComponent",
    "C_BodyComponentPoint",
    "C_BodyComponentSkeletonInstance",
    "C_LightComponent",
    "C_PhysicsComponent",
    "C_RenderComponent",
    "PhysicsRagdollPose_t",
    "CEntityIdentity",
    "CPlayerLocalData",
    "CPlayer_CameraServices",
];

#[derive(Debug, Clone)]
pub enum VectorElement {
    Value(Decoder),
    Serializer(Arc<Serializer>),
}

#[derive(Debug, Clone)]
pub enum FieldKind {
    /// A single value.
    Value(Decoder),
    /// `T[N]`: every element uses the same decoder.
    FixedArray { len: usize, decoder: Decoder },
    /// Length-prefixed dynamic array of values or of nested objects.
    Vector { element: VectorElement },
    /// Optional embedded object: a presence bit at the field itself, children below it.
    Pointer {
        serializer: Arc<Serializer>,
        polymorphic: Vec<Arc<Serializer>>,
    },
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: Arc<str>,
    pub var_type: Arc<str>,
    pub send_node: Arc<str>,
    pub encoder: Option<Arc<str>>,
    pub bit_count: i32,
    pub low_value: f32,
    pub high_value: f32,
    pub encode_flags: i32,
    pub kind: FieldKind,
}

#[derive(Debug, Clone)]
pub struct Serializer {
    pub name: Arc<str>,
    pub version: i32,
    pub fields: Vec<Arc<Field>>,
    by_name: FxHashMap<Arc<str>, usize>,
}

impl Serializer {
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.by_name.get(name).copied()
    }

    /// Whether a polymorphic pointer is reachable from this serializer: then
    /// decoding a field may depend on a subtype set by an earlier update.
    pub fn has_polymorphic_fields(&self) -> bool {
        fn rec(ser: &Serializer, depth: usize) -> bool {
            depth <= 16
                && ser.fields.iter().any(|f| match &f.kind {
                    FieldKind::Pointer {
                        serializer,
                        polymorphic,
                    } => !polymorphic.is_empty() || rec(serializer, depth + 1),
                    FieldKind::Vector {
                        element: VectorElement::Serializer(s),
                    } => rec(s, depth + 1),
                    _ => false,
                })
        }
        rec(self, 0)
    }

    /// Returns the decoder used for the value at `path`. Paths below a
    /// polymorphic pointer resolve against its base type; use
    /// [`Serializer::decoder_for_with`] to supply the concrete subtype.
    pub fn decoder_for(&self, path: &[i32]) -> Option<Decoder> {
        self.decoder_for_with(path, &|_| None)
    }

    /// Like [`Serializer::decoder_for`], with `subtype(pointer_path)` returning
    /// the current subtype index of a polymorphic pointer (its decoded value).
    pub fn decoder_for_with(&self, path: &[i32], subtype: &dyn Fn(&[i32]) -> Option<i32>) -> Option<Decoder> {
        self.decoder_ref(path, subtype).copied()
    }

    /// [`Serializer::decoder_for_with`] without copying the decoder.
    #[inline]
    pub(crate) fn decoder_ref(
        &self,
        path: &[i32],
        subtype: &dyn Fn(&[i32]) -> Option<i32>,
    ) -> Option<&Decoder> {
        static LENGTH: Decoder = Decoder::VarU32;
        static PRESENCE: Decoder = Decoder::Bool;
        static SUBTYPE: Decoder = Decoder::PolymorphicPointer;
        let mut ser = self;
        let mut i = 0;
        loop {
            let field = ser.fields.get(*path.get(i)? as usize)?;
            i += 1;
            let remaining = path.len() - i;
            match &field.kind {
                FieldKind::Value(d) => return Some(d),
                FieldKind::FixedArray { decoder, .. } => return Some(decoder),
                FieldKind::Vector {
                    element: VectorElement::Value(d),
                } => return Some(if remaining == 0 { &LENGTH } else { d }),
                FieldKind::Vector {
                    element: VectorElement::Serializer(s),
                } => {
                    if remaining >= 2 {
                        ser = s;
                        i += 1; // skip the element index
                    } else {
                        return Some(&LENGTH);
                    }
                }
                FieldKind::Pointer {
                    serializer,
                    polymorphic,
                } => {
                    if polymorphic.is_empty() {
                        if remaining == 0 {
                            return Some(&PRESENCE);
                        }
                        ser = serializer;
                    } else {
                        if remaining == 0 {
                            return Some(&SUBTYPE);
                        }
                        ser = subtype(&path[..i])
                            .and_then(|t| polymorphic.get(usize::try_from(t).ok()?))
                            .unwrap_or(serializer);
                    }
                }
            }
        }
    }

    /// Human-readable name of the value at `path`, e.g.
    /// `m_pWeaponServices.m_hMyWeapons.2` or `CBodyComponent.m_cellX`.
    pub fn name_for(&self, path: &[i32]) -> Option<String> {
        let mut out = String::new();
        let mut ser = self;
        let mut i = 0;
        loop {
            let field = ser.fields.get(*path.get(i)? as usize)?;
            if !out.is_empty() {
                out.push('.');
            }
            out.push_str(&field.name);
            i += 1;
            if i >= path.len() {
                return Some(out);
            }
            match &field.kind {
                FieldKind::Value(_) => return Some(out),
                FieldKind::FixedArray { .. }
                | FieldKind::Vector {
                    element: VectorElement::Value(_),
                } => {
                    let _ = write!(out, ".{}", path[i]);
                    return Some(out);
                }
                FieldKind::Vector {
                    element: VectorElement::Serializer(s),
                } => {
                    let _ = write!(out, ".{}", path[i]);
                    i += 1;
                    if i >= path.len() {
                        return Some(out);
                    }
                    ser = s;
                }
                FieldKind::Pointer {
                    serializer,
                    polymorphic,
                } => {
                    // Without entity state, pick the first type that has this field.
                    let next = path[i] as usize;
                    ser = std::iter::once(serializer)
                        .chain(polymorphic)
                        .find(|s| next < s.fields.len())
                        .unwrap_or(serializer);
                }
            }
        }
    }

    /// Resolves a dotted property name (as produced by [`Serializer::name_for`])
    /// back to its field path components.
    pub fn path_for(&self, name: &str) -> Option<Vec<i32>> {
        let mut out = Vec::with_capacity(4);
        let mut ser = self;
        let mut parts = name.split('.');
        while let Some(part) = parts.next() {
            let idx = ser.field_index(part)?;
            out.push(idx as i32);
            let field = &ser.fields[idx];
            match &field.kind {
                FieldKind::Value(_) => return parts.next().is_none().then_some(out),
                FieldKind::FixedArray { .. }
                | FieldKind::Vector {
                    element: VectorElement::Value(_),
                } => {
                    if let Some(n) = parts.next() {
                        out.push(n.parse().ok()?);
                    }
                    return parts.next().is_none().then_some(out);
                }
                FieldKind::Vector {
                    element: VectorElement::Serializer(s),
                } => {
                    match parts.next() {
                        Some(n) => out.push(n.parse().ok()?),
                        None => return Some(out),
                    }
                    ser = s;
                }
                FieldKind::Pointer {
                    serializer,
                    polymorphic,
                } => {
                    // Choose the (sub)type that declares the next name component.
                    ser = match parts.clone().next() {
                        Some(next) => std::iter::once(serializer)
                            .chain(polymorphic)
                            .find(|s| s.field_index(next).is_some())
                            .unwrap_or(serializer),
                        None => serializer,
                    };
                }
            }
        }
        Some(out)
    }

    /// Visits every leaf property name reachable from this serializer, without
    /// expanding array/vector elements (use `.N` suffixes for those).
    pub fn walk_names(&self, mut f: impl FnMut(&str, &Field)) {
        fn rec(ser: &Serializer, prefix: &mut String, f: &mut dyn FnMut(&str, &Field), depth: usize) {
            if depth > 16 {
                return;
            }
            for field in &ser.fields {
                let len = prefix.len();
                if !prefix.is_empty() {
                    prefix.push('.');
                }
                prefix.push_str(&field.name);
                match &field.kind {
                    FieldKind::Pointer { serializer, .. } => {
                        f(prefix, field);
                        rec(serializer, prefix, f, depth + 1)
                    }
                    FieldKind::Vector {
                        element: VectorElement::Serializer(s),
                    } => {
                        f(prefix, field);
                        prefix.push_str(".N");
                        rec(s, prefix, f, depth + 1);
                    }
                    _ => f(prefix, field),
                }
                prefix.truncate(len);
            }
        }
        let mut prefix = String::new();
        rec(self, &mut prefix, &mut f, 0);
    }
}

/// All serializers from one `CSVCMsg_FlattenedSerializer`.
#[derive(Debug, Clone, Default)]
pub struct SendTables {
    by_name: FxHashMap<Arc<str>, Arc<Serializer>>,
    pub coord_params: CoordParams,
}

impl SendTables {
    pub fn get(&self, name: &str) -> Option<&Arc<Serializer>> {
        self.by_name.get(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<Serializer>> {
        self.by_name.values()
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Parses a DEM_SendTables payload (`CDemoSendTables`).
    pub fn from_demo_send_tables(payload: &[u8]) -> Result<SendTables> {
        let st: CDemoSendTables = decode_proto("CDemoSendTables", payload)?;
        let data = st.data.unwrap_or_default();
        // data = varint length + CSVCMsg_FlattenedSerializer
        let mut pos = 0usize;
        let mut len = 0usize;
        let mut shift = 0;
        loop {
            let b = *data.get(pos).ok_or(Error::UnexpectedEof("send tables length"))?;
            pos += 1;
            len |= ((b & 0x7f) as usize) << shift;
            if b & 0x80 == 0 {
                break;
            }
            shift += 7;
        }
        let body = data
            .get(pos..pos + len)
            .ok_or(Error::UnexpectedEof("send tables body"))?;
        let msg: CsvcMsgFlattenedSerializer = decode_proto("CSVCMsg_FlattenedSerializer", body)?;
        SendTables::from_flattened(&msg)
    }

    pub fn from_flattened(msg: &CsvcMsgFlattenedSerializer) -> Result<SendTables> {
        let symbols: Vec<Arc<str>> = msg.symbols.iter().map(|s| Arc::from(s.as_str())).collect();
        let sym = |i: Option<i32>| -> Option<Arc<str>> { symbols.get(i? as usize).cloned() };

        let mut coord_params = CoordParams::default();
        if let Some(cp) = &msg.coord_size_params {
            if let Some(v) = cp.coord_integer_bits.filter(|&v| v > 0) {
                coord_params.coord_integer_bits = v as u32;
            }
            if let Some(v) = cp.coord_fractional_bits.filter(|&v| v > 0) {
                coord_params.coord_fractional_bits = v as u32;
            }
            if let Some(v) = cp.normal_fractional_bits.filter(|&v| v > 0) {
                coord_params.normal_fractional_bits = v as u32;
            }
        }

        let mut by_key: FxHashMap<(Arc<str>, i32), Arc<Serializer>> = FxHashMap::default();
        let mut by_name: FxHashMap<Arc<str>, Arc<Serializer>> = FxHashMap::default();
        let mut field_cache: Vec<Option<Arc<Field>>> = vec![None; msg.fields.len()];

        for ps in &msg.serializers {
            let name =
                sym(ps.serializer_name_sym).ok_or_else(|| Error::malformed("serializer without name"))?;
            let version = ps.serializer_version.unwrap_or(0);
            let mut fields = Vec::with_capacity(ps.fields_index.len());
            for &fi in &ps.fields_index {
                let fi = fi as usize;
                if field_cache.get(fi).is_none() {
                    return Err(Error::malformed(format!(
                        "serializer {name} references missing field {fi}"
                    )));
                }
                if field_cache[fi].is_none() {
                    let field = build_field(&msg.fields[fi], &sym, &by_key)?;
                    field_cache[fi] = Some(Arc::new(field));
                }
                fields.push(field_cache[fi].clone().unwrap());
            }
            let by_field_name = fields
                .iter()
                .enumerate()
                .map(|(i, f)| (f.name.clone(), i))
                .collect();
            let ser = Arc::new(Serializer {
                name: name.clone(),
                version,
                fields,
                by_name: by_field_name,
            });
            by_key.insert((name.clone(), version), ser.clone());
            by_name.insert(name, ser);
        }
        Ok(SendTables {
            by_name,
            coord_params,
        })
    }
}

fn build_field(
    pf: &crate::proto::ProtoFlattenedSerializerFieldT,
    sym: &dyn Fn(Option<i32>) -> Option<Arc<str>>,
    serializers: &FxHashMap<(Arc<str>, i32), Arc<Serializer>>,
) -> Result<Field> {
    let empty: Arc<str> = Arc::from("");
    let name = sym(pf.var_name_sym).unwrap_or_else(|| empty.clone());
    let var_type = sym(pf.var_type_sym).unwrap_or_else(|| empty.clone());
    let send_node = sym(pf.send_node_sym).unwrap_or_else(|| empty.clone());
    let encoder = sym(pf.var_encoder_sym);
    let ft = FieldType::parse(&var_type);

    let lookup = |name_sym: Option<i32>, version: Option<i32>| -> Result<Option<Arc<Serializer>>> {
        let Some(n) = sym(name_sym) else { return Ok(None) };
        let key = (n.clone(), version.unwrap_or(0));
        match serializers.get(&key) {
            Some(s) => Ok(Some(s.clone())),
            None => Err(Error::malformed(format!(
                "field {name} references serializer {n} v{} before it is defined",
                key.1
            ))),
        }
    };
    let child = lookup(pf.field_serializer_name_sym, pf.field_serializer_version)?;

    let decoder_for = |base: &str| {
        find_decoder(&DecoderInput {
            var_name: &name,
            base_type: base,
            encoder: encoder.as_deref(),
            bit_count: pf.bit_count.unwrap_or(0),
            low: pf.low_value.unwrap_or(0.0),
            high: pf.high_value.unwrap_or(0.0),
            flags: pf.encode_flags.unwrap_or(0),
            signed_enum: pf
                .var_enum_info
                .as_ref()
                .and_then(|e| e.is_signed_enum)
                .unwrap_or(false),
        })
    };

    let is_pointer = ft.pointer || POINTER_TYPES.contains(&ft.base.as_str());
    let kind = if let Some(ser) = child {
        if is_pointer {
            let mut polymorphic = Vec::new();
            for p in &pf.polymorphic_types {
                if let Some(s) = lookup(
                    p.polymorphic_field_serializer_name_sym,
                    p.polymorphic_field_serializer_version,
                )? {
                    polymorphic.push(s);
                }
            }
            FieldKind::Pointer {
                serializer: ser,
                polymorphic,
            }
        } else {
            FieldKind::Vector {
                element: VectorElement::Serializer(ser),
            }
        }
    } else if ft.is_dynamic_vector() {
        let elem = ft.generic.as_ref().map(|g| g.base.as_str()).unwrap_or("uint32");
        FieldKind::Vector {
            element: VectorElement::Value(decoder_for(elem)),
        }
    } else if let Some(len) = ft.count.filter(|_| ft.base != "char") {
        FieldKind::FixedArray {
            len,
            decoder: decoder_for(&ft.base),
        }
    } else {
        FieldKind::Value(decoder_for(&ft.base))
    };

    Ok(Field {
        name,
        var_type,
        send_node,
        encoder,
        bit_count: pf.bit_count.unwrap_or(0),
        low_value: pf.low_value.unwrap_or(0.0),
        high_value: pf.high_value.unwrap_or(0.0),
        encode_flags: pf.encode_flags.unwrap_or(0),
        kind,
    })
}
