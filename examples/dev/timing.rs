//! Times the parse modes on one demo, on one thread and on several.
//! cargo run --release --example timing -- demo.dem [--count]
use cs2_demo::entities::FieldPath;
use cs2_demo::{Context, Demo, Entity, EntityOp, ParseOptions, StatsCollector, Visitor};
use std::time::Instant;

#[derive(Default)]
struct CountFields(u64, u64);
impl Visitor for CountFields {
    fn on_entity(&mut self, _: &Context, _: &Entity, _: EntityOp, changed: &[FieldPath]) {
        self.0 += 1;
        self.1 += changed.len() as u64;
    }
}

/// Best of five runs, in seconds.
fn best(mut f: impl FnMut() -> Result<(), cs2_demo::Error>) -> Result<f64, cs2_demo::Error> {
    let mut best = f64::MAX;
    for _ in 0..5 {
        let t = Instant::now();
        f()?;
        best = best.min(t.elapsed().as_secs_f64());
    }
    Ok(best)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: timing <demo.dem> [--count]")?;
    let demo = Demo::open(&path)?;
    let mb = demo.bytes().len() as f64 / 1e6;
    let one = ParseOptions {
        threads: 1,
        ..Default::default()
    };
    let report = |label: &str, secs: f64| println!("{label:28} {secs:6.3} s  ({:5.0} MB/s)", mb / secs);

    report(
        "events only",
        best(|| demo.parse(ParseOptions::events_only(), &mut ()).map(drop))?,
    );
    report(
        "full, 1 thread",
        best(|| demo.parse(one.clone(), &mut ()).map(drop))?,
    );
    report(
        "full",
        best(|| demo.parse(ParseOptions::default(), &mut ()).map(drop))?,
    );
    report(
        "match_stats, 1 thread",
        best(|| {
            let mut c = StatsCollector::new();
            let opts = ParseOptions {
                threads: 1,
                ..StatsCollector::parse_options()
            };
            let ctx = demo.parse(opts, &mut c)?;
            std::hint::black_box(c.into_stats(&ctx));
            Ok(())
        })?,
    );
    report("match_stats", best(|| demo.match_stats().map(drop))?);
    if std::env::args().any(|a| a == "--count") {
        let mut c = CountFields::default();
        demo.parse(ParseOptions::default(), &mut c)?;
        println!("entity changes {}, fields decoded {}", c.0, c.1);
    }
    Ok(())
}
