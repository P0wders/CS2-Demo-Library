# Match statistics

Call `match_stats()` when importing a match. It reconstructs entity state and produces the scoreboard, rounds, kills, economy, clutches, and grenade records. Keep the result while serving multiple queries.

```rust
use cs2_demo::{Demo, Result};

fn main() -> Result<()> {
    let stats = Demo::open("match.dem")?.match_stats()?;
    println!("{}: {} rounds", stats.map, stats.rounds.len());
    if let Some(round) = stats.round(12) {
        for kill in stats.round_kills(round.number) {
            println!("tick {}: {}", kill.tick, kill.victim_name);
        }
    }
    Ok(())
}
```

Round numbers come from the match. A recording that starts mid-match may begin above round 1. Use `round(number)` to look up a round by its recorded number. The kill, grenade, and clutch iterators borrow the existing records.

For an HTTP API, enable the optional `serde` feature to serialize statistics. The JSON example shows the output for a complete match:

```sh
cargo run --release --features serde --example json -- "match.dem"
```
