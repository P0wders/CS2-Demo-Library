# cs2_demo

A fast Counter-Strike 2 demo (`.dem`) parser for Rust, written from scratch.

It gives you the game events, the full game state at every tick (positions,
health, money, weapons, …) and ready-made match statistics: K/D/A, ADR, KAST,
HLTV rating, opening duels, trades, multi-kills and utility.

```rust
let stats = cs2_demo::Demo::open("match.dem")?.match_stats()?;
for p in &stats.players {
    println!("{:16} {}-{}-{}  ADR {:.1}  rating {:.2}", p.name, p.kills, p.deaths, p.assists, p.adr(), p.rating());
}
```

- **Fast.** On a 342 MB, 48-minute pro demo: game events in 0.13 s, every
  entity update in under 2 s, with about 12 MB of memory.
- **Checked.** Every entity update is verified against the bit length the
  server recorded, and kills, deaths and assists match the game's own
  scoreboard. Tested on 32 demos from six game builds: tournaments, FACEIT and
  Valve matchmaking.
- **Simple.** One call for a scoreboard, or implement a `Visitor` to see
  everything as it happens.
- **Pure Rust.** No `protoc` or other tools to install.

## Contents

- [Installation](#installation)
- [Quick start](#quick-start)
- [What the stats contain](#what-the-stats-contain)
- [Examples](#examples)
- [Performance](#performance)
- [Compatibility](#compatibility)
- [How it works](#how-it-works)
- [Development](#development)
- [Acknowledgements](#acknowledgements)
- [License](#license)

## Installation

The crate is not on crates.io yet. Add it from GitHub:

```toml
[dependencies]
cs2_demo = { git = "https://github.com/P0wders/CS2-Demo-Library" }

# Optional: keep parsing fast in debug builds of your own program.
[profile.dev.package.cs2_demo]
opt-level = 3
```

Without the `opt-level` lines, a debug build of your program parses 10–20×
slower. Release builds are always fast.

Developed with Rust 1.100; needs at least 1.88 (2024 edition with let
chains). The first build of a project using the crate takes about 20 s on an
8-core desktop, dependencies included; after that it is cached.

Optional features:

| Feature | Adds |
|---|---|
| `serde` | `serde::Serialize` for the stats types (see [Export to JSON](#export-to-json)) |
| `user-messages` | protobuf types for user messages, temp entities and CS game events, to decode what `on_net_message` delivers. Off by default because it triples the generated code and doubles the crate's compile time |

## Quick start

### Match statistics in one call

```rust
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
            "{:16} {:>2}-{:>2}-{:>2}  ADR {:5.1}  HS {:3.0}%  KAST {:3.0}%  rating {:.2}",
            p.name, p.kills, p.deaths, p.assists,
            p.adr(), p.hs_ratio() * 100.0, p.kast() * 100.0, p.rating()
        );
    }
    for r in &stats.rounds {
        println!("round {:2}: {:?} won ({:?})", r.number, r.winner_name, r.reason);
    }
    Ok(())
}
```

This is that data for Nemiga vs Vitality on Cache, as rendered by the `report`
example ([full report with rounds, highlights and kill feed](docs/report-nemiga-vs-vitality.md)):

**Nemiga 13 – 8 Team Vitality** (de_cache)

| Nemiga | K | D | A | +/− | ADR | HS % | KAST | Rating |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| robo | 24 | 11 | 4 | +13 | 95.2 | 46 % | 81 % | 1.73 |
| Xant3r | 20 | 15 | 4 | +5 | 87.3 | 40 % | 67 % | 1.31 |
| khaN | 13 | 8 | 3 | +5 | 53.1 | 46 % | 81 % | 1.11 |
| SYPH0 | 13 | 10 | 6 | +3 | 80.5 | 69 % | 86 % | 1.06 |
| KaiR0N- | 17 | 15 | 7 | +2 | 105.1 | 65 % | 86 % | 1.05 |

| Team Vitality | K | D | A | +/− | ADR | HS % | KAST | Rating |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| ZywOo | 18 | 15 | 5 | +3 | 93.0 | 33 % | 86 % | 1.23 |
| jL | 13 | 19 | 5 | -6 | 66.8 | 46 % | 67 % | 0.76 |
| ropz | 12 | 18 | 3 | -6 | 65.7 | 42 % | 62 % | 0.73 |
| flameZ | 12 | 18 | 5 | -6 | 49.6 | 33 % | 71 % | 0.68 |
| mezii | 3 | 17 | 8 | -14 | 49.6 | 67 % | 52 % | 0.30 |

### Your own analysis: implement `Visitor`

The parser calls your methods while it reads the demo. Every method has an
empty default, so implement only the ones you need:

| Method | Called |
|---|---|
| `on_game_event` | for every game event: kills, damage, bomb, grenades, purchases, … |
| `on_tick_end` | after every tick; return `ControlFlow::Break(())` to stop early |
| `on_entity` | when an entity is created, updated, leaves visibility or is deleted |
| `on_string_table` | when string table entries change (player info, …) |
| `on_net_message` | for net messages the parser doesn't use itself (opt-in, see below) |
| `on_start`, `on_finish` | once, at each end |

Each callback gets a `Context`: the complete game state at that moment. This
visitor records where the killer and the victim were standing for every kill:

```rust
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

### Reading game state

- **Players:** `ctx.players()`, `ctx.active_players()` (on T or CT),
  `ctx.player_by_slot(..)`, `ctx.player_by_steam_id(..)`. A `Player` has
  - identity: `name()`, `steam_id()`, `team()`, `clan()`, `is_bot()`
  - status: `is_alive()`, `health()`, `armor()`, `has_helmet()`, `has_defuser()`,
    `flash_duration()`, `is_scoped()`, `is_defusing()`, `is_ducking()`
  - economy: `money()`, `equipment_value()`, `cash_spent_this_round()`
  - position: `position()`, `eye_angles()`, `place()` (callout name)
  - equipment: `active_weapon()`, `weapons()` (name, clip, reserve ammo)
  - scoreboard: `scoreboard()`, `score()`, `mvps()`, `ping()`
- **Teams:** `ctx.teams()`, `ctx.team(Team::CounterTerrorist)`: name, score,
  score per half.
- **Match state:** `ctx.game_rules()`: warmup, freeze time, rounds played,
  bomb planted, last round's winner and reason.
- **Anything else:** every networked property of every entity, by name.
  Nested fields use dots:

  ```rust
  let armor: Option<i32> = player.pawn().and_then(|p| p.get("m_ArmorValue"));
  let cash: Option<i32> = player.controller().get("m_pInGameMoneyServices.m_iAccount");
  ```

  To list the properties of a class:
  `cargo run --release --example dump_schema -- match.dem CCSPlayerPawn`.

### Choosing what to decode

| `ParseOptions` | Time for a 342 MB demo | Use it when |
|---|--:|---|
| `ParseOptions::default()` | 2 s | you need positions, health, money or any other entity state |
| `ParseOptions::events_only()` | 0.13 s | game events and player names are enough |
| `end_tick: Some(n)`, or `Break` from `on_tick_end` | proportional | you only need part of the demo |
| `raw_net_messages: true` | +0.04 s | you want `on_net_message` for user messages, chat, … |

The raw messages are protobuf bytes. Decode them with the types in
`cs2_demo::proto`, after enabling the `user-messages` feature.

### Many demos at once

Parsing one demo uses one core. Parse several demos in parallel to use them all:

```rust
let paths = ["a.dem", "b.dem", "c.dem"];
let all_stats = std::thread::scope(|s| {
    let jobs: Vec<_> = paths
        .iter()
        .map(|path| s.spawn(move || cs2_demo::Demo::open(path)?.match_stats()))
        .collect();
    jobs.into_iter().map(|j| j.join().unwrap()).collect::<Vec<_>>()
});
```

### Export to JSON

With the `serde` feature, `MatchStats` and everything in it implements
`serde::Serialize`:

```toml
cs2_demo = { git = "https://github.com/P0wders/CS2-Demo-Library", features = ["serde"] }
serde_json = "1"
```

```rust
let stats = cs2_demo::Demo::open("match.dem")?.match_stats()?;
std::fs::write("stats.json", serde_json::to_string_pretty(&stats)?)?;
```

## What the stats contain

`Demo::match_stats()` returns a `MatchStats`:

| Field | Contents |
|---|---|
| `teams` | name, final score, score per half and in overtime |
| `players` | per player: kills, deaths, assists, flash assists; headshot, wallbang, no-scope and through-smoke kills; team kills, suicides; damage and utility damage; opening kills and deaths; trade kills and traded deaths; KAST rounds; rounds with 2–5 kills; enemies and teammates flashed and blind time caused; grenades thrown by type; bomb plants and defuses; MVPs and score. Methods: `adr()`, `kpr()`, `kd()`, `hs_ratio()`, `kast()`, `rating()` |
| `rounds` | number, winner (team name and side), how it ended, bomb planted / defused / exploded, kills, score after the round |
| `kills` | the kill feed: tick, round, attacker, victim, assister, weapon, and headshot, wallbang, no-scope, through smoke, attacker blind, flash assist, team kill and traded flags |

Definitions:

- Kills exclude team kills and suicides; deaths include them.
- ADR counts health damage to enemies, capped at what the victim had left.
- A death is traded when a teammate of the victim kills the killer within
  5 seconds.
- KAST is the share of rounds with a kill, assist, survival or trade.
- Rating is HLTV Rating 1.0, the published formula. Later HLTV ratings are not
  public.
- Enemies flashed come from `player_blind` events. Some tournament servers
  don't record those; for their demos a blind is a victim's flash duration
  rising when a flashbang detonates, credited to the detonation nearest to the
  victim.
- Warmup doesn't count, and neither does anything after the final scoreboard
  appears (players leaving then are recorded as suicides by the game).
- Demos that start mid-match, or matches split over several files, number
  their rounds from the game's own round counter.

## Examples

Run any example with `cargo run --release --example <name> -- match.dem`.

| Example | What it does |
|---|---|
| `scoreboard` | HLTV-style scoreboard and round history in the terminal |
| `report` | Markdown report: result, scoreboard, rounds, highlights, kill feed, checks and parse times. Takes several demos: `-- a.dem b.dem > report.md` |
| `dump_events` | every game event with player names, optionally filtered: `-- match.dem player_death,bomb_planted` |
| `positions` | every living player's position as CSV, for heatmaps or 2D replays: `-- match.dem 64 > positions.csv` |
| `inspect` | structural summary: entity counts, string tables, event counts and parse speed |
| `dump_schema` | every networked class and property, with its encoding |
| `round_check` | one player's per-round kills and deaths from events next to the server's per-round counters: `-- match.dem <player name>` |
| `timing` | parse time of each mode |

## Performance

Measured on Nemiga vs Vitality, Cache (342 MB, 48 minutes, 184,457 ticks) with
the `timing` and `report` examples: AMD Ryzen 7 5800X, Windows 11, one thread,
file already in the OS cache.

| Step | Time | Speed |
|---|--:|--:|
| `Demo::open`: map the file, read header and footer | 0.2 ms | |
| Ready for the first tick: schema, string tables, baselines | 6 ms | |
| Game events only | 0.13 s | 2.6 GB/s |
| Full parse: 4.76 million entity updates | 1.95 s | 175 MB/s |
| `match_stats()`: full parse plus statistics | 2.01 s | 171 MB/s |

Memory: about 12 MB besides the memory-mapped file, which stays in the OS page
cache rather than being copied.

Compile time: about 8 s for the crate itself in release mode, 20 s for a clean
build of a project using it, dependencies included. The protobuf code is
generated at build time from `proto/` by [protox](https://github.com/andrewhickman/protox)
and [prost](https://github.com/tokio-rs/prost), with no `protoc` needed.

What makes it fast:

- The file is memory-mapped instead of read, and a helper thread maps its pages
  just ahead of the parser.
- Frames are decompressed into one reused buffer. Messages you didn't ask for
  are skipped without being decoded or copied, which is why events-only runs
  are 15× faster.
- Each entity class gets its decoders resolved once per demo. Field paths are
  decoded with a lookup table rather than a tree walk, and variable-length
  integers come out of a single 64-bit read.
- Entity properties live in a compact trie keyed by field path, and values fit
  in 24 bytes, with rare large values boxed.

## Compatibility

Tested on 32 demos. All of them decode bit-exact, and in every demo that holds
a whole match, each player's kills, deaths and assists equal the game's own
scoreboard:

| Game builds | Network patches | Recorded by |
|---|---|---|
| 10680, 10772, 10847, 10896, 10924, 10955 | 14134 to 14197 | tournament servers (BLAST, ESL and others), FACEIT, Valve matchmaking |

CS2 updates sometimes change how data is encoded. The parser reads the encoding
from the schema stored in each demo, so most updates need no change. It also
checks every entity update against the bit length the server recorded, so a
change it doesn't understand shows up in `Context::entity_diagnostics()`
instead of as wrong values.

Not supported or not tested:

- CS:GO demos (`HL2DEMO`) are not supported, only CS2 (`PBDEMS2`).
- Only server-recorded (GOTV) demos of 5v5 matches have been tested. Demos
  recorded on a player's own client, and other modes (Wingman, Deathmatch,
  Arms Race), may need work. The stats assume bomb defusal rounds.

## How it works

```
.dem file
 └─ frames: command, tick, size, Snappy-compressed payload      container
     └─ net messages, packed in a bit stream                     parser, bitreader
         ├─ string tables: player info, entity baselines         stringtables
         ├─ class schema: flattened serializers -> decoders      sendtables
         ├─ entity updates: Huffman-coded field paths + values   entities
         └─ game events: descriptor list, then events            events
              └─ Visitor callbacks with the current Context
                   ├─ typed views: Player, TeamInfo, GameRules    model
                   └─ StatsCollector -> MatchStats                stats
```

| Module | Contents |
|---|---|
| `container` | file header and footer, frame iterator |
| `bitreader` | bit-level reader: varints, coordinates, normals, angles, strings |
| `stringtables` | string table updates (player info, instance baselines, …) |
| `sendtables` | the class schema: property types, encoders and decoders |
| `entities` | field-path decoding, property values, entity state |
| `events` | game event descriptors and decoded events |
| `parser` | `DemoParser`, `ParseOptions`, `Visitor`, `Context` |
| `model` | `Player`, `TeamInfo`, `GameRules`, `Weapon`, `Team` |
| `stats` | `StatsCollector` and `MatchStats` |
| `proto` | protobuf types generated from Valve's `.proto` files in `proto/` |

## Development

```sh
cargo test                                # unit tests (integration tests skip without demos)
CS2_DEMO_DIR=path/to/demos cargo test     # run the integration suite on your demos
CS2_DEMO_BENCH=path/to/demo.dem cargo bench
cargo clippy --all-targets --features serde,user-messages
```

In PowerShell, set the variable first: `$env:CS2_DEMO_DIR = "D:\demos"`.

Demos are not committed (see `.gitignore`). Put them in `Demo/`, subfolders
included, or point `CS2_DEMO_DIR` at a folder. The integration suite parses
every demo it finds, in parallel, and checks for each one that:

- every entity update decodes to exactly the bit length the server recorded;
- there are 10 players whose names and SteamIDs agree with the server's player
  table, and their positions stay inside the map;
- rounds are numbered consecutively, each has a winner, and they add up to the
  final score;
- for demos that contain the whole match, every player's kills, deaths and
  assists equal the game's own scoreboard;
- flash statistics are filled in, and where a demo has `player_blind` events,
  they agree with the pawn flash state used as a fallback.

After a CS2 update, run the suite on a new demo. If decoding breaks,
`entity_diagnostics()` names the first entity that went wrong, `dump_schema`
shows how each property is encoded and which decoder reads it, and
`CS2_DEMO_TRACE=1` logs every decoded field.

## Acknowledgements

- `proto/` contains Valve's protobuf definitions, as tracked by
  [SteamDatabase/GameTracking-CS2](https://github.com/SteamDatabase/GameTracking-CS2).
- The Source 2 field-path and quantized-float decoding follows the algorithms
  documented by [dotabuff/manta](https://github.com/dotabuff/manta).
- Other CS2 demo parsers:
  [demoparser2](https://github.com/LaihoE/demoparser) (Rust, Python and
  JavaScript bindings) and
  [demoinfocs-golang](https://github.com/markus-wa/demoinfocs-golang) (Go).

## License

Licensed under the [Apache License, Version 2.0](LICENSE).

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license, shall
be licensed as above, without any additional terms or conditions.
