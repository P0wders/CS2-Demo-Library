# Examples

The [`examples/`](https://github.com/P0wders/CS2-Demo-Library/tree/main/examples) folder has short programs, each showing one part of the API. Each one takes a demo path, prints plain text, and explains its API calls in comments. They are listed here in reading order.

```sh
cargo run --release --example quick_start -- path/to/match.dem
```

> [!TIP]
> Use `--release`: a debug build parses 10–20× slower.

## Ready-made statistics

One call, `Demo::match_stats()`, parses the demo and returns a plain struct with everything below.

| Example | Shows |
|---|---|
| [`quick_start`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/quick_start.rs) | Open a demo, read its header, get the score and the scoreboard (K/D/A, ADR, HS %, KAST, rating, T/CT splits) |
| [`rounds`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/rounds.rs) | Every round: winner, how it ended, each team's buy, the bomb; one player's rounds; clutches; custom buy thresholds |
| [`kill_feed`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/kill_feed.rs) | Every kill with weapon, callouts, distance and flags; kills per weapon; opening duels |
| [`grenades`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/grenades.rs) | Every grenade: thrower, spot, aim, landing, effect; utility per player; repeated lineups |
| [`aim`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/aim.rs) | Accuracy, head hits, first-shot accuracy, counter-strafing, crosshair placement, time to kill; per weapon |

## Your own analysis

The parser calls your methods while it reads the demo. Each callback gets a `Context` that holds the game state at that moment. See [Writing a Visitor](Visitor.md).

| Example | Shows |
|---|---|
| [`events`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/events.rs) | A first visitor: game events and their fields, player names, `ParseOptions::events_only()`. Run without a filter to list the event names in a demo |
| [`game_state`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/game_state.rs) | Reading the `Context` every N ticks: score, players' positions, callouts, health, money and weapons, raw entity properties; stopping early |
| [`combine`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/combine.rs) | Your visitor and the built-in statistics in one parse (a tuple of visitors); entity diagnostics |
| [`replay`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/replay.rs) | `PositionRecorder` for 2D replays: players, grenades, smokes, fires and the bomb over time, in radar pixels with `MapOverview` |

## Many demos

| Example | Shows |
|---|---|
| [`many_demos`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/many_demos.rs) | `match_stats_many`: parse a folder of demos in parallel, one result per demo |
| [`scouting`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/scouting.rs) | `Scouting`: a team's maps, sides, pistols, buys, bomb sites, setups, opening duels, lineups and players across many demos |

## Optional features

These two need a crate feature, passed with `--features`:

| Example | Feature | Shows |
|---|---|---|
| [`json`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/json.rs) | `serde` | Save `MatchStats` as JSON and load it back |
| [`chat`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/chat.rs) | `user-messages` | Raw net messages decoded with the `proto` types: the in-game chat |

```sh
cargo run --release --features serde --example json -- match.dem stats.json
cargo run --release --features user-messages --example chat -- match.dem
```
