//! Aim and mechanics, from every shot: accuracy, head hits, first-shot
//! accuracy, shooting while moving, crosshair placement and time to kill.
//!
//! A shot hits when the shooter damages an enemy with a gun in the same tick
//! (the game resolves bullets the tick they are fired). Its target is the enemy
//! it hit, else the enemy closest to the crosshair within
//! [`TARGET_CONE_DEGREES`]. An *engagement* opens with the first shot at an
//! enemy after [`ENGAGEMENT_GAP_SECONDS`] without shooting at them.
//!
//! The game's own "spotted" flags are only updated about twice a second in
//! demos, too coarse for reaction times, so nothing here relies on them.

use super::motion::{Motion, Sample};
use rustc_hash::FxHashMap;

/// Shots at the same enemy closer together than this belong to one engagement.
pub const ENGAGEMENT_GAP_SECONDS: f32 = 2.0;
/// Crosshair placement is measured this long before an engagement's first shot.
pub const CROSSHAIR_LOOKBACK_SECONDS: f32 = 0.2;
/// A shot that hits nobody aims at the enemy closest to the crosshair, if
/// within this angle.
pub const TARGET_CONE_DEGREES: f32 = 10.0;
/// A weapon is accurate when its user moves slower than this fraction of the
/// weapon's maximum speed.
pub const ACCURATE_SPEED_FRACTION: f32 = 0.34;

/// Eye height above the feet, standing and crouched.
const EYE_STANDING: f32 = 64.0;
const EYE_DUCKED: f32 = 46.0;

/// Maximum running speed with a weapon in hand (units/s), for the rifles,
/// snipers and pistols whose accuracy depends on standing still. `None` for
/// other weapons (SMGs, shotguns and machine guns are made to be used moving).
pub fn max_speed(weapon: &str) -> Option<f32> {
    Some(match weapon {
        "ak47" | "galilar" => 215.0,
        "m4a1" | "m4a1_silencer" => 225.0,
        "famas" | "aug" => 220.0,
        "sg556" => 210.0,
        "awp" => 200.0,
        "ssg08" => 230.0,
        "g3sg1" | "scar20" => 215.0,
        "deagle" => 230.0,
        "revolver" => 220.0,
        "glock" | "hkp2000" | "usp_silencer" | "p250" | "elite" | "fiveseven" | "tec9" | "cz75a" => 240.0,
        _ => return None,
    })
}

/// Whether a weapon name (as in `weapon_fire` without `weapon_`, or
/// `player_hurt` / `player_death`) is a gun.
pub fn is_gun(weapon: &str) -> bool {
    !(weapon.is_empty()
        || weapon.contains("knife")
        || weapon.contains("bayonet")
        || matches!(
            weapon,
            "flashbang"
                | "hegrenade"
                | "smokegrenade"
                | "molotov"
                | "incgrenade"
                | "decoy"
                | "inferno"
                | "c4"
                | "planted_c4"
                | "world"
                | "worldspawn"
                | "trigger_hurt"
        ))
}

/// One weapon's numbers for a player.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WeaponStats {
    /// Weapon name as in the kill feed (`ak47`, `m4a1_silencer`, ...).
    pub weapon: String,
    pub shots: u32,
    pub hits: u32,
    pub head_hits: u32,
    pub kills: u32,
    pub headshot_kills: u32,
    /// Health damage to enemies.
    pub damage: u32,
}

impl WeaponStats {
    pub fn accuracy(&self) -> f32 {
        ratio(self.hits, self.shots)
    }

    pub fn merge(&mut self, o: &WeaponStats) {
        self.shots += o.shots;
        self.hits += o.hits;
        self.head_hits += o.head_hits;
        self.kills += o.kills;
        self.headshot_kills += o.headshot_kills;
        self.damage += o.damage;
    }
}

/// A player's aim and mechanics, from every shot fired with a gun.
///
/// * A shot *hits* when the shooter damages an enemy in the same tick (the
///   game resolves bullets the tick they are fired).
/// * Its target is the enemy it hit, else the enemy closest to the crosshair
///   within [`TARGET_CONE_DEGREES`].
/// * An *engagement* opens with the first shot at an enemy after
///   [`ENGAGEMENT_GAP_SECONDS`] without shooting at them.
///
/// Approximate: demos are recorded at 64 tick, and the head is reduced to a
/// point at eye height. The game's "spotted" flags are not used: in demos they
/// are only updated about twice a second, too coarse for reaction times.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AimStats {
    /// Shots fired with guns.
    pub shots: u32,
    /// Shots that hit an enemy (a shotgun blast counts once).
    pub hits: u32,
    /// Hits on the head.
    pub head_hits: u32,
    /// Rifle, sniper and pistol shots whose speed is known.
    pub speed_checked_shots: u32,
    /// Of those, shots fired slower than [`ACCURATE_SPEED_FRACTION`] of the
    /// weapon's maximum speed: stopped or counter-strafed.
    pub accurate_speed_shots: u32,
    /// Engagements opened (first shots at an enemy, see above).
    pub engagements: u32,
    /// Engagements whose first shot hit.
    pub first_shot_hits: u32,
    /// Engagements that ended with this player killing that enemy.
    pub engagement_kills: u32,
    /// Degrees between the crosshair and the enemy's head
    /// [`CROSSHAIR_LOOKBACK_SECONDS`] before each engagement's first shot.
    pub crosshair_offsets: Vec<f32>,
    /// Seconds from an engagement's first shot to the kill, for those won.
    pub kill_times: Vec<f32>,
    /// Per weapon, most used first.
    pub weapons: Vec<WeaponStats>,
}

fn ratio(a: u32, b: u32) -> f32 {
    if b == 0 { 0.0 } else { a as f32 / b as f32 }
}

fn median(v: &[f32]) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_by(f32::total_cmp);
    let m = s.len() / 2;
    Some(if s.len().is_multiple_of(2) {
        (s[m - 1] + s[m]) / 2.0
    } else {
        s[m]
    })
}

impl AimStats {
    /// Hits per shot, in [0, 1].
    pub fn accuracy(&self) -> f32 {
        ratio(self.hits, self.shots)
    }

    /// Share of hits on the head, in [0, 1].
    pub fn head_hit_ratio(&self) -> f32 {
        ratio(self.head_hits, self.hits)
    }

    /// Share of engagements whose first shot hit, in [0, 1].
    pub fn first_shot_accuracy(&self) -> f32 {
        ratio(self.first_shot_hits, self.engagements)
    }

    /// Share of rifle, sniper and pistol shots fired at an accurate speed.
    pub fn accurate_speed_ratio(&self) -> f32 {
        ratio(self.accurate_speed_shots, self.speed_checked_shots)
    }

    /// Median crosshair offset in degrees (lower: less to correct).
    pub fn crosshair_placement(&self) -> Option<f32> {
        median(&self.crosshair_offsets)
    }

    /// Median seconds from the first shot to the kill.
    pub fn time_to_kill(&self) -> Option<f32> {
        median(&self.kill_times)
    }

    pub fn weapon(&self, name: &str) -> Option<&WeaponStats> {
        self.weapons.iter().find(|w| w.weapon == name)
    }

    fn weapon_mut(&mut self, name: &str) -> &mut WeaponStats {
        match self.weapons.iter().position(|w| w.weapon == name) {
            Some(i) => &mut self.weapons[i],
            None => {
                self.weapons.push(WeaponStats {
                    weapon: name.to_string(),
                    ..Default::default()
                });
                self.weapons.last_mut().unwrap()
            }
        }
    }

    /// Adds another match's (or player's) numbers to these.
    pub fn merge(&mut self, o: &AimStats) {
        self.shots += o.shots;
        self.hits += o.hits;
        self.head_hits += o.head_hits;
        self.speed_checked_shots += o.speed_checked_shots;
        self.accurate_speed_shots += o.accurate_speed_shots;
        self.engagements += o.engagements;
        self.first_shot_hits += o.first_shot_hits;
        self.engagement_kills += o.engagement_kills;
        self.crosshair_offsets.extend_from_slice(&o.crosshair_offsets);
        self.kill_times.extend_from_slice(&o.kill_times);
        for w in &o.weapons {
            self.weapon_mut(&w.weapon).merge(w);
        }
        self.weapons
            .sort_by(|a, b| b.shots.cmp(&a.shots).then(a.weapon.cmp(&b.weapon)));
    }
}

/// Eye (or head) position of a sample.
fn eye(s: &Sample) -> [f32; 3] {
    let h = if s.ducked { EYE_DUCKED } else { EYE_STANDING };
    [s.pos[0], s.pos[1], s.pos[2] + h]
}

/// Degrees between the view of `from` and the direction to `to`'s head.
pub(crate) fn angle_to(from: &Sample, to: &Sample) -> f32 {
    let (a, b) = (eye(from), eye(to));
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len < 1.0 {
        return 0.0;
    }
    let (pitch, yaw) = (from.pitch.to_radians(), from.yaw.to_radians());
    // Source angles: positive pitch looks down.
    let v = [pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), -pitch.sin()];
    let cos = (v[0] * d[0] + v[1] * d[1] + v[2] * d[2]) / len;
    cos.clamp(-1.0, 1.0).acos().to_degrees()
}

/// A gun shot waiting for the end of its tick to learn whether it hit.
#[derive(Debug, Clone)]
pub(crate) struct Shot {
    pub tick: i32,
    pub shooter: u64,
    pub pawn: u32,
    pub weapon: String,
    /// Enemy closest to the crosshair: (player key, pawn index).
    pub aimed_at: Option<(u64, u32)>,
    /// Horizontal speed, when the weapon's accuracy depends on it.
    pub speed: Option<f32>,
}

#[derive(Debug, Clone)]
pub(crate) struct Hit {
    pub tick: i32,
    pub shooter: u64,
    pub victim: u64,
    pub victim_pawn: Option<u32>,
    pub head: bool,
    pub damage: u32,
}

#[derive(Debug, Clone)]
struct PendingKill {
    killer: u64,
    victim: u64,
    weapon: String,
    headshot: bool,
    tick: i32,
}

#[derive(Debug, Clone, Copy)]
struct Engagement {
    start: i32,
    last_shot: i32,
}

/// A shooter's last shot, for the later bullets of a burst.
#[derive(Debug, Clone)]
struct LastShot {
    tick: i32,
    weapon: String,
    hit: bool,
    head: bool,
}

/// Burst fire (Glock-18, FAMAS) sends one `weapon_fire` for three bullets:
/// hits this many ticks after a shot belong to it.
const BURST_TICKS: i32 = 8;

/// Collects shots, hits and kills during the parse.
#[derive(Debug, Default)]
pub(crate) struct AimTracker {
    stats: FxHashMap<u64, AimStats>,
    shots: Vec<Shot>,
    hits: Vec<Hit>,
    kills: Vec<PendingKill>,
    engagements: FxHashMap<(u64, u64), Engagement>,
    last: FxHashMap<u64, LastShot>,
}

impl AimTracker {
    pub fn shot(&mut self, s: Shot) {
        self.shots.push(s);
    }

    pub fn hit(&mut self, h: Hit) {
        self.hits.push(h);
    }

    pub fn kill(&mut self, killer: u64, victim: u64, weapon: &str, headshot: bool, tick: i32) {
        self.kills.push(PendingKill {
            killer,
            victim,
            weapon: weapon.to_string(),
            headshot,
            tick,
        });
    }

    /// Resolves the tick's shots, hits and kills.
    pub fn end_tick(&mut self, motion: &Motion, tick_rate: f32) {
        if self.shots.is_empty() && self.kills.is_empty() && self.hits.is_empty() {
            return;
        }
        let gap = (ENGAGEMENT_GAP_SECONDS * tick_rate) as i32;
        let lookback = (CROSSHAIR_LOOKBACK_SECONDS * tick_rate).round() as i32;
        let mut hits = std::mem::take(&mut self.hits);
        for shot in std::mem::take(&mut self.shots) {
            // Hits go to the shooter's first shot of the tick.
            let mine: Vec<Hit> = hits
                .iter()
                .filter(|h| h.shooter == shot.shooter && h.tick == shot.tick)
                .cloned()
                .collect();
            hits.retain(|h| !(h.shooter == shot.shooter && h.tick == shot.tick));
            let hit = !mine.is_empty();
            let head = mine.iter().any(|h| h.head);
            let st = self.stats.entry(shot.shooter).or_default();
            st.shots += 1;
            st.hits += hit as u32;
            st.head_hits += head as u32;
            if let (Some(speed), Some(max)) = (shot.speed, max_speed(&shot.weapon)) {
                st.speed_checked_shots += 1;
                st.accurate_speed_shots += (speed <= max * ACCURATE_SPEED_FRACTION) as u32;
            }
            let w = st.weapon_mut(&shot.weapon);
            w.shots += 1;
            w.hits += hit as u32;
            w.head_hits += head as u32;
            w.damage += mine.iter().map(|h| h.damage).sum::<u32>();
            self.last.insert(
                shot.shooter,
                LastShot {
                    tick: shot.tick,
                    weapon: shot.weapon.clone(),
                    hit,
                    head,
                },
            );

            let target = mine
                .first()
                .map(|h| (h.victim, h.victim_pawn))
                .or(shot.aimed_at.map(|(k, p)| (k, Some(p))));
            let Some((victim, victim_pawn)) = target else {
                continue;
            };
            match self.engagements.get_mut(&(shot.shooter, victim)) {
                Some(e) if shot.tick - e.last_shot <= gap => e.last_shot = shot.tick,
                _ => {
                    st.engagements += 1;
                    st.first_shot_hits += hit as u32;
                    let back = shot.tick - lookback;
                    if let Some(vp) = victim_pawn
                        && let (Some(a), Some(b)) = (motion.at(shot.pawn, back), motion.at(vp, back))
                    {
                        st.crosshair_offsets.push(angle_to(a, b));
                    }
                    self.engagements.insert(
                        (shot.shooter, victim),
                        Engagement {
                            start: shot.tick,
                            last_shot: shot.tick,
                        },
                    );
                }
            }
        }
        // Hits without a shot in their tick: the later bullets of a burst.
        for h in hits {
            let Some(ls) = self.last.get_mut(&h.shooter) else {
                continue;
            };
            if h.tick - ls.tick > BURST_TICKS {
                continue;
            }
            let (new_hit, new_head) = (!ls.hit, h.head && !ls.head);
            ls.hit = true;
            ls.head |= h.head;
            let st = self.stats.entry(h.shooter).or_default();
            st.hits += new_hit as u32;
            st.head_hits += new_head as u32;
            let w = st.weapon_mut(&ls.weapon);
            w.hits += new_hit as u32;
            w.head_hits += new_head as u32;
            w.damage += h.damage;
        }
        for k in std::mem::take(&mut self.kills) {
            let st = self.stats.entry(k.killer).or_default();
            let w = st.weapon_mut(&k.weapon);
            w.kills += 1;
            w.headshot_kills += k.headshot as u32;
            if let Some(e) = self.engagements.remove(&(k.killer, k.victim))
                && k.tick - e.last_shot <= gap
            {
                st.engagement_kills += 1;
                st.kill_times.push((k.tick - e.start) as f32 / tick_rate);
            }
        }
    }

    /// Final numbers per player key.
    pub fn finish(self) -> FxHashMap<u64, AimStats> {
        let mut stats = self.stats;
        for s in stats.values_mut() {
            s.weapons
                .sort_by(|a, b| b.shots.cmp(&a.shots).then(a.weapon.cmp(&b.weapon)));
        }
        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(tick: i32, pos: [f32; 3], pitch: f32, yaw: f32) -> Sample {
        Sample {
            tick,
            pos,
            pitch,
            yaw,
            ducked: false,
        }
    }

    #[test]
    fn angles() {
        let me = sample(0, [0.0, 0.0, 0.0], 0.0, 0.0);
        // Straight ahead at the same height.
        assert!(angle_to(&me, &sample(0, [500.0, 0.0, 0.0], 0.0, 0.0)) < 0.01);
        // 90° to the left (yaw grows counter-clockwise).
        assert!((angle_to(&me, &sample(0, [0.0, 500.0, 0.0], 0.0, 0.0)) - 90.0).abs() < 0.01);
        // Looking 10° down at someone on the same level.
        let down = sample(0, [0.0, 0.0, 0.0], 10.0, 0.0);
        assert!((angle_to(&down, &sample(0, [500.0, 0.0, 0.0], 0.0, 0.0)) - 10.0).abs() < 0.01);
    }

    #[test]
    fn engagements_and_kill_times() {
        let mut motion = Motion::default();
        for t in 0..40 {
            motion.record(1, sample(t, [0.0, 0.0, 0.0], 0.0, 0.0));
            motion.record(2, sample(t, [1000.0, 100.0, 0.0], 0.0, 0.0));
        }
        let mut aim = AimTracker::default();
        let shot = |tick, speed| Shot {
            tick,
            shooter: 10,
            pawn: 1,
            weapon: "ak47".into(),
            aimed_at: Some((20, 2)),
            speed: Some(speed),
        };
        // Miss, then two hits, the last one a kill.
        aim.shot(shot(30, 200.0));
        aim.end_tick(&motion, 64.0);
        for t in [36, 42] {
            aim.shot(shot(t, 10.0));
            aim.hit(Hit {
                tick: t,
                shooter: 10,
                victim: 20,
                victim_pawn: Some(2),
                head: t == 42,
                damage: 50,
            });
            if t == 42 {
                aim.kill(10, 20, "ak47", true, 42);
            }
            aim.end_tick(&motion, 64.0);
        }
        let s = &aim.finish()[&10];
        assert_eq!((s.shots, s.hits, s.head_hits), (3, 2, 1));
        assert_eq!((s.speed_checked_shots, s.accurate_speed_shots), (3, 2));
        assert_eq!((s.engagements, s.first_shot_hits, s.engagement_kills), (1, 0, 1));
        assert_eq!(s.kill_times, vec![12.0 / 64.0]);
        // atan(100 / 1000) ≈ 5.7° off at the time of the lookback.
        assert!(
            (s.crosshair_offsets[0] - 5.71).abs() < 0.05,
            "{:?}",
            s.crosshair_offsets
        );
        let w = s.weapon("ak47").unwrap();
        assert_eq!(
            (w.shots, w.hits, w.kills, w.headshot_kills, w.damage),
            (3, 2, 1, 1, 100)
        );
    }

    #[test]
    fn burst_hits_count_for_their_shot() {
        let motion = Motion::default();
        let mut aim = AimTracker::default();
        aim.shot(Shot {
            tick: 100,
            shooter: 1,
            pawn: 5,
            weapon: "glock".into(),
            aimed_at: None,
            speed: None,
        });
        aim.end_tick(&motion, 64.0);
        for (tick, head) in [(103, false), (107, true)] {
            aim.hit(Hit {
                tick,
                shooter: 1,
                victim: 2,
                victim_pawn: None,
                head,
                damage: 28,
            });
            aim.end_tick(&motion, 64.0);
        }
        // Too late to be part of the burst.
        aim.hit(Hit {
            tick: 140,
            shooter: 1,
            victim: 2,
            victim_pawn: None,
            head: false,
            damage: 10,
        });
        aim.end_tick(&motion, 64.0);
        let s = &aim.finish()[&1];
        assert_eq!((s.shots, s.hits, s.head_hits), (1, 1, 1));
        assert_eq!(s.weapon("glock").unwrap().damage, 56);
    }

    #[test]
    fn guns() {
        assert!(is_gun("ak47") && is_gun("m4a1_silencer") && is_gun("nova"));
        assert!(!is_gun("knife_karambit") && !is_gun("hegrenade") && !is_gun("inferno"));
        // The Zeus is fired and hits like a gun.
        assert!(is_gun("taser"));
        assert_eq!(max_speed("mac10"), None);
    }
}
