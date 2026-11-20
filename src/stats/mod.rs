//! A ready-made analyzer: scoreboard, per-round lines, economy, clutches and a
//! kill feed with positions.
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
//! * Rating is HLTV Rating 1.0 (the published formula). [`PlayerStats::impact`]
//!   and [`PlayerStats::rating2_approx`] are community approximations of HLTV's
//!   unpublished Impact and Rating 2.0.
//! * Enemies flashed come from `player_blind` events. Some tournament servers
//!   don't record those; then a blind is a victim's flash duration rising when
//!   a flashbang detonates, credited to the detonation nearest to the victim.
//! * Economy is read when freeze time ends: start money, and the equipment each
//!   player carries then. [`BuyThresholds`] turns it into a [`BuyType`].
//! * A clutch begins when a side is down to one living player while the other
//!   side still has at least one; both players of a 1v1 are in a clutch.
//! * Only rounds after the last `begin_new_match` (end of warmup / restarts) count.

mod aim;
mod economy;
mod fields;
mod motion;
mod rounds;
mod teams;
mod utility;

pub use aim::{
    ACCURATE_SPEED_FRACTION, AimStats, CROSSHAIR_LOOKBACK_SECONDS, ENGAGEMENT_GAP_SECONDS,
    TARGET_CONE_DEGREES, WeaponStats,
};
pub use economy::{BuyThresholds, BuyType, TeamEconomy};
pub use rounds::{BombSite, Clutch, PlayerRound, PlayerSpot, SideStats};
pub use utility::{Blind, Grenade, GrenadeKind, Lineup, TRAJECTORY_TICKS, lineups};

use crate::entities::{Entity, EntityOp, FieldPath, handle_to_index};
use crate::events::GameEvent;
use crate::model::{Player, RoundEndReason, Team};
use crate::parser::{Context, EntitySkip, ParseOptions, Visitor};
use aim::{AimTracker, Hit, Shot};
use fields::{BodyPaths, ClassKinds, Kind, PawnPaths, Watched};
use motion::Motion;
use rounds::{AlivePlayer, ClutchTracker};
use rustc_hash::FxHashMap;
use std::ops::ControlFlow;
use teams::Teams;
use utility::GrenadeTracker;

pub const TRADE_WINDOW_SECONDS: f32 = 5.0;

/// [`RoundSummary::setup`] is taken this many seconds after freeze time ends.
pub const SETUP_SECONDS: f32 = 20.0;

/// Rate methods shared by [`PlayerStats`] and [`SideStats`] (same field names).
macro_rules! rate_methods {
    ($t:ty) => {
        impl $t {
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

            /// Kills per round.
            pub fn kpr(&self) -> f32 {
                self.per_round(self.kills as f32)
            }

            /// Deaths per round.
            pub fn dpr(&self) -> f32 {
                self.per_round(self.deaths as f32)
            }

            /// Assists per round.
            pub fn apr(&self) -> f32 {
                self.per_round(self.assists as f32)
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

            /// HLTV Rating 1.0, the published formula.
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

            /// Impact, as approximated by the community: `2.13·KPR + 0.42·APR − 0.41`.
            /// HLTV has not published its formula.
            pub fn impact(&self) -> f32 {
                if self.rounds_played == 0 {
                    return 0.0;
                }
                2.13 * self.kpr() + 0.42 * self.apr() - 0.41
            }

            /// An approximation of HLTV Rating 2.0 (a community regression on
            /// KAST, KPR, DPR, impact and ADR). HLTV has not published the real
            /// formula, so expect differences of a few hundredths.
            pub fn rating2_approx(&self) -> f32 {
                if self.rounds_played == 0 {
                    return 0.0;
                }
                0.0073 * self.kast() * 100.0 + 0.3591 * self.kpr() - 0.5329 * self.dpr()
                    + 0.2372 * self.impact()
                    + 0.0032 * self.adr()
                    + 0.1587
            }
        }
    };
}

#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// Rounds played as T.
    pub t: SideStats,
    /// Rounds played as CT.
    pub ct: SideStats,
    /// Clutches entered (see [`Clutch`]).
    pub clutches: u32,
    pub clutches_won: u32,
    /// Flashbangs that blinded an enemy who was then killed by the thrower's
    /// team while still blind.
    pub flashes_leading_to_kill: u32,
    /// Value of the grenades still carried at death, summed over the match:
    /// utility bought and never used.
    pub utility_unused_value: u32,
    /// Shots, hits, first-shot accuracy, movement and crosshair placement.
    pub aim: AimStats,
}

rate_methods!(PlayerStats);
rate_methods!(SideStats);

impl PlayerStats {
    /// Adds another match's numbers to these, for totals over several
    /// matches. Name and team are taken from `o` when it has them.
    pub fn merge(&mut self, o: &PlayerStats) {
        // Destructured so that a new field can't be forgotten here.
        let PlayerStats {
            steam_id,
            name,
            team,
            team_name,
            rounds_played,
            kills,
            deaths,
            assists,
            flash_assists,
            headshot_kills,
            wallbang_kills,
            noscope_kills,
            through_smoke_kills,
            team_kills,
            suicides,
            damage,
            utility_damage,
            opening_kills,
            opening_deaths,
            trade_kills,
            traded_deaths,
            kast_rounds,
            multi_kills,
            enemies_flashed,
            teammates_flashed,
            enemy_flash_duration,
            flashbangs_thrown,
            smokes_thrown,
            he_thrown,
            molotovs_thrown,
            bomb_plants,
            bomb_defuses,
            mvps,
            score,
            t,
            ct,
            clutches,
            clutches_won,
            flashes_leading_to_kill,
            utility_unused_value,
            aim,
        } = o;
        if self.steam_id == 0 {
            self.steam_id = *steam_id;
        }
        if !name.is_empty() {
            self.name.clone_from(name);
        }
        if team.is_some() {
            self.team = *team;
        }
        if !team_name.is_empty() {
            self.team_name.clone_from(team_name);
        }
        self.rounds_played += rounds_played;
        self.kills += kills;
        self.deaths += deaths;
        self.assists += assists;
        self.flash_assists += flash_assists;
        self.headshot_kills += headshot_kills;
        self.wallbang_kills += wallbang_kills;
        self.noscope_kills += noscope_kills;
        self.through_smoke_kills += through_smoke_kills;
        self.team_kills += team_kills;
        self.suicides += suicides;
        self.damage += damage;
        self.utility_damage += utility_damage;
        self.opening_kills += opening_kills;
        self.opening_deaths += opening_deaths;
        self.trade_kills += trade_kills;
        self.traded_deaths += traded_deaths;
        self.kast_rounds += kast_rounds;
        for (a, b) in self.multi_kills.iter_mut().zip(multi_kills) {
            *a += b;
        }
        self.enemies_flashed += enemies_flashed;
        self.teammates_flashed += teammates_flashed;
        self.enemy_flash_duration += enemy_flash_duration;
        self.flashbangs_thrown += flashbangs_thrown;
        self.smokes_thrown += smokes_thrown;
        self.he_thrown += he_thrown;
        self.molotovs_thrown += molotovs_thrown;
        self.bomb_plants += bomb_plants;
        self.bomb_defuses += bomb_defuses;
        self.mvps += mvps;
        self.score += score;
        self.t.merge(t);
        self.ct.merge(ct);
        self.clutches += clutches;
        self.clutches_won += clutches_won;
        self.flashes_leading_to_kill += flashes_leading_to_kill;
        self.utility_unused_value += utility_unused_value;
        self.aim.merge(aim);
    }

    /// Average enemy blind time per flashbang thrown, in seconds.
    pub fn blind_time_per_flash(&self) -> f32 {
        if self.flashbangs_thrown == 0 {
            0.0
        } else {
            self.enemy_flash_duration / self.flashbangs_thrown as f32
        }
    }

    /// Stats of one side (T or CT).
    pub fn side(&self, side: Team) -> Option<&SideStats> {
        match side {
            Team::Terrorist => Some(&self.t),
            Team::CounterTerrorist => Some(&self.ct),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Kill {
    pub tick: i32,
    /// 1-based round number.
    pub round: u32,
    /// Seconds since freeze time ended (`None` when the demo misses the round start).
    pub round_time: Option<f32>,
    pub attacker: Option<u64>,
    pub attacker_name: Option<String>,
    pub attacker_side: Option<Team>,
    pub attacker_pos: Option<[f32; 3]>,
    /// Callout name of the attacker's position.
    pub attacker_place: Option<String>,
    pub victim: u64,
    pub victim_name: String,
    pub victim_side: Team,
    pub victim_pos: Option<[f32; 3]>,
    pub victim_place: Option<String>,
    /// Weapon the victim was holding.
    pub victim_weapon: Option<String>,
    /// Attacker–victim distance in world units (1 unit ≈ 2.5 cm).
    pub distance: Option<f32>,
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

#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RoundSummary {
    /// 1-based.
    pub number: u32,
    pub start_tick: i32,
    /// When freeze time ended (`None` when the demo starts mid-round).
    pub freeze_end_tick: Option<i32>,
    pub end_tick: Option<i32>,
    /// Team names on each side this round.
    pub t_name: String,
    pub ct_name: String,
    pub winner: Option<Team>,
    pub winner_name: Option<String>,
    pub reason: Option<RoundEndReason>,
    pub bomb_planted: bool,
    pub bomb_defused: bool,
    pub bomb_exploded: bool,
    pub bomb_site: Option<BombSite>,
    pub bomb_plant_tick: Option<i32>,
    pub bomb_planter: Option<u64>,
    pub kills: u32,
    /// Scores after the round: (T, CT).
    pub score_t: i32,
    pub score_ct: i32,
    pub t_economy: Option<TeamEconomy>,
    pub ct_economy: Option<TeamEconomy>,
    /// Where each living player stood [`SETUP_SECONDS`] after freeze time
    /// (empty if the round was over by then).
    pub setup: Vec<PlayerSpot>,
    /// Every player's round, T first.
    pub players: Vec<PlayerRound>,
}

impl RoundSummary {
    pub fn team_name(&self, side: Team) -> &str {
        match side {
            Team::Terrorist => &self.t_name,
            Team::CounterTerrorist => &self.ct_name,
            _ => "",
        }
    }

    pub fn economy(&self, side: Team) -> Option<&TeamEconomy> {
        match side {
            Team::Terrorist => self.t_economy.as_ref(),
            Team::CounterTerrorist => self.ct_economy.as_ref(),
            _ => None,
        }
    }

    pub fn player(&self, steam_id: u64) -> Option<&PlayerRound> {
        self.players.iter().find(|p| p.steam_id == steam_id)
    }

    /// Ticks from the end of freeze time to `tick`.
    pub fn ticks_since_freeze_end(&self, tick: i32) -> Option<i32> {
        self.freeze_end_tick.map(|f| tick - f)
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TeamStats {
    pub team: Team,
    pub name: String,
    pub score: i32,
    pub score_first_half: i32,
    pub score_second_half: i32,
    pub score_overtime: i32,
}

#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MatchStats {
    pub map: String,
    /// Ticks per second (64 for CS2 servers).
    pub tick_rate: f32,
    pub teams: Vec<TeamStats>,
    /// Sorted by team, then rating (best first).
    pub players: Vec<PlayerStats>,
    pub rounds: Vec<RoundSummary>,
    pub kills: Vec<Kill>,
    pub clutches: Vec<Clutch>,
    /// Every grenade thrown during the rounds, in throw order.
    pub grenades: Vec<Grenade>,
}

impl MatchStats {
    /// Borrows the recorded kills for a round, in match order.
    pub fn round_kills(&self, number: u32) -> impl Iterator<Item = &Kill> {
        self.kills.iter().filter(move |k| k.round == number)
    }

    /// Borrows the grenades thrown in a round, in throw order.
    pub fn round_grenades(&self, number: u32) -> impl Iterator<Item = &Grenade> {
        self.grenades.iter().filter(move |g| g.round == number)
    }

    /// Borrows clutch attempts for a round, including unsuccessful attempts.
    pub fn round_clutches(&self, number: u32) -> impl Iterator<Item = &Clutch> {
        self.clutches.iter().filter(move |c| c.round == number)
    }

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

    pub fn round(&self, number: u32) -> Option<&RoundSummary> {
        self.rounds.iter().find(|r| r.number == number)
    }

    /// Radar calibration of the map, if built in (see [`crate::maps`]).
    pub fn map_overview(&self) -> Option<crate::maps::MapOverview> {
        crate::maps::MapOverview::builtin(&self.map)
    }

    /// The grenades grouped into lineups (see [`lineups`]).
    pub fn lineups(&self) -> Vec<Lineup> {
        lineups(&self.grenades)
    }

    /// Converts a tick count to seconds.
    pub fn seconds(&self, ticks: i32) -> f32 {
        ticks as f32
            / if self.tick_rate > 0.0 {
                self.tick_rate
            } else {
                64.0
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

/// A player's line for the current round, plus scratch state.
#[derive(Debug, Clone)]
struct RoundLine {
    line: PlayerRound,
    /// Health left, to cap damage.
    hp: i32,
}

impl RoundLine {
    fn new(steam_id: u64, side: Team) -> Self {
        RoundLine {
            line: PlayerRound {
                steam_id,
                side: side.is_playing().then_some(side),
                ..Default::default()
            },
            hp: 100,
        }
    }
}

/// The line of `key` in the current round, created if the player was not
/// there when the round started.
fn round_line(lines: &mut FxHashMap<u64, RoundLine>, key: u64, steam_id: u64, side: Team) -> &mut RoundLine {
    lines.entry(key).or_insert_with(|| RoundLine::new(steam_id, side))
}

/// Lookups that depend only on the demo, kept across match restarts.
#[derive(Default, Clone)]
struct Cache {
    tick_rate: f32,
    kinds: ClassKinds,
    pawn: Option<Option<PawnPaths>>,
    freeze_period: Option<Option<FieldPath>>,
    /// Position paths per class id (grenade projectiles).
    bodies: FxHashMap<u32, Option<BodyPaths>>,
}

/// The [`Visitor`] that builds [`MatchStats`]. Use [`crate::Demo::match_stats`]
/// for the common case, or drive it yourself next to other visitors. When
/// wrapping it in your own visitor, forward every callback: it uses game
/// events, entity updates and tick ends. Parsing with
/// [`StatsCollector::parse_options`] leaves out the entity data it doesn't use.
#[derive(Default)]
pub struct StatsCollector {
    thresholds: BuyThresholds,
    players: FxHashMap<u64, PlayerStats>,
    rounds: Vec<RoundSummary>,
    kills: Vec<Kill>,
    clutches: Vec<Clutch>,
    /// Kill indices (into `kills`) of the current round, for trades.
    round_kills: Vec<usize>,
    round_players: FxHashMap<u64, RoundLine>,
    clutch: ClutchTracker,
    in_round: bool,
    match_over: bool,
    /// Freeze time ended this tick: sample the economy at the end of the tick,
    /// once the equipment values are up to date.
    freeze_end_pending: bool,
    setup_taken: bool,
    /// Blinds per thrower, from `player_blind` events.
    blinds_from_events: FxHashMap<u64, FlashTally>,
    /// Blinds per thrower, from pawns' flash duration rising when a flashbang
    /// detonates. Used for demos without `player_blind` events (some tournament
    /// servers don't record them).
    blinds_from_entities: FxHashMap<u64, FlashTally>,
    blind_events_seen: bool,
    /// Flashbang detonations of the last few ticks: (tick, thrower slot,
    /// position, grenade).
    recent_flashes: Vec<(i32, i32, [f32; 3], Option<usize>)>,
    /// Pawns whose flash duration rose this tick: (pawn handle, duration).
    pending_blinds: Vec<(u32, f32)>,
    grenades: GrenadeTracker,
    /// `grenade_thrown` events of the last ticks: (tick, thrower key).
    throw_events: Vec<(i32, u64)>,
    /// The demo has `grenade_thrown` events (Valve matchmaking demos don't).
    grenade_events_seen: bool,
    /// Deaths of the last ticks: (tick, player key).
    recent_deaths: Vec<(i32, u64)>,
    /// Team names, for servers that don't set them.
    teams: Teams,
    aim: AimTracker,
    /// Living players of `roster_tick`: (key, side, pawn index), for shot targets.
    roster: Vec<(u64, Team, u32)>,
    roster_tick: i32,
    motion: Motion,
    cache: Cache,
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

    /// Entity data the statistics never read: chickens, the animation state
    /// of player pawns and weapons, and the controllers' tick base. Together
    /// they are about two thirds of a demo's entity data.
    pub fn entity_skip() -> EntitySkip {
        EntitySkip {
            classes: vec!["CChicken".into()],
            props: vec![
                "CBodyComponent.m_SerializePoseRecipeAG2Dynamic".into(),
                "m_pWeaponServices.m_networkAnimTiming".into(),
                "m_nTickBase".into(),
            ],
        }
    }

    /// Options for a parse that only feeds this collector: everything, minus
    /// [`StatsCollector::entity_skip`]. When other visitors share the parse,
    /// use options that keep what they need.
    pub fn parse_options() -> ParseOptions {
        ParseOptions {
            skip: Self::entity_skip(),
            ..Default::default()
        }
    }

    /// A collector that classifies buys with other thresholds.
    pub fn with_buy_thresholds(thresholds: BuyThresholds) -> Self {
        StatsCollector {
            thresholds,
            ..Default::default()
        }
    }

    fn live(ctx: &Context) -> bool {
        ctx.game_rules().is_some_and(|g| !g.is_warmup())
    }

    fn reset(&mut self) {
        *self = StatsCollector {
            thresholds: self.thresholds,
            blind_events_seen: self.blind_events_seen,
            grenade_events_seen: self.grenade_events_seen,
            cache: std::mem::take(&mut self.cache),
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

    /// Seconds since freeze time ended in the current round.
    fn round_time(&self, tick: i32) -> Option<f32> {
        let start = self.rounds.last()?.freeze_end_tick?;
        Some((tick - start) as f32 / self.cache.tick_rate.max(1.0))
    }

    /// A round that started but never ended (restart, demo cut) does not count
    /// towards rounds played.
    fn discard_unfinished_round(&mut self) {
        if self.in_round && self.rounds.last().is_some_and(|r| r.end_tick.is_none()) {
            self.rounds.pop();
            self.round_players.clear();
            self.round_kills.clear();
            self.clutch.reset();
            self.in_round = false;
        }
    }

    fn start_round(&mut self, ctx: &Context) {
        self.discard_unfinished_round();
        self.close_round();
        self.in_round = true;
        self.freeze_end_pending = false;
        self.setup_taken = false;
        self.round_kills.clear();
        self.round_players.clear();
        self.clutch.reset();
        self.motion.new_epoch();
        for p in ctx.active_players() {
            self.round_players.insert(
                player_key(p.steam_id(), p.slot()),
                RoundLine::new(p.steam_id(), p.team()),
            );
        }
        let t_name = self.teams.name(ctx, Team::Terrorist);
        let ct_name = self.teams.name(ctx, Team::CounterTerrorist);
        self.rounds.push(RoundSummary {
            // The game's own round counter keeps numbering right for demos that
            // start mid-match.
            number: ctx
                .game_rules()
                .map(|g| g.total_rounds_played() as u32 + 1)
                .unwrap_or(self.rounds.len() as u32 + 1),
            start_tick: ctx.tick(),
            t_name,
            ct_name,
            ..Default::default()
        });
    }

    /// Freeze time just ended: record each player's money and equipment, the
    /// team economies, and who is alive (for clutches).
    fn sample_freeze_end(&mut self, ctx: &Context) {
        if self
            .rounds
            .last()
            .is_none_or(|r| r.freeze_end_tick.is_some() || r.end_tick.is_some())
        {
            return;
        }
        let t_name = self.teams.name(ctx, Team::Terrorist);
        let ct_name = self.teams.name(ctx, Team::CounterTerrorist);
        let Some(round) = self.rounds.last_mut() else {
            return;
        };
        round.freeze_end_tick = Some(ctx.tick());
        round.t_name = t_name;
        round.ct_name = ct_name;
        let mut alive = Vec::new();
        for p in ctx.active_players() {
            let key = player_key(p.steam_id(), p.slot());
            let side = p.team();
            let rl = round_line(&mut self.round_players, key, p.steam_id(), side);
            rl.line.side = Some(side);
            rl.line.money_start = p.start_money();
            rl.line.equipment_value = p.freeze_end_equipment_value();
            if p.is_alive() {
                alive.push(AlivePlayer {
                    key,
                    steam_id: p.steam_id(),
                    name: p.name(),
                    side,
                });
            }
        }
        let rules = ctx.game_rules();
        let streak = |field: &str| {
            rules
                .and_then(|g| g.entity().get::<i32>(&format!("m_pGameRules.{field}")))
                .unwrap_or(0)
        };
        for (side, field) in [
            (Team::Terrorist, "m_iNumConsecutiveTerroristLoses"),
            (Team::CounterTerrorist, "m_iNumConsecutiveCTLoses"),
        ] {
            let lines = self.round_players.values().filter(|l| l.line.side == Some(side));
            let (mut players, mut money, mut max_money, mut equipment) = (0, 0, 0, 0);
            for l in lines {
                players += 1;
                money += l.line.money_start;
                max_money = max_money.max(l.line.money_start);
                equipment += l.line.equipment_value;
            }
            let eco = TeamEconomy {
                side,
                players,
                money_start: money,
                equipment_value: equipment,
                cash_spent: 0,
                loss_streak: streak(field),
                buy: self.thresholds.classify(players, max_money, equipment),
            };
            match side {
                Team::Terrorist => round.t_economy = Some(eco),
                _ => round.ct_economy = Some(eco),
            }
        }
        self.clutch.seed(alive);
    }

    /// Records where everyone stands [`SETUP_SECONDS`] into the round.
    fn take_setup(&mut self, ctx: &Context) {
        let Some(round) = self.rounds.last_mut() else {
            return;
        };
        let (Some(freeze_end), None) = (round.freeze_end_tick, round.end_tick) else {
            if round.end_tick.is_some() {
                self.setup_taken = true;
            }
            return;
        };
        if ((ctx.tick() - freeze_end) as f32) < SETUP_SECONDS * self.cache.tick_rate {
            return;
        }
        self.setup_taken = true;
        round.setup = ctx
            .active_players()
            .filter(|p| p.is_alive())
            .filter_map(|p| {
                Some(PlayerSpot {
                    steam_id: p.steam_id(),
                    side: p.team(),
                    place: p.place().unwrap_or_default(),
                    pos: p.position()?,
                })
            })
            .collect();
    }

    /// Records the result of the current round. Events keep counting towards it
    /// (exit frags) until the next round starts.
    fn record_round_end(&mut self, ctx: &Context, ev: &GameEvent) {
        if self.rounds.last().is_none_or(|r| r.end_tick.is_some()) {
            return;
        }
        let winner = Team::from_num(ev.i32("winner").unwrap_or(0) as u32);
        let winner_name = winner.is_playing().then(|| self.teams.name(ctx, winner));
        let Some(round) = self.rounds.last_mut() else {
            return;
        };
        round.end_tick = Some(ctx.tick());
        if winner.is_playing() {
            round.winner = Some(winner);
            round.winner_name = winner_name;
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
        for p in ctx.active_players() {
            if let Some(rl) = self.round_players.get_mut(&player_key(p.steam_id(), p.slot())) {
                rl.line.cash_spent = p.cash_spent_this_round();
            }
        }
        self.clutch.on_round_end(round.winner);
    }

    /// Closes the current round: KAST, multi-kills, rounds played, side splits,
    /// economy totals and clutches.
    fn close_round(&mut self) {
        if !self.in_round {
            return;
        }
        let winner = self.rounds.last().and_then(|r| r.winner);
        let mut lines = Vec::with_capacity(self.round_players.len());
        for (key, rl) in self.round_players.drain() {
            let mut line = rl.line;
            line.kast = line.kills > 0 || line.assists > 0 || !line.died || line.traded;
            // A player who never appeared in an event still played the round.
            let ps = self.players.entry(key).or_insert_with(|| PlayerStats {
                steam_id: line.steam_id,
                ..Default::default()
            });
            ps.rounds_played += 1;
            ps.kast_rounds += line.kast as u32;
            ps.multi_kills[line.kills.min(5) as usize] += 1;
            match line.side {
                Some(Team::Terrorist) => ps.t.add_round(&line, winner == line.side),
                Some(Team::CounterTerrorist) => ps.ct.add_round(&line, winner == line.side),
                _ => {}
            }
            lines.push(line);
        }
        lines.sort_by_key(|l| (l.side, l.steam_id));
        if let Some(r) = self.rounds.last_mut() {
            for (side, eco) in [
                (Team::Terrorist, &mut r.t_economy),
                (Team::CounterTerrorist, &mut r.ct_economy),
            ] {
                if let Some(e) = eco {
                    e.cash_spent = lines
                        .iter()
                        .filter(|l| l.side == Some(side))
                        .map(|l| l.cash_spent)
                        .sum();
                }
            }
            r.players = lines;
        }
        for (key, c) in self.clutch.take() {
            if let Some(ps) = self.players.get_mut(&key) {
                ps.clutches += 1;
                ps.clutches_won += c.won as u32;
            }
            self.clutches.push(c);
        }
        self.in_round = false;
    }

    fn on_death(&mut self, ctx: &Context, ev: &GameEvent) {
        let Some(victim_slot) = ev.player_slot("userid") else {
            return;
        };
        // Game events arrive before the tick's entity updates: positions and
        // inventories are still those of the moment of death.
        let (victim_pos, victim_place, victim_weapon, unused) = match ctx.player_by_slot(victim_slot) {
            Some(p) => (
                p.position(),
                p.place(),
                p.active_weapon().map(|w| w.name()),
                grenades_value(&p),
            ),
            None => return,
        };
        let attacker_slot = ev.player_slot("attacker").filter(|&s| s != victim_slot);
        let (attacker_pos, attacker_place) = match attacker_slot.and_then(|s| ctx.player_by_slot(s)) {
            Some(p) => (p.position(), p.place()),
            None => (None, None),
        };
        let Some((victim_key, victim_team, v)) = self.entry(ctx, victim_slot) else {
            return;
        };
        v.deaths += 1;
        v.utility_unused_value += unused;
        let victim_name = v.name.clone();
        let victim_steam = v.steam_id;

        let attacker = attacker_slot.and_then(|s| {
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
        let opening = first_kill_of_round && self.in_round && attacker.is_some() && !team_kill;
        match &attacker {
            None => {
                if ev.player_slot("attacker") == Some(victim_slot) || ev.player_slot("attacker").is_none() {
                    self.players.get_mut(&victim_key).unwrap().suicides += 1;
                }
            }
            Some((akey, ..)) if team_kill => self.players.get_mut(akey).unwrap().team_kills += 1,
            Some((akey, ateam, _, asteam)) => {
                let a = self.players.get_mut(akey).unwrap();
                a.kills += 1;
                a.headshot_kills += headshot as u32;
                a.wallbang_kills += wallbang as u32;
                a.noscope_kills += noscope as u32;
                a.through_smoke_kills += through_smoke as u32;
                if opening {
                    a.opening_kills += 1;
                    self.players.get_mut(&victim_key).unwrap().opening_deaths += 1;
                }
                let line = &mut round_line(&mut self.round_players, *akey, *asteam, *ateam).line;
                line.kills += 1;
                line.headshot_kills += headshot as u32;
                line.opening_kill |= opening;
            }
        }

        let mut assister_steam = None;
        if let Some(slot) = ev.player_slot("assister")
            && let Some((key, team, a)) = self.entry(ctx, slot)
            && team != victim_team
        {
            a.assists += 1;
            a.flash_assists += flash_assist as u32;
            let steam = a.steam_id;
            assister_steam = Some(steam);
            round_line(&mut self.round_players, key, steam, team).line.assists += 1;
        }
        {
            let rl = round_line(&mut self.round_players, victim_key, victim_steam, victim_team);
            rl.line.died = true;
            rl.line.opening_death |= opening;
            rl.hp = 0;
        }

        // Trades: the victim had killed one of the attacker's teammates recently.
        let window = (TRADE_WINDOW_SECONDS * self.cache.tick_rate) as i32;
        let mut traded_victims = Vec::new();
        if let Some((akey, ateam, _, asteam)) = &attacker
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
                round_line(&mut self.round_players, *akey, *asteam, *ateam)
                    .line
                    .trade_kills += 1;
            }
        }
        for i in traded_victims {
            self.kills[i].traded = true;
            let victim = self.kills[i].victim;
            let key = self.find_key(victim);
            if let Some(p) = self.players.get_mut(&key) {
                p.traded_deaths += 1;
            }
            if let Some(rl) = self.round_players.get_mut(&key) {
                rl.line.traded = true;
            }
        }

        // Clutches, while the round is undecided (not the exit frags after it).
        // Demos that start mid-round have no freeze-end sample: take who is
        // alive now (the victim's entity is not updated yet).
        let round = self.rounds.last().map_or(0, |r| r.number);
        let undecided = self.in_round && self.rounds.last().is_some_and(|r| r.end_tick.is_none());
        if undecided && !self.clutch.is_seeded() {
            let alive = ctx
                .active_players()
                .filter(|p| p.is_alive())
                .map(|p| AlivePlayer {
                    key: player_key(p.steam_id(), p.slot()),
                    steam_id: p.steam_id(),
                    name: p.name(),
                    side: p.team(),
                })
                .collect();
            self.clutch.seed(alive);
        }
        let enemy_killer = attacker.as_ref().filter(|_| !team_kill).map(|a| a.0);
        if undecided {
            self.clutch.on_death(round, ev.tick, victim_key, enemy_killer);
        }
        self.recent_deaths.push((ev.tick, victim_key));
        let weapon = ev.str("weapon").unwrap_or("");
        if let Some(killer) = enemy_killer
            && aim::is_gun(weapon)
        {
            self.aim.kill(killer, victim_key, weapon, headshot, ev.tick);
        }

        if let Some(r) = self.rounds.last_mut() {
            r.kills += 1;
        }
        let distance = match (attacker_pos, victim_pos) {
            (Some(a), Some(v)) => Some((0..3).map(|i| (a[i] - v[i]).powi(2)).sum::<f32>().sqrt()),
            _ => None,
        };
        self.round_kills.push(self.kills.len());
        self.kills.push(Kill {
            tick: ev.tick,
            round,
            round_time: self.round_time(ev.tick),
            attacker: attacker.as_ref().map(|a| a.3),
            attacker_name: attacker.as_ref().map(|a| a.2.clone()),
            attacker_side: attacker.as_ref().map(|a| a.1),
            attacker_pos,
            attacker_place,
            victim: victim_steam,
            victim_name,
            victim_side: victim_team,
            victim_pos,
            victim_place,
            victim_weapon,
            distance,
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
        let Some(vs) = ev.player_slot("userid") else {
            return;
        };
        let Some(vkey) = ctx.player_by_slot(vs).map(|p| player_key(p.steam_id(), vs)) else {
            return;
        };
        // Follow the victim's health through every hit, including the bomb,
        // falls and their own grenades, so damage is capped at what they had left.
        let remaining = ev.i32("health").unwrap_or(0).max(0);
        let dmg = ev.i32("dmg_health").unwrap_or(0).max(0);
        let before = self.round_players.get(&vkey).map_or(remaining + dmg, |rl| rl.hp);
        // `dmg_health` is rounded down while health drops by the real amount
        // (fire deals fractions every tick): count the health lost when it is
        // that rounding, otherwise the damage capped at the health left.
        let lost = before - remaining;
        let real = if lost == dmg || lost == dmg + 1 {
            lost
        } else {
            dmg.min(before)
        }
        .max(0) as u32;
        if let Some(rl) = self.round_players.get_mut(&vkey) {
            rl.hp = remaining;
        }
        let Some(as_) = ev.player_slot("attacker").filter(|&a| a != vs) else {
            return;
        };
        let Some((_, vteam, _)) = self.entry(ctx, vs) else {
            return;
        };
        let Some((akey, ateam, a)) = self.entry(ctx, as_) else {
            return;
        };
        let weapon = ev.str("weapon").unwrap_or("");
        let utility = matches!(weapon, "hegrenade" | "inferno" | "molotov" | "incgrenade");
        let team_damage = ateam == vteam;
        if !team_damage {
            a.damage += real;
            if utility {
                a.utility_damage += real;
            }
            let steam = a.steam_id;
            let line = &mut round_line(&mut self.round_players, akey, steam, ateam).line;
            line.damage += real;
            if utility {
                line.utility_damage += real;
            }
        }
        if !team_damage && aim::is_gun(weapon) {
            self.aim.hit(Hit {
                tick: ev.tick,
                shooter: akey,
                victim: vkey,
                victim_pawn: ev
                    .i32("userid_pawn")
                    .and_then(|h| handle_to_index(h as u32))
                    .map(|i| i as u32),
                head: ev.i32("hitgroup") == Some(1),
                damage: real,
            });
        }
        if utility {
            let victim_pos = ctx.player_by_slot(vs).and_then(|p| p.position());
            self.grenades
                .add_damage(akey, weapon, ev.tick, victim_pos, real, team_damage);
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
        let grenade = ev
            .i32("entityid")
            .and_then(|id| self.grenades.flashbang(id as u32))
            .or_else(|| self.grenades.recent_flashbang(key, ev.tick));
        if let Some(g) = grenade {
            let blind = Blind {
                steam_id: ctx.player_by_slot(vs).map_or(0, |p| p.steam_id()),
                duration,
                teammate: ateam == vteam,
            };
            self.grenades.add_blind(g, blind, true);
        }
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
            let Some(&(_, slot, _, grenade)) = self
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
            if let Some(g) = grenade {
                let blind = Blind {
                    steam_id: victim.steam_id(),
                    duration,
                    teammate: ateam == vteam,
                };
                self.grenades.add_blind(g, blind, false);
            }
        }
    }

    fn on_pawn(&mut self, e: &Entity, op: EntityOp, changed: &[FieldPath]) {
        if !matches!(op, EntityOp::Created | EntityOp::Updated) || !self.in_round || self.match_over {
            return;
        }
        // Borrowed, not copied: the paths are a few hundred bytes.
        let Some(paths) = self
            .cache
            .pawn
            .get_or_insert_with(|| PawnPaths::resolve(e.class()))
            .as_ref()
        else {
            return;
        };
        let index = e.index();
        let (track, epoch) = self.motion.track(index);
        let mut moved = false;
        // Updates are not followed between rounds: read the whole state once.
        if (op == EntityOp::Created || !track.is_current(epoch))
            && let Some(live) = paths.read_live(e)
        {
            track.live = live;
            track.set_current(epoch);
            moved = true;
        }
        for p in changed {
            match paths.watched(p) {
                None => {}
                Some(Watched::FlashDuration) => {
                    if op == EntityOp::Updated
                        && let Some(duration) = e.value_at(p).and_then(|v| v.as_f32())
                        && duration > 0.0
                    {
                        self.pending_blinds.push((e.handle(), duration));
                    }
                }
                Some(w) => {
                    paths.apply(e, p, w, &mut track.live);
                    moved = true;
                }
            }
        }
        if moved {
            self.motion.mark(index);
        }
    }

    /// A gun shot: who fired, how fast they moved, and at whom they aimed.
    fn on_fire(&mut self, ctx: &Context, ev: &GameEvent) {
        let weapon = ev.str("weapon").unwrap_or("").trim_start_matches("weapon_");
        if !aim::is_gun(weapon) {
            return;
        }
        let Some(p) = ev.player_slot("userid").and_then(|s| ctx.player_by_slot(s)) else {
            return;
        };
        let Some(pawn) = p.pawn().map(|e| e.index()) else {
            return;
        };
        let tick = ev.tick;
        if self.roster_tick != tick {
            self.roster_tick = tick;
            self.roster.clear();
            for q in ctx.active_players().filter(|q| q.is_alive()) {
                if let Some(e) = q.pawn() {
                    self.roster
                        .push((player_key(q.steam_id(), q.slot()), q.team(), e.index()));
                }
            }
        }
        // Game events come before the tick's entity updates: the state is the
        // previous tick's.
        let enemy = p.team().opponent();
        let aimed_at = self.motion.at(pawn, tick - 1).and_then(|me| {
            self.roster
                .iter()
                .filter(|r| r.1 == enemy)
                .filter_map(|&(key, _, ep)| Some((key, ep, aim::angle_to(me, self.motion.at(ep, tick - 1)?))))
                .min_by(|a, b| a.2.total_cmp(&b.2))
                .filter(|t| t.2 <= TARGET_CONE_DEGREES)
                .map(|t| (t.0, t.1))
        });
        let speed = aim::max_speed(weapon).and_then(|_| self.motion.speed(pawn, tick, self.cache.tick_rate));
        self.aim.shot(Shot {
            tick,
            shooter: player_key(p.steam_id(), p.slot()),
            pawn,
            weapon: weapon.to_string(),
            aimed_at,
            speed,
        });
    }

    /// Position of an entity, with its class's paths resolved once.
    fn body_position(&mut self, e: &Entity) -> Option<[f32; 3]> {
        let paths = *self
            .cache
            .bodies
            .entry(e.class().id)
            .or_insert_with(|| BodyPaths::resolve(e.class()));
        paths?.position(e)
    }

    fn on_projectile(&mut self, ctx: &Context, e: &Entity, op: EntityOp, kind: GrenadeKind) {
        if self.match_over {
            return;
        }
        match op {
            EntityOp::Created if self.in_round => self.on_throw(ctx, e, kind),
            EntityOp::Updated => {
                if self.grenades.wants_point(e.index(), ctx.tick())
                    && let Some(pos) = self.body_position(e)
                {
                    self.grenades.projectile_moved(e.index(), ctx.tick(), pos);
                }
            }
            EntityOp::Deleted => {
                let pos = self.body_position(e);
                self.grenades.projectile_deleted(e.index(), ctx.tick(), pos);
            }
            _ => {}
        }
    }

    /// A grenade projectile appeared: record the throw.
    fn on_throw(&mut self, ctx: &Context, e: &Entity, kind: GrenadeKind) {
        let Some(p) = e
            .get::<u32>("m_hThrower")
            .and_then(|h| ctx.player_by_pawn_handle(h))
        else {
            return;
        };
        let tick = ctx.tick();
        let kind = match kind {
            GrenadeKind::Molotov if e.get::<bool>("m_bIsIncGrenade") == Some(true) => GrenadeKind::Incendiary,
            k => k,
        };
        let release = e
            .get::<[f32; 3]>("m_vInitialPosition")
            .or_else(|| self.body_position(e))
            .unwrap_or_default();
        let pawn = p.pawn();
        let on_ground = pawn
            .and_then(|w| w.get::<u32>("m_fFlags"))
            .is_none_or(|f| f & 1 != 0);
        let speed = pawn
            .and_then(|w| self.motion.speed(w.index(), tick, self.cache.tick_rate))
            .unwrap_or(0.0);
        // A throw comes with a `grenade_thrown` event in the same tick; a
        // primed grenade falling from a dying player doesn't. Demos without
        // those events (Valve matchmaking): it fell if its thrower just died.
        let key = player_key(p.steam_id(), p.slot());
        let dropped = if self.grenade_events_seen {
            match self
                .throw_events
                .iter()
                .position(|&(t, k)| k == key && tick - t <= 1)
            {
                Some(i) => {
                    self.throw_events.swap_remove(i);
                    false
                }
                None => true,
            }
        } else {
            self.recent_deaths.iter().any(|&(t, k)| k == key && tick - t <= 1)
        };
        let g = Grenade {
            kind,
            round: self.rounds.last().map_or(0, |r| r.number),
            thrower: p.steam_id(),
            thrower_name: p.name(),
            side: p.team(),
            throw_tick: tick,
            throw_round_time: self.round_time(tick),
            throw_pos: p.position().unwrap_or(release),
            throw_angles: p.eye_angles().map_or([0.0; 2], |a| [a[0], a[1]]),
            throw_place: p.place(),
            ducking: p.is_ducking(),
            jump_throw: !on_ground,
            dropped,
            throw_speed: speed,
            release_pos: release,
            release_velocity: e.get::<[f32; 3]>("m_vInitialVelocity").unwrap_or_default(),
            trajectory: vec![release],
            detonate_tick: None,
            detonate_pos: None,
            end_tick: None,
            blinded: Vec::new(),
            flash_led_to_kill: false,
            enemy_damage: 0,
            team_damage: 0,
        };
        self.grenades.add(e.index(), key, g);
    }

    /// A fire started: link it to the molotov that started it.
    fn on_inferno(&mut self, ctx: &Context, e: &Entity) {
        if !self.in_round || self.match_over {
            return;
        }
        let Some(p) = e
            .get::<u32>("m_hOwnerEntity")
            .and_then(|h| ctx.player_by_pawn_handle(h))
        else {
            return;
        };
        let pos = e.get::<[f32; 3]>("m_firePositions.0");
        self.grenades
            .inferno_created(e.index(), player_key(p.steam_id(), p.slot()), ctx.tick(), pos);
    }

    fn on_game_rules(&mut self, e: &Entity, op: EntityOp, changed: &[FieldPath]) {
        if op != EntityOp::Updated || !self.in_round {
            return;
        }
        let path = *self
            .cache
            .freeze_period
            .get_or_insert_with(|| e.class().prop_path("m_pGameRules.m_bFreezePeriod"));
        if let Some(path) = path
            && changed.contains(&path)
            && e.value_at(&path).and_then(|v| v.as_bool()) == Some(false)
        {
            self.freeze_end_pending = true;
        }
    }

    fn on_planted_c4(&mut self, e: &Entity) {
        if !self.in_round || self.match_over {
            return;
        }
        if let Some(site) = e.get::<i32>("m_nBombSite").and_then(BombSite::from_index)
            && let Some(r) = self.rounds.last_mut()
        {
            r.bomb_site = Some(site);
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
        let (grenades, throwers) = std::mem::take(&mut self.grenades).finish(
            self.blind_events_seen,
            &self.kills,
            self.cache.tick_rate,
        );
        for (key, aim) in std::mem::take(&mut self.aim).finish() {
            if let Some(p) = self.players.get_mut(&key) {
                p.aim = aim;
            }
        }
        for (g, key) in grenades.iter().zip(&throwers) {
            let Some(p) = self.players.get_mut(key) else {
                continue;
            };
            p.flashes_leading_to_kill += g.flash_led_to_kill as u32;
            if g.dropped {
                continue;
            }
            match g.kind {
                GrenadeKind::Flashbang => p.flashbangs_thrown += 1,
                GrenadeKind::Smoke => p.smokes_thrown += 1,
                GrenadeKind::HeGrenade => p.he_thrown += 1,
                GrenadeKind::Molotov | GrenadeKind::Incendiary => p.molotovs_thrown += 1,
                GrenadeKind::Decoy => {}
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
                p.team_name = self.teams.name(ctx, team);
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
        let mut teams = Vec::new();
        for t in ctx.teams().filter(|t| t.team().is_playing()) {
            teams.push(TeamStats {
                team: t.team(),
                name: self.teams.name(ctx, t.team()),
                score: t.score(),
                score_first_half: t.score_first_half(),
                score_second_half: t.score_second_half(),
                score_overtime: t.score_overtime(),
            });
        }
        MatchStats {
            map: ctx.header().map_name.clone(),
            tick_rate: self.cache.tick_rate,
            teams,
            players,
            rounds: std::mem::take(&mut self.rounds),
            kills: std::mem::take(&mut self.kills),
            clutches: std::mem::take(&mut self.clutches),
            grenades,
        }
    }

    /// Consumes the collector after parsing.
    pub fn into_stats(mut self, ctx: &Context) -> MatchStats {
        self.finish(ctx)
    }
}

/// `x`, `y`, `z` of a grenade event.
fn event_pos(ev: &GameEvent) -> [f32; 3] {
    ["x", "y", "z"].map(|k| ev.f32(k).unwrap_or(0.0))
}

/// Value of the grenades a player carries (a second flashbang is an ammo count).
fn grenades_value(p: &Player) -> u32 {
    let Some(pawn) = p.pawn() else { return 0 };
    p.weapons()
        .iter()
        .filter_map(|w| GrenadeKind::from_weapon_name(&w.name()))
        .map(|k| {
            let count = match k {
                GrenadeKind::Flashbang => pawn
                    .get::<u32>("m_pWeaponServices.m_iAmmo.14")
                    .unwrap_or(1)
                    .clamp(1, 2),
                _ => 1,
            };
            k.price() * count
        })
        .sum()
}

impl Visitor for StatsCollector {
    fn on_start(&mut self, ctx: &Context) {
        self.cache.tick_rate = ctx.header().tick_rate();
    }

    fn on_game_event(&mut self, ctx: &Context, ev: &GameEvent) {
        match &*ev.name {
            "player_blind" => self.blind_events_seen = true,
            "grenade_thrown" => self.grenade_events_seen = true,
            _ => {}
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
            "round_freeze_end" => self.freeze_end_pending = true,
            "player_death" => self.on_death(ctx, ev),
            "player_hurt" => self.on_hurt(ctx, ev),
            "weapon_fire" => self.on_fire(ctx, ev),
            "player_blind" => self.on_blind(ctx, ev),
            "flashbang_detonate" => {
                let pos = event_pos(ev);
                let grenade = ev
                    .i32("entityid")
                    .and_then(|id| self.grenades.detonated(id as u32, ev.tick, pos));
                if let Some(slot) = ev.player_slot("userid") {
                    self.recent_flashes.push((ctx.tick(), slot, pos, grenade));
                }
            }
            "hegrenade_detonate" | "smokegrenade_detonate" | "decoy_started" => {
                if let Some(id) = ev.i32("entityid") {
                    self.grenades.detonated(id as u32, ev.tick, event_pos(ev));
                }
            }
            "smokegrenade_expired" | "decoy_detonate" => {
                if let Some(id) = ev.i32("entityid") {
                    self.grenades.ended(id as u32, ev.tick);
                }
            }
            "inferno_startburn" => {
                if let Some(id) = ev.i32("entityid") {
                    self.grenades.startburn(id as u32, event_pos(ev));
                }
            }
            "inferno_expire" => {
                if let Some(id) = ev.i32("entityid") {
                    self.grenades.inferno_ended(id as u32, ev.tick);
                }
            }
            "player_spawn" => {
                if let Some(slot) = ev.player_slot("userid")
                    && let Some(p) = ctx.player_by_slot(slot)
                    && let Some(rl) = self.round_players.get_mut(&player_key(p.steam_id(), slot))
                {
                    rl.hp = 100;
                }
            }
            // Throws are counted from the grenades themselves (Valve
            // matchmaking demos have no `grenade_thrown` events); the events
            // only tell a throw from a grenade dropped by a dying player.
            "grenade_thrown" => {
                if let Some(p) = ev.player_slot("userid").and_then(|s| ctx.player_by_slot(s)) {
                    self.throw_events
                        .push((ev.tick, player_key(p.steam_id(), p.slot())));
                }
            }
            "bomb_planted" => {
                let planter = ev.player_slot("userid").and_then(|s| ctx.player_by_slot(s));
                // The planted bomb entity, created in this tick, names the site;
                // the planter's callout is the fallback.
                let place_site = planter
                    .and_then(|p| p.place())
                    .and_then(|p| BombSite::from_place(&p));
                if let Some(r) = self.rounds.last_mut() {
                    r.bomb_planted = true;
                    r.bomb_plant_tick = Some(ev.tick);
                    r.bomb_planter = planter.map(|p| p.steam_id());
                    r.bomb_site = r.bomb_site.or(place_site);
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

    fn on_entity(&mut self, ctx: &Context, e: &Entity, op: EntityOp, changed: &[FieldPath]) {
        match self.cache.kinds.get(e.class()) {
            Kind::Projectile(kind) => self.on_projectile(ctx, e, op, kind),
            Kind::Inferno if op == EntityOp::Created => self.on_inferno(ctx, e),
            Kind::Pawn => self.on_pawn(e, op, changed),
            Kind::GameRules => self.on_game_rules(e, op, changed),
            Kind::PlantedC4 if op == EntityOp::Created => self.on_planted_c4(e),
            _ => {}
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
        if std::mem::take(&mut self.freeze_end_pending) && self.in_round && !self.match_over {
            self.sample_freeze_end(ctx);
        }
        if self.in_round && !self.setup_taken && !self.match_over {
            self.take_setup(ctx);
        }
        self.grenades.end_tick(ctx.tick());
        self.aim.end_tick(&self.motion, self.cache.tick_rate);
        if !self.throw_events.is_empty() {
            let tick = ctx.tick();
            self.throw_events.retain(|t| tick - t.0 < 2);
        }
        if !self.recent_deaths.is_empty() {
            let tick = ctx.tick();
            self.recent_deaths.retain(|t| tick - t.0 < 2);
        }
        self.motion.commit(ctx.tick());
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rating_formulas() {
        let p = PlayerStats {
            rounds_played: 20,
            kills: 14,
            deaths: 13,
            assists: 3,
            damage: 1_600,
            kast_rounds: 14,
            multi_kills: [10, 6, 4, 0, 0, 0],
            ..Default::default()
        };
        // KPR 0.7, DPR 0.65, APR 0.15, ADR 80, KAST 70%.
        assert!((p.impact() - (2.13 * 0.7 + 0.42 * 0.15 - 0.41)).abs() < 1e-5);
        let expected =
            0.0073 * 70.0 + 0.3591 * 0.7 - 0.5329 * 0.65 + 0.2372 * p.impact() + 0.0032 * 80.0 + 0.1587;
        assert!((p.rating2_approx() - expected).abs() < 1e-5);
        assert!((1.0..1.2).contains(&p.rating2_approx()), "{}", p.rating2_approx());
        assert_eq!(PlayerStats::default().rating2_approx(), 0.0);
        // Side stats share the formulas.
        let s = SideStats {
            rounds_played: 20,
            kills: 14,
            deaths: 13,
            assists: 3,
            damage: 1_600,
            kast_rounds: 14,
            multi_kills: [10, 6, 4, 0, 0, 0],
            ..Default::default()
        };
        assert_eq!(s.rating(), p.rating());
        assert_eq!(s.rating2_approx(), p.rating2_approx());
    }
}
