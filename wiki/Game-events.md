# Game events

Use a callback for a feed or a counter. The names you pass select the events delivered to your callback. An empty list selects all recorded events.

```rust
use cs2_demo::{Demo, Result};

fn main() -> Result<()> {
    let demo = Demo::open("match.dem")?;
    let mut deaths = 0;
    demo.for_each_event(&["player_death"], |_, _| deaths += 1)?;
    println!("{deaths} recorded deaths");
    Ok(())
}
```

The streaming path skips entity decoding. Player identities can be resolved from the callback context, but health and positions at the instant of an event need a visitor with entity decoding enabled. Some demos require entity state to reconstruct round starts and ends.

Discover which event names occur in your recording by running the events example without a filter:

```sh
cargo run --release --example events -- "match.dem"
```

[Example combining custom event analysis with match statistics](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/combine.rs)
