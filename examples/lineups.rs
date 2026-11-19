//! Every grenade of a demo as CSV: who threw it, from where, how (view
//! angles, jump, crouch, speed), where it landed and what it did. With
//! `--lineups`, the throws grouped into lineups instead.
//!
//! cargo run --release --example lineups -- match.dem > grenades.csv
//! cargo run --release --example lineups -- match.dem --lineups > lineups.csv

use cs2_demo::Demo;
use std::io::{BufWriter, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut path = None;
    let mut grouped = false;
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--lineups" => grouped = true,
            _ => path = Some(a),
        }
    }
    let path = path.ok_or("usage: lineups <demo.dem> [--lineups] > out.csv")?;
    let stats = Demo::open(&path)?.match_stats()?;
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let csv = |s: &str| {
        if s.contains([',', '"', '\n']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };
    let opt = |v: Option<f32>| v.map_or(String::new(), |v| format!("{v:.1}"));

    if grouped {
        writeln!(
            out,
            "kind,side,count,throwers,throw_place,throw_x,throw_y,throw_z,pitch,yaw,jump_throw,land_x,land_y,land_z"
        )?;
        for l in stats.lineups() {
            let throwers: Vec<String> = l
                .throwers
                .iter()
                .map(|id| {
                    stats
                        .player(*id)
                        .map_or_else(|| id.to_string(), |p| p.name.clone())
                })
                .collect();
            writeln!(
                out,
                "{},{},{},{},{},{:.0},{:.0},{:.0},{:.2},{:.2},{},{:.0},{:.0},{:.0}",
                l.kind.weapon_name(),
                l.side.short_name(),
                l.count(),
                csv(&throwers.join(" ")),
                csv(l.throw_place.as_deref().unwrap_or("")),
                l.throw_pos[0],
                l.throw_pos[1],
                l.throw_pos[2],
                l.throw_angles[0],
                l.throw_angles[1],
                l.jump_throw,
                l.land_pos[0],
                l.land_pos[1],
                l.land_pos[2]
            )?;
        }
        return Ok(out.flush()?);
    }

    writeln!(
        out,
        "round,tick,round_time,kind,thrower,steam_id,side,throw_place,throw_x,throw_y,throw_z,pitch,yaw,\
         jump_throw,ducking,speed,dropped,land_x,land_y,land_z,airtime,enemies_blinded,enemy_blind_s,\
         teammates_blinded,flash_led_to_kill,enemy_damage,team_damage,duration"
    )?;
    let rate = stats.tick_rate;
    for g in &stats.grenades {
        let land = g.land_pos();
        writeln!(
            out,
            "{},{},{},{},{},{},{},{},{:.0},{:.0},{:.0},{:.2},{:.2},{},{},{:.0},{},{},{},{},{},{},{:.2},{},{},{},{},{}",
            g.round,
            g.throw_tick,
            opt(g.throw_round_time),
            g.kind.weapon_name(),
            csv(&g.thrower_name),
            g.thrower,
            g.side.short_name(),
            csv(g.throw_place.as_deref().unwrap_or("")),
            g.throw_pos[0],
            g.throw_pos[1],
            g.throw_pos[2],
            g.throw_angles[0],
            g.throw_angles[1],
            g.jump_throw,
            g.ducking,
            g.throw_speed,
            g.dropped,
            land.map_or(String::new(), |p| format!("{:.0}", p[0])),
            land.map_or(String::new(), |p| format!("{:.0}", p[1])),
            land.map_or(String::new(), |p| format!("{:.0}", p[2])),
            opt(g.detonate_tick.map(|d| (d - g.throw_tick) as f32 / rate)),
            g.enemies_blinded(),
            g.enemy_blind_time(),
            g.teammates_blinded(),
            g.flash_led_to_kill,
            g.enemy_damage,
            g.team_damage,
            opt(g.duration(rate))
        )?;
    }
    Ok(out.flush()?)
}
