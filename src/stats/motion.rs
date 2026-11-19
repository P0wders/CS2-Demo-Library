//! The last half-second of every pawn's position and view: speeds when
//! shooting or throwing, and where a player was aiming a moment before a shot.
//!
//! Each pawn's current state is kept up to date from the properties each
//! update changes (a few reads), and copied into the history at the end of
//! every tick in which it changed (no reads).

use crate::model::cell_coord;

/// Ticks of history kept per pawn (half a second at 64 tick).
const LEN: usize = 32;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Sample {
    pub tick: i32,
    pub pos: [f32; 3],
    pub pitch: f32,
    pub yaw: f32,
    pub ducked: bool,
}

/// A pawn's networked position and view, as last updated.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Live {
    pub cell: [u32; 3],
    pub offset: [f32; 3],
    pub pitch: f32,
    pub yaw: f32,
    pub ducked: bool,
}

impl Live {
    fn sample(&self, tick: i32) -> Sample {
        Sample {
            tick,
            pos: std::array::from_fn(|i| cell_coord(self.cell[i], self.offset[i])),
            pitch: self.pitch,
            yaw: self.yaw,
            ducked: self.ducked,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Track {
    samples: [Sample; LEN],
    /// Index of the latest sample.
    head: usize,
    len: usize,
    /// Changed this tick: sample it at the end of the tick.
    dirty: bool,
    pub live: Live,
    /// The epoch in which `live` was last read in full.
    epoch: u32,
}

impl Default for Track {
    fn default() -> Self {
        Track {
            samples: [Sample::default(); LEN],
            head: 0,
            len: 0,
            dirty: false,
            live: Live::default(),
            epoch: u32::MAX,
        }
    }
}

impl Track {
    /// Whether `live` is complete for the current epoch.
    pub fn is_current(&self, epoch: u32) -> bool {
        self.epoch == epoch
    }

    /// `live` was just read in full.
    pub fn set_current(&mut self, epoch: u32) {
        self.epoch = epoch;
    }

    fn record(&mut self, s: Sample) {
        if self.len > 0 && self.samples[self.head].tick == s.tick {
            self.samples[self.head] = s;
            return;
        }
        self.head = (self.head + 1) % LEN;
        self.samples[self.head] = s;
        self.len = (self.len + 1).min(LEN);
    }
}

/// History per pawn entity index.
#[derive(Debug, Default)]
pub(crate) struct Motion {
    tracks: Vec<Option<Box<Track>>>,
    dirty: Vec<u32>,
    /// Updates are not followed between rounds: a new epoch makes every
    /// pawn's next update read its whole state again.
    epoch: u32,
}

impl Motion {
    fn track_mut(&mut self, pawn: u32) -> &mut Track {
        let i = pawn as usize;
        if self.tracks.len() <= i {
            self.tracks.resize_with(i + 1, || None);
        }
        self.tracks[i].get_or_insert_with(Box::default)
    }

    /// A pawn's track, and the current epoch (see [`Track::is_current`]).
    pub fn track(&mut self, pawn: u32) -> (&mut Track, u32) {
        let epoch = self.epoch;
        (self.track_mut(pawn), epoch)
    }

    pub fn new_epoch(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == u32::MAX {
            self.epoch = 0;
        }
    }

    /// The pawn's position or view changed in this tick.
    #[inline]
    pub fn mark(&mut self, pawn: u32) {
        let track = self.track_mut(pawn);
        if !track.dirty {
            track.dirty = true;
            self.dirty.push(pawn);
        }
    }

    /// Records the pawns that changed this tick.
    pub fn commit(&mut self, tick: i32) {
        for i in 0..self.dirty.len() {
            let p = self.dirty[i] as usize;
            if let Some(Some(t)) = self.tracks.get_mut(p) {
                t.dirty = false;
                let s = t.live.sample(tick);
                t.record(s);
            }
        }
        self.dirty.clear();
    }

    /// The latest sample at or before `tick` (the pawn's state then, as long
    /// as `tick` is within the kept history).
    pub fn at(&self, pawn: u32, tick: i32) -> Option<&Sample> {
        let t = self.tracks.get(pawn as usize)?.as_deref()?;
        (0..t.len)
            .map(|back| &t.samples[(t.head + LEN - back) % LEN])
            .find(|s| s.tick <= tick)
    }

    /// Horizontal speed in units per second over the two ticks before `tick`.
    pub fn speed(&self, pawn: u32, tick: i32, tick_rate: f32) -> Option<f32> {
        let now = self.at(pawn, tick - 1)?;
        let before = self.at(pawn, tick - 3).unwrap_or(now);
        let d = ((now.pos[0] - before.pos[0]).powi(2) + (now.pos[1] - before.pos[1]).powi(2)).sqrt();
        Some(d / 2.0 * tick_rate)
    }

    #[cfg(test)]
    pub fn record(&mut self, pawn: u32, s: Sample) {
        self.track_mut(pawn).record(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(tick: i32, x: f32) -> Sample {
        Sample {
            tick,
            pos: [x, 0.0, 0.0],
            ..Default::default()
        }
    }

    #[test]
    fn latest_sample_at_or_before() {
        let mut m = Motion::default();
        m.record(7, at(10, 0.0));
        m.record(7, at(12, 4.0));
        m.record(7, at(12, 5.0)); // same tick: replaced
        assert_eq!(m.at(7, 11).unwrap().pos[0], 0.0);
        assert_eq!(m.at(7, 12).unwrap().pos[0], 5.0);
        assert_eq!(m.at(7, 99).unwrap().pos[0], 5.0);
        assert!(m.at(7, 9).is_none());
        assert!(m.at(8, 12).is_none());
    }

    #[test]
    fn speed_from_positions() {
        let mut m = Motion::default();
        // 4 units per tick at 64 tick = 256 u/s.
        for t in 0..10 {
            m.record(1, at(t, t as f32 * 4.0));
        }
        assert_eq!(m.speed(1, 9, 64.0), Some(256.0));
        // Standing still since tick 2: no samples after it.
        m.record(2, at(0, 0.0));
        m.record(2, at(2, 8.0));
        assert_eq!(m.speed(2, 20, 64.0), Some(0.0));
    }

    #[test]
    fn ring_keeps_the_last_ticks() {
        let mut m = Motion::default();
        for t in 0..100 {
            m.record(3, at(t, t as f32));
        }
        assert_eq!(m.at(3, 99).unwrap().tick, 99);
        assert_eq!(m.at(3, 99 - LEN as i32 + 1).unwrap().tick, 99 - LEN as i32 + 1);
        assert!(m.at(3, 99 - LEN as i32).is_none());
    }

    #[test]
    fn live_state_is_committed_once_per_tick() {
        let mut m = Motion::default();
        let (t, epoch) = m.track(4);
        assert!(!t.is_current(epoch));
        t.live.cell = [32, 32, 32]; // cell 32 = coordinate 0
        t.live.offset = [10.0, 20.0, 30.0];
        t.set_current(epoch);
        m.mark(4);
        m.mark(4);
        m.commit(5);
        assert_eq!(m.at(4, 5).unwrap().pos, [10.0, 20.0, 30.0]);
        m.new_epoch();
        let (t, epoch) = m.track(4);
        assert!(!t.is_current(epoch));
        // Not marked: nothing recorded.
        m.commit(6);
        assert_eq!(m.at(4, 6).unwrap().tick, 5);
    }
}
