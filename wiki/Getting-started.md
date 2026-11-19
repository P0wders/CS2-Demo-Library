# Getting started

From the repository root, run the scoreboard example against a local CS2 demo. Substitute the path to your own file and quote paths containing spaces.

```sh
cargo run --release --example quick_start -- "path/to/match.dem"
```

To use the library from another Rust crate, add this entry under `[dependencies]` in its Cargo.toml:

```toml
cs2_demo = { git = "https://github.com/P0wders/CS2-Demo-Library.git" }
```

For a directory listing, read each header and release the mapping immediately:

```rust
use cs2_demo::{Demo, Result};

fn main() -> Result<()> {
    let header = Demo::read_header("match.dem")?;
    println!("Map: {}", header.map_name);
    if let Some(ticks) = header.playback_ticks {
        println!("Length: {ticks} ticks");
    }
    Ok(())
}
```

Playback length comes from the footer and can be absent in a truncated recording. If you plan to parse the same file immediately, use `Demo::open` once and read `demo.header()` from that handle.

## Match statistics in one call

`Demo::match_stats()` parses the whole demo and returns the teams, the scoreboard and every round:

```rust title="examples/quick_start.rs"
use cs2_demo::Demo;

fn main() -> Result<(), cs2_demo::Error> {
    let demo = Demo::open("match.dem")?;
    let h = demo.header();
    println!("{} on {} (build {})", h.map_name, h.server_name, h.build_num);

    let stats = demo.match_stats()?;
    for t in &stats.teams {
        println!("{}: {}", t.name, t.score);
    }
    for p in &stats.players {
        println!(
            "{:16} {:>2}-{:>2}-{:>2}  ADR {:5.1}  KAST {:3.0}%  rating {:.2}",
            p.name, p.kills, p.deaths, p.assists, p.adr(), p.kast() * 100.0, p.rating()
        );
    }
    Ok(())
}
```

This is the scoreboard part of that data for Nemiga vs Vitality on Cache:

<!-- scoreboard: de_cache -->
**Nemiga 13 – 8 Team Vitality** (de_cache)

| Nemiga | K | D | A | +/− | ADR | HS % | KAST | Rating |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| robo | 24 | 11 | 4 | +13 | 96.1 | 46 % | 81 % | 1.73 |
| Xant3r | 20 | 15 | 4 | +5 | 88.1 | 40 % | 67 % | 1.31 |
| khaN | 13 | 8 | 3 | +5 | 53.3 | 46 % | 81 % | 1.11 |
| SYPH0 | 13 | 10 | 6 | +3 | 81.0 | 69 % | 86 % | 1.06 |
| KaiR0N- | 17 | 15 | 7 | +2 | 105.5 | 65 % | 86 % | 1.05 |

| Team Vitality | K | D | A | +/− | ADR | HS % | KAST | Rating |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| ZywOo | 18 | 15 | 5 | +3 | 93.2 | 33 % | 86 % | 1.23 |
| jL | 13 | 19 | 5 | -6 | 67.1 | 46 % | 67 % | 0.76 |
| ropz | 12 | 18 | 3 | -6 | 66.1 | 42 % | 62 % | 0.73 |
| flameZ | 12 | 18 | 5 | -6 | 50.4 | 33 % | 71 % | 0.68 |
| mezii | 3 | 17 | 8 | -14 | 50.6 | 67 % | 52 % | 0.30 |

Next: [Match statistics](Match-statistics.md) for rounds and kills, or [MatchStats reference](Stats-reference.md) for every field.

[All runnable examples](https://github.com/P0wders/CS2-Demo-Library/tree/main/examples)
