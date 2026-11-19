# Aim metrics

`PlayerStats::aim` holds shots, hits, head hits, first-shot accuracy, shots at accurate speed, crosshair offsets, kill times and per-weapon numbers. They come from every shot fired with a gun.

## How they are measured

- A shot **hits** when the shooter damages an enemy in the same tick, or within 8 ticks for the later bullets of a burst (Glock-18, FAMAS), which share one `weapon_fire` event. The Zeus counts as a gun.
- An **engagement** opens with the first shot at an enemy after 2 s without shooting at them. The enemy is the one hit, or else the one closest to the crosshair within 10°.
- **First-shot accuracy** is the share of engagements whose first shot hit.
- **Shots at accurate speed** are rifle, sniper and pistol shots fired below 34 % of the weapon's maximum speed, i.e. after stopping or counter-strafing.
- **Crosshair placement** is the median angle between the crosshair and the enemy's head 0.2 s before an engagement's first shot.
- **Time to kill** is the median time from that shot to the kill.

> [!NOTE]
> These are approximations: demos are recorded at 64 tick, and the head is a point at eye height. The game's own "spotted" flags are not used: in demos they are updated only about twice a second.

Runnable version: the [`aim`](https://github.com/P0wders/CS2-Demo-Library/blob/main/examples/aim.rs) example prints every player's aim, per weapon too.

```sh
cargo run --release --example aim -- match.dem
```
