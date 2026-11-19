//! A ready-made analyzer: per-player scoreboard, per-round summaries and a kill feed.
//!
//! ```no_run
//! let demo = cs2_demo::Demo::open("match.dem")?;
//! let stats = demo.match_stats()?;
//! for p in &stats.players {
//!     println!("{:16} {:>2}-{:>2}-{:>2}  ADR {:.1}  KAST {:.0}%  rating {:.2}",
//!         p.name, p.kills, p.deaths, p.assists, p.adr(), p.kast() * 100.0, p.rating());
//! }
//! # Ok::<(), cs2_demo::Error>(())
//! ```
//!
//! Definitions:
//! * Kills exclude team kills and suicides; deaths include them.
//! * Damage (ADR) is health damage to enemies, capped at the victim's remaining health.
//! * A death is *traded* when the killer is killed by a teammate of the victim
//!   within [`TRADE_WINDOW_SECONDS`].
//! * KAST: rounds with a Kill, Assist, Survival or Trade.
//! * Rating is HLTV Rating 1.0 (the published formula).
//! * Enemies flashed come from `player_blind` events. Some tournament servers
//!   don't record those; then a blind is a victim's flash duration rising when
//!   a flashbang detonates, credited to the detonation nearest to the victim.
//! * Only rounds after the last `begin_new_match` (end of warmup / restarts) count.

use crate::entities::{Entity, EntityOp, FieldPath};
use crate::events::GameEvent;
use crate::model::{RoundEndReason, Team};
use crate::parser::{Context, Visitor};
use rustc_hash::FxHashMap;
use std::ops::ControlFlow;

pub const TRADE_WINDOW_SECONDS: f32 = 5.0;

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct PlayerStats {
    pub steam_id: u64,
    pub name: String,
    /// Team side at the end of the match.
    pub team: Option<Team>,
    /// Clan / team name at the end of the match ("Vitality").
    pub team_name: String,
    pub rounds_played: u32,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub flash_assists: u32,
    pub headshot_kills: u32,
    pub wallbang_kills: u32,
    pub noscope_kills: u32,
    pub through_smoke_kills: u32,
    pub team_kills: u32,
    pub suicides: u32,
    /// Health damage dealt to enemies (capped at remaining health).
    pub damage: u32,
    /// Part of `damage` dealt with HE grenades and fire.
    pub utility_damage: u32,
    pub opening_kills: u32,
    pub opening_deaths: u32,
    pub trade_kills: u32,
    pub traded_deaths: u32,
    pub kast_rounds: u32,
    /// `multi_kills[n]` = rounds with exactly n kills (0 = rounds without a kill, 5 = ace).
    pub multi_kills: [u32; 6],
    pub enemies_flashed: u32,
    pub teammates_flashed: u32,
    /// Total enemy blind time caused, in seconds.
    pub enemy_flash_duration: f32,
    pub flashbangs_thrown: u32,
    pub smokes_thrown: u32,
    pub he_thrown: u32,
    pub molotovs_thrown: u32,
    pub bomb_plants: u32,
    pub bomb_defuses: u32,
    pub mvps: i32,
    pub score: i32,
}

impl PlayerStats {
    fn per_round(&self, v: f32) -> f32 {
        if self.rounds_played == 0 {
            0.0
        } else {
            v / self.rounds_played as f32
        }
    }

    /// Average damage per round.
    pub fn adr(&self) -> f32 {
        self.per_round(self.damage as f32)
    }

    pub fn kpr(&self) -> f32 {
        self.per_round(self.kills as f32)
    }

    pub fn kd(&self) -> f32 {
        if self.deaths == 0 {
            self.kills as f32
        } else {
            self.kills as f32 / self.deaths as f32
        }
    }

    /// Headshot kill percentage in [0, 1].
    pub fn hs_ratio(&self) -> f32 {
        if self.kills == 0 {
            0.0
        } else {
            self.headshot_kills as f32 / self.kills as f32
        }
    }

    /// KAST in [0, 1].
    pub fn kast(&self) -> f32 {
        self.per_round(self.kast_rounds as f32)
    }

    /// HLTV Rating 1.0.
    pub fn rating(&self) -> f32 {
        if self.rounds_played == 0 {
            return 0.0;
        }
        let r = self.rounds_played as f32;
        let kill = self.kills as f32 / r / 0.679;
        let survival = (r - self.deaths as f32) / r / 0.317;
        let m = &self.multi_kills;
        let rmk = (m[1] + 4 * m[2] + 9 * m[3] + 16 * m[4] + 25 * m[5]) as f32 / r / 1.277;
        (kill + 0.7 * survival + rmk) / 2.7
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Kill {
    pub tick: i32,
    /// 1-based round number.
    pub round: u32,
    pub attacker: Option<u64>,
    pub attacker_name: Option<String>,
    pub victim: u64,
    pub victim_name: String,
    pub assister: Option<u64>,
    pub weapon: String,
    pub headshot: bool,
    pub wallbang: bool,
    pub noscope: bool,
    pub through_smoke: bool,
    pub attacker_blind: bool,
    pub flash_assist: bool,
    pub team_kill: bool,
    pub traded: bool,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct RoundSummary {
    /// 1-based.
    pub number: u32,
    pub start_tick: i32,
    pub end_tick: Option<i32>,
    pub winner: Option<Team>,
    pub winner_name: Option<String>,
    pub reason: Option<RoundEndReason>,
    pub bomb_planted: bool,
    pub bomb_defused: bool,
    pub bomb_exploded: bool,
    pub kills: u32,
    /// Scores after the round: (T, CT).
    pub score_t: i32,
    pub score_ct: i32,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct TeamStats {
    pub team: Team,
    pub name: String,
    pub score: i32,
    pub score_first_half: i32,
    pub score_second_half: i32,
    pub score_overtime: i32,
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct MatchStats {
    pub map: String,
    pub teams: Vec<TeamStats>,
    /// Sorted by team, then rating (best first).
    pub players: Vec<PlayerStats>,
    pub rounds: Vec<RoundSummary>,
    pub kills: Vec<Kill>,
}

impl MatchStats {
    pub fn player(&self, steam_id: u64) -> Option<&PlayerStats> {
        self.players.iter().find(|p| p.steam_id == steam_id)
    }

    pub fn winner(&self) -> Option<&TeamStats> {
        let mut t = self.teams.iter().collect::<Vec<_>>();
        t.sort_by_key(|t| -t.score);
        match t.as_slice() {
            [a, b, ..] if a.score > b.score => Some(a),
            _ => None,
        }
    }
}

/// Players one thrower blinded.
#[derive(Default, Clone, Copy)]
struct FlashTally {
    enemies: u32,
    teammates: u32,
    enemy_duration: f32,
}

impl FlashTally {
    fn add(&mut self, teammate: bool, duration: f32) {
        if teammate {
            self.teammates += 1;
        } else {
            self.enemies += 1;
            self.enemy_duration += duration;
        }
    }
}

/// Per-round, per-player scratch state.
#[derive(Default, Clone)]
struct RoundPlayer {
    kills: u32,
    assisted: bool,
    died: bool,
    traded: bool,
    hp: i32,
}

/// The [`Visitor`] that builds [`MatchStats`]. Use [`crate::Demo::match_stats`]
/// for the common case, or drive it yourself next to other visitors. When
/// wrapping it in your own visitor, forward every callback: it uses game
/// events, entity updates and tick ends.
#[derive(Default)]
pub struct StatsCollector {
    players: FxHashMap<u64, PlayerStats>,
    rounds: Vec<RoundSummary>,
    kills: Vec<Kill>,
    /// Kill indices (into `kills`) of the current round, for trades.
    round_kills: Vec<usize>,
    round_players: FxHashMap<u64, RoundPlayer>,
    in_round: bool,
    match_over: bool,
    tick_rate: f32,
    /// Blinds per thrower, from `player_blind` events.
    blinds_from_events: FxHashMap<u64, FlashTally>,
    /// Blinds per thrower, from pawns' flash duration rising when a flashbang
    /// detonates. Used for demos without `player_blind` events (some tournament
    /// servers don't record them).
    blinds_from_entities: FxHashMap<u64, FlashTally>,
    blind_events_seen: bool,
    /// Flashbang detonations of the last few ticks: (tick, thrower slot, position).
    recent_flashes: Vec<(i32, i32, [f32; 3])>,
    /// Pawns whose flash duration rose this tick: (pawn handle, duration).
    pending_blinds: Vec<(u32, f32)>,
    /// `m_flFlashDuration` of `CCSPlayerPawn`, resolved on first use.
    flash_field: Option<Option<FieldPath>>,
}

/// Identifies a player across the match: SteamID, or a synthetic id for bots.
fn player_key(steam_id: u64, slot: i32) -> u64 {
    if steam_id != 0 {
        steam_id
    } else {
        (1 << 63) | slot as u64
    }
}

impl StatsCollector {
    pub fn new() -> Self {
        Self::default()
    }

    fn live(ctx: &Context) -> bool {
        ctx.game_rules().is_some_and(|g| !g.is_warmup())
    }

    fn reset(&mut self) {
        *self = StatsCollector {
            tick_rate: self.tick_rate,
            blind_events_seen: self.blind_events_seen,
            flash_field: self.flash_field,
            ..Default::default()
        };
    }

    fn entry(&mut self, ctx: &Context, slot: i32) -> Option<(u64, Team, &mut PlayerStats)> {
        let p = ctx.player_by_slot(slot)?;
        let key = player_key(p.steam_id(), slot);
        let team = p.team();
        let e = self.players.entry(key).or_insert_with(|| PlayerStats {
            steam_id: p.steam_id(),
            ..Default::default()
        });
        e.name = p.name();
        Some((key, team, e))
    }

    /// A round that started but never ended (restart, demo cut) does not count
    /// towards rounds played.
    fn discard_unfinished_round(&mut self) {
        if self.in_round && self.rounds.last().is_some_and(|r| r.end_tick.is_none()) {
            self.rounds.pop();
            self.round_players.clear();
            self.round_kills.clear();
            self.in_round = false;
        }
    }

    fn start_round(&mut self, ctx: &Context) {
        self.discard_unfinished_round();
        self.close_round();
        self.in_round = true;
        self.round_kills.clear();
        self.round_players.clear();
        for p in ctx.active_players() {
            self.round_players.insert(
                player_key(p.steam_id(), p.slot()),
                RoundPlayer {
                    hp: 100,
                    ..Default::default()
                },
            );
        }
        self.rounds.push(RoundSummary {
            // The game's own round counter keeps numbering right for demos that
            // start mid-match.
            number: ctx
                .game_rules()
                .map(|g| g.total_rounds_played() as u32 + 1)
                .unwrap_or(self.rounds.len() as u32 + 1),
            start_tick: ctx.tick(),
            end_tick: None,
            winner: None,
            winner_name: None,
            reason: None,
            bomb_planted: false,
            bomb_defused: false,
            bomb_exploded: false,
            kills: 0,
            score_t: 0,
            score_ct: 0,
        });
    }

    /// Records the result of the current round. Events keep counting towards it
    /// (exit frags) until the next round starts.
    fn record_round_end(&mut self, ctx: &Context, ev: &GameEvent) {
        if let Some(round) = self.rounds.last_mut().filter(|r| r.end_tick.is_none()) {
            round.end_tick = Some(ctx.tick());
            let winner = Team::from_num(ev.i32("winner").unwrap_or(0) as u32);
            if winner.is_playing() {
                round.winner = Some(winner);
                round.winner_name = ctx.team(winner).map(|t| t.name());
            }
            round.reason = ev.i32("reason").map(RoundEndReason::from_num);
            let score = |t| ctx.team(t).map_or(0, |t| t.score());
            round.score_t = score(Team::Terrorist);
            round.score_ct = score(Team::CounterTerrorist);
            // Round-end events can be ahead of the team score update by a tick.
            match winner {
                Team::Terrorist if round.score_t + round.score_ct < round.number as i32 => round.score_t += 1,
                Team::CounterTerrorist if round.score_t + round.score_ct < round.number as i32 => {
                    round.score_ct += 1
                }
                _ => {}
            }
        }
    }

    /// Closes the current round: KAST, multi-kills, rounds played.
    fn close_round(&mut self) {
        if !self.in_round {
            return;
        }
        for (key, rp) in self.round_players.drain() {
            if let Some(ps) = self.players.get_mut(&key) {
                ps.rounds_played += 1;
                if rp.kills > 0 || rp.assisted || !rp.died || rp.traded {
                    ps.kast_rounds += 1;
                }
                ps.multi_kills[rp.kills.min(5) as usize] += 1;
            } else {
                // Player never appeared in an event; still count the round.
                let ps = self.players.entry(key).or_insert_with(|| PlayerStats {
                    steam_id: if key >> 63 == 1 { 0 } else { key },
                    ..Default::default()
                });
                ps.rounds_played += 1;
                if !rp.died {
                    ps.kast_rounds += 1;
                }
                ps.multi_kills[0] += 1;
            }
        }
        self.in_round = false;
    }

    fn on_death(&mut self, ctx: &Context, ev: &GameEvent) {
        let Some(victim_slot) = ev.player_slot("userid") else {
            return;
        };
        let Some((victim_key, victim_team, v)) = self.entry(ctx, victim_slot) else {
            return;
        };
        v.deaths += 1;
        let victim_name = v.name.clone();
        let victim_steam = v.steam_id;

        let attacker = ev
            .player_slot("attacker")
            .filter(|&s| s != victim_slot)
            .and_then(|s| {
                self.entry(ctx, s)
                    .map(|(k, t, a)| (k, t, a.name.clone(), a.steam_id))
            });
        let team_kill = attacker.as_ref().is_some_and(|(_, t, ..)| *t == victim_team);
        let headshot = ev.bool("headshot").unwrap_or(false);
        let wallbang = ev.i32("penetrated").unwrap_or(0) > 0;
        let noscope = ev.bool("noscope").unwrap_or(false);
        let through_smoke = ev.bool("thrusmoke").unwrap_or(false);
        let flash_assist = ev.bool("assistedflash").unwrap_or(false);

        let first_kill_of_round = self.round_kills.iter().all(|&i| self.kills[i].team_kill);
        match &attacker {
            None => {
                if ev.player_slot("attacker") == Some(victim_slot) || ev.player_slot("attacker").is_none() {
                    self.players.get_mut(&victim_key).unwrap().suicides += 1;
                }
            }
            Some((akey, ..)) if team_kill => self.players.get_mut(akey).unwrap().team_kills += 1,
            Some((akey, ..)) => {
                let a = self.players.get_mut(akey).unwrap();
                a.kills += 1;
                a.headshot_kills += headshot as u32;
                a.wallbang_kills += wallbang as u32;
                a.noscope_kills += noscope as u32;
                a.through_smoke_kills += through_smoke as u32;
                if first_kill_of_round && self.in_round {
                    a.opening_kills += 1;
                    self.players.get_mut(&victim_key).unwrap().opening_deaths += 1;
                }
                self.round_players.entry(*akey).or_default().kills += 1;
            }
        }

        let mut assister_steam = None;
        if let Some(slot) = ev.player_slot("assister")
            && let Some((key, team, a)) = self.entry(ctx, slot)
            && team != victim_team
        {
            a.assists += 1;
            a.flash_assists += flash_assist as u32;
            assister_steam = Some(a.steam_id);
            self.round_players.entry(key).or_default().assisted = true;
        }
        if let Some(rp) = self.round_players.get_mut(&victim_key) {
            rp.died = true;
            rp.hp = 0;
        }

        // Trades: the victim had killed one of the attacker's teammates recently.
        let window = (TRADE_WINDOW_SECONDS * self.tick_rate) as i32;
        let mut traded_victims = Vec::new();
        if let Some((akey, ..)) = &attacker
            && !team_kill
        {
            for &i in &self.round_kills {
                let k = &self.kills[i];
                if !k.team_kill && !k.traded && k.attacker == Some(victim_steam) && ev.tick - k.tick <= window
                {
                    traded_victims.push(i);
                }
            }
            if !traded_victims.is_empty() {
                self.players.get_mut(akey).unwrap().trade_kills += 1;
            }
        }
        for i in traded_victims {
            self.kills[i].traded = true;
            let victim = self.kills[i].victim;
            let key = self.find_key(victim);
            if let Some(p) = self.players.get_mut(&key) {
                p.traded_deaths += 1;
            }
            if let Some(rp) = self.round_players.get_mut(&key) {
                rp.traded = true;
            }
        }

        let round = self.rounds.last().map_or(0, |r| r.number);
        if let Some(r) = self.rounds.last_mut() {
            r.kills += 1;
        }
        self.round_kills.push(self.kills.len());
        self.kills.push(Kill {
            tick: ev.tick,
            round,
            attacker: attacker.as_ref().map(|a| a.3),
            attacker_name: attacker.as_ref().map(|a| a.2.clone()),
            victim: victim_steam,
            victim_name,
            assister: assister_steam,
            weapon: ev.str("weapon").unwrap_or("").to_string(),
            headshot,
            wallbang,
            noscope,
            through_smoke,
            attacker_blind: ev.bool("attackerblind").unwrap_or(false),
            flash_assist,
            team_kill,
            traded: false,
        });
    }

    fn find_key(&self, steam_id: u64) -> u64 {
        if steam_id != 0 {
            return steam_id;
        }
        self.players.keys().copied().find(|k| k >> 63 == 1).unwrap_or(0)
    }

    fn on_hurt(&mut self, ctx: &Context, ev: &GameEvent) {
        let (Some(vs), Some(as_)) = (ev.player_slot("userid"), ev.player_slot("attacker")) else {
            return;
        };
        if vs == as_ {
            return;
        }
        let Some((vkey, vteam, _)) = self.entry(ctx, vs) else {
            return;
        };
        let remaining = ev.i32("health").unwrap_or(0).max(0);
        let dmg = ev.i32("dmg_health").unwrap_or(0).max(0);
        let before = self.round_players.get(&vkey).map_or(remaining + dmg, |rp| rp.hp);
        let real = dmg.min(before.max(0));
        if let Some(rp) = self.round_players.get_mut(&vkey) {
            rp.hp = remaining;
        }
        let Some((_, ateam, a)) = self.entry(ctx, as_) else {
            return;
        };
        if ateam == vteam {
            return;
        }
        a.damage += real as u32;
        if matches!(
            ev.str("weapon"),
            Some("hegrenade" | "inferno" | "molotov" | "incgrenade")
        ) {
            a.utility_damage += real as u32;
        }
    }

    fn on_blind(&mut self, ctx: &Context, ev: &GameEvent) {
        let (Some(vs), Some(as_)) = (ev.player_slot("userid"), ev.player_slot("attacker")) else {
            return;
        };
        let duration = ev.f32("blind_duration").unwrap_or(0.0);
        if vs == as_ || duration <= 0.0 {
            return;
        }
        let Some(vteam) = ctx.player_by_slot(vs).map(|p| p.team()) else {
            return;
        };
        if !vteam.is_playing() {
            return; // spectators / casters
        }
        let Some((key, ateam, _)) = self.entry(ctx, as_) else {
            return;
        };
        self.blinds_from_events
            .entry(key)
            .or_default()
            .add(ateam == vteam, duration);
    }

    /// Credits the blinds seen this tick to the flashbang that detonated
    /// nearest to each victim.
    fn resolve_entity_blinds(&mut self, ctx: &Context) {
        let tick = ctx.tick();
        for (handle, duration) in std::mem::take(&mut self.pending_blinds) {
            let Some(victim) = ctx.player_by_pawn_handle(handle) else {
                continue;
            };
            let vteam = victim.team();
            if !vteam.is_playing() {
                continue;
            }
            let vpos = victim.position().unwrap_or_default();
            let dist = |p: &[f32; 3]| (0..3).map(|i| (p[i] - vpos[i]).powi(2)).sum::<f32>();
            let Some(&(_, slot, _)) = self
                .recent_flashes
                .iter()
                .filter(|f| tick - f.0 <= 2)
                .min_by(|a, b| dist(&a.2).total_cmp(&dist(&b.2)))
            else {
                continue;
            };
            if slot == victim.slot() {
                continue; // own flash
            }
            let Some((key, ateam, _)) = self.entry(ctx, slot) else {
                continue;
            };
            self.blinds_from_entities
                .entry(key)
                .or_default()
                .add(ateam == vteam, duration);
        }
    }

    fn finish(&mut self, ctx: &Context) -> MatchStats {
        self.discard_unfinished_round();
        self.close_round();
        let blinds = if self.blind_events_seen {
            &self.blinds_from_events
        } else {
            &self.blinds_from_entities
        };
        for (key, t) in blinds {
            if let Some(p) = self.players.get_mut(key) {
                p.enemies_flashed = t.enemies;
                p.teammates_flashed = t.teammates;
                p.enemy_flash_duration = t.enemy_duration;
            }
        }
        let mut players: Vec<PlayerStats> = std::mem::take(&mut self.players).into_values().collect();
        for p in &mut players {
            let live = if p.steam_id != 0 {
                ctx.player_by_steam_id(p.steam_id)
            } else {
                ctx.players().find(|c| c.name() == p.name)
            };
            if let Some(c) = live {
                let team = c.team();
                p.team = Some(team);
                p.team_name = ctx.team(team).map(|t| t.name()).unwrap_or_default();
                p.mvps = c.mvps();
                p.score = c.score();
                if p.name.is_empty() {
                    p.name = c.name();
                }
            }
        }
        players.retain(|p| p.rounds_played > 0 || p.kills > 0 || p.deaths > 0);
        players.sort_by(|a, b| {
            a.team.cmp(&b.team).then(
                b.rating()
                    .partial_cmp(&a.rating())
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        });
        let teams = ctx
            .teams()
            .filter(|t| t.team().is_playing())
            .map(|t| TeamStats {
                team: t.team(),
                name: t.name(),
                score: t.score(),
                score_first_half: t.score_first_half(),
                score_second_half: t.score_second_half(),
                score_overtime: t.score_overtime(),
            })
            .collect();
        MatchStats {
            map: ctx.header().map_name.clone(),
            teams,
            players,
            rounds: std::mem::take(&mut self.rounds),
            kills: std::mem::take(&mut self.kills),
        }
    }

    /// Consumes the collector after parsing.
    pub fn into_stats(mut self, ctx: &Context) -> MatchStats {
        self.finish(ctx)
    }
}

impl Visitor for StatsCollector {
    fn on_start(&mut self, ctx: &Context) {
        self.tick_rate = ctx.header().tick_rate();
    }

    fn on_game_event(&mut self, ctx: &Context, ev: &GameEvent) {
        if &*ev.name == "player_blind" {
            self.blind_events_seen = true;
        }
        // Demos recorded after the match started (or continuation files of a
        // split match) begin mid-round, with no begin_new_match / round_start:
        // open the round on its first round-scoped event.
        if !self.in_round
            && !self.match_over
            && matches!(
                &*ev.name,
                "round_freeze_end"
                    | "round_announce_match_start"
                    | "player_death"
                    | "player_hurt"
                    | "player_blind"
                    | "grenade_thrown"
                    | "weapon_fire"
                    | "bomb_planted"
                    | "bomb_defused"
                    | "bomb_exploded"
                    | "round_end"
            )
            && Self::live(ctx)
        {
            self.start_round(ctx);
        }
        match &*ev.name {
            // The match (re)starts with a fresh round. In recent demos this event
            // arrives a tick *after* that round's start, so open the round here;
            // a round_start right after replaces it (unfinished rounds are dropped).
            "begin_new_match" => {
                self.reset();
                self.start_round(ctx);
            }
            // Nothing after the end-of-match screen counts (players often
            // suicide or leave there).
            "cs_win_panel_match" => self.match_over = true,
            _ if self.match_over => {}
            "round_start" if Self::live(ctx) => self.start_round(ctx),
            "round_end" if self.in_round => self.record_round_end(ctx, ev),
            _ if !self.in_round => {}
            "player_death" => self.on_death(ctx, ev),
            "player_hurt" => self.on_hurt(ctx, ev),
            "player_blind" => self.on_blind(ctx, ev),
            "flashbang_detonate" => {
                if let Some(slot) = ev.player_slot("userid") {
                    let pos = ["x", "y", "z"].map(|k| ev.f32(k).unwrap_or(0.0));
                    self.recent_flashes.push((ctx.tick(), slot, pos));
                }
            }
            "player_spawn" => {
                if let Some(slot) = ev.player_slot("userid")
                    && let Some(p) = ctx.player_by_slot(slot)
                    && let Some(rp) = self.round_players.get_mut(&player_key(p.steam_id(), slot))
                {
                    rp.hp = 100;
                }
            }
            "grenade_thrown" => {
                if let Some((_, _, p)) = ev.player_slot("userid").and_then(|s| self.entry(ctx, s)) {
                    match ev.str("weapon") {
                        Some("flashbang") => p.flashbangs_thrown += 1,
                        Some("smokegrenade") => p.smokes_thrown += 1,
                        Some("hegrenade") => p.he_thrown += 1,
                        Some("molotov" | "incgrenade") => p.molotovs_thrown += 1,
                        _ => {}
                    }
                }
            }
            "bomb_planted" => {
                if let Some(r) = self.rounds.last_mut() {
                    r.bomb_planted = true;
                }
                if let Some((_, _, p)) = ev.player_slot("userid").and_then(|s| self.entry(ctx, s)) {
                    p.bomb_plants += 1;
                }
            }
            "bomb_defused" => {
                if let Some(r) = self.rounds.last_mut() {
                    r.bomb_defused = true;
                }
                if let Some((_, _, p)) = ev.player_slot("userid").and_then(|s| self.entry(ctx, s)) {
                    p.bomb_defuses += 1;
                }
            }
            "bomb_exploded" => {
                if let Some(r) = self.rounds.last_mut() {
                    r.bomb_exploded = true;
                }
            }
            _ => {}
        }
    }

    fn on_entity(&mut self, _ctx: &Context, e: &Entity, op: EntityOp, changed: &[FieldPath]) {
        if op != EntityOp::Updated || !self.in_round || self.match_over || e.class_name() != "CCSPlayerPawn" {
            return;
        }
        let field = *self
            .flash_field
            .get_or_insert_with(|| e.class().prop_path("m_flFlashDuration"));
        if let Some(field) = field
            && changed.contains(&field)
            && let Some(duration) = e.value_at(&field).and_then(|v| v.as_f32())
            && duration > 0.0
        {
            self.pending_blinds.push((e.handle(), duration));
        }
    }

    fn on_tick_end(&mut self, ctx: &Context) -> ControlFlow<()> {
        if !self.pending_blinds.is_empty() {
            self.resolve_entity_blinds(ctx);
        }
        if !self.recent_flashes.is_empty() {
            let tick = ctx.tick();
            self.recent_flashes.retain(|f| tick - f.0 <= 2);
        }
        ControlFlow::Continue(())
    }
}
