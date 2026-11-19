# Installation

The crate is not on crates.io yet. Add it from GitHub in your project's `Cargo.toml`:

```toml title="Cargo.toml"
[dependencies]
cs2_demo = { git = "https://github.com/P0wders/CS2-Demo-Library" }

# Optional: keep parsing fast in debug builds of your own program.
[profile.dev.package.cs2_demo]
opt-level = 3
```

> [!TIP]
> Without the `opt-level` lines, a debug build of your program parses 10–20× slower. Release builds are always fast.

## Rust version

Developed with Rust 1.100; needs at least 1.88 (2024 edition with let chains). The first build of a project using the crate takes about 20 s on an 8-core desktop, dependencies included; after that it is cached.

No `protoc` or other tools need to be installed: the protobuf code is generated at build time from `proto/` by [protox](https://github.com/andrewhickman/protox) and [prost](https://github.com/tokio-rs/prost).

## Optional features

| Feature | Adds |
|---|---|
| `serde` | `serde::Serialize` and `Deserialize` for the stats types (see [Export to JSON](JSON-export.md)) |
| `user-messages` | protobuf types for user messages, temp entities and CS game events, to decode what `on_net_message` delivers. Off by default because it triples the generated code and doubles the crate's compile time |

Enable them in the dependency line:

```toml
cs2_demo = { git = "https://github.com/P0wders/CS2-Demo-Library", features = ["serde"] }
```

Next: [Getting started](Getting-started.md).
