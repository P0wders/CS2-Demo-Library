# Choosing what to decode

Decoding entity updates is most of the work. Tell the parser what you need with `ParseOptions` and it can skip the rest.

| `ParseOptions` | Time for a 355 MB demo | Use it when |
|---|--:|---|
| `ParseOptions::default()` | 0.9 s | you need positions, health, money or any other entity state |
| `StatsCollector::parse_options()` | 0.6 s | only the built-in statistics read the parse (what `match_stats()` uses) |
| `ParseOptions::events_only()` | 0.09 s | game events and player names are enough |
| `skip: EntitySkip { .. }` | less | your visitor never reads some classes or properties |
| `threads: 1` | 2.1 s | everything should run on the calling thread |
| `end_tick: Some(n)`, or `Break` from `on_tick_end` | proportional | you only need part of the demo |
| `raw_net_messages: true` | +0.04 s | you want `on_net_message` for user messages, chat, … |

```rust
use cs2_demo::{Demo, ParseOptions};

let demo = Demo::open("match.dem")?;
demo.parse(ParseOptions::events_only(), &mut my_visitor)?;
```

## Threads

A parse with entities uses up to 8 threads. `ParseOptions::threads` sets the number; `0`, the default, picks from the number of cores, up to `ParseOptions::AUTO_THREADS`.

A reader thread walks the frames, other threads decode entity data ahead, and the calling thread applies it and runs your visitor in demo order. Your callbacks are exactly those of a single-threaded parse, so a visitor needs no locking.

```rust
let opts = ParseOptions { threads: 4, ..ParseOptions::default() };
```

`Demo::parse_single_threaded` runs everything on the calling thread, without touching file pages ahead of the parser: useful for short queries or applications that manage their own worker pool.

## Skipping entity data

`skip` leaves out entity data your visitor doesn't read. Entities of a skipped class still exist, but their updates aren't decoded; skipped properties aren't stored or listed as changed. Skipping an array, vector or object skips all of it.

```rust
use cs2_demo::{EntitySkip, ParseOptions};

let opts = ParseOptions {
    skip: EntitySkip {
        classes: vec!["CChicken".into()],
        props: vec!["m_nTickBase".into()],
    },
    ..ParseOptions::default()
};
```

`StatsCollector::entity_skip()` is what the statistics never read (chickens, the animation state of players and weapons, the controllers' tick base): three quarters of a demo's entity fields.

> [!WARNING]
> When your own visitor shares the parse with `StatsCollector` ([a tuple of visitors](Visitor.md)), use the default, or a skip list that keeps what your visitor reads.

## Raw net messages

The raw messages are protobuf bytes. Decode them with the types in `cs2_demo::proto`, after enabling the `user-messages` feature. The [`chat`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/chat.rs) example decodes the in-game chat this way:

```sh
cargo run --release --features user-messages --example chat -- match.dem
```
