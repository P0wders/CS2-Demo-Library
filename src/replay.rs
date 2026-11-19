//! Positions over time, for 2D replays: players, grenades in flight, smokes,
//! fires and the bomb, sampled every few ticks.
//!
//! [`PositionRecorder`] is a [`Visitor`]; run it next to the
//! [`crate::StatsCollector`] to get both in one parse:
//!
//! ```no_run
//! use cs2_demo::replay::PositionRecorder;
//! use cs2_demo::{Demo, ParseOptions, StatsCollector};
//!
//! let demo = Demo::open("match.dem")?;
//! let mut both = (StatsCollector::new(), PositionRecorder::every(16));
//! let ctx = demo.parse(ParseOptions::default(), &mut both)?;
//! let (collector, recorder) = both;
//! let stats = collector.into_stats(&ctx);
//! let round = &stats.rounds[0];
//! let frames = recorder.frames_between(round.start_tick, round.end_tick.unwrap_or(i32::MAX));
//! println!("{} frames in round {}", frames.len(), round.number);
//! # Ok::<(), cs2_demo::Error>(())
//! ```

use crate::entities::{Entity, EntityOp, FieldPath};
use crate::model::{Team, entity_position};
use crate::parser::{Context, Visitor};
use crate::stats::GrenadeKind;
use std::ops::ControlFlow;

/// One player in a [`Frame`].
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlayerFrame {
    pub steam_id: u64,
    pub side: Team,
    pub alive: bool,
    pub pos: [f32; 3],
    /// View direction, degrees.
    pub yaw: f32,
    pub health: i32,
    pub armor: i32,
    /// Item definition index of the weapon in hand (see [`crate::model::weapon_name`]).
    pub weapon: Option<u32>,
    pub has_bomb: bool,
    pub has_defuser: bool,
    pub money: i32,
}

/// A grenade in flight.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GrenadeFrame {
    pub kind: GrenadeKind,
    pub pos: [f32; 3],
}

/// Where the bomb is.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BombFrame {
    pub pos: [f32; 3],
    pub planted: bool,
    /// Who carries it (SteamID), if anyone.
    pub carrier: Option<u64>,
}

/// The state of the round at one tick.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Frame {
    pub tick: i32,
    pub players: Vec<PlayerFrame>,
    pub grenades: Vec<GrenadeFrame>,
    /// Centres of active smokes.
    pub smokes: Vec<[f32; 3]>,
    /// Burning patches of each fire.
    pub fires: Vec<Vec<[f32; 3]>>,
    pub bomb: Option<BombFrame>,
}

/// Records a [`Frame`] every N ticks while the game is live (not in warmup).
#[derive(Debug, Clone)]
pub struct PositionRecorder {
    every: i32,
    next_tick: i32,
    frames: Vec<Frame>,
    projectiles: Vec<(usize, GrenadeKind)>,
    infernos: Vec<usize>,
    c4: Option<usize>,
    planted: Option<usize>,
}

impl Default for PositionRecorder {
    fn default() -> Self {
        Self::every(8)
    }
}

impl PositionRecorder {
    /// A frame every `ticks` ticks (8 = every 1/8 s at 64 tick).
    pub fn every(ticks: i32) -> Self {
        PositionRecorder {
            every: ticks.max(1),
            next_tick: 0,
            frames: Vec::new(),
            projectiles: Vec::new(),
            infernos: Vec::new(),
            c4: None,
            planted: None,
        }
    }

    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    pub fn into_frames(self) -> Vec<Frame> {
        self.frames
    }

    /// Frames with `from <= tick <= to`.
    pub fn frames_between(&self, from: i32, to: i32) -> &[Frame] {
        let a = self.frames.partition_point(|f| f.tick < from);
        let b = self.frames.partition_point(|f| f.tick <= to);
        &self.frames[a..b.max(a)]
    }

    fn sample(&self, ctx: &Context) -> Frame {
        let carrier_pawn = self
            .c4
            .and_then(|i| ctx.entity(i))
            .and_then(|c4| c4.get::<u32>("m_hOwnerEntity"))
            .and_then(|h| ctx.entities().by_handle(h))
            .map(|e| e.index());
        let mut carrier = None;
        let players = ctx
            .active_players()
            .filter_map(|p| {
                let pawn = p.pawn();
                let pos = p.position()?;
                let has_bomb = pawn.is_some_and(|e| Some(e.index()) == carrier_pawn);
                if has_bomb {
                    carrier = Some((p.steam_id(), pos));
                }
                Some(PlayerFrame {
                    steam_id: p.steam_id(),
                    side: p.team(),
                    alive: p.is_alive(),
                    pos,
                    yaw: p.eye_angles().map_or(0.0, |a| a[1]),
                    health: p.health(),
                    armor: p.armor(),
                    weapon: p.active_weapon().and_then(|w| w.item_def_index()),
                    has_bomb,
                    has_defuser: p.has_defuser(),
                    money: p.money(),
                })
            })
            .collect();
        let mut grenades = Vec::new();
        let mut smokes = Vec::new();
        for &(i, kind) in &self.projectiles {
            let Some(e) = ctx.entity(i) else { continue };
            let Some(pos) = entity_position(e) else { continue };
            if kind == GrenadeKind::Smoke && e.get::<bool>("m_bDidSmokeEffect") == Some(true) {
                smokes.push(pos);
            } else {
                grenades.push(GrenadeFrame { kind, pos });
            }
        }
        let fires = self
            .infernos
            .iter()
            .filter_map(|&i| ctx.entity(i))
            .map(|e| {
                let n = e.get::<i32>("m_fireCount").unwrap_or(0).clamp(0, 64);
                (0..n)
                    .filter(|k| e.get::<bool>(&format!("m_bFireIsBurning.{k}")) != Some(false))
                    .filter_map(|k| e.get::<[f32; 3]>(&format!("m_firePositions.{k}")))
                    .collect::<Vec<_>>()
            })
            .filter(|f| !f.is_empty())
            .collect();
        let bomb = if let Some(e) = self.planted.and_then(|i| ctx.entity(i)) {
            entity_position(e).map(|pos| BombFrame {
                pos,
                planted: true,
                carrier: None,
            })
        } else if let Some((steam_id, pos)) = carrier {
            Some(BombFrame {
                pos,
                planted: false,
                carrier: Some(steam_id),
            })
        } else {
            self.c4
                .and_then(|i| ctx.entity(i))
                .and_then(entity_position)
                .map(|pos| BombFrame {
                    pos,
                    planted: false,
                    carrier: None,
                })
        };
        Frame {
            tick: ctx.tick(),
            players,
            grenades,
            smokes,
            fires,
            bomb,
        }
    }
}

fn projectile_kind(class: &str) -> Option<GrenadeKind> {
    Some(match class {
        "CFlashbangProjectile" => GrenadeKind::Flashbang,
        "CSmokeGrenadeProjectile" => GrenadeKind::Smoke,
        "CHEGrenadeProjectile" => GrenadeKind::HeGrenade,
        "CMolotovProjectile" => GrenadeKind::Molotov,
        "CDecoyProjectile" => GrenadeKind::Decoy,
        _ => return None,
    })
}

impl Visitor for PositionRecorder {
    fn on_entity(&mut self, _ctx: &Context, e: &Entity, op: EntityOp, _changed: &[FieldPath]) {
        // Only creations and deletions: updates are read when sampling.
        let index = e.index() as usize;
        match op {
            EntityOp::Created => match e.class_name() {
                "CC4" => self.c4 = Some(index),
                "CPlantedC4" => self.planted = Some(index),
                "CInferno" => self.infernos.push(index),
                name => {
                    if let Some(kind) = projectile_kind(name) {
                        let kind = match kind {
                            GrenadeKind::Molotov if e.get::<bool>("m_bIsIncGrenade") == Some(true) => {
                                GrenadeKind::Incendiary
                            }
                            k => k,
                        };
                        self.projectiles.push((index, kind));
                    }
                }
            },
            EntityOp::Deleted => {
                if self.c4 == Some(index) {
                    self.c4 = None;
                }
                if self.planted == Some(index) {
                    self.planted = None;
                }
                self.infernos.retain(|&i| i != index);
                self.projectiles.retain(|&(i, _)| i != index);
            }
            _ => {}
        }
    }

    fn on_tick_end(&mut self, ctx: &Context) -> ControlFlow<()> {
        let tick = ctx.tick();
        if tick >= self.next_tick && ctx.game_rules().is_some_and(|g| !g.is_warmup()) {
            self.next_tick = tick + self.every;
            let frame = self.sample(ctx);
            self.frames.push(frame);
        }
        ControlFlow::Continue(())
    }
}
