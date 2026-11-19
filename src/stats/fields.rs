//! Class kinds and property paths resolved once per demo, so the work done on
//! each of the millions of entity updates is a few comparisons rather than
//! name lookups.

use super::motion::Live;
use super::utility::GrenadeKind;
use crate::entities::{Class, Entity, FieldPath};
use crate::model::cell_coord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Unresolved,
    Other,
    Pawn,
    GameRules,
    PlantedC4,
    Projectile(GrenadeKind),
    Inferno,
}

/// Kind of each class, indexed by class id.
#[derive(Debug, Clone, Default)]
pub(crate) struct ClassKinds(Vec<Kind>);

impl ClassKinds {
    #[inline]
    pub fn get(&mut self, class: &Class) -> Kind {
        let id = class.id as usize;
        if let Some(&k) = self.0.get(id)
            && k != Kind::Unresolved
        {
            return k;
        }
        if self.0.len() <= id {
            self.0.resize(id + 1, Kind::Unresolved);
        }
        let k = classify(&class.name);
        self.0[id] = k;
        k
    }
}

fn classify(name: &str) -> Kind {
    match name {
        "CCSPlayerPawn" => Kind::Pawn,
        "CCSGameRulesProxy" => Kind::GameRules,
        "CPlantedC4" => Kind::PlantedC4,
        "CInferno" => Kind::Inferno,
        "CFlashbangProjectile" => Kind::Projectile(GrenadeKind::Flashbang),
        "CSmokeGrenadeProjectile" => Kind::Projectile(GrenadeKind::Smoke),
        "CHEGrenadeProjectile" => Kind::Projectile(GrenadeKind::HeGrenade),
        // Incendiaries use the molotov projectile with `m_bIsIncGrenade` set.
        "CMolotovProjectile" => Kind::Projectile(GrenadeKind::Molotov),
        "CDecoyProjectile" => Kind::Projectile(GrenadeKind::Decoy),
        _ => Kind::Other,
    }
}

/// Paths of a class's `CBodyComponent` position (cell index + offset per axis).
#[derive(Debug, Clone, Copy)]
pub(crate) struct BodyPaths {
    cell: [FieldPath; 3],
    offset: [FieldPath; 3],
}

impl BodyPaths {
    pub fn resolve(class: &Class) -> Option<BodyPaths> {
        let p = |n: &str| class.prop_path(n);
        Some(BodyPaths {
            cell: [
                p("CBodyComponent.m_cellX")?,
                p("CBodyComponent.m_cellY")?,
                p("CBodyComponent.m_cellZ")?,
            ],
            offset: [
                p("CBodyComponent.m_vecX")?,
                p("CBodyComponent.m_vecY")?,
                p("CBodyComponent.m_vecZ")?,
            ],
        })
    }

    pub fn position(&self, e: &Entity) -> Option<[f32; 3]> {
        let axis = |i: usize| {
            let cell = e.value_at(&self.cell[i])?.as_u32()?;
            let off = e
                .value_at(&self.offset[i])
                .and_then(|v| v.as_f32())
                .unwrap_or(0.0);
            Some(cell_coord(cell, off))
        };
        Some([axis(0)?, axis(1)?, axis(2)?])
    }
}

/// What a watched pawn property is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Watched {
    /// `CBodyComponent.m_cell{X,Y,Z}` (axis 0, 1, 2).
    Cell(u8),
    /// `CBodyComponent.m_vec{X,Y,Z}`.
    Offset(u8),
    EyeAngles,
    FlashDuration,
    Ducked,
}

/// The `CCSPlayerPawn` properties followed on every update.
#[derive(Debug, Clone)]
pub(crate) struct PawnPaths {
    pub body: BodyPaths,
    pub eye_angles: FieldPath,
    pub ducked: Option<FieldPath>,
    /// Watched paths of depth 1, by first component.
    depth1: Vec<Option<Watched>>,
    /// Watched paths of depth 2, by first then second component. Pawn
    /// updates change ~16 properties each (mostly animation state next to
    /// the position): two array reads per changed path tell them apart.
    depth2: Vec<Option<Box<[Option<Watched>; SECOND]>>>,
    /// Watched paths that don't fit the tables (none in current builds).
    others: Vec<(FieldPath, Watched)>,
}

/// Second components below this fit the depth-2 table.
const SECOND: usize = 64;

impl PawnPaths {
    pub fn resolve(class: &Class) -> Option<PawnPaths> {
        let body = BodyPaths::resolve(class)?;
        let eye_angles = class.prop_path("m_angEyeAngles")?;
        let flash_duration = class.prop_path("m_flFlashDuration")?;
        let ducked = class.prop_path("m_pMovementServices.m_bDucked");
        let mut watched = Vec::new();
        for axis in 0..3 {
            watched.push((body.cell[axis], Watched::Cell(axis as u8)));
            watched.push((body.offset[axis], Watched::Offset(axis as u8)));
        }
        watched.push((eye_angles, Watched::EyeAngles));
        watched.push((flash_duration, Watched::FlashDuration));
        watched.extend(ducked.map(|p| (p, Watched::Ducked)));
        let mut paths = PawnPaths {
            body,
            eye_angles,
            ducked,
            depth1: Vec::new(),
            depth2: Vec::new(),
            others: Vec::new(),
        };
        for (p, what) in watched {
            let s = p.as_slice();
            let first = usize::try_from(s[0]).ok().filter(|&f| f < 4096);
            let second = s
                .get(1)
                .and_then(|&i| usize::try_from(i).ok())
                .filter(|&i| i < SECOND);
            match (first, s.len(), second) {
                (Some(f), 1, _) => {
                    if paths.depth1.len() <= f {
                        paths.depth1.resize(f + 1, None);
                    }
                    paths.depth1[f] = Some(what);
                }
                (Some(f), 2, Some(i)) => {
                    if paths.depth2.len() <= f {
                        paths.depth2.resize_with(f + 1, || None);
                    }
                    paths.depth2[f].get_or_insert_with(|| Box::new([None; SECOND]))[i] = Some(what);
                }
                _ => paths.others.push((p, what)),
            }
        }
        Some(paths)
    }

    /// The pawn's whole position and view.
    pub fn read_live(&self, e: &Entity) -> Option<Live> {
        let u = |p: &FieldPath| e.value_at(p).and_then(|v| v.as_u32());
        let f = |p: &FieldPath| e.value_at(p).and_then(|v| v.as_f32()).unwrap_or(0.0);
        let angles = e
            .value_at(&self.eye_angles)
            .and_then(|v| v.as_vec3())
            .unwrap_or_default();
        Some(Live {
            cell: [
                u(&self.body.cell[0])?,
                u(&self.body.cell[1])?,
                u(&self.body.cell[2])?,
            ],
            offset: self.body.offset.each_ref().map(f),
            pitch: angles[0],
            yaw: angles[1],
            ducked: self
                .ducked
                .and_then(|p| e.value_at(&p))
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        })
    }

    /// Copies one changed property into `live`.
    #[inline]
    pub fn apply(&self, e: &Entity, path: &FieldPath, what: Watched, live: &mut Live) {
        let Some(v) = e.value_at(path) else { return };
        match what {
            Watched::Cell(axis) => {
                if let Some(c) = v.as_u32() {
                    live.cell[axis as usize] = c;
                }
            }
            Watched::Offset(axis) => {
                if let Some(o) = v.as_f32() {
                    live.offset[axis as usize] = o;
                }
            }
            Watched::EyeAngles => {
                if let Some(a) = v.as_vec3() {
                    live.pitch = a[0];
                    live.yaw = a[1];
                }
            }
            Watched::Ducked => live.ducked = v.as_bool().unwrap_or(false),
            Watched::FlashDuration => {}
        }
    }

    /// Which watched property a changed path is, if any.
    #[inline]
    pub fn watched(&self, path: &FieldPath) -> Option<Watched> {
        let s = path.as_slice();
        let found = usize::try_from(s[0]).ok().and_then(|f| match s.len() {
            1 => self.depth1.get(f).copied().flatten(),
            2 => self
                .depth2
                .get(f)?
                .as_ref()?
                .get(usize::try_from(s[1]).ok()?)
                .copied()
                .flatten(),
            _ => None,
        });
        if found.is_some() || self.others.is_empty() {
            return found;
        }
        self.others
            .iter()
            .find(|(w, _)| w.as_slice() == s)
            .map(|&(_, k)| k)
    }
}
