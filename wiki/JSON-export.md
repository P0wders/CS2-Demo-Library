# Export to JSON

With the `serde` feature, `MatchStats` and everything in it implements `serde::Serialize` and `Deserialize`, so analysed matches can be saved and loaded back without parsing the demos again.

```toml title="Cargo.toml"
cs2_demo = { git = "https://github.com/P0wders/CS2-Demo-Library", features = ["serde"] }
serde_json = "1"
```

```rust
let stats = cs2_demo::Demo::open("match.dem")?.match_stats()?;
std::fs::write("stats.json", serde_json::to_string_pretty(&stats)?)?;

let back: cs2_demo::MatchStats =
    serde_json::from_str(&std::fs::read_to_string("stats.json")?)?;
```

A 54-minute match is 1.1 MB of compact JSON and loads back in about 3 ms, so a service can parse each demo once and answer from the saved statistics: parse on import, store the JSON, and send only what the browser asks for.

Runnable version:

```sh
cargo run --release --features serde --example json -- match.dem stats.json
```
