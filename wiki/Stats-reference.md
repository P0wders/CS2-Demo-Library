# MatchStats reference

`Demo::match_stats()` returns a `MatchStats`. This page lists what it contains; [Definitions](Definitions.md) explains how each number is counted.

## Fields

| Field | Contents |
|---|---|
| `teams` | name, final score, score per half and in overtime |
| `players` | per player: kills, deaths, assists, flash assists; headshot, wallbang, no-scope and through-smoke kills; team kills, suicides; damage and utility damage; opening kills and deaths; trade kills and traded deaths; KAST rounds; rounds with 2–5 kills; clutches entered and won; enemies and teammates flashed, blind time caused, flashes that led to a kill; grenades thrown by type, utility value left unused at death; bomb plants and defuses; MVPs and score. `t` and `ct` hold the same per side. `aim` holds the [aim metrics](Aim-metrics.md) |
| `rounds` | number, start, end of freeze time and end ticks; team names on each side; winner and how the round ended; bomb planted, site, time and planter, defused or exploded; kills; score after the round; each team's economy (start money, equipment value, money spent, loss streak, buy type); where every player stood 20 s into the round; every player's round (side, kills, assists, death, damage, trade, KAST, opening duel, money, equipment, spending) |
| `kills` | the kill feed: tick, round, time in the round, attacker, victim and assister, their sides, positions and callouts, distance, weapon, the victim's weapon, and headshot, wallbang, no-scope, through smoke, attacker blind, flash assist, team kill and traded flags |
| `clutches` | round, player, side, enemies left (1v1 to 1v5), kills during the clutch, won, survived |
| `grenades` | every grenade: type, round, thrower and side, throw tick and time, where the thrower stood, view angles, crouch, jump throw and speed, release point and velocity, trajectory, detonation tick and point, end of smoke or fire, players blinded and for how long, whether the flash led to a kill, damage to enemies and teammates |

## Player methods

Available on `PlayerStats` and on its `t` and `ct` halves:

| Method | Returns |
|---|---|
| `adr()`, `kpr()`, `dpr()`, `apr()` | damage, kills, deaths and assists per round |
| `kd()`, `hs_ratio()`, `kast()` | kill/death ratio, headshot share, KAST share |
| `rating()` | HLTV Rating 1.0 |
| `impact()`, `rating2_approx()` | community approximations of HLTV Impact and Rating 2.0 |

## Lookups

| Method | Returns |
|---|---|
| `round(number)` | the round with that recorded number |
| `round_kills(n)`, `round_grenades(n)`, `round_clutches(n)` | the records of one round, borrowed, in match order |
| `player(steam_id)` | one player's stats |
| `winner()` | the winning team, if any |
| `lineups()` | grenades grouped into repeated lineups |
| `map_overview()` | built-in radar calibration for the map, if any |
| `seconds(ticks)` | converts a tick count to seconds |
