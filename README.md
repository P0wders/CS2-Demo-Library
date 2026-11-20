# cs2_demo

A fast Counter-Strike 2 demo (`.dem`) parser and match analyzer for Rust,
written from scratch.

It gives you the game events, the full game state at every tick (positions,
health, money, weapons, …) and the numbers a team analyst works with:

- **Scoreboard:** K/D/A, ADR, KAST, HLTV Rating 1.0, opening duels, trades and multi-kills, split by T and CT side.
- **Rounds and economy:** every player's round, each team's buy (pistol, eco, semi-eco, force, full buy), bomb sites, clutches and where everyone stood.
- **Utility:** every grenade, with its lineup, flight, and effect (players blinded, damage, smoke and fire duration).
- **Aim:** accuracy, head hits, first-shot accuracy, counter-strafing, crosshair placement and time to kill.
- **Scouting across many demos:** map records, side and pistol win rates, buys, site preferences, setups and lineups.
- **2D replay and radar positions:** players, grenades, smokes, fires and the bomb over time, placed on the map's radar image.

```rust
let stats = cs2_demo::Demo::open("match.dem")?.match_stats()?;
for p in &stats.players {
    println!("{:16} {}-{}-{}  ADR {:.1}  rating {:.2}", p.name, p.kills, p.deaths, p.assists, p.adr(), p.rating());
}
```

- **Fast.** On a 355 MB, 54-minute match, on an 8-core desktop: game events
  in 0.09 s, all the statistics above in 0.6 s, every entity update in 0.9 s.
  A parse uses several cores and gives exactly the results of a
  single-threaded one. Saved statistics load back in 3 ms.
- **Checked.** Every entity update is verified against the bit length the
  server recorded, and kills, deaths and assists match the game's own
  scoreboard: tested on 32 demos from six game builds (tournaments, FACEIT and
  Valve matchmaking). Per round, kills, damage and equipment values match the
  server's own per-round counters (see [Compatibility](#compatibility)).
- **Simple.** One call for everything, or implement a `Visitor` to see
  everything as it happens.
- **Pure Rust.** No `protoc` or other tools to install.

## Contents

- [Installation](#installation)
- [Quick start](#quick-start)
- [What the stats contain](#what-the-stats-contain)
- [Scouting several demos](#scouting-several-demos)
- [2D replay and radar positions](#2d-replay-and-radar-positions)
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
| `serde` | `serde::Serialize` and `Deserialize` for the stats types (see [Export to JSON](#export-to-json)) |
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
            "{:16} {:>2}-{:>2}-{:>2}  ADR {:5.1}  KAST {:3.0}%  rating {:.2}  T {:.2} / CT {:.2}",
            p.name, p.kills, p.deaths, p.assists,
            p.adr(), p.kast() * 100.0, p.rating(), p.t.rating(), p.ct.rating()
        );
    }
    for r in &stats.rounds {
        let buy = |side| r.economy(side).map(|e| e.buy.label()).unwrap_or("?");
        println!(
            "round {:2}: {:?} won ({:?}); T {} vs CT {}",
            r.number, r.winner_name, r.reason,
            buy(cs2_demo::Team::Terrorist), buy(cs2_demo::Team::CounterTerrorist)
        );
    }
    Ok(())
}
```

This is the scoreboard part of that data for Nemiga vs Vitality on Cache:

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

To run your visitor and the built-in statistics in one parse, pass a tuple:
every callback goes to both.

```rust
let mut both = (cs2_demo::StatsCollector::new(), KillPositions::default());
let ctx = demo.parse(ParseOptions::default(), &mut both)?;
let stats = both.0.into_stats(&ctx);
```

### Reading game state

- **Players:** `ctx.players()`, `ctx.active_players()` (on T or CT),
  `ctx.player_by_slot(..)`, `ctx.player_by_steam_id(..)`. A `Player` has
  - identity: `name()`, `steam_id()`, `team()`, `clan()`, `is_bot()`
  - status: `is_alive()`, `health()`, `armor()`, `has_helmet()`, `has_defuser()`,
    `flash_duration()`, `is_scoped()`, `is_defusing()`, `is_ducking()`
  - economy: `money()`, `start_money()`, `equipment_value()`, `cash_spent_this_round()`
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

| `ParseOptions` | Time for a 355 MB demo | Use it when |
|---|--:|---|
| `ParseOptions::default()` | 0.9 s | you need positions, health, money or any other entity state |
| `StatsCollector::parse_options()` | 0.6 s | only the built-in statistics read the parse (what `match_stats()` uses) |
| `ParseOptions::events_only()` | 0.09 s | game events and player names are enough |
| `skip: EntitySkip { .. }` | less | your visitor never reads some classes or properties |
| `threads: 1` | 2.1 s | everything should run on the calling thread |
| `end_tick: Some(n)`, or `Break` from `on_tick_end` | proportional | you only need part of the demo |
| `raw_net_messages: true` | +0.04 s | you want `on_net_message` for user messages, chat, … |

A parse with entities uses up to 8 threads (`ParseOptions::threads`; 0, the
default, picks from the number of cores). A reader thread walks the frames,
other threads decode entity data ahead, and the calling thread applies it and
runs your visitor in demo order, so the callbacks are exactly those of a
single-threaded parse.

`skip` leaves out entity data your visitor doesn't read: entities of a skipped
class still exist, but their updates aren't decoded; skipped properties aren't
stored or listed as changed. `StatsCollector::entity_skip()` is what the
statistics never read (chickens, the animation state of players and weapons,
the controllers' tick base): three quarters of a demo's entity fields. When
your own visitor shares the parse with `StatsCollector`, use the default, or a
skip list that keeps what your visitor reads.

The raw messages are protobuf bytes. Decode them with the types in
`cs2_demo::proto`, after enabling the `user-messages` feature.

### Many demos at once

`match_stats_many` parses several demos at once, sharing the cores between
them, and returns the results in order:

```rust
let paths = ["a.dem", "b.dem", "c.dem"];
for stats in cs2_demo::match_stats_many(&paths) {
    let stats = stats?;
    println!("{}: {} rounds", stats.map, stats.rounds.len());
}
```

### Export to JSON

With the `serde` feature, `MatchStats` and everything in it implements
`serde::Serialize` and `Deserialize`, so analysed matches can be saved and
loaded back without parsing the demos again:

```toml
cs2_demo = { git = "https://github.com/P0wders/CS2-Demo-Library", features = ["serde"] }
serde_json = "1"
```

```rust
let stats = cs2_demo::Demo::open("match.dem")?.match_stats()?;
std::fs::write("stats.json", serde_json::to_string_pretty(&stats)?)?;
let back: cs2_demo::MatchStats = serde_json::from_str(&std::fs::read_to_string("stats.json")?)?;
```

A 54-minute match is 1.1 MB of compact JSON and loads back in about 3 ms, so
a service can parse each demo once and answer from the saved statistics.

## What the stats contain

`Demo::match_stats()` returns a `MatchStats`:

| Field | Contents |
|---|---|
| `teams` | name, final score, score per half and in overtime |
| `players` | per player: kills, deaths, assists, flash assists; headshot, wallbang, no-scope and through-smoke kills; team kills, suicides; damage and utility damage; opening kills and deaths; trade kills and traded deaths; KAST rounds; rounds with 2–5 kills; clutches entered and won; enemies and teammates flashed, blind time caused, flashes that led to a kill; grenades thrown by type, utility value left unused at death; bomb plants and defuses; MVPs and score. `t` and `ct` hold the same per side. `aim` holds shots, hits, head hits, first-shot accuracy, shots at accurate speed, crosshair offsets, kill times and per-weapon numbers. Methods: `adr()`, `kpr()`, `dpr()`, `apr()`, `kd()`, `hs_ratio()`, `kast()`, `rating()`, `impact()`, `rating2_approx()` (also on `t` and `ct`) |
| `rounds` | number, start, end of freeze time and end ticks; team names on each side; winner and how the round ended; bomb planted, site, time and planter, defused or exploded; kills; score after the round; each team's economy (start money, equipment value, money spent, loss streak, buy type); where every player stood 20 s into the round; every player's round (side, kills, assists, death, damage, trade, KAST, opening duel, money, equipment, spending) |
| `kills` | the kill feed: tick, round, time in the round, attacker, victim and assister, their sides, positions and callouts, distance, weapon, the victim's weapon, and headshot, wallbang, no-scope, through smoke, attacker blind, flash assist, team kill and traded flags |
| `clutches` | round, player, side, enemies left (1v1 to 1v5), kills during the clutch, won, survived |
| `grenades` | every grenade: type, round, thrower and side, throw tick and time, where the thrower stood, view angles, crouch, jump throw and speed, release point and velocity, trajectory, detonation tick and point, end of smoke or fire, players blinded and for how long, whether the flash led to a kill, damage to enemies and teammates. `MatchStats::lineups()` groups repeated throws |

Definitions:

- Kills exclude team kills and suicides; deaths include them.
- ADR counts health damage to enemies. A victim's health is followed through
  every hit (the bomb, falls and fire included), so damage is capped at what
  they had left. Fire deals fractions of a point per tick: the real health
  lost is counted, not the rounded-down `dmg_health` of the event.
- A death is traded when a teammate of the victim kills the killer within
  5 seconds.
- KAST is the share of rounds with a kill, assist, survival or trade.
- Rating is HLTV Rating 1.0, the published formula. `impact()` and
  `rating2_approx()` are community approximations of HLTV's Impact and
  Rating 2.0, whose formulas are not public.
- Economy is read when freeze time ends: each player's money at the start of
  the round and the value of what they carry. A team's buy type comes from
  its average equipment value per player: eco under \$1,000, semi-eco under
  \$2,000, force buy under \$4,000, full buy above (`BuyThresholds`, which can
  be changed). Pistol rounds are rounds where every player started with
  \$1,000 or less: the first round of each half.
- A clutch starts when a player is the last one alive on their side and at
  least one enemy is alive. In a 1v1 both players are clutching.
- The bomb site comes from the planted bomb (`m_nBombSite`), else from the
  planter's callout.
- Enemies flashed come from `player_blind` events. Some tournament servers
  don't record those; for their demos a blind is a victim's flash duration
  rising when a flashbang detonates, credited to the detonation nearest to the
  victim.
- A grenade is its projectile: thrower, release point and velocity come from
  the projectile entity, the thrower's stance and aim from their pawn, the
  detonation from the game event naming that projectile. A fire is linked to
  the molotov its thrower just threw. A grenade marked `dropped` fell from a
  player who died holding it primed; it is not counted as a throw. Throws are
  counted from the grenades, since Valve matchmaking demos have no
  `grenade_thrown` events.
- Valve matchmaking doesn't name the teams (its side names "TERRORIST" and
  "CT" change hands at half-time). Such a team is named after one of its
  players ("Team ZyWho") and followed across side switches by its players.
- Aim, from every shot fired with a gun:
  - A shot **hits** when the shooter damages an enemy in the same tick, or
    within 8 ticks for the later bullets of a burst (Glock-18, FAMAS), which
    share one `weapon_fire` event. The Zeus counts as a gun.
  - An **engagement** opens with the first shot at an enemy after 2 s without
    shooting at them. The enemy is the one hit, or else the one closest to the
    crosshair within 10°.
  - **First-shot accuracy** is the share of engagements whose first shot hit.
  - **Shots at accurate speed** are rifle, sniper and pistol shots fired below
    34 % of the weapon's maximum speed, i.e. after stopping or counter-strafing.
  - **Crosshair placement** is the median angle between the crosshair and the
    enemy's head 0.2 s before an engagement's first shot.
  - **Time to kill** is the median time from that shot to the kill.

  These are approximations: demos are recorded at 64 tick, and the head is a
  point at eye height. The game's own "spotted" flags are not used: in demos
  they are updated only about twice a second.
- Warmup doesn't count, and neither does anything after the final scoreboard
  appears (players leaving then are recorded as suicides by the game).
- Demos that start mid-match, or matches split over several files, number
  their rounds from the game's own round counter.

## Scouting several demos

`Scouting` collects `MatchStats` and answers questions about a team or a
player across all of them:

```rust
use cs2_demo::scouting::Scouting;

let mut scouting = Scouting::new();
for stats in cs2_demo::match_stats_many(&paths) {
    scouting.add(stats?);
}
let team = scouting.team("Vitality").expect("in these demos");
for map in &team.maps {
    println!(
        "{}: won {} of {}; T rounds {:.0} %, CT rounds {:.0} %, pistols {}/{}, plants A {} / B {}",
        map.map, map.wins, map.matches, map.t.rate() * 100.0, map.ct.rate() * 100.0,
        map.pistols.won, map.pistols.played, map.plants_a, map.plants_b
    );
}
```

A `TeamProfile` has, per map: matches and wins, T and CT rounds, pistol
rounds, rounds won with each buy and against each buy, bomb plants per site and
average plant time, the most frequent CT setups and T positions 20 s into the
round, opening duels per side with the callouts where they were won and lost,
grenades per round and the team's repeated lineups. It also lists the team's
players. A `PlayerProfile` holds a player's summed stats (rates recomputed),
per-map stats and usual callouts on each side.

Teams are matched by name, ignoring case, punctuation and a leading "Team"
("Team Vitality" = "Vitality"). `Scouting::alias` merges other names.

## 2D replay and radar positions

`replay::PositionRecorder` is a visitor that samples players, grenades, smokes,
fires and the bomb every few ticks. Run it next to the statistics to get both
in one parse:

```rust
use cs2_demo::replay::PositionRecorder;
use cs2_demo::{Demo, ParseOptions, StatsCollector};

let demo = Demo::open("match.dem")?;
let mut both = (StatsCollector::new(), PositionRecorder::every(16));
let ctx = demo.parse(ParseOptions::default(), &mut both)?;
let (collector, recorder) = both;
let stats = collector.into_stats(&ctx);
let round = &stats.rounds[0];
let frames = recorder.frames_between(round.start_tick, round.end_tick.unwrap_or(i32::MAX));
```

Positions are placed on radar images with `maps::MapOverview`. It has
calibration for Ancient, Anubis, Cache, Dust2, Inferno, Mirage, Nuke,
Overpass, Train and Vertigo. The tests check those in the test demos (Anubis,
Cache, Dust2, Inferno, Mirage and Nuke); the others are untested.
`MapOverview::from_kv` reads the game's own overview files
(`resource/overviews/<map>.txt`), to fix a value or for any other map. The
[`replay` example](examples/replay.rs) prints a round in radar pixels.

## Examples

[`examples/`](examples/) has short programs that each show one part of the
API, with comments. Read them in this order (details in
[examples/README.md](examples/README.md)), and run any of them with
`cargo run --release --example <name> -- match.dem`.

| Example | Shows |
|---|---|
| `quick_start` | open a demo, read its header, get the score and the scoreboard |
| `rounds` | rounds, buys, the bomb, one player's rounds, clutches, custom buy thresholds |
| `kill_feed` | every kill with weapon, callouts and flags; kills per weapon; opening duels |
| `grenades` | every grenade and its effect; utility per player; repeated lineups |
| `aim` | accuracy, first-shot accuracy, counter-strafing, crosshair placement, time to kill |
| `events` | a first `Visitor`: game events, their fields and player names |
| `game_state` | reading the `Context` every N ticks: positions, health, money, raw properties |
| `combine` | your visitor and the built-in statistics in one parse |
| `replay` | `PositionRecorder` frames in radar pixels |
| `many_demos` | `match_stats_many` over a folder of demos |
| `scouting` | a team's profile across many demos |
| `json` | save and load `MatchStats` as JSON (`--features serde`) |
| `chat` | raw net messages decoded with `proto`: the in-game chat (`--features user-messages`) |

[`examples/dev/`](examples/dev/) holds tools for working on the parser:
`dump_schema`, `inspect`, `round_check` and `timing`.

## Performance

Measured with the `timing` example on a 355 MB, 54-minute Valve matchmaking
demo (207,782 ticks, 6.06 million entity updates): AMD Ryzen 7 5800X (8 cores,
16 threads), Windows 11, file already in the OS cache, best of five runs.

| Step | Time | Speed |
|---|--:|--:|
| `Demo::open`: map the file, read header and footer | 0.2 ms | |
| Ready for the first tick: schema, string tables, baselines | 11 ms | |
| Game events only | 0.09 s | 3.9 GB/s |
| `match_stats()`: every statistic above | 0.59 s | 600 MB/s |
| Full parse: every entity update | 0.89 s | 400 MB/s |
| `match_stats()` on one thread | 1.96 s | 181 MB/s |
| Full parse on one thread | 2.13 s | 166 MB/s |
| Load saved `MatchStats` (1.1 MB of JSON, `serde` feature) | 3 ms | |

With 4 threads, `match_stats()` takes 0.8 s; a 174 MB demo takes 0.25 s.

Memory: about 35 MB with the default threads, 16 MB on one thread, besides the
memory-mapped file, which stays in the OS page cache rather than being copied.

Compile time: about 8 s for the crate itself in release mode, 20 s for a clean
build of a project using it, dependencies included. The protobuf code is
generated at build time from `proto/` by [protox](https://github.com/andrewhickman/protox)
and [prost](https://github.com/tokio-rs/prost), with no `protoc` needed.

What makes it fast:

- The file is memory-mapped instead of read, and a helper thread maps its pages
  just ahead of the parser.
- Entity data is decoded on several threads. Demos give the bit length of
  every entity's data, so a reader thread finds where each one starts without
  decoding it; decoding needs only the entity's class, so other threads do it
  ahead; the calling thread applies the values in order and runs the visitor.
  Only the game rules, whose encoding depends on their current state, are
  decoded on the calling thread.
- `match_stats()` skips the entity data the statistics never read, so the
  calling thread applies a quarter of the fields.
- Frames are decompressed into one reused buffer. Messages you didn't ask for
  are skipped without being decoded or copied, which is why events-only runs
  are 15× faster.
- Each entity class gets its decoders resolved once per demo. Field paths are
  decoded with a lookup table rather than a tree walk, and variable-length
  integers come out of a single 64-bit read.
- Entity properties live in a compact trie keyed by field path, and values fit
  in 24 bytes, with rare large values boxed.
- The statistics read entity state through property paths resolved once per
  demo. Player updates change about 16 properties each, mostly animation
  state: two array lookups per changed property find the few that matter
  (position, view, crouch, flash).

## Compatibility

Tested on 32 demos. All of them decode bit-exact, and in every demo that holds
a whole match, each player's kills, deaths and assists equal the game's own
scoreboard:

| Game builds | Network patches | Recorded by |
|---|---|---|
| 10680, 10772, 10847, 10896, 10924, 10955 | 14134 to 14197 | tournament servers (BLAST, ESL and others), FACEIT, Valve matchmaking |

The per-round statistics, economy, grenades, aim, radar calibration and
scouting were added later. They were checked on 5 tournament demos of build
10924 (ESL Pro League and PGL Masters; Anubis, Cache, Dust2, Mirage and Nuke)
and 4 Valve matchmaking demos of builds 10896 and 10955 (Dust2, Inferno and
Mirage, including a match with overtime):

- per-round kills, deaths, assists, damage and equipment values equal the
  server's own per-round counters;
- every grenade is linked to its detonation, and matches a throw event where
  the demo has them;
- grenade damage adds up to utility damage.

Run the suite on your own demos (see [Development](#development)) before
relying on them for other builds or for FACEIT demos.

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
                   ├─ StatsCollector -> MatchStats                stats
                   ├─ PositionRecorder -> replay frames           replay
                   └─ many MatchStats -> team and player profiles scouting
```

| Module | Contents |
|---|---|
| `container` | file header and footer, frame iterator |
| `bitreader` | bit-level reader: varints, coordinates, normals, angles, strings |
| `stringtables` | string table updates (player info, instance baselines, …) |
| `sendtables` | the class schema: property types, encoders and decoders |
| `entities` | field-path decoding, property values, entity state |
| `events` | game event descriptors and decoded events |
| `parser` | `DemoParser`, `ParseOptions` (threads, `EntitySkip`), `Visitor` (and tuples of visitors), `Context` |
| `model` | `Player`, `TeamInfo`, `GameRules`, `Weapon`, `Team` |
| `stats` | `StatsCollector` and `MatchStats`: scoreboard, rounds and economy, clutches, grenades and lineups, aim |
| `maps` | radar calibration: world positions to radar pixels |
| `replay` | `PositionRecorder`: positions over time for 2D replays |
| `scouting` | `Scouting`: team and player profiles over many matches |
| `proto` | protobuf types generated from Valve's `.proto` files in `proto/` |

## Development

```sh
cargo test                                # unit tests (integration tests skip without demos)
CS2_DEMO_DIR=path/to/demos cargo test     # run the integration suite on your demos
CS2_DEMO_DIR=path/to/demos cargo test --features serde   # also checks the JSON round trip
CS2_DEMO_BENCH=path/to/demo.dem cargo bench
cargo clippy --all-targets --features serde,user-messages
```

In PowerShell, set the variable first: `$env:CS2_DEMO_DIR = "D:\demos"`.

Demos are not committed (see `.gitignore`). Put them in `Demo/`, subfolders
included, or point `CS2_DEMO_DIR` at a folder. The integration suite parses
every demo it finds, in parallel. It checks for each one that:

- every entity update decodes to exactly the bit length the server recorded;
- there are 10 players whose names and SteamIDs agree with the server's player
  table, and their positions stay inside the map;
- rounds are numbered consecutively, each has a winner, and they add up to the
  final score;
- for demos that contain the whole match, every player's kills, deaths and
  assists equal the game's own scoreboard;
- each player's round lines and T/CT splits add up to their totals. Per round,
  kills, deaths, assists and equipment value equal the server's per-round
  counters, and damage is within a point of them;
- every round has both teams' economies; pistol rounds are round 1 and the
  first round after the side switch; every planted round has a bomb site;
  clutches are consistent with the round results;
- every grenade detonated and, where the demo has `grenade_thrown` events,
  matches one; grenades counted as dropped belong to players who died then;
  grenade damage equals utility damage; grenade blinds equal enemies
  flashed; smokes and fires last as long as they can;
- kills per weapon match the kill feed, and the aim numbers are in the ranges
  of professional players;
- kill, setup and grenade positions fall on the map's radar image and cover
  most of it;
- flash statistics are filled in, and where a demo has `player_blind` events,
  they agree with the pawn flash state used as a fallback.

A second test runs `match_stats_many` on all the demos and checks it against
`match_stats`, and that the scouting profiles add up. A third checks that a
parse on several threads delivers exactly the callbacks of a single-threaded
one, and that `StatsCollector::entity_skip()` leaves the statistics unchanged.

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
