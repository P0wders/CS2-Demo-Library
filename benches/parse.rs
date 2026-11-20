//! Parse throughput on a real demo. Set CS2_DEMO_BENCH to a .dem path, or put
//! `vitality-vs-falcons-m2-dust2.dem` in `Demo/`.
//!
//! cargo bench

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use cs2_demo::{Demo, ParseOptions};
use std::path::PathBuf;
use std::time::Duration;

fn demo_path() -> Option<PathBuf> {
    let p = std::env::var_os("CS2_DEMO_BENCH")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Demo/vitality-vs-falcons-m2-dust2.dem")
        });
    p.exists().then_some(p)
}

fn bench(c: &mut Criterion) {
    let Some(path) = demo_path() else {
        eprintln!("no demo found; set CS2_DEMO_BENCH");
        return;
    };
    let demo = Demo::open(&path).unwrap();
    let mut g = c.benchmark_group("parse");
    g.sample_size(10)
        .measurement_time(Duration::from_secs(40))
        .throughput(Throughput::Bytes(demo.bytes().len() as u64));
    g.bench_function("events_only", |b| {
        b.iter(|| demo.parse(ParseOptions::events_only(), &mut ()).unwrap())
    });
    g.bench_function("full", |b| {
        b.iter(|| demo.parse(ParseOptions::default(), &mut ()).unwrap())
    });
    g.bench_function("full_one_thread", |b| {
        let one = ParseOptions {
            threads: 1,
            ..Default::default()
        };
        b.iter(|| demo.parse(one.clone(), &mut ()).unwrap())
    });
    g.bench_function("match_stats", |b| b.iter(|| demo.match_stats().unwrap()));
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
