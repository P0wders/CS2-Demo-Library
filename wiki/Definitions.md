# Definitions

How the numbers in [`MatchStats`](Stats-reference.md) are counted.

## Scoreboard

- Kills exclude team kills and suicides; deaths include them.
- **ADR** counts health damage to enemies. A victim's health is followed through every hit (the bomb, falls and fire included), so damage is capped at what they had left. Fire deals fractions of a point per tick: the real health lost is counted, not the rounded-down `dmg_health` of the event.
- A death is **traded** when a teammate of the victim kills the killer within 5 seconds.
- **KAST** is the share of rounds with a kill, assist, survival or trade.
- **Rating** is HLTV Rating 1.0, the published formula. `impact()` and `rating2_approx()` are community approximations of HLTV's Impact and Rating 2.0, whose formulas are not public.

## Economy

Economy is read when freeze time ends: each player's money at the start of the round and the value of what they carry. A team's buy type comes from its average equipment value per player:

| Buy type | Average equipment value |
|---|--:|
| Eco | under $1,000 |
| Semi-eco | under $2,000 |
| Force buy | under $4,000 |
| Full buy | $4,000 and above |

The thresholds are `BuyThresholds`, which can be changed. **Pistol rounds** are rounds where every player started with $1,000 or less: the first round of each half.

## Rounds

- A **clutch** starts when a player is the last one alive on their side and at least one enemy is alive. In a 1v1 both players are clutching.
- The **bomb site** comes from the planted bomb (`m_nBombSite`), else from the planter's callout.
- Warmup doesn't count, and neither does anything after the final scoreboard appears (players leaving then are recorded as suicides by the game).
- Demos that start mid-match, or matches split over several files, number their rounds from the game's own round counter.

## Utility

- **Enemies flashed** come from `player_blind` events. Some tournament servers don't record those; for their demos a blind is a victim's flash duration rising when a flashbang detonates, credited to the detonation nearest to the victim.
- A **grenade** is its projectile: thrower, release point and velocity come from the projectile entity, the thrower's stance and aim from their pawn, the detonation from the game event naming that projectile.
- A fire is linked to the molotov its thrower just threw.
- A grenade marked `dropped` fell from a player who died holding it primed; it is not counted as a throw.
- Throws are counted from the grenades, since Valve matchmaking demos have no `grenade_thrown` events.

## Team names

Valve matchmaking doesn't name the teams (its side names "TERRORIST" and "CT" change hands at half-time). Such a team is named after one of its players ("Team ZyWho") and followed across side switches by its players.

## Aim

See [Aim metrics](Aim-metrics.md).
