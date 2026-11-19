# Writing a Visitor

For your own analysis, implement `Visitor`. The parser calls your methods while it reads the demo. Every method has an empty default, so implement only the ones you need.

| Method | Called |
|---|---|
| `on_game_event` | for every game event: kills, damage, bomb, grenades, purchases, … |
| `on_tick_end` | after every tick; return `ControlFlow::Break(())` to stop early |
| `on_entity` | when an entity is created, updated, leaves visibility or is deleted |
| `on_string_table` | when string table entries change (player info, …) |
| `on_net_message` | for net messages the parser doesn't use itself (opt-in, see [Choosing what to decode](Parse-options.md)) |
| `on_start`, `on_finish` | once, at each end |

## A first visitor

Each callback gets a `Context`: the complete game state at that moment. This visitor records where the killer and the victim were standing for every kill:

```rust title="kill_positions.rs"
use cs2_demo::{Context, Demo, GameEvent, ParseOptions, Visitor};

#[derive(Default)]
struct KillPositions(Vec<(String, [f32; 3], String, [f32; 3])>);

impl Visitor for KillPositions {
    fn on_game_event(&mut self, ctx: &Context, ev: &GameEvent) {
        if &*ev.name != "player_death" {
            return;
        }
        // Player fields of events are slots; turn them into players.
        let victim = ev.player_slot("userid").and_then(|s| ctx.player_by_slot(s));
        let killer = ev.player_slot("attacker").and_then(|s| ctx.player_by_slot(s));
        if let (Some(v), Some(k)) = (victim, killer)
            && let (Some(vp), Some(kp)) = (v.position(), k.position())
        {
            self.0.push((k.name(), kp, v.name(), vp));
        }
    }
}

fn main() -> Result<(), cs2_demo::Error> {
    let mut kills = KillPositions::default();
    Demo::open("match.dem")?.parse(ParseOptions::default(), &mut kills)?;
    println!("{} kills", kills.0.len());
    Ok(())
}
```

`parse` also returns the `Context` as it was at the end of the demo.

## Your visitor and the statistics in one parse

To run your visitor and the built-in statistics in one parse, pass a tuple: every callback goes to both.

```rust
let mut both = (cs2_demo::StatsCollector::new(), KillPositions::default());
let ctx = demo.parse(ParseOptions::default(), &mut both)?;
let stats = both.0.into_stats(&ctx);
```

> [!NOTE]
> If you only need events (a kill feed, a counter), [`for_each_event`](Game-events.md) is simpler and skips entity decoding entirely.

Runnable versions: the [`events`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/events.rs) and [`combine`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/combine.rs) examples.
