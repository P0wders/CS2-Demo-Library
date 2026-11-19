//! The match viewer's analysis. Compiled to WebAssembly, it runs inside the
//! viewer page (`web/viewer.html`): the page copies the dropped demo into the
//! module's memory, calls `analyze` and draws the JSON it gets back. Nothing
//! leaves the browser.
//!
//! Build the page with `cargo run -p cs2_demo_web` (see `web/src/main.rs`).

use cs2_demo::replay::{Frame, PositionRecorder};
use cs2_demo::stats::MatchStats;
use cs2_demo::{Context, Demo, ParseOptions, StatsCollector, Visitor};
use serde_json::{Value, json};
use std::ops::ControlFlow;

/// Parses a demo and returns what the viewer draws: `meta`, `stats`,
/// `lineups` and `replay`. The replay records positions every `every` ticks
/// (16 = 4 per second; the page interpolates between them). `progress` gets
/// the share of the demo parsed so far, from 0 to 1, a few hundred times
/// (never when the demo has no footer giving its length).
///
/// The page adds `meta.file`, `meta.radar`, `meta.radar_lower` and
/// `meta.parse_seconds` itself.
pub fn report(bytes: Vec<u8>, every: i32, progress: &mut dyn FnMut(f32)) -> cs2_demo::Result<Value> {
    let demo = Demo::from_bytes(bytes)?;
    let total = demo.header().playback_ticks.filter(|&t| t > 0);
    let mut visitors = (
        (StatsCollector::new(), PositionRecorder::every(every)),
        Progress {
            total,
            next: 0,
            report: progress,
        },
    );
    let ctx = demo.parse(ParseOptions::default(), &mut visitors)?;
    // The demo is the bulk of the memory: free it before building the JSON.
    drop(demo);
    let ((collector, recorder), _) = visitors;
    let stats = collector.into_stats(&ctx);

    let h = ctx.header();
    let frames = replay_frames(&stats, recorder.frames());
    let mut data = json!({
        "meta": {
            "map": h.map_name,
            "server": h.server_name,
            "build": h.build_num,
            "patch": h.patch_version,
            "duration": h.playback_time,
            "tick_rate": stats.tick_rate,
            "overview": stats.map_overview(),
            "generator": concat!("cs2_demo ", env!("CARGO_PKG_VERSION")),
        },
        "stats": stats,
        "lineups": stats.lineups(),
        "replay": {
            "every": every,
            "players": stats.players.iter().map(|p| p.steam_id.to_string()).collect::<Vec<_>>(),
            "frames": frames,
        },
    });
    // SteamIDs exceed the 2^53 integers JavaScript reads exactly: write them as strings.
    stringify_ids(&mut data);
    Ok(data)
}

/// An error as the page shows it, after "Could not open <file>: ".
pub fn describe(e: &cs2_demo::Error) -> String {
    use cs2_demo::Error;
    match e {
        Error::BadMagic(m) if m.starts_with(b"HL2DEMO") => {
            "this is a CS:GO demo; the viewer reads Counter-Strike 2 demos only.".into()
        }
        Error::BadMagic(_) => "this is not a Counter-Strike 2 demo (.dem) file.".into(),
        Error::UnexpectedEof("file header") => "this is not a Counter-Strike 2 demo (.dem) file.".into(),
        _ => format!("the demo looks damaged or incomplete ({e})."),
    }
}

/// Reports how far the parse is, about every 0.5 % of the demo.
struct Progress<'a> {
    total: Option<i32>,
    next: i32,
    report: &'a mut dyn FnMut(f32),
}

impl Visitor for Progress<'_> {
    fn on_tick_end(&mut self, ctx: &Context) -> ControlFlow<()> {
        if let Some(total) = self.total
            && ctx.tick() >= self.next
        {
            (self.report)((ctx.tick() as f32 / total as f32).clamp(0.0, 1.0));
            self.next = ctx.tick() + (total / 200).max(64);
        }
        ControlFlow::Continue(())
    }
}

/// Replay frames inside the rounds (from round start to 3 s after the end),
/// as compact arrays: `[tick, players, bomb]`, each player
/// `[index, x, y, z, yaw, health, armor, weapon, flags, money]` with `index`
/// into `replay.players` and flags 1 = alive, 2 = bomb, 4 = defuser; the bomb
/// `[x, y, z, planted, carrier index or -1]` or 0.
fn replay_frames(stats: &MatchStats, frames: &[Frame]) -> Vec<Value> {
    let after = (3.0 * stats.tick_rate) as i32;
    let in_round = |tick: i32| {
        stats
            .rounds
            .iter()
            .any(|r| tick >= r.start_tick && tick <= r.end_tick.map_or(i32::MAX, |e| e + after))
    };
    let index = |steam_id: u64| stats.players.iter().position(|p| p.steam_id == steam_id);
    frames
        .iter()
        .filter(|f| in_round(f.tick))
        .map(|f| {
            let players: Vec<Value> = f
                .players
                .iter()
                .filter_map(|p| {
                    let i = index(p.steam_id)?;
                    let flags = p.alive as u8 | (p.has_bomb as u8) << 1 | (p.has_defuser as u8) << 2;
                    Some(json!([
                        i,
                        p.pos[0].round() as i32,
                        p.pos[1].round() as i32,
                        p.pos[2].round() as i32,
                        p.yaw.round() as i32,
                        p.health,
                        p.armor,
                        p.weapon.unwrap_or(0),
                        flags,
                        p.money
                    ]))
                })
                .collect();
            let bomb = match &f.bomb {
                Some(b) => json!([
                    b.pos[0].round() as i32,
                    b.pos[1].round() as i32,
                    b.pos[2].round() as i32,
                    b.planted as u8,
                    b.carrier.and_then(index).map_or(-1, |i| i as i64)
                ]),
                None => json!(0),
            };
            json!([f.tick, players, bomb])
        })
        .collect()
}

/// Turns every SteamID-like number (`steam_id`, `attacker`, `victim`, ...) into a string.
fn stringify_ids(v: &mut Value) {
    const KEYS: &[&str] = &[
        "steam_id",
        "attacker",
        "victim",
        "assister",
        "thrower",
        "bomb_planter",
    ];
    match v {
        Value::Object(map) => {
            for (k, child) in map.iter_mut() {
                if KEYS.contains(&k.as_str()) {
                    if let Value::Number(n) = child {
                        *child = Value::String(n.to_string());
                    }
                } else if k == "throwers" {
                    if let Value::Array(a) = child {
                        for x in a {
                            if let Value::Number(n) = x {
                                *x = Value::String(n.to_string());
                            }
                        }
                    }
                } else {
                    stringify_ids(child);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(stringify_ids),
        _ => {}
    }
}

/// What the page calls. One demo per module instance: the page starts a
/// fresh instance (in a fresh worker) for each demo, so memory is returned.
#[cfg(target_arch = "wasm32")]
mod exports {
    use std::sync::Mutex;

    #[link(wasm_import_module = "viewer")]
    unsafe extern "C" {
        /// Share of the demo parsed so far, from 0 to 1.
        fn progress(fraction: f32);
        /// A panic message (UTF-8), just before the module traps.
        fn panicked(ptr: *const u8, len: usize);
    }

    /// The demo's room (`bytes`, still empty for Rust) and its length.
    struct Input {
        bytes: Vec<u8>,
        len: usize,
    }

    static INPUT: Mutex<Input> = Mutex::new(Input {
        bytes: Vec::new(),
        len: 0,
    });
    static OUTPUT: Mutex<Vec<u8>> = Mutex::new(Vec::new());

    /// Makes room for a demo of `len` bytes and returns where the page writes
    /// it, or null if the browser can't give the module that much memory.
    ///
    /// The room is left as it is, not zeroed: the page overwrites all of it,
    /// and zeroing first would write the whole demo's size once more.
    #[unsafe(no_mangle)]
    pub extern "C" fn input(len: usize) -> *mut u8 {
        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(len).is_err() {
            return std::ptr::null_mut();
        }
        let mut input = INPUT.lock().unwrap_or_else(|e| e.into_inner());
        *input = Input { bytes, len };
        input.bytes.as_mut_ptr()
    }

    /// Analyzes the demo written at [`input`]. Returns 0 with the viewer's
    /// JSON at [`output_ptr`], or 1 with an error message there.
    #[unsafe(no_mangle)]
    pub extern "C" fn analyze(every: i32) -> u32 {
        std::panic::set_hook(Box::new(|info| {
            let msg = info.to_string();
            // SAFETY: the page reads `len` bytes at `ptr` before returning.
            unsafe { panicked(msg.as_ptr(), msg.len()) }
        }));
        let mut input = INPUT.lock().unwrap_or_else(|e| e.into_inner());
        let len = std::mem::take(&mut input.len);
        let mut bytes = std::mem::take(&mut input.bytes);
        drop(input);
        // SAFETY: `input` reserved `len` bytes and the page has written all of
        // them (WebAssembly memory is always initialized; the page fills it).
        unsafe { bytes.set_len(len.min(bytes.capacity())) };
        // SAFETY: a plain call into the page's JavaScript.
        let mut report = |fraction| unsafe { progress(fraction) };
        let (status, out) = match super::report(bytes, every, &mut report) {
            Ok(data) => (
                0,
                serde_json::to_vec(&data).expect("a JSON value always serializes"),
            ),
            Err(e) => (1, super::describe(&e).into_bytes()),
        };
        *OUTPUT.lock().unwrap_or_else(|e| e.into_inner()) = out;
        status
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn output_ptr() -> *const u8 {
        OUTPUT.lock().unwrap_or_else(|e| e.into_inner()).as_ptr()
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn output_len() -> usize {
        OUTPUT.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

#[cfg(test)]
mod tests {
    /// Runs the viewer's analysis natively on one demo of `CS2_DEMO_DIR`
    /// (default: the project's `Demo/`), if there is one.
    #[test]
    fn report_on_a_demo() {
        let dir = std::env::var("CS2_DEMO_DIR")
            .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../Demo").into());
        let Some(path) = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "dem"))
        else {
            eprintln!("no demo in {dir}; skipped");
            return;
        };
        let mut last = -1.0f32;
        let mut calls = 0;
        let data = super::report(std::fs::read(&path).unwrap(), 16, &mut |f| {
            assert!(f >= last, "progress went back from {last} to {f}");
            last = f;
            calls += 1;
        })
        .unwrap();
        assert!(calls > 50, "{calls} progress calls");
        assert!(last > 0.95, "progress ended at {last}");
        assert!(data["stats"]["rounds"].as_array().is_some_and(|r| !r.is_empty()));
        assert!(data["replay"]["frames"].as_array().is_some_and(|f| !f.is_empty()));
        assert!(data["stats"]["players"][0]["steam_id"].is_string());
    }
}
