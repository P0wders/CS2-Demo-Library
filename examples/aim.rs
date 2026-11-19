//! Aim: accuracy, head hits, first-shot accuracy, counter-strafing,
//! crosshair placement and time to kill per player, then per weapon.
//!
//! cargo run --release --example aim -- match.dem

use cs2_demo::Demo;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: cargo run --release --example aim -- <match.dem>")?;
    let stats = Demo::open(&path)?.match_stats()?;

    // `PlayerStats::aim` is an `AimStats`: raw counters (shots, hits,
    // engagements, ...) and methods for the rates. Rates are in [0, 1].
    println!(
        "{:16} {:>5} {:>5} {:>6} {:>6} {:>8} {:>10} {:>5}",
        "player", "shots", "acc", "head", "first", "stopped", "crosshair", "TTK"
    );
    for p in &stats.players {
        let a = &p.aim;
        println!(
            "{:16} {:>5} {:>4.0}% {:>5.0}% {:>5.0}% {:>7.0}% {:>10} {:>5}",
            p.name,
            a.shots,
            a.accuracy() * 100.0,
            a.head_hit_ratio() * 100.0,
            a.first_shot_accuracy() * 100.0,
            a.accurate_speed_ratio() * 100.0,
            // Median angle between the crosshair and the enemy's head just
            // before the first shot of an engagement.
            a.crosshair_placement()
                .map_or("-".to_string(), |deg| format!("{deg:.1}°")),
            // Median seconds from that first shot to the kill.
            a.time_to_kill().map_or("-".to_string(), |s| format!("{s:.2}s")),
        );
    }

    // `AimStats::weapons` splits the same numbers per weapon.
    for p in &stats.players {
        println!("\n{}:", p.name);
        for w in p.aim.weapons.iter().filter(|w| w.shots > 0) {
            println!(
                "  {:14} {:>4} shots, {:>3.0}% hit, {:>3} head hits, {:>2} kills ({} headshots), {:>4} damage",
                w.weapon,
                w.shots,
                w.accuracy() * 100.0,
                w.head_hits,
                w.kills,
                w.headshot_kills,
                w.damage
            );
        }
    }
    Ok(())
}
