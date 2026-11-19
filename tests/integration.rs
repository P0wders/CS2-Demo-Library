//! End-to-end tests against real demos. Demos are not part of the repository:
//! put `.dem` files in `Demo/` (git-ignored, subfolders included) or point
//! `CS2_DEMO_DIR` at a folder. Tests pass with a note when no demo is found.
//!
//! `all_demos_full_parse` checks *every* demo found, so dropping a new demo in
//! adds it to the suite.

use cs2_demo::entities::FieldPath;
use cs2_demo::{Context, Demo, Entity, EntityOp, GameEvent, ParseOptions, StatsCollector, Visitor};
use std::path::{Path, PathBuf};

/// `CS2_DEMO_DIR`, or `Demo/` next to `Cargo.toml`.
fn demo_dir() -> PathBuf {
    std::env::var_os("CS2_DEMO_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Demo"))
}

/// Every `.dem` under the demo folder, including subfolders (but not build
/// output or hidden folders, so `CS2_DEMO_DIR=.` works from the repo root).
fn all_demos() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for p in rd.filter_map(|e| e.ok().map(|e| e.path())) {
            let name = p.file_name().unwrap_or_default().to_string_lossy();
            if p.is_dir() {
                if name != "target" && !name.starts_with('.') {
                    walk(&p, out);
                }
            } else if p.extension().is_some_and(|e| e == "dem") {
                out.push(p);
            }
        }
    }
    let mut v = Vec::new();
    walk(&demo_dir(), &mut v);
    v.sort();
    v
}

/// The smallest demo found, for the tests that need just one.
fn any_demo() -> Option<Demo> {
    let path = all_demos()
        .into_iter()
        .min_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
    match path {
        Some(p) => Some(Demo::open(p).expect("open demo")),
        None => {
            eprintln!("skipping: no demos in {}", demo_dir().display());
            None
        }
    }
}

#[test]
fn header_and_footer() {
    for path in all_demos() {
        let d = Demo::open(&path).expect("open demo");
        let h = d.header();
        let name = path.display();
        assert!(!h.map_name.is_empty(), "{name}: map");
        assert!(h.playback_ticks.is_some_and(|t| t > 0), "{name}: footer ticks");
        assert_eq!(h.tick_rate(), 64.0, "{name}: tick rate");
        assert!(h.build_num > 0, "{name}: build");
    }
}

/// Collects what a full parse must get right.
#[derive(Default)]
struct Checks {
    stats: StatsCollector,
    max_alive_pawn_coord: f32,
    match_over: bool,
    /// Deaths and assists after the end-of-match screen (players disconnecting
    /// count as suicides, credited to their last enemy damager), per SteamID.
    post_match: std::collections::HashMap<u64, PostMatch>,
    /// `player_blind` events: (tick, victim slot, attacker slot).
    blind_events: Vec<(i32, i32, i32)>,
    /// Pawn flash duration rising: (tick, victim slot). The stats fall back
    /// to these when a demo has no `player_blind` events.
    flash_rises: std::collections::HashSet<(i32, i32)>,
    /// `flashbang_detonate`: (tick, thrower slot).
    detonations: Vec<(i32, i32)>,
}

#[derive(Default, Clone, Copy)]
struct PostMatch {
    deaths: i32,
    assists: i32,
}

impl Visitor for Checks {
    fn on_start(&mut self, ctx: &Context) {
        self.stats.on_start(ctx);
    }

    fn on_game_event(&mut self, ctx: &Context, ev: &GameEvent) {
        match &*ev.name {
            "cs_win_panel_match" => self.match_over = true,
            "player_death" if self.match_over => {
                let mut credit = |field: &str, add: fn(&mut PostMatch)| {
                    if let Some(p) = ev.player_slot(field).and_then(|s| ctx.player_by_slot(s)) {
                        add(self.post_match.entry(p.steam_id()).or_default());
                    }
                };
                credit("userid", |e| e.deaths += 1);
                credit("assister", |e| e.assists += 1);
            }
            "player_blind" => {
                if let (Some(v), Some(a)) = (ev.player_slot("userid"), ev.player_slot("attacker"))
                    && v != a
                    && ev.f32("blind_duration").unwrap_or(0.0) > 0.0
                {
                    self.blind_events.push((ev.tick, v, a));
                }
            }
            "flashbang_detonate" => {
                if let Some(s) = ev.player_slot("userid") {
                    self.detonations.push((ev.tick, s));
                }
            }
            _ => {}
        }
        self.stats.on_game_event(ctx, ev);
    }

    fn on_entity(&mut self, ctx: &Context, e: &Entity, op: EntityOp, changed: &[FieldPath]) {
        if op == EntityOp::Updated
            && e.class_name() == "CCSPlayerPawn"
            && let Some(field) = e.class().prop_path("m_flFlashDuration")
            && changed.contains(&field)
            && e.value_at(&field)
                .and_then(|v| v.as_f32())
                .is_some_and(|d| d > 0.0)
            && let Some(p) = ctx.player_by_pawn_handle(e.handle())
        {
            self.flash_rises.insert((ctx.tick(), p.slot()));
        }
        self.stats.on_entity(ctx, e, op, changed);
    }

    fn on_tick_end(&mut self, ctx: &Context) -> std::ops::ControlFlow<()> {
        let _ = self.stats.on_tick_end(ctx);
        if ctx.tick() % 256 == 0 {
            for p in ctx.active_players().filter(|p| p.is_alive()) {
                if let Some(pos) = p.position() {
                    for c in pos {
                        self.max_alive_pawn_coord = self.max_alive_pawn_coord.max(c.abs());
                    }
                }
            }
        }
        std::ops::ControlFlow::Continue(())
    }
}

/// Parses one demo; returns a one-line summary and every violated expectation.
fn check_demo(path: &Path) -> (String, Vec<String>) {
    let mut errors = Vec::new();
    let mut fail = |msg: String| errors.push(msg);
    let d = match Demo::open(path) {
        Ok(d) => d,
        Err(e) => return (String::new(), vec![format!("open: {e}")]),
    };
    let mut v = Checks::default();
    let ctx = match d.parse(ParseOptions::default(), &mut v) {
        Ok(c) => c,
        Err(e) => return (String::new(), vec![format!("parse: {e}")]),
    };

    // Every entity update decoded to exactly the bit length the server announced.
    let diag = ctx.entity_diagnostics();
    if diag.size_mismatches != 0 || diag.baseline_errors != 0 {
        fail(format!(
            "{} size mismatches, {} baseline errors: {:?}",
            diag.size_mismatches, diag.baseline_errors, diag.samples
        ));
    }

    // World state at the end of the recording.
    if ctx.teams().count() != 4 {
        fail(format!("{} team entities", ctx.teams().count()));
    }
    let players: Vec<_> = ctx.active_players().collect();
    if players.len() != 10 {
        fail(format!("{} active players: {players:?}", players.len()));
    }
    for p in &players {
        if p.name().is_empty() || p.steam_id() < 76_561_197_960_265_728 {
            fail(format!("bad identity {p:?}"));
        }
        if !ctx
            .player_info(p.slot())
            .is_some_and(|i| i.steam_id == p.steam_id() && i.name == p.name())
        {
            fail(format!("userinfo mismatch for {}", p.name()));
        }
    }
    // Positions decoded from cell + offset stay inside the map.
    if !(100.0..4096.0).contains(&v.max_alive_pawn_coord) {
        fail(format!("max |coordinate| {}", v.max_alive_pawn_coord));
    }

    // Demos without `player_blind` events get their flash stats from pawn flash
    // durations and flashbang detonations. Where both exist, they must agree:
    // each blind event has a flash-duration rise for the victim and a
    // detonation by the attacker within two ticks.
    let flashes = if v.blind_events.is_empty() {
        "flash stats from pawn state".to_string()
    } else {
        let explained = v
            .blind_events
            .iter()
            .filter(|&&(t, victim, attacker)| {
                (t - 1..=t + 1).any(|t| v.flash_rises.contains(&(t, victim)))
                    && v.detonations
                        .iter()
                        .any(|&(dt, s)| s == attacker && (t - 2..=t).contains(&dt))
            })
            .count();
        let n = v.blind_events.len();
        if explained * 100 < n * 95 {
            fail(format!(
                "only {explained} of {n} player_blind events match pawn flash state"
            ));
        }
        format!("{explained}/{n} blind events match pawn flash state")
    };

    let post_match = std::mem::take(&mut v.post_match);
    let stats = v.stats.into_stats(&ctx);
    let total: i32 = stats.teams.iter().map(|t| t.score).sum();
    let Some(first) = stats.rounds.first() else {
        fail("no rounds".into());
        return (String::new(), errors);
    };
    let score: Vec<String> = stats
        .teams
        .iter()
        .map(|t| format!("{} {}", t.name, t.score))
        .collect();
    let summary = format!(
        "build {}, {}, {}, rounds {}-{}, {}, {flashes}",
        d.header().build_num,
        d.header().map_name,
        score.join(" : "),
        first.number,
        stats.rounds.last().unwrap().number,
        if first.number == 1 {
            "K/D/A checked against server"
        } else {
            "continuation file"
        }
    );
    // Flash stats are filled in (a pro match has dozens of enemy blinds).
    let enemies_flashed: u32 = stats.players.iter().map(|p| p.enemies_flashed).sum();
    if enemies_flashed < stats.rounds.len() as u32 {
        fail(format!(
            "only {enemies_flashed} enemies flashed in {} rounds",
            stats.rounds.len()
        ));
    }
    // Round numbers are consecutive and end at the final score.
    for (i, r) in stats.rounds.iter().enumerate() {
        if r.number != first.number + i as u32 {
            fail(format!("round {} at position {i}", r.number));
        }
        if r.winner.is_none() {
            fail(format!("round {} has no winner", r.number));
        }
    }
    if stats.rounds.last().unwrap().number as i32 != total {
        fail(format!(
            "last round {} vs score total {total}",
            stats.rounds.last().unwrap().number
        ));
    }

    // With the whole match in the file, event-derived numbers must equal the
    // game's own scoreboard counters (continuation files lack earlier rounds).
    if first.number == 1 {
        for p in &players {
            let Some(ours) = stats.player(p.steam_id()) else {
                fail(format!("no stats for {}", p.name()));
                continue;
            };
            let mut server = p.scoreboard();
            // Some servers also count disconnect "suicides" after the match
            // ended (and the assist they credit); the stats deliberately
            // ignore those, so accept exactly that difference.
            let extra = post_match.get(&p.steam_id()).copied().unwrap_or_default();
            if server.deaths == ours.deaths as i32 + extra.deaths {
                server.deaths -= extra.deaths;
            }
            if server.assists == ours.assists as i32 + extra.assists {
                server.assists -= extra.assists;
            }
            // The server counter ignores team kills (they neither add nor subtract).
            let pairs = [
                ("kills", ours.kills as i32, server.kills),
                ("deaths", ours.deaths as i32, server.deaths),
                ("assists", ours.assists as i32, server.assists),
                ("rounds", ours.rounds_played as i32, total),
            ];
            for (what, a, b) in pairs {
                if a != b {
                    fail(format!("{what} of {}: ours {a}, server {b}", ours.name));
                }
            }
        }
    }
    (summary, errors)
}

#[test]
fn all_demos_full_parse() {
    let demos = all_demos();
    if demos.is_empty() {
        eprintln!("skipping: no demos in {}", demo_dir().display());
        return;
    }
    let results: Vec<(PathBuf, (String, Vec<String>))> = std::thread::scope(|s| {
        let handles: Vec<_> = demos
            .iter()
            .map(|p| s.spawn(move || (p.clone(), check_demo(p))))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut report = String::new();
    for (path, (summary, errors)) in &results {
        let name = path.strip_prefix(demo_dir()).unwrap_or(path).to_string_lossy();
        if errors.is_empty() {
            eprintln!("ok      {name}\n        {summary}");
        } else {
            eprintln!("FAILED  {name}");
            for e in errors {
                report.push_str(&format!("{name}: {e}\n"));
            }
        }
    }
    assert!(report.is_empty(), "\n{report}");
}

#[test]
fn events_only_matches_full_parse_event_stream() {
    let Some(d) = any_demo() else { return };
    #[derive(Default)]
    struct Count(Vec<(i32, String)>);
    impl Visitor for Count {
        fn on_game_event(&mut self, _ctx: &Context, ev: &GameEvent) {
            if ev.get("synthetic").is_none() {
                self.0.push((ev.tick, ev.name.to_string()));
            }
        }
    }
    let mut a = Count::default();
    let mut b = Count::default();
    d.parse(ParseOptions::events_only(), &mut a).unwrap();
    d.parse(ParseOptions::default(), &mut b).unwrap();
    assert_eq!(a.0, b.0);

    // Player identities are available without entities.
    let ctx = d.parse(ParseOptions::events_only(), &mut ()).unwrap();
    let real: Vec<_> = ctx
        .player_infos()
        .into_iter()
        .filter(|p| !p.is_hltv && p.steam_id != 0)
        .collect();
    assert!(real.len() >= 10, "{real:?}");
}

#[test]
fn early_stop() {
    let Some(d) = any_demo() else { return };
    struct StopAt(i32);
    impl Visitor for StopAt {
        fn on_tick_end(&mut self, ctx: &Context) -> std::ops::ControlFlow<()> {
            if ctx.tick() >= self.0 {
                std::ops::ControlFlow::Break(())
            } else {
                std::ops::ControlFlow::Continue(())
            }
        }
    }
    let ctx = d.parse(ParseOptions::default(), &mut StopAt(6400)).unwrap();
    // Stops at the first tick that reaches 6400 (ticks can be skipped).
    assert!(
        (6400..6400 + 64).contains(&ctx.tick()),
        "stopped at {}",
        ctx.tick()
    );
    let opts = ParseOptions {
        end_tick: Some(1000),
        ..Default::default()
    };
    let ctx = d.parse(opts, &mut ()).unwrap();
    assert!(ctx.tick() <= 1000);
    assert!(ctx.game_rules().is_some());
    assert!(ctx.players().count() > 0);
}
