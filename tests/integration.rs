//! End-to-end tests against real demos. Demos are not part of the repository:
//! put `.dem` files in `Demo/` (git-ignored, subfolders included) or point
//! `CS2_DEMO_DIR` at a folder. Tests pass with a note when no demo is found.
//!
//! `all_demos_full_parse` checks *every* demo found, so dropping a new demo in
//! adds it to the suite.

use cs2_demo::entities::FieldPath;
use cs2_demo::stats::{
    BuyType, GrenadeKind, MatchStats, PlayerRound, RoundSummary, SETUP_SECONDS, SideStats,
};
use cs2_demo::{Context, Demo, Entity, EntityOp, GameEvent, ParseOptions, StatsCollector, Team, Visitor};
use std::collections::{HashMap, HashSet};
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

/// Collects what a full parse must get right (runs next to the `StatsCollector`).
#[derive(Default)]
struct Checks {
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
    /// `grenade_thrown` events during play, per (SteamID, weapon). Valve
    /// matchmaking demos have none.
    thrown: HashMap<(u64, String), u32>,
    /// Damage to enemies that is neither gun nor HE/fire damage: knives and
    /// grenade impacts (`dmg_health`, uncapped), per attacker SteamID.
    other_damage: HashMap<u64, u32>,
}

#[derive(Default, Clone, Copy)]
struct PostMatch {
    deaths: i32,
    assists: i32,
}

impl Visitor for Checks {
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
            "grenade_thrown" if !self.match_over && ctx.game_rules().is_some_and(|g| !g.is_warmup()) => {
                if let Some(p) = ev.player_slot("userid").and_then(|s| ctx.player_by_slot(s)) {
                    let weapon = ev.str("weapon").unwrap_or("").to_string();
                    *self.thrown.entry((p.steam_id(), weapon)).or_default() += 1;
                }
            }
            "player_hurt" => {
                let weapon = ev.str("weapon").unwrap_or("");
                if (weapon.contains("knife")
                    || weapon.contains("bayonet")
                    || matches!(weapon, "flashbang" | "smokegrenade" | "decoy"))
                    && let (Some(a), Some(v)) = (
                        ev.player_slot("attacker").and_then(|s| ctx.player_by_slot(s)),
                        ev.player_slot("userid").and_then(|s| ctx.player_by_slot(s)),
                    )
                    && a.team() != v.team()
                {
                    *self.other_damage.entry(a.steam_id()).or_default() +=
                        ev.i32("dmg_health").unwrap_or(0) as u32;
                }
            }
            _ => {}
        }
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
    }

    fn on_tick_end(&mut self, ctx: &Context) -> std::ops::ControlFlow<()> {
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
    let mut both = (StatsCollector::new(), Checks::default());
    let ctx = match d.parse(ParseOptions::default(), &mut both) {
        Ok(c) => c,
        Err(e) => return (String::new(), vec![format!("parse: {e}")]),
    };
    let (collector, mut v) = both;

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
    let stats = collector.into_stats(&ctx);
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
    errors.extend(check_rounds(&ctx, &stats, &post_match));
    let (grenade_summary, grenade_errors) = check_grenades(&stats, &v.thrown);
    errors.extend(grenade_errors);
    let (aim_summary, aim_errors) = check_aim(&stats, &v.other_damage);
    errors.extend(aim_errors);
    let (radar_summary, radar_errors) = check_radar(&stats);
    errors.extend(radar_errors);
    (
        format!("{summary}\n        {grenade_summary}\n        {aim_summary}\n        {radar_summary}"),
        errors,
    )
}

/// Per-round lines, side splits, economy, bomb sites, setups, clutches and
/// kill positions.
fn check_rounds(ctx: &Context, stats: &MatchStats, post_match: &HashMap<u64, PostMatch>) -> Vec<String> {
    let mut errors = Vec::new();
    let mut fail = |msg: String| errors.push(msg);

    // Each player's round lines, and their T + CT splits, add up to their totals.
    for p in &stats.players {
        let lines: Vec<&PlayerRound> = stats.rounds.iter().filter_map(|r| r.player(p.steam_id)).collect();
        let rounds = |f: &dyn Fn(&PlayerRound) -> u32| lines.iter().map(|l| f(l)).sum::<u32>();
        let sides = |f: &dyn Fn(&SideStats) -> u32| f(&p.t) + f(&p.ct);
        let fields = [
            (
                "rounds",
                lines.len() as u32,
                sides(&|s| s.rounds_played),
                p.rounds_played,
            ),
            ("kills", rounds(&|l| l.kills), sides(&|s| s.kills), p.kills),
            (
                "deaths",
                rounds(&|l| l.died as u32),
                sides(&|s| s.deaths),
                p.deaths,
            ),
            (
                "assists",
                rounds(&|l| l.assists),
                sides(&|s| s.assists),
                p.assists,
            ),
            ("damage", rounds(&|l| l.damage), sides(&|s| s.damage), p.damage),
            (
                "utility damage",
                rounds(&|l| l.utility_damage),
                sides(&|s| s.utility_damage),
                p.utility_damage,
            ),
            (
                "headshot kills",
                rounds(&|l| l.headshot_kills),
                sides(&|s| s.headshot_kills),
                p.headshot_kills,
            ),
            (
                "KAST rounds",
                rounds(&|l| l.kast as u32),
                sides(&|s| s.kast_rounds),
                p.kast_rounds,
            ),
            (
                "opening kills",
                rounds(&|l| l.opening_kill as u32),
                sides(&|s| s.opening_kills),
                p.opening_kills,
            ),
            (
                "opening deaths",
                rounds(&|l| l.opening_death as u32),
                sides(&|s| s.opening_deaths),
                p.opening_deaths,
            ),
            (
                "trade kills",
                rounds(&|l| l.trade_kills),
                sides(&|s| s.trade_kills),
                p.trade_kills,
            ),
            (
                "traded deaths",
                rounds(&|l| l.traded as u32),
                sides(&|s| s.traded_deaths),
                p.traded_deaths,
            ),
        ];
        for (what, in_rounds, in_sides, total) in fields {
            if in_rounds != total || in_sides != total {
                fail(format!(
                    "{} {what}: rounds add up to {in_rounds}, sides to {in_sides}, total {total}",
                    p.name
                ));
            }
        }
        let mut multi = [0u32; 6];
        for l in &lines {
            multi[l.kills.min(5) as usize] += 1;
        }
        let side_multi: [u32; 6] = std::array::from_fn(|i| p.t.multi_kills[i] + p.ct.multi_kills[i]);
        if multi != p.multi_kills || side_multi != p.multi_kills {
            fail(format!(
                "{} multi-kills: rounds {multi:?}, sides {side_multi:?}, total {:?}",
                p.name, p.multi_kills
            ));
        }
        let clutches = stats.clutches.iter().filter(|c| c.steam_id == p.steam_id);
        let won = clutches.clone().filter(|c| c.won).count() as u32;
        if (clutches.count() as u32, won) != (p.clutches, p.clutches_won) {
            fail(format!(
                "{} clutch counters disagree with the clutch list",
                p.name
            ));
        }
    }

    // The game's own per-round counters (`m_perRoundStats`, index = round - 1).
    let last = stats.rounds.last().map_or(0, |r| r.number);
    for p in ctx.active_players() {
        let ctrl = p.controller();
        let server = |round: u32, k: &str| {
            ctrl.get::<i32>(&format!(
                "m_pActionTrackingServices.m_perRoundStats.{}.{k}",
                round - 1
            ))
        };
        for r in &stats.rounds {
            let Some(l) = r.player(p.steam_id()) else { continue };
            let Some(kills) = server(r.number, "m_iKills") else {
                continue;
            };
            let deaths = server(r.number, "m_iDeaths").unwrap_or(0);
            let assists = server(r.number, "m_iAssists").unwrap_or(0);
            // Disconnect "suicides" after the final scoreboard land in the last round.
            let extra = match r.number == last {
                true => post_match.get(&p.steam_id()).copied().unwrap_or_default(),
                false => PostMatch::default(),
            };
            let (d, a) = (l.died as i32, l.assists as i32);
            if kills != l.kills as i32
                || (deaths != d && deaths != d + extra.deaths)
                || (assists != a && assists != a + extra.assists)
            {
                fail(format!(
                    "{} round {}: ours K{} D{d} A{a}, server K{kills} D{deaths} A{assists}",
                    p.name(),
                    r.number,
                    l.kills
                ));
            }
            if r.freeze_end_tick.is_some()
                && let Some(eq) = server(r.number, "m_iEquipmentValue")
                && eq != l.equipment_value
            {
                fail(format!(
                    "{} round {}: equipment value ours {}, server {eq}",
                    p.name(),
                    r.number,
                    l.equipment_value
                ));
            }
            // The server adds up unrounded damage: allow a point of rounding.
            if let Some(dmg) = server(r.number, "m_iDamage")
                && !(-1..=1).contains(&(dmg - l.damage as i32))
            {
                fail(format!(
                    "{} round {}: damage ours {}, server {dmg}",
                    p.name(),
                    r.number,
                    l.damage
                ));
            }
        }
    }

    for r in &stats.rounds {
        if r.freeze_end_tick.is_some() {
            for side in [Team::Terrorist, Team::CounterTerrorist] {
                if !r
                    .economy(side)
                    .is_some_and(|e| e.players > 0 && e.equipment_value > 0)
                {
                    fail(format!("round {}: no {side:?} economy", r.number));
                }
            }
        }
        if r.bomb_planted && r.bomb_site.is_none() {
            fail(format!("round {}: bomb planted, no site", r.number));
        }
        if let (Some(fe), Some(end)) = (r.freeze_end_tick, r.end_tick)
            && (end - fe) as f32 > (SETUP_SECONDS + 1.0) * stats.tick_rate
            && r.setup.is_empty()
        {
            fail(format!("round {}: no setup", r.number));
        }
    }

    // Pistol rounds: round 1 and the first round after the half-time side switch.
    if stats.rounds.first().is_some_and(|r| r.number == 1) {
        let side_in = |r: &RoundSummary, id: u64| r.player(id).and_then(|l| l.side);
        let switches: Vec<u32> = stats
            .rounds
            .windows(2)
            .filter(|w| {
                let flipped = w[1]
                    .players
                    .iter()
                    .filter(|l| side_in(&w[0], l.steam_id).is_some_and(|s| Some(s) != l.side))
                    .count();
                flipped >= 8
            })
            .map(|w| w[1].number)
            .collect();
        let pistol = |r: &RoundSummary, side| r.economy(side).is_some_and(|e| e.buy == BuyType::Pistol);
        let pistols: Vec<u32> = stats
            .rounds
            .iter()
            .filter(|r| pistol(r, Team::Terrorist) || pistol(r, Team::CounterTerrorist))
            .map(|r| r.number)
            .collect();
        let mut expected = vec![1];
        expected.extend(switches.first());
        if pistols != expected {
            fail(format!(
                "pistol rounds {pistols:?}, expected {expected:?} (side switches at {switches:?})"
            ));
        }
        for n in &pistols {
            let r = stats.round(*n).unwrap();
            if !(pistol(r, Team::Terrorist) && pistol(r, Team::CounterTerrorist)) {
                fail(format!("round {n}: only one side on a pistol round"));
            }
        }
    }

    let mut clutch_sides = HashSet::new();
    for c in &stats.clutches {
        let winner = stats.round(c.round).and_then(|r| r.winner);
        if !(1..=5).contains(&c.opponents)
            || !clutch_sides.insert((c.round, c.side))
            || c.won != (winner == Some(c.side))
        {
            fail(format!("bad clutch {c:?} (round winner {winner:?})"));
        }
    }

    // Kills carry positions, and a time in the round.
    let duels: Vec<_> = stats
        .kills
        .iter()
        .filter(|k| k.attacker.is_some_and(|a| a != k.victim))
        .collect();
    let located = duels
        .iter()
        .filter(|k| k.attacker_pos.is_some() && k.victim_pos.is_some() && k.distance.is_some())
        .count();
    if located * 100 < duels.len() * 95 {
        fail(format!("only {located} of {} kills have positions", duels.len()));
    }
    for k in &stats.kills {
        if stats.round(k.round).is_some_and(|r| r.freeze_end_tick.is_some())
            && !k.round_time.is_some_and(|t| t >= 0.0)
        {
            fail(format!(
                "kill at tick {} has round time {:?}",
                k.tick, k.round_time
            ));
        }
    }
    errors
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

/// Grenades: one per throw, linked to their detonation, with damage and
/// blinds that add up to the players' totals.
fn check_grenades(stats: &MatchStats, thrown: &HashMap<(u64, String), u32>) -> (String, Vec<String>) {
    let mut errors = Vec::new();
    let mut fail = |msg: String| errors.push(msg);
    let g = &stats.grenades;
    for p in &stats.players {
        let mine: Vec<_> = g.iter().filter(|g| g.thrower == p.steam_id).collect();
        type IsKind<'a> = &'a dyn Fn(GrenadeKind) -> bool;
        let kinds: [(&str, IsKind, u32); 4] = [
            (
                "flashbangs",
                &|k| k == GrenadeKind::Flashbang,
                p.flashbangs_thrown,
            ),
            ("smokes", &|k| k == GrenadeKind::Smoke, p.smokes_thrown),
            ("HE grenades", &|k| k == GrenadeKind::HeGrenade, p.he_thrown),
            ("molotovs", &|k| k.is_fire(), p.molotovs_thrown),
        ];
        for (what, is_kind, counted) in kinds {
            let ticks: Vec<i32> = mine
                .iter()
                .filter(|g| is_kind(g.kind) && !g.dropped)
                .map(|g| g.throw_tick)
                .collect();
            // Where the demo has `grenade_thrown` events, every throw has one.
            let events: u32 = thrown
                .iter()
                .filter(|((id, w), _)| {
                    *id == p.steam_id && GrenadeKind::from_weapon_name(w).is_some_and(&is_kind)
                })
                .map(|(_, n)| n)
                .sum();
            if ticks.len() as u32 != counted || (!thrown.is_empty() && events != counted) {
                fail(format!(
                    "{} {what}: {} grenades (thrown at ticks {ticks:?}), counted {counted}, {events} grenade_thrown events",
                    p.name,
                    ticks.len()
                ));
            }
        }
        let damage: u32 = mine.iter().map(|g| g.enemy_damage).sum();
        if damage != p.utility_damage {
            fail(format!(
                "{} grenade damage {damage}, utility damage {}",
                p.name, p.utility_damage
            ));
        }
        let blinded: u32 = mine.iter().map(|g| g.enemies_blinded() as u32).sum();
        let blind_time: f32 = mine.iter().map(|g| g.enemy_blind_time()).sum();
        if blinded != p.enemies_flashed || (blind_time - p.enemy_flash_duration).abs() > 0.01 {
            fail(format!(
                "{} grenades blinded {blinded} enemies for {blind_time:.1} s, stats say {} for {:.1} s",
                p.name, p.enemies_flashed, p.enemy_flash_duration
            ));
        }
    }
    // A grenade counts as dropped when its thrower died holding it primed.
    for gr in g.iter().filter(|g| g.dropped) {
        if !stats
            .kills
            .iter()
            .any(|k| k.victim == gr.thrower && (k.tick - gr.throw_tick).abs() <= 1)
        {
            fail(format!(
                "{:?} by {} at tick {} counted as dropped, but the thrower didn't die then",
                gr.kind, gr.thrower_name, gr.throw_tick
            ));
        }
    }
    let missing = g
        .iter()
        .filter(|g| g.detonate_tick.is_none() || g.detonate_pos.is_none())
        .count();
    if missing > 2 {
        fail(format!("{missing} of {} grenades never detonated", g.len()));
    }
    let tick_rate = stats.tick_rate;
    let smokes: Vec<f32> = g
        .iter()
        .filter(|g| g.kind == GrenadeKind::Smoke)
        .filter_map(|g| g.duration(tick_rate))
        .collect();
    let fires: Vec<f32> = g
        .iter()
        .filter(|g| g.kind.is_fire())
        .filter_map(|g| g.duration(tick_rate))
        .collect();
    if smokes.iter().any(|&d| !(0.0..=23.0).contains(&d)) || fires.iter().any(|&d| !(0.0..=8.0).contains(&d))
    {
        fail(format!("smoke durations {smokes:?}, fire durations {fires:?}"));
    }
    for gr in g {
        let off = (0..3)
            .map(|i| (gr.release_pos[i] - gr.throw_pos[i]).powi(2))
            .sum::<f32>()
            .sqrt();
        if off > 120.0 || gr.trajectory.is_empty() {
            fail(format!(
                "{:?} by {} at tick {}: released {off:.0} units from the thrower, {} trajectory points",
                gr.kind,
                gr.thrower_name,
                gr.throw_tick,
                gr.trajectory.len()
            ));
        }
    }
    let led = g.iter().filter(|g| g.flash_led_to_kill).count();
    let dropped = g.iter().filter(|g| g.dropped).count();
    let lineups = stats.lineups();
    let repeated = lineups.iter().filter(|l| l.count() > 1).count();
    let summary = format!(
        "{} grenades ({dropped} dropped by dying players, {missing} without detonation), {} smokes lasting {:.1} s on average, {led} flashes led to a kill, {} lineups ({repeated} thrown more than once)",
        g.len(),
        smokes.len(),
        smokes.iter().sum::<f32>() / smokes.len().max(1) as f32,
        lineups.len()
    );
    (summary, errors)
}

/// Aim: per-weapon kills match the kill feed, gun damage fits in the
/// player's damage, and the rates are those of professional players.
fn check_aim(stats: &MatchStats, other_damage: &HashMap<u64, u32>) -> (String, Vec<String>) {
    let mut errors = Vec::new();
    let mut fail = |msg: String| errors.push(msg);
    let gun = |w: &str| {
        !(w.contains("knife")
            || w.contains("bayonet")
            || matches!(
                w,
                "hegrenade"
                    | "inferno"
                    | "molotov"
                    | "incgrenade"
                    | "flashbang"
                    | "smokegrenade"
                    | "decoy"
                    | "planted_c4"
                    | "world"
                    | ""
            ))
    };
    for p in &stats.players {
        let a = &p.aim;
        let feed_gun_kills = stats
            .kills
            .iter()
            .filter(|k| k.attacker == Some(p.steam_id) && !k.team_kill && gun(&k.weapon))
            .count() as u32;
        let weapon_kills: u32 = a.weapons.iter().map(|w| w.kills).sum();
        if weapon_kills != feed_gun_kills {
            fail(format!(
                "{}: {weapon_kills} kills by weapon, {feed_gun_kills} gun kills in the feed",
                p.name
            ));
        }
        // Damage is guns, HE and fire, plus knives and grenade impacts (whose
        // raw damage bounds the rest).
        let gun_damage: u32 = a.weapons.iter().map(|w| w.damage).sum();
        let other = other_damage.get(&p.steam_id).copied().unwrap_or(0);
        if gun_damage + p.utility_damage > p.damage || gun_damage + p.utility_damage + other < p.damage {
            fail(format!(
                "{}: gun damage {gun_damage} + utility {} (+ knife and impacts up to {other}) vs damage {}",
                p.name, p.utility_damage, p.damage
            ));
        }
        if a.hits > a.shots
            || a.head_hits > a.hits
            || a.first_shot_hits > a.engagements
            || a.engagement_kills > p.kills
        {
            fail(format!("{}: inconsistent aim counters {a:?}", p.name));
        }
        if a.shots < 100 {
            continue;
        }
        let ranges = [
            ("accuracy", a.accuracy(), 0.08, 0.6),
            ("head hits", a.head_hit_ratio(), 0.05, 0.7),
            ("first-shot accuracy", a.first_shot_accuracy(), 0.1, 0.9),
            ("accurate-speed shots", a.accurate_speed_ratio(), 0.3, 1.0),
            (
                "crosshair placement",
                a.crosshair_placement().unwrap_or(-1.0),
                0.5,
                25.0,
            ),
            ("time to kill", a.time_to_kill().unwrap_or(-1.0), 0.0, 2.0),
        ];
        for (what, v, lo, hi) in ranges {
            if !(lo..=hi).contains(&v) {
                fail(format!("{}: {what} {v:.3} outside [{lo}, {hi}]", p.name));
            }
        }
    }
    let mut parts = Vec::new();
    for t in &stats.teams {
        let mut team = cs2_demo::stats::AimStats::default();
        for p in stats.players.iter().filter(|p| p.team_name == t.name) {
            team.merge(&p.aim);
        }
        parts.push(format!(
            "{}: accuracy {:.0}%, head {:.0}%, first shot {:.0}%, still {:.0}%, crosshair {:.1}°, TTK {:.2} s",
            t.name,
            team.accuracy() * 100.0,
            team.head_hit_ratio() * 100.0,
            team.first_shot_accuracy() * 100.0,
            team.accurate_speed_ratio() * 100.0,
            team.crosshair_placement().unwrap_or(f32::NAN),
            team.time_to_kill().unwrap_or(f32::NAN),
        ));
    }
    (parts.join(" | "), errors)
}

/// Positions land on the map's radar image, and spread over most of it (a
/// wrong scale or offset would squeeze them or push them off the image).
fn check_radar(stats: &MatchStats) -> (String, Vec<String>) {
    let mut errors = Vec::new();
    let Some(overview) = stats.map_overview() else {
        return (format!("no built-in radar for {}", stats.map), errors);
    };
    let mut points: Vec<[f32; 3]> = Vec::new();
    for k in &stats.kills {
        points.extend(k.attacker_pos);
        points.extend(k.victim_pos);
    }
    for r in &stats.rounds {
        points.extend(r.setup.iter().map(|s| s.pos));
    }
    for g in &stats.grenades {
        points.push(g.throw_pos);
        points.extend(g.detonate_pos);
    }
    let outside: Vec<_> = points.iter().filter(|p| !overview.contains(**p, 8.0)).collect();
    if !outside.is_empty() {
        errors.push(format!(
            "{} of {} positions are off the radar, e.g. {:?} -> {:?}",
            outside.len(),
            points.len(),
            outside[0],
            overview.to_radar(*outside[0])
        ));
    }
    let px: Vec<[f32; 2]> = points.iter().map(|p| overview.to_radar(*p)).collect();
    let span = |i: usize| {
        let (lo, hi) = px
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[i]), hi.max(p[i])));
        (lo, hi)
    };
    let ((x0, x1), (y0, y1)) = (span(0), span(1));
    if x1 - x0 < 400.0 || y1 - y0 < 400.0 {
        errors.push(format!(
            "positions only cover x {x0:.0}..{x1:.0}, y {y0:.0}..{y1:.0} of the radar"
        ));
    }
    (
        format!(
            "radar: {} positions within x {x0:.0}..{x1:.0}, y {y0:.0}..{y1:.0}",
            points.len()
        ),
        errors,
    )
}

/// `match_stats_many` gives the same result as `match_stats`, and scouting
/// profiles add up the matches they come from.
#[test]
fn scouting_across_demos() {
    let demos = all_demos();
    if demos.is_empty() {
        eprintln!("skipping: no demos in {}", demo_dir().display());
        return;
    }
    let all: Vec<_> = cs2_demo::match_stats_many(&demos)
        .into_iter()
        .map(|r| r.expect("parse"))
        .collect();
    let smallest = demos
        .iter()
        .enumerate()
        .min_by_key(|(_, p)| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()))
        .unwrap();
    let alone = Demo::open(smallest.1).unwrap().match_stats().unwrap();
    assert!(alone == all[smallest.0], "parallel and single parses differ");

    let mut scouting = cs2_demo::scouting::Scouting::new();
    for m in &all {
        scouting.add(m.clone());
    }
    let names = scouting.teams();
    for name in &names {
        let team = scouting.team(name).unwrap();
        let played: Vec<&MatchStats> = all
            .iter()
            .filter(|m| {
                m.teams.iter().any(|t| {
                    cs2_demo::scouting::normalize_team_name(&t.name)
                        == cs2_demo::scouting::normalize_team_name(name)
                })
            })
            .collect();
        assert_eq!(team.matches as usize, played.len(), "{name}");
        let rounds: u32 = team.maps.iter().map(|m| m.rounds().played).sum();
        let expected: usize = played.iter().map(|m| m.rounds.len()).sum();
        assert_eq!(rounds as usize, expected, "{name} rounds");
        let pistols: u32 = team.maps.iter().map(|m| m.pistols.played).sum();
        assert!(pistols as usize <= 2 * played.len(), "{name} pistols");
        for p in &team.players {
            let kills: u32 = played
                .iter()
                .filter_map(|m| m.player(p.steam_id))
                .map(|s| s.kills)
                .sum();
            assert_eq!(p.stats.kills, kills, "{} kills", p.name);
        }
        let first = team.maps.first().unwrap();
        eprintln!(
            "{name}: {} matches, {} won; {}: T {}/{}, CT {}/{}, pistols {}/{}, plants A {} B {}, top CT setup {:?}",
            team.matches,
            team.wins,
            first.map,
            first.t.won,
            first.t.played,
            first.ct.won,
            first.ct.played,
            first.pistols.won,
            first.pistols.played,
            first.plants_a,
            first.plants_b,
            first.ct_setups.first().map(|f| (&f.what, f.count))
        );
    }
}

/// With the `serde` feature, a match saved as JSON loads back unchanged (to
/// scout from saved stats without parsing the demos again).
#[cfg(feature = "serde")]
#[test]
fn stats_json_round_trip() {
    let Some(d) = any_demo() else { return };
    let stats = d.match_stats().unwrap();
    let json = serde_json::to_string(&stats).unwrap();
    let back: MatchStats = serde_json::from_str(&json).unwrap();
    assert!(back == stats, "JSON round trip changed the stats");
}
