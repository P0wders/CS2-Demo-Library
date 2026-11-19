//! Per-round building blocks: each player's line for a round, side splits,
//! clutches and bomb sites.

use crate::model::Team;

/// Bomb site of a plant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BombSite {
    A,
    B,
}

impl BombSite {
    /// From `CPlantedC4.m_nBombSite` (0 = A, 1 = B).
    pub fn from_index(n: i32) -> Option<BombSite> {
        match n {
            0 => Some(BombSite::A),
            1 => Some(BombSite::B),
            _ => None,
        }
    }

    /// From a callout name such as `BombsiteA`.
    pub fn from_place(place: &str) -> Option<BombSite> {
        let p = place.to_ascii_lowercase();
        if p.contains("bombsitea") {
            Some(BombSite::A)
        } else if p.contains("bombsiteb") {
            Some(BombSite::B)
        } else {
            None
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            BombSite::A => "A",
            BombSite::B => "B",
        }
    }
}

/// One player's round: what they did and what they had.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlayerRound {
    /// SteamID64 (0 for bots).
    pub steam_id: u64,
    /// Side played this round.
    pub side: Option<Team>,
    pub kills: u32,
    pub headshot_kills: u32,
    pub assists: u32,
    pub died: bool,
    /// Health damage dealt to enemies (capped at remaining health).
    pub damage: u32,
    /// Part of `damage` dealt with HE grenades and fire.
    pub utility_damage: u32,
    /// Died, and a teammate killed the killer within the trade window.
    pub traded: bool,
    /// Kills that traded a teammate's death.
    pub trade_kills: u32,
    /// Kill, assist, survived or traded.
    pub kast: bool,
    pub opening_kill: bool,
    pub opening_death: bool,
    /// Money at the start of the round.
    pub money_start: i32,
    /// Value of the equipment carried when freeze time ended.
    pub equipment_value: i32,
    /// Money spent this round.
    pub cash_spent: i32,
}

/// Where a player stood [`super::SETUP_SECONDS`] after freeze time.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlayerSpot {
    pub steam_id: u64,
    pub side: Team,
    /// Callout name (`m_szLastPlaceName`), empty when the map has none there.
    pub place: String,
    pub pos: [f32; 3],
}

/// Statistics of one side (T or CT) for a player. Same rate methods as
/// [`super::PlayerStats`]: `adr()`, `kast()`, `rating()`, ...
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SideStats {
    pub rounds_played: u32,
    pub rounds_won: u32,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub headshot_kills: u32,
    pub damage: u32,
    pub utility_damage: u32,
    pub opening_kills: u32,
    pub opening_deaths: u32,
    pub trade_kills: u32,
    pub traded_deaths: u32,
    pub kast_rounds: u32,
    /// `multi_kills[n]` = rounds with exactly n kills.
    pub multi_kills: [u32; 6],
}

impl SideStats {
    pub(crate) fn add_round(&mut self, l: &PlayerRound, won: bool) {
        self.rounds_played += 1;
        self.rounds_won += won as u32;
        self.kills += l.kills;
        self.deaths += l.died as u32;
        self.assists += l.assists;
        self.headshot_kills += l.headshot_kills;
        self.damage += l.damage;
        self.utility_damage += l.utility_damage;
        self.opening_kills += l.opening_kill as u32;
        self.opening_deaths += l.opening_death as u32;
        self.trade_kills += l.trade_kills;
        self.traded_deaths += l.traded as u32;
        self.kast_rounds += l.kast as u32;
        self.multi_kills[l.kills.min(5) as usize] += 1;
    }

    /// Adds another player's (or match's) numbers to these.
    pub fn merge(&mut self, o: &SideStats) {
        self.rounds_played += o.rounds_played;
        self.rounds_won += o.rounds_won;
        self.kills += o.kills;
        self.deaths += o.deaths;
        self.assists += o.assists;
        self.headshot_kills += o.headshot_kills;
        self.damage += o.damage;
        self.utility_damage += o.utility_damage;
        self.opening_kills += o.opening_kills;
        self.opening_deaths += o.opening_deaths;
        self.trade_kills += o.trade_kills;
        self.traded_deaths += o.traded_deaths;
        self.kast_rounds += o.kast_rounds;
        for (a, b) in self.multi_kills.iter_mut().zip(o.multi_kills) {
            *a += b;
        }
    }
}

/// A player left alone against one or more enemies.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Clutch {
    /// 1-based round number.
    pub round: u32,
    pub steam_id: u64,
    pub name: String,
    pub side: Team,
    /// Enemies alive when the clutch began: 1 for a 1v1, up to 5 for a 1v5.
    pub opponents: u32,
    pub start_tick: i32,
    /// Enemies killed during the clutch.
    pub kills: u32,
    /// The clutcher's team won the round.
    pub won: bool,
    /// The clutcher was alive when the round ended.
    pub survived: bool,
}

/// A living player, as the clutch tracker knows them.
#[derive(Debug, Clone)]
pub(crate) struct AlivePlayer {
    pub key: u64,
    pub steam_id: u64,
    pub name: String,
    pub side: Team,
}

/// Follows who is alive on each side during a round and opens a clutch when
/// a side is down to one player while the other still has someone.
#[derive(Debug, Default)]
pub(crate) struct ClutchTracker {
    alive: Vec<AlivePlayer>,
    seeded: bool,
    /// Clutches of the current round, with the clutcher's player key.
    clutches: Vec<(u64, Clutch)>,
}

impl ClutchTracker {
    pub fn reset(&mut self) {
        self.alive.clear();
        self.seeded = false;
        self.clutches.clear();
    }

    pub fn is_seeded(&self) -> bool {
        self.seeded
    }

    /// Sets who is alive (at freeze end, or at the first death of a round
    /// whose start the demo doesn't contain).
    pub fn seed(&mut self, alive: Vec<AlivePlayer>) {
        self.alive = alive;
        self.seeded = true;
    }

    /// A player died. `killer` is the enemy credited with the kill, if any.
    pub fn on_death(&mut self, round: u32, tick: i32, victim: u64, killer: Option<u64>) {
        if !self.seeded {
            return;
        }
        if let Some(k) = killer {
            for (key, c) in &mut self.clutches {
                if *key == k {
                    c.kills += 1;
                }
            }
        }
        self.alive.retain(|a| a.key != victim);
        for side in [Team::Terrorist, Team::CounterTerrorist] {
            let mut mine = self.alive.iter().filter(|a| a.side == side);
            let (Some(last), None) = (mine.next(), mine.next()) else {
                continue;
            };
            let opponents = self.alive.iter().filter(|a| a.side == side.opponent()).count() as u32;
            if opponents == 0 || self.clutches.iter().any(|(_, c)| c.side == side) {
                continue;
            }
            self.clutches.push((
                last.key,
                Clutch {
                    round,
                    steam_id: last.steam_id,
                    name: last.name.clone(),
                    side,
                    opponents,
                    start_tick: tick,
                    kills: 0,
                    won: false,
                    survived: false,
                },
            ));
        }
    }

    /// Records the round's result in its clutches.
    pub fn on_round_end(&mut self, winner: Option<Team>) {
        for (key, c) in &mut self.clutches {
            c.won = winner == Some(c.side);
            c.survived = self.alive.iter().any(|a| a.key == *key);
        }
    }

    /// The round's clutches, with each clutcher's player key.
    pub fn take(&mut self) -> Vec<(u64, Clutch)> {
        std::mem::take(&mut self.clutches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(key: u64, side: Team) -> AlivePlayer {
        AlivePlayer {
            key,
            steam_id: key,
            name: format!("p{key}"),
            side,
        }
    }

    fn five_v_five() -> ClutchTracker {
        let mut t = ClutchTracker::default();
        let mut alive: Vec<_> = (1..=5).map(|k| p(k, Team::Terrorist)).collect();
        alive.extend((6..=10).map(|k| p(k, Team::CounterTerrorist)));
        t.seed(alive);
        t
    }

    #[test]
    fn one_v_four_won() {
        let mut t = five_v_five();
        // T player 5 is left alone against four CTs.
        for (victim, killer) in [(1, 6), (2, 6), (6, 3), (3, 7), (4, 8)] {
            t.on_death(3, 100, victim, Some(killer));
        }
        // Player 5 kills all three remaining CTs.
        for victim in [7, 8, 9] {
            t.on_death(3, 200, victim, Some(5));
        }
        t.on_death(3, 300, 10, Some(5));
        t.on_round_end(Some(Team::Terrorist));
        let c = t.take();
        assert_eq!(c.len(), 2, "{c:?}"); // player 5's 1v4, and CT 10's 1v1
        let (_, five) = c.iter().find(|(k, _)| *k == 5).unwrap();
        assert_eq!(
            (five.opponents, five.kills, five.won, five.survived),
            (4, 4, true, true)
        );
        let (_, ten) = c.iter().find(|(k, _)| *k == 10).unwrap();
        assert_eq!((ten.opponents, ten.won, ten.survived), (1, false, false));
    }

    #[test]
    fn both_sides_can_clutch() {
        let mut t = five_v_five();
        // Trades down to T 5 against CT 9 and 10 (a 1v2), then 5 kills 9 (CT 10's 1v1).
        for (victim, killer) in [(1, 6), (6, 2), (2, 7), (7, 3), (3, 8), (8, 4), (4, 9), (9, 5)] {
            t.on_death(1, 0, victim, Some(killer));
        }
        let c: Vec<(u64, u32, u32)> = t
            .take()
            .into_iter()
            .map(|(k, c)| (k, c.opponents, c.kills))
            .collect();
        assert_eq!(c, vec![(5, 2, 1), (10, 1, 0)]);
    }

    #[test]
    fn unseeded_tracker_ignores_deaths() {
        let mut t = ClutchTracker::default();
        t.on_death(1, 0, 1, None);
        assert!(t.take().is_empty());
    }
}
