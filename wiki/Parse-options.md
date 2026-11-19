# Choosing what to decode

Decoding entity updates is most of the work. If you don't need them, say so with `ParseOptions` and the parse gets much faster.

| `ParseOptions` | Time for a 342 MB demo | Use it when |
|---|--:|---|
| `ParseOptions::default()` | 2 s | you need positions, health, money or any other entity state |
| `ParseOptions::events_only()` | 0.13 s | game events and player names are enough |
| `end_tick: Some(n)`, or `Break` from `on_tick_end` | proportional | you only need part of the demo |
| `raw_net_messages: true` | +0.04 s | you want `on_net_message` for user messages, chat, … |

```rust
use cs2_demo::{Demo, ParseOptions};

let demo = Demo::open("match.dem")?;
demo.parse(ParseOptions::events_only(), &mut my_visitor)?;
```

## Raw net messages

The raw messages are protobuf bytes. Decode them with the types in `cs2_demo::proto`, after enabling the `user-messages` feature. The [`chat`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/chat.rs) example decodes the in-game chat this way:

```sh
cargo run --release --features user-messages --example chat -- match.dem
```

## Single-threaded parsing

`Demo::parse` uses a helper thread that maps file pages just ahead of the parser. `Demo::parse_single_threaded` skips it: useful for short queries or applications that manage their own worker pool.
