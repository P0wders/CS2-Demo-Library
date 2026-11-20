# Examples

Short programs, each showing one part of the `cs2_demo` API. They are listed
here in reading order. Each one takes a demo path and prints plain text, and
each file explains its API calls in comments.

```sh
cargo run --release --example quick_start -- path/to/match.dem
```

Use `--release`: a debug build parses 10–20× slower.

## Ready-made statistics: `MatchStats`

One call, `Demo::match_stats()`, parses the demo and returns a plain struct
with everything below.

| Example | Shows |
|---|---|
| [`quick_start`](quick_start.rs) | Open a demo, read its header, get the score and the scoreboard (K/D/A, ADR, HS %, KAST, rating, T/CT splits) |
| [`rounds`](rounds.rs) | Every round: winner, how it ended, each team's buy, the bomb; one player's rounds; clutches; custom buy thresholds |
| [`kill_feed`](kill_feed.rs) | Every kill with weapon, callouts, distance and flags; kills per weapon; opening duels |
| [`grenades`](grenades.rs) | Every grenade: thrower, spot, aim, landing, effect; utility per player; repeated lineups |
| [`aim`](aim.rs) | Accuracy, head hits, first-shot accuracy, counter-strafing, crosshair placement, time to kill; per weapon |

## Your own analysis: `Visitor`

The parser calls your methods while it reads the demo. Each callback gets a
`Context` that holds the game state at that moment.

| Example | Shows |
|---|---|
| [`events`](events.rs) | A first visitor: game events and their fields, player names, `ParseOptions::events_only()`. Run without a filter to list the event names in a demo |
| [`game_state`](game_state.rs) | Reading the `Context` every N ticks: score, players' positions, callouts, health, money and weapons, raw entity properties; stopping early |
| [`combine`](combine.rs) | Your visitor and the built-in statistics in one parse (a tuple of visitors); entity diagnostics |
| [`replay`](replay.rs) | `PositionRecorder` for 2D replays: players, grenades, smokes, fires and the bomb over time, in radar pixels with `MapOverview` |

## Many demos

| Example | Shows |
|---|---|
| [`many_demos`](many_demos.rs) | `match_stats_many`: parse a folder of demos in parallel, one result per demo |
| [`scouting`](scouting.rs) | `Scouting`: a team's maps, sides, pistols, buys, bomb sites, setups, opening duels, lineups and players across many demos |

## Optional features

These two need a crate feature, passed with `--features`:

| Example | Feature | Shows |
|---|---|---|
| [`json`](json.rs) | `serde` | Save `MatchStats` as JSON and load it back |
| [`chat`](chat.rs) | `user-messages` | Raw net messages decoded with the `proto` types: the in-game chat |

```sh
cargo run --release --features serde --example json -- match.dem stats.json
cargo run --release --features user-messages --example chat -- match.dem
```

## Parser development tools

[`dev/`](dev/) holds tools for working on the parser itself, for example
after a CS2 update changes the demo format. They are not usage guides.

| Example | What it does |
|---|---|
| `dump_schema` | Every networked class and property, with its encoding: `-- match.dem [CCSPlayerPawn]` |
| `inspect` | Structural summary: entity counts, string tables, event counts and parse speed |
| `round_check` | One player's per-round kills and deaths from events, next to the server's per-round counters: `-- match.dem <player>` |
| `timing` | Parse time of each mode, on one thread and on several |
