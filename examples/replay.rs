//! Positions over time, for 2D replays: `PositionRecorder` samples players,
//! grenades in flight, smokes, fires and the bomb every few ticks.
//! `MapOverview` turns world positions into pixels of the map's 1024 × 1024
//! radar image. Prints one round, one line per second.
//!
//! cargo run --release --example replay -- match.dem [round]

use cs2_demo::maps::MapOverview;
use cs2_demo::replay::PositionRecorder;
use cs2_demo::{Demo, ParseOptions, StatsCollector};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: cargo run --release --example replay -- <match.dem> [round]")?;
    let round_number: u32 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(1);
    let demo = Demo::open(&path)?;

    // The recorder is a `Visitor`: run it next to the statistics, which give
    // the round boundaries. 16 ticks = 4 frames per second at 64 tick.
    let mut both = (StatsCollector::new(), PositionRecorder::every(16));
    let ctx = demo.parse(ParseOptions::default(), &mut both)?;
    let (collector, recorder) = both;
    let stats = collector.into_stats(&ctx);

    let round = stats
        .round(round_number)
        .ok_or_else(|| format!("no round {round_number} (the demo has {})", stats.rounds.len()))?;
    let frames = recorder.frames_between(round.start_tick, round.end_tick.unwrap_or(i32::MAX));

    // Built-in radar calibration for the usual competitive maps. For others,
    // read the game's `resource/overviews/<map>.txt` with `MapOverview::from_kv`.
    let overview = stats.map_overview();
    let place = |pos: [f32; 3]| match &overview {
        Some(o) => {
            let [x, y] = o.to_radar(pos);
            format!(
                "px ({x:>4.0}, {y:>4.0}){}",
                if o.is_lower(pos[2]) { " lower" } else { "" }
            )
        }
        None => format!("({:>6.0}, {:>6.0})", pos[0], pos[1]),
    };
    println!(
        "round {} on {}: {} frames, positions {}",
        round.number,
        stats.map,
        frames.len(),
        if overview.is_some() {
            "in radar pixels"
        } else {
            "in world units"
        }
    );
    if overview.is_none() {
        println!(
            "  (radar calibration is built in for: {})",
            MapOverview::builtin_maps().collect::<Vec<_>>().join(", ")
        );
    }

    // One frame of every 4: one per second.
    for f in frames.iter().step_by(4) {
        let time = round
            .ticks_since_freeze_end(f.tick)
            .filter(|&t| t >= 0)
            .map_or("freeze".to_string(), |t| format!("{:.0}s", stats.seconds(t)));
        println!(
            "\n[{time}] {} grenades in flight, {} smokes, {} fires{}",
            f.grenades.len(),
            f.smokes.len(),
            f.fires.len(),
            f.bomb.as_ref().map_or(String::new(), |b| format!(
                ", bomb {} at {}",
                if b.planted {
                    "planted"
                } else if b.carrier.is_some() {
                    "carried"
                } else {
                    "dropped"
                },
                place(b.pos)
            )),
        );
        for p in f.players.iter().filter(|p| p.alive) {
            // Frames hold SteamIDs: names come from the statistics.
            let name = stats.player(p.steam_id).map_or("?", |s| s.name.as_str());
            let weapon = p.weapon.and_then(cs2_demo::model::weapon_name).unwrap_or("?");
            println!(
                "  {:2} {:16} {}  yaw {:>4.0}  {:>3} hp  {}",
                p.side.short_name(),
                name,
                place(p.pos),
                p.yaw,
                p.health,
                weapon
            );
        }
    }
    Ok(())
}
