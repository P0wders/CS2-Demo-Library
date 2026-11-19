# Compatibility

Tested on 32 demos. All of them decode bit-exact, and in every demo that holds a whole match, each player's kills, deaths and assists equal the game's own scoreboard.

| Game builds | Network patches | Recorded by |
|---|---|---|
| 10680, 10772, 10847, 10896, 10924, 10955 | 14134 to 14197 | tournament servers (BLAST, ESL and others), FACEIT, Valve matchmaking |

## Statistics checks

The per-round statistics, economy, grenades, aim, radar calibration and scouting were added later. They were checked on 5 tournament demos of build 10924 (ESL Pro League and PGL Masters; Anubis, Cache, Dust2, Mirage and Nuke) and 4 Valve matchmaking demos of builds 10896 and 10955 (Dust2, Inferno and Mirage, including a match with overtime):

- per-round kills, deaths, assists, damage and equipment values equal the server's own per-round counters;
- every grenade is linked to its detonation, and matches a throw event where the demo has them;
- grenade damage adds up to utility damage.

> [!TIP]
> Run the suite on your own demos (see [Development](Development.md)) before relying on them for other builds or for FACEIT demos.

## CS2 updates

CS2 updates sometimes change how data is encoded. The parser reads the encoding from the schema stored in each demo, so most updates need no change. It also checks every entity update against the bit length the server recorded, so a change it doesn't understand shows up in `Context::entity_diagnostics()` instead of as wrong values.

## Not supported or not tested

> [!WARNING]
> CS:GO demos (`HL2DEMO`) are not supported, only CS2 (`PBDEMS2`).

- Only server-recorded (GOTV) demos of 5v5 matches have been tested. Demos recorded on a player's own client, and other modes (Wingman, Deathmatch, Arms Race), may need work.
- The stats assume bomb defusal rounds.
