# Scouting several demos

`Scouting` collects `MatchStats` and answers questions about a team or a player across all of them:

```rust
use cs2_demo::scouting::Scouting;

let mut scouting = Scouting::new();
for stats in cs2_demo::match_stats_many(&paths) {
    scouting.add(stats?);
}
let team = scouting.team("Vitality").expect("in these demos");
for map in &team.maps {
    println!(
        "{}: won {} of {}; T rounds {:.0} %, CT rounds {:.0} %, pistols {}/{}, plants A {} / B {}",
        map.map, map.wins, map.matches, map.t.rate() * 100.0, map.ct.rate() * 100.0,
        map.pistols.won, map.pistols.played, map.plants_a, map.plants_b
    );
}
```

## Team profiles

A `TeamProfile` has, per map:

- matches and wins, T and CT rounds, pistol rounds;
- rounds won with each buy and against each buy;
- bomb plants per site and average plant time;
- the most frequent CT setups and T positions 20 s into the round;
- opening duels per side, with the callouts where they were won and lost;
- grenades per round and the team's repeated lineups.

It also lists the team's players.

## Player profiles

`scouting.player(steam_id)` returns a `PlayerProfile`: a player's summed stats (rates recomputed), per-map stats and usual callouts on each side.

## Team names

Teams are matched by name, ignoring case, punctuation and a leading "Team" ("Team Vitality" = "Vitality"). `Scouting::alias` merges other names:

```rust
scouting.alias("VIT", "Vitality");
```

Runnable version: the [`scouting`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/scouting.rs) example.
