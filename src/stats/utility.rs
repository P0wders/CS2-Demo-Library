//! Grenades: every throw with its lineup, flight and effect.
//!
//! A throw is the grenade's projectile entity appearing. It carries the thrower
//! (`m_hThrower`), the release point and velocity; the thrower's pawn gives
//! where they stood and aimed. Detonation events name the projectile by entity
//! index; a fire is the `CInferno` entity owned by the same thrower, created as
//! the molotov projectile disappears.

use super::Kill;
use crate::model::Team;
use rustc_hash::FxHashMap;

/// Grenade type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GrenadeKind {
    Flashbang,
    Smoke,
    HeGrenade,
    Molotov,
    Incendiary,
    Decoy,
}

impl GrenadeKind {
    pub const ALL: [GrenadeKind; 6] = [
        GrenadeKind::Flashbang,
        GrenadeKind::Smoke,
        GrenadeKind::HeGrenade,
        GrenadeKind::Molotov,
        GrenadeKind::Incendiary,
        GrenadeKind::Decoy,
    ];

    /// Weapon name as used by game events (`flashbang`, `smokegrenade`, ...).
    pub fn weapon_name(self) -> &'static str {
        match self {
            GrenadeKind::Flashbang => "flashbang",
            GrenadeKind::Smoke => "smokegrenade",
            GrenadeKind::HeGrenade => "hegrenade",
            GrenadeKind::Molotov => "molotov",
            GrenadeKind::Incendiary => "incgrenade",
            GrenadeKind::Decoy => "decoy",
        }
    }

    pub fn from_weapon_name(name: &str) -> Option<GrenadeKind> {
        Some(match name.trim_start_matches("weapon_") {
            "flashbang" => GrenadeKind::Flashbang,
            "smokegrenade" => GrenadeKind::Smoke,
            "hegrenade" => GrenadeKind::HeGrenade,
            "molotov" => GrenadeKind::Molotov,
            "incgrenade" => GrenadeKind::Incendiary,
            "decoy" => GrenadeKind::Decoy,
            _ => return None,
        })
    }

    /// Price in the buy menu.
    pub fn price(self) -> u32 {
        match self {
            GrenadeKind::Flashbang => 200,
            GrenadeKind::Smoke | GrenadeKind::HeGrenade => 300,
            GrenadeKind::Molotov => 400,
            GrenadeKind::Incendiary => 500,
            GrenadeKind::Decoy => 50,
        }
    }

    /// Molotov or incendiary.
    pub fn is_fire(self) -> bool {
        matches!(self, GrenadeKind::Molotov | GrenadeKind::Incendiary)
    }

    pub fn label(self) -> &'static str {
        match self {
            GrenadeKind::Flashbang => "Flashbang",
            GrenadeKind::Smoke => "Smoke",
            GrenadeKind::HeGrenade => "HE grenade",
            GrenadeKind::Molotov => "Molotov",
            GrenadeKind::Incendiary => "Incendiary",
            GrenadeKind::Decoy => "Decoy",
        }
    }
}

/// A player blinded by a flashbang.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Blind {
    pub steam_id: u64,
    /// Seconds.
    pub duration: f32,
    pub teammate: bool,
}

/// One grenade: who threw it from where, its flight, and what it did.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Grenade {
    pub kind: GrenadeKind,
    /// 1-based round number.
    pub round: u32,
    /// SteamID64 of the thrower (0 for bots).
    pub thrower: u64,
    pub thrower_name: String,
    pub side: Team,
    pub throw_tick: i32,
    /// Seconds since freeze time ended.
    pub throw_round_time: Option<f32>,
    /// Where the thrower stood (feet).
    pub throw_pos: [f32; 3],
    /// Thrower's view when throwing: [pitch, yaw] in degrees.
    pub throw_angles: [f32; 2],
    pub throw_place: Option<String>,
    pub ducking: bool,
    /// Thrown while in the air.
    pub jump_throw: bool,
    /// Not thrown: the thrower died holding it primed and it fell. Such
    /// grenades still go off, but are left out of throw counts and lineups.
    pub dropped: bool,
    /// Thrower's horizontal speed when throwing, units/s (about 250 running).
    pub throw_speed: f32,
    /// Where the grenade left the hand, and its velocity then.
    pub release_pos: [f32; 3],
    pub release_velocity: [f32; 3],
    /// Projectile position every [`TRAJECTORY_TICKS`] ticks until it stopped.
    pub trajectory: Vec<[f32; 3]>,
    /// When and where it went off: flash or HE explosion, smoke or fire start,
    /// decoy landing. `None` if it never did (thrown just before a restart).
    pub detonate_tick: Option<i32>,
    pub detonate_pos: Option<[f32; 3]>,
    /// When a smoke faded, a fire burnt out or a decoy exploded.
    pub end_tick: Option<i32>,
    /// Players a flashbang blinded.
    pub blinded: Vec<Blind>,
    /// A flashbang blinded an enemy who was then killed by the thrower's team
    /// while still blind.
    pub flash_led_to_kill: bool,
    /// Health damage of an HE grenade or fire, capped at the health left.
    pub enemy_damage: u32,
    pub team_damage: u32,
}

/// A trajectory point is kept every this many ticks.
pub const TRAJECTORY_TICKS: i32 = 4;

impl Grenade {
    /// Seconds from detonation to the end of the smoke or fire.
    pub fn duration(&self, tick_rate: f32) -> Option<f32> {
        Some((self.end_tick? - self.detonate_tick?) as f32 / tick_rate)
    }

    pub fn enemies_blinded(&self) -> usize {
        self.blinded.iter().filter(|b| !b.teammate).count()
    }

    pub fn teammates_blinded(&self) -> usize {
        self.blinded.iter().filter(|b| b.teammate).count()
    }

    /// Total seconds of blindness caused to enemies.
    pub fn enemy_blind_time(&self) -> f32 {
        // A fold, not sum(): an empty f32 sum is -0.0.
        self.blinded
            .iter()
            .filter(|b| !b.teammate)
            .fold(0.0, |a, b| a + b.duration)
    }

    /// Where it ended: the detonation point, else the last trajectory point.
    pub fn land_pos(&self) -> Option<[f32; 3]> {
        self.detonate_pos.or_else(|| self.trajectory.last().copied())
    }
}

/// Throws of the same grenade from the same spot with the same aim: a lineup.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Lineup {
    pub kind: GrenadeKind,
    pub side: Team,
    /// Average throw position, view angles and landing point.
    pub throw_pos: [f32; 3],
    pub throw_angles: [f32; 2],
    pub land_pos: [f32; 3],
    pub jump_throw: bool,
    pub throw_place: Option<String>,
    /// Indices of the grenades in the slice given to [`lineups`].
    pub grenades: Vec<usize>,
    /// Who threw it (SteamIDs, each once).
    pub throwers: Vec<u64>,
}

impl Lineup {
    pub fn count(&self) -> usize {
        self.grenades.len()
    }
}

/// How close throws must be to count as the same lineup.
const LINEUP_POS: f32 = 32.0;
const LINEUP_ANGLE: f32 = 2.0;
const LINEUP_LAND: f32 = 150.0;

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn angle_diff(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

/// Groups grenades into lineups: same type (molotovs and incendiaries
/// together), same side, thrown within 32 units and 2° of each other, the same
/// way (jumping or not), landing within 150 units. Most used first; lineups
/// thrown once are included.
pub fn lineups(grenades: &[Grenade]) -> Vec<Lineup> {
    struct Acc {
        lineup: Lineup,
        sum_pos: [f32; 3],
        sum_land: [f32; 3],
        sum_pitch: f32,
        /// Yaw of the first throw: the others are averaged as offsets from it
        /// (yaw wraps around at ±180°).
        yaw0: f32,
        sum_dyaw: f32,
    }
    let same_kind = |a: GrenadeKind, b: GrenadeKind| a == b || (a.is_fire() && b.is_fire());
    let mut acc: Vec<Acc> = Vec::new();
    for (i, g) in grenades.iter().enumerate() {
        let Some(land) = g.land_pos().filter(|_| !g.dropped) else {
            continue;
        };
        let found = acc.iter_mut().find(|a| {
            let l = &a.lineup;
            same_kind(l.kind, g.kind)
                && l.side == g.side
                && l.jump_throw == g.jump_throw
                && dist(l.throw_pos, g.throw_pos) <= LINEUP_POS
                && (l.throw_angles[0] - g.throw_angles[0]).abs() <= LINEUP_ANGLE
                && angle_diff(l.throw_angles[1], g.throw_angles[1]) <= LINEUP_ANGLE
                && dist(l.land_pos, land) <= LINEUP_LAND
        });
        match found {
            Some(a) => {
                let n = a.lineup.grenades.len() as f32 + 1.0;
                for k in 0..3 {
                    a.sum_pos[k] += g.throw_pos[k];
                    a.sum_land[k] += land[k];
                }
                a.sum_pitch += g.throw_angles[0];
                let dyaw = (g.throw_angles[1] - a.yaw0 + 540.0).rem_euclid(360.0) - 180.0;
                a.sum_dyaw += dyaw;
                let l = &mut a.lineup;
                l.throw_pos = a.sum_pos.map(|v| v / n);
                l.land_pos = a.sum_land.map(|v| v / n);
                l.throw_angles = [
                    a.sum_pitch / n,
                    ((a.yaw0 + a.sum_dyaw / n + 540.0).rem_euclid(360.0)) - 180.0,
                ];
                l.grenades.push(i);
                if !l.throwers.contains(&g.thrower) {
                    l.throwers.push(g.thrower);
                }
            }
            None => acc.push(Acc {
                lineup: Lineup {
                    kind: g.kind,
                    side: g.side,
                    throw_pos: g.throw_pos,
                    throw_angles: g.throw_angles,
                    land_pos: land,
                    jump_throw: g.jump_throw,
                    throw_place: g.throw_place.clone(),
                    grenades: vec![i],
                    throwers: vec![g.thrower],
                },
                sum_pos: g.throw_pos,
                sum_land: land,
                sum_pitch: g.throw_angles[0],
                yaw0: g.throw_angles[1],
                sum_dyaw: 0.0,
            }),
        }
    }
    let mut out: Vec<Lineup> = acc.into_iter().map(|a| a.lineup).collect();
    out.sort_by(|a, b| b.count().cmp(&a.count()).then(a.kind.cmp(&b.kind)));
    out
}

/// Follows grenades from throw to effect during the parse.
#[derive(Debug, Default)]
pub(crate) struct GrenadeTracker {
    grenades: Vec<Grenade>,
    /// Player key of each grenade's thrower.
    keys: Vec<u64>,
    /// Tick of each grenade's last trajectory point.
    last_point: Vec<i32>,
    /// Projectile entity index → grenade.
    projectiles: FxHashMap<u32, usize>,
    /// Inferno entity index → grenade.
    infernos: FxHashMap<u32, usize>,
    /// `inferno_startburn` of this tick: (inferno entity, position). The event
    /// comes before the entity is created in the same tick.
    startburns: Vec<(u32, [f32; 3])>,
    /// Fire projectiles gone in the last ticks, not linked to a fire yet:
    /// (tick, grenade, last position).
    gone_fires: Vec<(i32, usize, Option<[f32; 3]>)>,
    /// Blinds per grenade, and whether they come from `player_blind` events.
    blinds: Vec<(usize, Blind, bool)>,
}

impl GrenadeTracker {
    pub fn add(&mut self, entity: u32, key: u64, g: Grenade) {
        self.projectiles.insert(entity, self.grenades.len());
        self.last_point.push(g.throw_tick);
        self.keys.push(key);
        self.grenades.push(g);
    }

    /// Whether the projectile `entity` is due a trajectory point (in flight,
    /// and the last point is [`TRAJECTORY_TICKS`] old).
    #[inline]
    pub fn wants_point(&self, entity: u32, tick: i32) -> bool {
        self.projectiles.get(&entity).is_some_and(|&i| {
            tick - self.last_point[i] >= TRAJECTORY_TICKS
                && self.grenades[i].detonate_tick.is_none_or(|d| d >= tick)
        })
    }

    pub fn projectile_moved(&mut self, entity: u32, tick: i32, pos: [f32; 3]) {
        if self.wants_point(entity, tick)
            && let Some(&i) = self.projectiles.get(&entity)
        {
            self.last_point[i] = tick;
            self.grenades[i].trajectory.push(pos);
        }
    }

    pub fn projectile_deleted(&mut self, entity: u32, tick: i32, pos: Option<[f32; 3]>) {
        let Some(i) = self.projectiles.remove(&entity) else {
            return;
        };
        let g = &mut self.grenades[i];
        if let Some(p) = pos
            && g.detonate_tick.is_none()
            && g.trajectory.last() != Some(&p)
        {
            g.trajectory.push(p);
        }
        if g.kind.is_fire() && g.detonate_tick.is_none() {
            self.gone_fires.push((tick, i, pos));
        }
    }

    /// A detonation event naming the projectile entity.
    pub fn detonated(&mut self, entity: u32, tick: i32, pos: [f32; 3]) -> Option<usize> {
        let &i = self.projectiles.get(&entity)?;
        let g = &mut self.grenades[i];
        if g.detonate_tick.is_none() {
            g.detonate_tick = Some(tick);
            g.detonate_pos = Some(pos);
            if g.trajectory.last() != Some(&pos) {
                g.trajectory.push(pos);
            }
        }
        Some(i)
    }

    /// A smoke faded or a decoy exploded.
    pub fn ended(&mut self, entity: u32, tick: i32) {
        if let Some(&i) = self.projectiles.get(&entity) {
            self.grenades[i].end_tick = Some(tick);
        }
    }

    pub fn startburn(&mut self, inferno: u32, pos: [f32; 3]) {
        self.startburns.push((inferno, pos));
    }

    /// A fire appeared: link it to the thrower's molotov that just landed.
    pub fn inferno_created(&mut self, inferno: u32, thrower: u64, tick: i32, pos: Option<[f32; 3]>) {
        let pos = self
            .startburns
            .iter()
            .find(|s| s.0 == inferno)
            .map(|s| s.1)
            .or(pos);
        let gone = self.gone_fires.iter().map(|&(_, i, p)| (i, p));
        let flying = self
            .projectiles
            .values()
            .map(|&i| (i, self.grenades[i].trajectory.last().copied()));
        let near = |p: Option<[f32; 3]>| match (p, pos) {
            (Some(a), Some(b)) => dist(a, b),
            _ => f32::MAX / 2.0,
        };
        let best = gone
            .chain(flying)
            .filter(|&(i, _)| {
                let g = &self.grenades[i];
                self.keys[i] == thrower && g.kind.is_fire() && g.detonate_tick.is_none()
            })
            .min_by(|a, b| near(a.1).total_cmp(&near(b.1)));
        if let Some((i, last)) = best {
            self.infernos.insert(inferno, i);
            self.gone_fires.retain(|g| g.1 != i);
            let g = &mut self.grenades[i];
            g.detonate_tick = Some(tick);
            g.detonate_pos = pos.or(last);
        }
    }

    pub fn inferno_ended(&mut self, inferno: u32, tick: i32) {
        if let Some(i) = self.infernos.remove(&inferno) {
            self.grenades[i].end_tick = Some(tick);
        }
    }

    /// Called at the end of every tick.
    pub fn end_tick(&mut self, tick: i32) {
        self.startburns.clear();
        if !self.gone_fires.is_empty() {
            // A molotov that disappeared without starting a fire (extinguished
            // by a smoke, or in water) "detonated" where it disappeared.
            let grenades = &mut self.grenades;
            self.gone_fires.retain(|&(t, i, pos)| {
                if tick - t < 2 {
                    return true;
                }
                let g = &mut grenades[i];
                if g.detonate_tick.is_none() {
                    g.detonate_tick = Some(t);
                    g.detonate_pos = pos;
                }
                false
            });
        }
    }

    /// The flashbang projectile `entity`, if tracked.
    pub fn flashbang(&self, entity: u32) -> Option<usize> {
        let &i = self.projectiles.get(&entity)?;
        (self.grenades[i].kind == GrenadeKind::Flashbang).then_some(i)
    }

    /// The thrower's flashbang that went off in the last two ticks.
    pub fn recent_flashbang(&self, thrower: u64, tick: i32) -> Option<usize> {
        (0..self.grenades.len()).rev().find(|&i| {
            let g = &self.grenades[i];
            self.keys[i] == thrower
                && g.kind == GrenadeKind::Flashbang
                && g.detonate_tick.is_some_and(|d| (tick - 2..=tick).contains(&d))
        })
    }

    pub fn add_blind(&mut self, grenade: usize, blind: Blind, from_events: bool) {
        self.blinds.push((grenade, blind, from_events));
    }

    /// Credits HE or fire damage to the grenade that dealt it.
    pub fn add_damage(
        &mut self,
        thrower: u64,
        weapon: &str,
        tick: i32,
        victim: Option<[f32; 3]>,
        damage: u32,
        team: bool,
    ) {
        let he = match weapon {
            "hegrenade" => true,
            "inferno" | "molotov" | "incgrenade" => false,
            _ => return,
        };
        let near = |g: &Grenade| match (g.land_pos(), victim) {
            (Some(a), Some(b)) => dist(a, b),
            _ => f32::MAX / 2.0,
        };
        // Fires burn for 7 s; look back a little further.
        let oldest = tick - 64 * 10;
        let best = (0..self.grenades.len())
            .rev()
            .take_while(|&i| self.grenades[i].throw_tick >= oldest)
            .filter(|&i| {
                let g = &self.grenades[i];
                self.keys[i] == thrower
                    // A grenade can also hit someone in flight, before it goes off.
                    && if he {
                        g.kind == GrenadeKind::HeGrenade && g.detonate_tick.is_none_or(|d| tick - d <= 2)
                    } else {
                        g.kind.is_fire() && g.end_tick.is_none_or(|e| e >= tick - 2)
                    }
            })
            .min_by(|&a, &b| near(&self.grenades[a]).total_cmp(&near(&self.grenades[b])));
        if let Some(i) = best {
            let g = &mut self.grenades[i];
            if team {
                g.team_damage += damage;
            } else {
                g.enemy_damage += damage;
            }
        }
    }

    /// The grenades, with the blinds of the chosen source and whether flashes
    /// led to kills; and each grenade's thrower key.
    pub fn finish(
        mut self,
        blinds_from_events: bool,
        kills: &[Kill],
        tick_rate: f32,
    ) -> (Vec<Grenade>, Vec<u64>) {
        for (i, blind, from_events) in std::mem::take(&mut self.blinds) {
            if from_events == blinds_from_events {
                self.grenades[i].blinded.push(blind);
            }
        }
        for g in &mut self.grenades {
            let Some(det) = g.detonate_tick else { continue };
            g.flash_led_to_kill = g.blinded.iter().filter(|b| !b.teammate).any(|b| {
                let until = det + (b.duration * tick_rate).ceil() as i32;
                kills.iter().any(|k| {
                    k.victim == b.steam_id
                        && !k.team_kill
                        && k.attacker_side == Some(g.side)
                        && (det..=until).contains(&k.tick)
                })
            });
        }
        (self.grenades, self.keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn throw(kind: GrenadeKind, pos: [f32; 3], angles: [f32; 2], land: [f32; 3], thrower: u64) -> Grenade {
        Grenade {
            kind,
            round: 1,
            thrower,
            thrower_name: String::new(),
            side: Team::Terrorist,
            throw_tick: 0,
            throw_round_time: None,
            throw_pos: pos,
            throw_angles: angles,
            throw_place: None,
            ducking: false,
            jump_throw: false,
            dropped: false,
            throw_speed: 0.0,
            release_pos: pos,
            release_velocity: [0.0; 3],
            trajectory: Vec::new(),
            detonate_tick: Some(10),
            detonate_pos: Some(land),
            end_tick: None,
            blinded: Vec::new(),
            flash_led_to_kill: false,
            enemy_damage: 0,
            team_damage: 0,
        }
    }

    #[test]
    fn lineups_group_the_same_throw() {
        use GrenadeKind::*;
        let g = [
            throw(Smoke, [100.0, 100.0, 0.0], [-30.0, 179.5], [900.0, 0.0, 0.0], 1),
            // Same spot and aim, yaw across the ±180° seam.
            throw(Smoke, [110.0, 95.0, 0.0], [-31.0, -179.5], [920.0, 10.0, 0.0], 2),
            // Same spot, different aim.
            throw(Smoke, [100.0, 100.0, 0.0], [-10.0, 90.0], [100.0, 900.0, 0.0], 1),
            // A molotov and an incendiary from the same spot are one lineup.
            throw(Molotov, [0.0, 0.0, 0.0], [-45.0, 0.0], [500.0, 0.0, 0.0], 3),
            throw(Incendiary, [5.0, 0.0, 0.0], [-45.5, 0.5], [510.0, 0.0, 0.0], 3),
        ];
        let l = lineups(&g);
        assert_eq!(l.len(), 3);
        assert_eq!(l[0].grenades, vec![0, 1]);
        assert_eq!(l[0].throwers, vec![1, 2]);
        assert!(
            (l[0].throw_angles[1].abs() - 180.0).abs() < 0.01,
            "{:?}",
            l[0].throw_angles
        );
        assert_eq!(l[1].grenades, vec![3, 4]);
        assert_eq!(l[2].grenades, vec![2]);
    }

    #[test]
    fn fire_links_to_the_throwers_molotov() {
        let mut t = GrenadeTracker::default();
        let mut g = throw(GrenadeKind::Molotov, [0.0; 3], [0.0; 2], [0.0; 3], 7);
        g.detonate_tick = None;
        g.detonate_pos = None;
        t.add(100, 7, g);
        t.projectile_moved(100, 8, [50.0, 0.0, 0.0]);
        t.startburn(300, [52.0, 0.0, 0.0]);
        t.projectile_deleted(100, 10, Some([51.0, 0.0, 0.0]));
        t.inferno_created(300, 7, 10, None);
        t.end_tick(10);
        t.add_damage(7, "inferno", 50, Some([60.0, 0.0, 0.0]), 8, false);
        t.add_damage(7, "inferno", 60, Some([60.0, 0.0, 0.0]), 5, true);
        t.inferno_ended(300, 458);
        let (g, keys) = t.finish(false, &[], 64.0);
        assert_eq!(keys, vec![7]);
        assert_eq!(g[0].detonate_tick, Some(10));
        assert_eq!(g[0].detonate_pos, Some([52.0, 0.0, 0.0]));
        assert_eq!((g[0].enemy_damage, g[0].team_damage), (8, 5));
        assert_eq!(g[0].duration(64.0), Some(7.0));
    }

    #[test]
    fn extinguished_molotov_lands_where_it_vanished() {
        let mut t = GrenadeTracker::default();
        let mut g = throw(GrenadeKind::Molotov, [0.0; 3], [0.0; 2], [0.0; 3], 7);
        g.detonate_tick = None;
        g.detonate_pos = None;
        t.add(100, 7, g);
        t.projectile_deleted(100, 10, Some([40.0, 0.0, 0.0]));
        t.end_tick(10);
        t.end_tick(11);
        t.end_tick(12);
        let (g, _) = t.finish(false, &[], 64.0);
        assert_eq!(
            (g[0].detonate_tick, g[0].detonate_pos),
            (Some(10), Some([40.0, 0.0, 0.0]))
        );
    }
}
