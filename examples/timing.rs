//! Times the parse modes on one demo.
//! cargo run --release --example timing -- demo.dem
use cs2_demo::entities::FieldPath;
use cs2_demo::{Context, Demo, Entity, EntityOp, ParseOptions, Visitor};
use std::time::Instant;

#[derive(Default)]
struct CountFields(u64, u64);
impl Visitor for CountFields {
    fn on_entity(&mut self, _: &Context, _: &Entity, _: EntityOp, changed: &[FieldPath]) {
        self.0 += 1;
        self.1 += changed.len() as u64;
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: timing <demo.dem>")?;
    let demo = Demo::open(&path)?;
    let mb = demo.bytes().len() as f64 / 1e6;
    for (label, opts) in [
        ("events only", ParseOptions::events_only()),
        ("full", ParseOptions::default()),
    ] {
        let mut best = f64::MAX;
        for _ in 0..3 {
            let t = Instant::now();
            demo.parse(opts.clone(), &mut ())?;
            best = best.min(t.elapsed().as_secs_f64());
        }
        println!("{label:12} {best:.2} s  ({:.0} MB/s)", mb / best);
    }
    let t = Instant::now();
    demo.match_stats()?;
    let s = t.elapsed().as_secs_f64();
    println!("{:12} {s:.2} s  ({:.0} MB/s)", "match_stats", mb / s);
    if std::env::args().any(|a| a == "--count") {
        let mut c = CountFields::default();
        demo.parse(ParseOptions::default(), &mut c)?;
        println!("entity changes {}, fields decoded {}", c.0, c.1);
    }
    Ok(())
}
