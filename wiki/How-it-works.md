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
              └─ Visitor callbacks with the current Context
                   ├─ typed views: Player, TeamInfo, GameRules    model
                   ├─ StatsCollector -> MatchStats                stats
                   ├─ PositionRecorder -> replay frames           replay
                   └─ many MatchStats -> team and player profiles scouting
```

## Modules

| Module | Contents |
|---|---|
| `container` | file header and footer, frame iterator |
| `bitreader` | bit-level reader: varints, coordinates, normals, angles, strings |
| `stringtables` | string table updates (player info, instance baselines, …) |
| `sendtables` | the class schema: property types, encoders and decoders |
| `entities` | field-path decoding, property values, entity state |
| `events` | game event descriptors and decoded events |
| `parser` | `DemoParser`, `ParseOptions`, `Visitor` (and tuples of visitors), `Context` |
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
