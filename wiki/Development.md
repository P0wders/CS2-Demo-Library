# Development

```sh
cargo test                                # unit tests (integration tests skip without demos)
CS2_DEMO_DIR=path/to/demos cargo test     # run the integration suite on your demos
CS2_DEMO_DIR=path/to/demos cargo test --features serde   # also checks the JSON round trip
CS2_DEMO_BENCH=path/to/demo.dem cargo bench
cargo clippy --all-targets --features serde,user-messages
```

In PowerShell, set the variable first: `$env:CS2_DEMO_DIR = "D:\demos"`.

Demos are not committed (see `.gitignore`). Put them in `Demo/`, subfolders included, or point `CS2_DEMO_DIR` at a folder.

## What the integration suite checks

It parses every demo it finds, in parallel, and checks for each one that:

- every entity update decodes to exactly the bit length the server recorded;
- there are 10 players whose names and SteamIDs agree with the server's player table, and their positions stay inside the map;
- rounds are numbered consecutively, each has a winner, and they add up to the final score;
- for demos that contain the whole match, every player's kills, deaths and assists equal the game's own scoreboard;
- each player's round lines and T/CT splits add up to their totals. Per round, kills, deaths, assists and equipment value equal the server's per-round counters, and damage is within a point of them;
- every round has both teams' economies; pistol rounds are round 1 and the first round after the side switch; every planted round has a bomb site; clutches are consistent with the round results;
- every grenade detonated and, where the demo has `grenade_thrown` events, matches one; grenades counted as dropped belong to players who died then; grenade damage equals utility damage; grenade blinds equal enemies flashed; smokes and fires last as long as they can;
- kills per weapon match the kill feed, and the aim numbers are in the ranges of professional players;
- kill, setup and grenade positions fall on the map's radar image and cover most of it;
- flash statistics are filled in, and where a demo has `player_blind` events, they agree with the pawn flash state used as a fallback.

A second test runs `match_stats_many` on all the demos and checks it against `match_stats`, and that the scouting profiles add up.

## After a CS2 update

Run the suite on a new demo. If decoding breaks, see [Parser dev tools](Dev-tools.md).

## License

Licensed under the [Apache License, Version 2.0](https://github.com/P0wders/CS2-Demo-Library/blob/main/LICENSE). Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this crate by you, as defined in the Apache-2.0 license, shall be licensed as above, without any additional terms or conditions.
