# Many demos at once

Parsing one demo uses one core. `match_stats_many` parses several in parallel, one per core, and returns the results in the order of the paths:

```rust
let paths = ["a.dem", "b.dem", "c.dem"];
for stats in cs2_demo::match_stats_many(&paths) {
    let stats = stats?;
    println!("{}: {} rounds", stats.map, stats.rounds.len());
}
```

Each result is a separate `Result`, so one broken file doesn't stop the others.

To answer questions about a team or a player across all of them, feed the results to [`Scouting`](Scouting.md).

Runnable version: the [`many_demos`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/many_demos.rs) example parses a folder of demos.
