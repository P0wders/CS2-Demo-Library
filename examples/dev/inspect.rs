//! Parses a demo end-to-end and prints a structural summary.
//!
//! cargo run --release --example inspect -- path/to/demo.dem [--events-only]

use cs2_demo::entities::FieldPath;
use cs2_demo::{Context, Demo, Entity, EntityOp, GameEvent, ParseOptions, Visitor};
use std::collections::BTreeMap;
use std::time::Instant;

#[derive(Default)]
struct Inspect {
    events: BTreeMap<String, u64>,
    entity_ops: [u64; 4],
    net_messages: BTreeMap<u32, u64>,
    ticks: u64,
}

impl Visitor for Inspect {
    fn on_game_event(&mut self, _ctx: &Context, ev: &GameEvent) {
        *self.events.entry(ev.name.to_string()).or_default() += 1;
    }

    fn on_entity(&mut self, _ctx: &Context, _e: &Entity, op: EntityOp, _changed: &[FieldPath]) {
        self.entity_ops[op as usize] += 1;
    }

    fn on_net_message(&mut self, _ctx: &Context, msg_type: u32, _data: &[u8]) {
        *self.net_messages.entry(msg_type).or_default() += 1;
    }

    fn on_tick_end(&mut self, _ctx: &Context) -> std::ops::ControlFlow<()> {
        self.ticks += 1;
        std::ops::ControlFlow::Continue(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: inspect <demo.dem> [--events-only]")?;
    let events_only = args.any(|a| a == "--events-only");

    let demo = Demo::open(&path)?;
    let h = demo.header();
    println!("map        : {}", h.map_name);
    println!("server     : {}", h.server_name);
    println!("build      : {} (patch {})", h.build_num, h.patch_version);
    println!(
        "length     : {:?} s, {:?} ticks ({} tick)",
        h.playback_time,
        h.playback_ticks,
        h.tick_rate()
    );

    let mut opts = if events_only {
        ParseOptions::events_only()
    } else {
        ParseOptions::default()
    };
    opts.raw_net_messages = true;
    let mut v = Inspect::default();
    let start = Instant::now();
    let ctx = demo.parse(opts, &mut v)?;
    let secs = start.elapsed().as_secs_f64();
    let mb = demo.bytes().len() as f64 / 1e6;
    println!(
        "parsed     : {mb:.0} MB in {secs:.2} s ({:.0} MB/s), {} ticks",
        mb / secs,
        v.ticks
    );

    if let Some(st) = ctx.send_tables() {
        println!("serializers: {} (coord params {:?})", st.len(), st.coord_params);
    }
    println!("classes    : {}", ctx.classes().count());
    println!(
        "entity ops : created {} updated {} left {} deleted {}",
        v.entity_ops[0], v.entity_ops[1], v.entity_ops[2], v.entity_ops[3]
    );

    let diag = ctx.entity_diagnostics();
    println!(
        "entity diagnostics: {} of {} updates size-verified, {} size mismatches, {} baseline errors",
        diag.verified_updates,
        v.entity_ops[0] + v.entity_ops[1],
        diag.size_mismatches,
        diag.baseline_errors
    );
    for s in diag.samples.iter().take(10) {
        println!("  {s}");
    }

    let mut by_class: BTreeMap<&str, usize> = BTreeMap::new();
    for e in ctx.entities().iter() {
        *by_class.entry(e.class_name()).or_default() += 1;
    }
    println!("live entities at end: {}", ctx.entities().len());
    for name in [
        "CCSPlayerController",
        "CCSPlayerPawn",
        "CCSTeam",
        "CCSGameRulesProxy",
    ] {
        println!("  {name:24} {}", by_class.get(name).copied().unwrap_or(0));
    }

    println!("string tables:");
    for t in ctx.string_tables().iter() {
        println!("  {:24} {} entries", t.name, t.iter().count());
    }

    println!("game events ({} kinds):", v.events.len());
    let mut evs: Vec<_> = v.events.iter().collect();
    evs.sort_by(|a, b| b.1.cmp(a.1));
    for (name, n) in evs.iter() {
        println!("  {name:32} {n}");
    }
    println!("other net messages: {:?}", v.net_messages);
    Ok(())
}
