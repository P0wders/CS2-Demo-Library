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

[All runnable examples](https://github.com/P0wders/CS2-Demo-Library/tree/main/examples)
