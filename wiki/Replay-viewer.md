# Replay viewer

For a round-by-round viewer, collect positions and statistics together. The stats collector supplies round boundaries; the position recorder supplies frames.

```rust
use cs2_demo::{Demo, ParseOptions, Result, StatsCollector};
use cs2_demo::replay::PositionRecorder;

fn main() -> Result<()> {
    let demo = Demo::open("match.dem")?;
    let mut both = (StatsCollector::new(), PositionRecorder::every(16));
    let ctx = demo.parse(ParseOptions::default(), &mut both)?;
    let (collector, recorder) = both;
    let stats = collector.into_stats(&ctx);
    if let Some(round) = stats.round(1) {
        let frames = recorder.frames_between(round.start_tick, round.end_tick.unwrap_or(i32::MAX));
        println!("{} sampled frames for round {}", frames.len(), round.number);
    }
    Ok(())
}
```

At 64 ticks per second, an interval of 16 gives four samples per second. A smaller interval makes movement smoother and retains more frames. Frames include players, grenades in flight, active smokes and fires, and bomb state.

Use `stats.map_overview()` for built-in radar calibration and `to_radar(pos)` for pixel coordinates. Handle an unknown map by supplying an overview or displaying world coordinates. The recorder’s range lookup works on frames already collected. For seeking after a server restart, store frames during import and retrieve them by match and tick.

[Runnable radar example](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/replay.rs)
