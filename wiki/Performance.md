# Performance

Measured on Nemiga vs Vitality, Cache (342 MB, 48 minutes, 184,457 ticks) with the `timing` example: AMD Ryzen 7 5800X, Windows 11, one thread, file already in the OS cache, best of several runs.

| Step | Time | Speed |
|---|--:|--:|
| `Demo::open`: map the file, read header and footer | 0.2 ms | |
| Ready for the first tick: schema, string tables, baselines | 6 ms | |
| Game events only | 0.13 s | 2.6 GB/s |
| Full parse: 4.76 million entity updates | 1.95 s | 175 MB/s |
| `match_stats()`: full parse plus every statistic | 2.12 s | 162 MB/s |

**Memory:** about 12 MB besides the memory-mapped file, which stays in the OS page cache rather than being copied.

**Compile time:** about 8 s for the crate itself in release mode, 20 s for a clean build of a project using it, dependencies included.

## What makes it fast

- The file is memory-mapped instead of read, and a helper thread maps its pages just ahead of the parser.
- Frames are decompressed into one reused buffer. Messages you didn't ask for are skipped without being decoded or copied, which is why events-only runs are 15× faster.
- Each entity class gets its decoders resolved once per demo. Field paths are decoded with a lookup table rather than a tree walk, and variable-length integers come out of a single 64-bit read.
- Entity properties live in a compact trie keyed by field path, and values fit in 24 bytes, with rare large values boxed.
- The statistics read entity state through property paths resolved once per demo. Player updates change about 16 properties each, mostly animation state: two array lookups per changed property find the few that matter (position, view, crouch, flash).

## Measure it yourself

```sh
cargo run --release --example timing -- match.dem
CS2_DEMO_BENCH=path/to/demo.dem cargo bench
```
