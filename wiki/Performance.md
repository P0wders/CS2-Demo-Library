# Performance

Measured with the `timing` example on a 355 MB, 54-minute Valve matchmaking demo (207,782 ticks, 6.06 million entity updates): AMD Ryzen 7 5800X (8 cores, 16 threads), Windows 11, file already in the OS cache, best of five runs.

| Step | Time | Speed |
|---|--:|--:|
| `Demo::open`: map the file, read header and footer | 0.2 ms | |
| Ready for the first tick: schema, string tables, baselines | 11 ms | |
| Game events only | 0.09 s | 3.9 GB/s |
| `match_stats()`: every statistic | 0.59 s | 600 MB/s |
| Full parse: every entity update | 0.89 s | 400 MB/s |
| `match_stats()` on one thread | 1.96 s | 181 MB/s |
| Full parse on one thread | 2.13 s | 166 MB/s |
| Load saved `MatchStats` (1.1 MB of JSON, `serde` feature) | 3 ms | |

With 4 threads, `match_stats()` takes 0.8 s; a 174 MB demo takes 0.25 s.

**Memory:** about 35 MB with the default threads, 16 MB on one thread, besides the memory-mapped file, which stays in the OS page cache rather than being copied.

**Compile time:** about 8 s for the crate itself in release mode, 20 s for a clean build of a project using it, dependencies included.

## What makes it fast

- The file is memory-mapped instead of read, and a helper thread maps its pages just ahead of the parser.
- Entity data is decoded on several threads. Demos give the bit length of every entity's data, so a reader thread finds where each one starts without decoding it; decoding needs only the entity's class, so other threads do it ahead; the calling thread applies the values in order and runs the visitor. Only the game rules, whose encoding depends on their current state, are decoded on the calling thread.
- `match_stats()` skips the entity data the statistics never read, so the calling thread applies a quarter of the fields.
- Frames are decompressed into one reused buffer. Messages you didn't ask for are skipped without being decoded or copied, which is why events-only runs are 15× faster.
- Each entity class gets its decoders resolved once per demo. Field paths are decoded with a lookup table rather than a tree walk, and variable-length integers come out of a single 64-bit read.
- Entity properties live in a compact trie keyed by field path, and values fit in 24 bytes, with rare large values boxed.
- The statistics read entity state through property paths resolved once per demo. Player updates change about 16 properties each, mostly animation state: two array lookups per changed property find the few that matter (position, view, crouch, flash).

## Measure it yourself

```sh
cargo run --release --example timing -- match.dem
CS2_DEMO_BENCH=path/to/demo.dem cargo bench
```

`timing` reports each mode on one thread and on several.
