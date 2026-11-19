//! Prints the networked schema (every class's properties and how they are encoded).
//!
//! cargo run --release --example dump_schema -- demo.dem [ClassName]
//!
//! Without a class name, prints a summary of every distinct
//! (type, encoder, bit count, flags) combination and the decoder chosen for it.

use cs2_demo::Demo;
use cs2_demo::sendtables::{FieldKind, SendTables, VectorElement};
use std::collections::BTreeMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: dump_schema <demo.dem> [ClassName]")?;
    let class = args.next();
    let demo = Demo::open(&path)?;

    let mut buf = Vec::new();
    let st = demo
        .frames()
        .filter_map(Result::ok)
        .find(|f| f.cmd == 4)
        .ok_or("no DEM_SendTables frame")
        .and_then(|f| {
            f.data(&mut buf)
                .map_err(|_| "bad frame")
                .and_then(|d| SendTables::from_demo_send_tables(d).map_err(|_| "bad send tables"))
        })?;

    if let Some(class) = class {
        let ser = st.get(&class).ok_or("unknown class")?;
        ser.walk_names(|name, f| {
            let kind = match &f.kind {
                FieldKind::Value(d) => format!("{d:?}"),
                FieldKind::FixedArray { len, decoder } => format!("[{len}] {decoder:?}"),
                FieldKind::Vector {
                    element: VectorElement::Value(d),
                } => format!("vec<{d:?}>"),
                FieldKind::Vector {
                    element: VectorElement::Serializer(s),
                } => format!("vec<{}>", s.name),
                FieldKind::Pointer {
                    serializer,
                    polymorphic,
                } => {
                    let poly: Vec<String> = polymorphic
                        .iter()
                        .map(|s| format!("{}({} fields)", s.name, s.fields.len()))
                        .collect();
                    format!("ptr<{}> poly{:?}", serializer.name, poly)
                }
            };
            println!(
                "{name:70} {:40} enc={:?} bits={} -> {kind}",
                f.var_type,
                f.encoder.as_deref(),
                f.bit_count
            );
        });
        return Ok(());
    }

    let mut combos: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for ser in st.iter() {
        for f in &ser.fields {
            let decoder = match &f.kind {
                FieldKind::Value(d) | FieldKind::FixedArray { decoder: d, .. } => format!("{d:?}"),
                FieldKind::Vector {
                    element: VectorElement::Value(d),
                } => format!("vec<{d:?}>"),
                _ => continue,
            };
            let key = format!(
                "{:45} enc={:<22} bits={:<3} flags={} range=[{}, {}]",
                f.var_type,
                format!("{:?}", f.encoder.as_deref()),
                f.bit_count,
                f.encode_flags,
                f.low_value,
                f.high_value
            );
            let e = combos.entry(key).or_insert((0, decoder));
            e.0 += 1;
        }
    }
    for (k, (n, d)) in combos {
        let d: String = d.chars().take(60).collect();
        println!("{n:5}  {k}  ->  {d}");
    }
    Ok(())
}
