# Parser dev tools

[`examples/dev/`](https://github.com/P0wders/CS2-Demo-Library/tree/main/examples/dev) holds tools for working on the parser itself, for example after a CS2 update changes the demo format. They are not usage guides.

| Tool | What it does |
|---|---|
| `dump_schema` | Every networked class and property, with its encoding: `-- match.dem [CCSPlayerPawn]` |
| `inspect` | Structural summary: entity counts, string tables, event counts and parse speed |
| `round_check` | One player's per-round kills and deaths from events, next to the server's per-round counters: `-- match.dem <player>` |
| `timing` | Parse time of each mode, on one thread and on several |

```sh
cargo run --release --example dump_schema -- match.dem CCSPlayerPawn
cargo run --release --example round_check -- match.dem ZywOo
```

## After a CS2 update

Run the [test suite](Development.md) on a new demo. If decoding breaks:

- `Context::entity_diagnostics()` names the first entity that went wrong;
- `dump_schema` shows how each property is encoded and which decoder reads it;
- `CS2_DEMO_TRACE=1` logs every decoded field.
