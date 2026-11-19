# Reading game state

Every `Visitor` callback receives a `Context` holding the complete game state at that moment. `Demo::parse` also returns the final one.

## Players

`ctx.players()`, `ctx.active_players()` (on T or CT), `ctx.player_by_slot(..)`, `ctx.player_by_steam_id(..)`. A `Player` has:

| Group | Methods |
|---|---|
| Identity | `name()`, `steam_id()`, `team()`, `clan()`, `is_bot()` |
| Status | `is_alive()`, `health()`, `armor()`, `has_helmet()`, `has_defuser()`, `flash_duration()`, `is_scoped()`, `is_defusing()`, `is_ducking()` |
| Economy | `money()`, `start_money()`, `equipment_value()`, `cash_spent_this_round()` |
| Position | `position()`, `eye_angles()`, `place()` (callout name) |
| Equipment | `active_weapon()`, `weapons()` (name, clip, reserve ammo) |
| Scoreboard | `scoreboard()`, `score()`, `mvps()`, `ping()` |

## Teams and match state

- **Teams:** `ctx.teams()`, `ctx.team(Team::CounterTerrorist)`: name, score, score per half.
- **Match state:** `ctx.game_rules()`: warmup, freeze time, rounds played, bomb planted, last round's winner and reason.

## Any other property

Every networked property of every entity can be read by name. Nested fields use dots:

```rust
let armor: Option<i32> = player.pawn().and_then(|p| p.get("m_ArmorValue"));
let cash: Option<i32> = player.controller().get("m_pInGameMoneyServices.m_iAccount");
```

To list the properties of a class:

```sh
cargo run --release --example dump_schema -- match.dem CCSPlayerPawn
```

Runnable version: the [`game_state`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/game_state.rs) example reads the `Context` every N ticks and stops early.
