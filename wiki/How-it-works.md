# How it works

A demo is a stream of compressed frames. Each layer below is decoded by its own module, and everything ends up in `Visitor` callbacks with the current `Context`.

```text title="pipeline"
.dem file
 └─ frames: command, tick, size, Snappy-compressed payload      container
     └─ net messages, packed in a bit stream                     parser, bitreader
         ├─ string tables: player info, entity baselines         stringtables
         ├─ class schema: flattened serializers -> decoders      sendtables
         ├─ entity updates: Huffman-coded field paths + values   entities
         └─ game events: descriptor list, then events            events
              └─ Visitor callbacks with the current Context, in demo order
                   ├─ typed views: Player, TeamInfo, GameRules    model
                   ├─ StatsCollector -> MatchStats                stats
                   ├─ PositionRecorder -> replay frames           replay
                   └─ many MatchStats -> team and player profiles scouting
```

## Threads

A parse with entities runs as a pipeline. A reader thread walks the frames and finds where each entity's data starts from the bit length the demo records for it; decoding threads decode the fields, which needs only the entity's class; the calling thread applies the values in order and runs the visitor. Classes whose encoding depends on entity state (the game rules) are decoded on the calling thread, and demos without entity lengths fall back to the one-step decoder. See [Choosing what to decode](Parse-options.md#threads).

## Modules

| Module | Contents |
|---|---|
| `container` | file header and footer, frame iterator |
| `bitreader` | bit-level reader: varints, coordinates, normals, angles, strings |
| `stringtables` | string table updates (player info, instance baselines, …) |
| `sendtables` | the class schema: property types, encoders and decoders |
| `entities` | field-path decoding, property values, entity state |
| `events` | game event descriptors and decoded events |
| `parser` | `DemoParser`, `ParseOptions` (threads, `EntitySkip`), `Visitor` (and tuples of visitors), `Context` |
| `model` | `Player`, `TeamInfo`, `GameRules`, `Weapon`, `Team` |
| `stats` | `StatsCollector` and `MatchStats`: scoreboard, rounds and economy, clutches, grenades and lineups, aim |
| `maps` | radar calibration: world positions to radar pixels |
| `replay` | `PositionRecorder`: positions over time for 2D replays |
| `scouting` | `Scouting`: team and player profiles over many matches |
| `proto` | protobuf types generated from Valve's `.proto` files in `proto/` |

## Acknowledgements

- `proto/` contains Valve's protobuf definitions, as tracked by [SteamDatabase/GameTracking-CS2](https://github.com/SteamDatabase/GameTracking-CS2).
- The Source 2 field-path and quantized-float decoding follows the algorithms documented by [dotabuff/manta](https://github.com/dotabuff/manta).
- Other CS2 demo parsers: [demoparser2](https://github.com/LaihoE/demoparser) (Rust, Python and JavaScript bindings) and [demoinfocs-golang](https://github.com/markus-wa/demoinfocs-golang) (Go).
