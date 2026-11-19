//! Scouting: what a team or a player does across many matches.
//!
//! ```no_run
//! use cs2_demo::scouting::Scouting;
//!
//! let paths = ["a.dem", "b.dem", "c.dem"];
//! let mut scouting = Scouting::new();
//! for stats in cs2_demo::match_stats_many(&paths) {
//!     scouting.add(stats?);
//! }
//! let vitality = scouting.team("Vitality").expect("played in these demos");
//! for map in &vitality.maps {
//!     println!("{}: won {} of {}, T rounds {:.0}%, pistols {}/{}",
//!         map.map, map.wins, map.matches, map.t.rate() * 100.0, map.pistols.won, map.pistols.played);
//! }
//! # Ok::<(), cs2_demo::Error>(())
//! ```
//!
//! Teams are matched by name, ignoring case, spaces and punctuation and a
//! leading "Team" ("Team Vitality" = "vitality"). Use [`Scouting::alias`] when a
//! team appears under different names.

use crate::model::Team;
use crate::stats::{BombSite, BuyType, Grenade, Lineup, MatchStats, PlayerStats, RoundSummary, lineups};
use std::collections::BTreeMap;

/// Rounds (or matches) played and won.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Record {
    pub played: u32,
    pub won: u32,
}

impl Record {
    fn add(&mut self, won: bool) {
        self.played += 1;
        self.won += won as u32;
    }

    /// Share won, in [0, 1].
    pub fn rate(&self) -> f32 {
        if self.played == 0 {
            0.0
        } else {
            self.won as f32 / self.played as f32
        }
    }
}

/// Rounds played and won with one kind of buy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BuyRecord {
    pub buy: BuyType,
    pub record: Record,
}

/// Something that happened, and how often.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Frequency {
    pub what: String,
    pub count: u32,
}

fn top(counts: BTreeMap<String, u32>, n: usize) -> Vec<Frequency> {
    let mut v: Vec<Frequency> = counts
        .into_iter()
        .map(|(what, count)| Frequency { what, count })
        .collect();
    v.sort_by(|a, b| b.count.cmp(&a.count).then(a.what.cmp(&b.what)));
    v.truncate(n);
    v
}

/// A team on one map.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MapProfile {
    pub map: String,
    pub matches: u32,
    pub wins: u32,
    /// Rounds on each side.
    pub t: Record,
    pub ct: Record,
    pub pistols: Record,
    /// Rounds by the team's own buy.
    pub buys: Vec<BuyRecord>,
    /// Rounds by the opponent's buy.
    pub against_buys: Vec<BuyRecord>,
    /// Bombs planted on each site (T side).
    pub plants_a: u32,
    pub plants_b: u32,
    /// Average seconds from the end of freeze time to the plant.
    pub avg_plant_time: Option<f32>,
    /// Where the team stood [`crate::stats::SETUP_SECONDS`] into CT rounds:
    /// players per callout, most frequent first.
    pub ct_setups: Vec<Frequency>,
    /// The same for T rounds.
    pub t_setups: Vec<Frequency>,
    /// First kill of the round (opening duel) by side.
    pub t_openings: Record,
    pub ct_openings: Record,
    /// Where the team's opening kills were made, and its opening deaths happened.
    pub opening_kill_places: Vec<Frequency>,
    pub opening_death_places: Vec<Frequency>,
    pub grenades_per_round: f32,
    /// Lineups the team threw more than once.
    pub lineups: Vec<Lineup>,
}

impl MapProfile {
    pub fn rounds(&self) -> Record {
        Record {
            played: self.t.played + self.ct.played,
            won: self.t.won + self.ct.won,
        }
    }

    pub fn buy(&self, buy: BuyType) -> Record {
        self.buys
            .iter()
            .find(|b| b.buy == buy)
            .map_or_else(Record::default, |b| b.record)
    }

    pub fn against_buy(&self, buy: BuyType) -> Record {
        self.against_buys
            .iter()
            .find(|b| b.buy == buy)
            .map_or_else(Record::default, |b| b.record)
    }
}

/// A team across all the matches it played.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TeamProfile {
    pub name: String,
    pub matches: u32,
    pub wins: u32,
    /// Most played first.
    pub maps: Vec<MapProfile>,
    /// Players who played for the team, most rounds first.
    pub players: Vec<PlayerProfile>,
}

impl TeamProfile {
    pub fn map(&self, map: &str) -> Option<&MapProfile> {
        self.maps.iter().find(|m| m.map.eq_ignore_ascii_case(map))
    }
}

/// One map's totals for a player.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlayerMap {
    pub map: String,
    pub matches: u32,
    pub stats: PlayerStats,
}

/// A player across matches: summed stats (rates recomputed from the sums).
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlayerProfile {
    pub steam_id: u64,
    pub name: String,
    /// Teams played for, most recent last.
    pub teams: Vec<String>,
    pub matches: u32,
    pub stats: PlayerStats,
    pub maps: Vec<PlayerMap>,
    /// Callouts where the player stood [`crate::stats::SETUP_SECONDS`] into rounds.
    pub t_places: Vec<Frequency>,
    pub ct_places: Vec<Frequency>,
}

/// Matches collected for scouting.
#[derive(Debug, Clone, Default)]
pub struct Scouting {
    matches: Vec<MatchStats>,
    /// Normalized name → normalized canonical name.
    aliases: Vec<(String, String)>,
}

/// Lowercase letters and digits only, without a leading "team".
pub fn normalize_team_name(name: &str) -> String {
    let s: String = name
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    match s.strip_prefix("team") {
        Some(rest) if !rest.is_empty() => rest.to_string(),
        _ => s,
    }
}

impl Scouting {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, stats: MatchStats) {
        self.matches.push(stats);
    }

    pub fn matches(&self) -> &[MatchStats] {
        &self.matches
    }

    /// Counts `name` as `canonical` (e.g. an academy roster, or a renamed team).
    pub fn alias(&mut self, name: &str, canonical: &str) {
        self.aliases
            .push((normalize_team_name(name), normalize_team_name(canonical)));
    }

    fn key(&self, name: &str) -> String {
        let n = normalize_team_name(name);
        self.aliases
            .iter()
            .find(|(a, _)| *a == n)
            .map_or(n, |(_, c)| c.clone())
    }

    /// Teams in the matches, most matches first.
    pub fn teams(&self) -> Vec<String> {
        let mut seen: BTreeMap<String, (String, u32)> = BTreeMap::new();
        for m in &self.matches {
            for t in &m.teams {
                let e = seen.entry(self.key(&t.name)).or_insert((t.name.clone(), 0));
                e.1 += 1;
            }
        }
        let mut v: Vec<(String, u32)> = seen.into_values().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.into_iter().map(|(n, _)| n).collect()
    }

    /// The side `team` played in a round.
    fn side_in(&self, team: &str, r: &RoundSummary) -> Option<Team> {
        if self.key(&r.t_name) == team {
            Some(Team::Terrorist)
        } else if self.key(&r.ct_name) == team {
            Some(Team::CounterTerrorist)
        } else {
            None
        }
    }

    /// Everything about one team, or `None` if it played none of the matches.
    pub fn team(&self, name: &str) -> Option<TeamProfile> {
        let key = self.key(name);
        let mut profile = TeamProfile::default();
        let mut maps: BTreeMap<String, MapAcc> = BTreeMap::new();
        let mut players: BTreeMap<u64, PlayerProfile> = BTreeMap::new();
        for m in &self.matches {
            let Some(us) = m.teams.iter().find(|t| self.key(&t.name) == key) else {
                continue;
            };
            if profile.name.is_empty() {
                profile.name = us.name.clone();
            }
            profile.matches += 1;
            let won = m.winner().is_some_and(|w| self.key(&w.name) == key);
            profile.wins += won as u32;
            let acc = maps.entry(m.map.clone()).or_default();
            acc.add_match(self, &key, m, won);
            for p in m.players.iter().filter(|p| self.key(&p.team_name) == key) {
                let e = players.entry(p.steam_id).or_default();
                add_player_match(e, p, m);
            }
        }
        if profile.matches == 0 {
            return None;
        }
        let mut maps: Vec<MapProfile> = maps.into_iter().map(|(map, acc)| acc.finish(map)).collect();
        maps.sort_by(|a, b| b.matches.cmp(&a.matches).then(a.map.cmp(&b.map)));
        profile.maps = maps;
        let mut players: Vec<PlayerProfile> = players
            .into_values()
            .map(|mut p| {
                self.add_places(&mut p);
                p
            })
            .collect();
        players.sort_by_key(|p| std::cmp::Reverse(p.stats.rounds_played));
        profile.players = players;
        Some(profile)
    }

    /// One player across all matches.
    pub fn player(&self, steam_id: u64) -> Option<PlayerProfile> {
        let mut profile = PlayerProfile::default();
        for m in &self.matches {
            if let Some(p) = m.player(steam_id) {
                add_player_match(&mut profile, p, m);
            }
        }
        if profile.matches == 0 {
            return None;
        }
        self.add_places(&mut profile);
        Some(profile)
    }

    fn add_places(&self, p: &mut PlayerProfile) {
        let mut t: BTreeMap<String, u32> = BTreeMap::new();
        let mut ct: BTreeMap<String, u32> = BTreeMap::new();
        for spot in self
            .matches
            .iter()
            .flat_map(|m| &m.rounds)
            .flat_map(|r| &r.setup)
            .filter(|s| s.steam_id == p.steam_id && !s.place.is_empty())
        {
            let counts = if spot.side == Team::Terrorist {
                &mut t
            } else {
                &mut ct
            };
            *counts.entry(spot.place.clone()).or_default() += 1;
        }
        p.t_places = top(t, 8);
        p.ct_places = top(ct, 8);
    }
}

fn add_player_match(profile: &mut PlayerProfile, p: &PlayerStats, m: &MatchStats) {
    profile.steam_id = p.steam_id;
    profile.name.clone_from(&p.name);
    if !p.team_name.is_empty() && profile.teams.last() != Some(&p.team_name) {
        profile.teams.retain(|t| *t != p.team_name);
        profile.teams.push(p.team_name.clone());
    }
    profile.matches += 1;
    profile.stats.merge(p);
    match profile.maps.iter_mut().find(|x| x.map == m.map) {
        Some(x) => {
            x.matches += 1;
            x.stats.merge(p);
        }
        None => profile.maps.push(PlayerMap {
            map: m.map.clone(),
            matches: 1,
            stats: p.clone(),
        }),
    }
}

/// A team's numbers on one map while they are being added up.
#[derive(Default)]
struct MapAcc {
    p: MapProfile,
    buys: BTreeMap<BuyType, Record>,
    against: BTreeMap<BuyType, Record>,
    plant_times: Vec<f32>,
    ct_setups: BTreeMap<String, u32>,
    t_setups: BTreeMap<String, u32>,
    kill_places: BTreeMap<String, u32>,
    death_places: BTreeMap<String, u32>,
    grenades: Vec<Grenade>,
}

impl MapAcc {
    fn add_match(&mut self, s: &Scouting, team: &str, m: &MatchStats, won: bool) {
        self.p.matches += 1;
        self.p.wins += won as u32;
        let rate = if m.tick_rate > 0.0 { m.tick_rate } else { 64.0 };
        for r in &m.rounds {
            let Some(side) = s.side_in(team, r) else { continue };
            let won = r.winner == Some(side);
            match side {
                Team::Terrorist => self.p.t.add(won),
                _ => self.p.ct.add(won),
            }
            let ours = r.economy(side);
            let theirs = r.economy(side.opponent());
            if ours.is_some_and(|e| e.buy == BuyType::Pistol) {
                self.p.pistols.add(won);
            }
            if let Some(e) = ours {
                self.buys.entry(e.buy).or_default().add(won);
            }
            if let Some(e) = theirs {
                self.against.entry(e.buy).or_default().add(won);
            }
            if side == Team::Terrorist && r.bomb_planted {
                match r.bomb_site {
                    Some(BombSite::A) => self.p.plants_a += 1,
                    Some(BombSite::B) => self.p.plants_b += 1,
                    None => {}
                }
                if let (Some(plant), Some(start)) = (r.bomb_plant_tick, r.freeze_end_tick) {
                    self.plant_times.push((plant - start) as f32 / rate);
                }
            }
            let mut places: BTreeMap<&str, u32> = BTreeMap::new();
            for spot in r.setup.iter().filter(|p| p.side == side) {
                let place = if spot.place.is_empty() { "?" } else { &spot.place };
                *places.entry(place).or_default() += 1;
            }
            if !places.is_empty() {
                let setup = places
                    .iter()
                    .map(|(p, n)| {
                        if *n > 1 {
                            format!("{p} ×{n}")
                        } else {
                            p.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let setups = if side == Team::Terrorist {
                    &mut self.t_setups
                } else {
                    &mut self.ct_setups
                };
                *setups.entry(setup).or_default() += 1;
            }
            if let Some(k) = m
                .kills
                .iter()
                .find(|k| k.round == r.number && !k.team_kill && k.attacker.is_some())
            {
                let ours = k.attacker_side == Some(side);
                match side {
                    Team::Terrorist => self.p.t_openings.add(ours),
                    _ => self.p.ct_openings.add(ours),
                }
                let (places, place) = if ours {
                    (&mut self.kill_places, &k.attacker_place)
                } else {
                    (&mut self.death_places, &k.victim_place)
                };
                if let Some(place) = place.as_ref().filter(|p| !p.is_empty()) {
                    *places.entry(place.clone()).or_default() += 1;
                }
            }
            self.grenades.extend(
                m.grenades
                    .iter()
                    .filter(|g| g.round == r.number && g.side == side)
                    .cloned(),
            );
        }
    }

    fn finish(mut self, map: String) -> MapProfile {
        let records = |m: BTreeMap<BuyType, Record>| {
            m.into_iter()
                .map(|(buy, record)| BuyRecord { buy, record })
                .collect()
        };
        self.p.map = map;
        self.p.buys = records(std::mem::take(&mut self.buys));
        self.p.against_buys = records(std::mem::take(&mut self.against));
        self.p.avg_plant_time = (!self.plant_times.is_empty())
            .then(|| self.plant_times.iter().sum::<f32>() / self.plant_times.len() as f32);
        self.p.ct_setups = top(std::mem::take(&mut self.ct_setups), 10);
        self.p.t_setups = top(std::mem::take(&mut self.t_setups), 10);
        self.p.opening_kill_places = top(std::mem::take(&mut self.kill_places), 8);
        self.p.opening_death_places = top(std::mem::take(&mut self.death_places), 8);
        let rounds = self.p.rounds().played.max(1);
        self.p.grenades_per_round =
            self.grenades.iter().filter(|g| !g.dropped).count() as f32 / rounds as f32;
        self.p.lineups = lineups(&self.grenades)
            .into_iter()
            .filter(|l| l.count() > 1)
            .take(15)
            .collect();
        self.p
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::{Kill, TeamEconomy, TeamStats};

    fn round(number: u32, t: &str, ct: &str, winner: Team, t_buy: BuyType, ct_buy: BuyType) -> RoundSummary {
        let eco = |side, buy| TeamEconomy {
            side,
            players: 5,
            money_start: 0,
            equipment_value: 0,
            cash_spent: 0,
            loss_streak: 0,
            buy,
        };
        RoundSummary {
            number,
            freeze_end_tick: Some(number as i32 * 1000),
            t_name: t.into(),
            ct_name: ct.into(),
            winner: Some(winner),
            t_economy: Some(eco(Team::Terrorist, t_buy)),
            ct_economy: Some(eco(Team::CounterTerrorist, ct_buy)),
            ..Default::default()
        }
    }

    fn game(map: &str, a_score: i32, b_score: i32, rounds: Vec<RoundSummary>) -> MatchStats {
        MatchStats {
            map: map.into(),
            tick_rate: 64.0,
            teams: vec![
                TeamStats {
                    team: Team::Terrorist,
                    name: "Team Alpha".into(),
                    score: a_score,
                    score_first_half: 0,
                    score_second_half: 0,
                    score_overtime: 0,
                },
                TeamStats {
                    team: Team::CounterTerrorist,
                    name: "Beta".into(),
                    score: b_score,
                    score_first_half: 0,
                    score_second_half: 0,
                    score_overtime: 0,
                },
            ],
            rounds,
            ..Default::default()
        }
    }

    #[test]
    fn team_profiles_add_up_matches() {
        use BuyType::*;
        use Team::{CounterTerrorist as CT, Terrorist as T};
        let mut r1 = round(1, "Team Alpha", "Beta", T, Pistol, Pistol);
        r1.bomb_planted = true;
        r1.bomb_site = Some(BombSite::B);
        r1.bomb_plant_tick = Some(1000 + 64 * 40);
        let m1 = game(
            "de_mirage",
            2,
            1,
            vec![
                r1,
                round(2, "Team Alpha", "Beta", CT, FullBuy, Eco),
                round(3, "Beta", "Team Alpha", CT, Eco, FullBuy),
            ],
        );
        let mut m1 = m1;
        m1.kills.push(Kill {
            tick: 1500,
            round: 1,
            round_time: Some(8.0),
            attacker: Some(1),
            attacker_name: Some("a".into()),
            attacker_side: Some(T),
            attacker_pos: None,
            attacker_place: Some("Palace".into()),
            victim: 2,
            victim_name: "b".into(),
            victim_side: CT,
            victim_pos: None,
            victim_place: Some("BombsiteA".into()),
            victim_weapon: None,
            distance: None,
            assister: None,
            weapon: "ak47".into(),
            headshot: true,
            wallbang: false,
            noscope: false,
            through_smoke: false,
            attacker_blind: false,
            flash_assist: false,
            team_kill: false,
            traded: false,
        });
        let m2 = game(
            "de_mirage",
            0,
            1,
            vec![round(1, "alpha", "Beta", CT, Pistol, Pistol)],
        );
        let mut s = Scouting::new();
        s.add(m1);
        s.add(m2);
        assert_eq!(s.teams(), vec!["Beta", "Team Alpha"]);
        let alpha = s.team("ALPHA").unwrap();
        assert_eq!((alpha.matches, alpha.wins), (2, 1));
        let mirage = alpha.map("de_mirage").unwrap();
        assert_eq!(mirage.t, Record { played: 3, won: 1 });
        assert_eq!(mirage.ct, Record { played: 1, won: 1 });
        assert_eq!(mirage.pistols, Record { played: 2, won: 1 });
        assert_eq!(mirage.buy(FullBuy), Record { played: 2, won: 1 });
        assert_eq!(mirage.against_buy(Eco), Record { played: 2, won: 1 });
        assert_eq!((mirage.plants_a, mirage.plants_b), (0, 1));
        assert_eq!(mirage.avg_plant_time, Some(40.0));
        assert_eq!(mirage.t_openings, Record { played: 1, won: 1 });
        assert_eq!(mirage.opening_kill_places[0].what, "Palace");
        assert!(s.team("Gamma").is_none());
        // The same team under another name.
        let mut m3 = game(
            "de_nuke",
            1,
            0,
            vec![round(1, "ALPHA-X", "Beta", T, Pistol, Pistol)],
        );
        m3.teams[0].name = "ALPHA-X".into();
        s.add(m3);
        assert_eq!(s.team("alpha").unwrap().matches, 2);
        s.alias("ALPHA-X", "Alpha");
        let alpha = s.team("alpha").unwrap();
        assert_eq!((alpha.matches, alpha.wins), (3, 2));
        assert_eq!(alpha.maps[0].map, "de_mirage");
        assert_eq!(alpha.map("de_nuke").unwrap().t, Record { played: 1, won: 1 });
    }

    #[test]
    fn names() {
        assert_eq!(normalize_team_name("Team Vitality"), "vitality");
        assert_eq!(normalize_team_name("Natus Vincere"), "natusvincere");
        assert_eq!(normalize_team_name("TEAM"), "team");
        assert_eq!(normalize_team_name("G2 Esports"), "g2esports");
    }
}
