//! Utility: every grenade with who threw it, from where and how, where it
//! landed and what it did; per-player utility totals; and the lineups,
//! throws repeated from the same spot to the same place.
//!
//! cargo run --release --example grenades -- match.dem

use cs2_demo::Demo;
use cs2_demo::stats::GrenadeKind;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: cargo run --release --example grenades -- <match.dem>")?;
    let stats = Demo::open(&path)?.match_stats()?;
    let rate = stats.tick_rate;

    // One `Grenade` per projectile, in throw order.
    for g in &stats.grenades {
        // A grenade dropped by a player who died holding it primed is not a throw.
        if g.dropped {
            continue;
        }
        let effect = match g.kind {
            GrenadeKind::Flashbang => format!(
                "{} enemies blinded for {:.1}s in total, {} teammates{}",
                g.enemies_blinded(),
                g.enemy_blind_time(),
                g.teammates_blinded(),
                if g.flash_led_to_kill {
                    ", led to a kill"
                } else {
                    ""
                }
            ),
            GrenadeKind::HeGrenade => format!("{} damage ({} to teammates)", g.enemy_damage, g.team_damage),
            k if k.is_fire() => format!(
                "burned {:.1}s, {} damage",
                g.duration(rate).unwrap_or(0.0),
                g.enemy_damage
            ),
            GrenadeKind::Smoke => format!("smoke for {:.1}s", g.duration(rate).unwrap_or(0.0)),
            _ => String::new(),
        };
        println!(
            "round {:>2} {:>6}  {:10} {:16} {} from {} (pitch {:.1}, yaw {:.1}{}{}) -> {}  {}",
            g.round,
            g.throw_round_time.map_or(String::new(), |t| format!("{t:.1}s")),
            g.kind.label(),
            g.thrower_name,
            g.side.short_name(),
            g.throw_place.as_deref().unwrap_or("?"),
            g.throw_angles[0],
            g.throw_angles[1],
            if g.jump_throw { ", jump throw" } else { "" },
            if g.ducking { ", crouched" } else { "" },
            g.land_pos().map_or("?".to_string(), |p| format!(
                "({:.0}, {:.0}, {:.0})",
                p[0], p[1], p[2]
            )),
            effect,
        );
    }

    // The per-player totals are in `PlayerStats`.
    println!("\nutility per player:");
    for p in &stats.players {
        println!(
            "  {:16} {} flashes, {} smokes, {} HE, {} fire | {} enemies flashed, {} utility damage, ${} unused at death",
            p.name,
            p.flashbangs_thrown,
            p.smokes_thrown,
            p.he_thrown,
            p.molotovs_thrown,
            p.enemies_flashed,
            p.utility_damage,
            p.utility_unused_value,
        );
    }

    // `lineups()` groups throws from the same spot, with the same aim, that
    // landed in the same place.
    let mut lineups = stats.lineups();
    lineups.sort_by_key(|l| std::cmp::Reverse(l.count()));
    println!("\nrepeated lineups:");
    for l in lineups.iter().filter(|l| l.count() > 1) {
        let throwers: Vec<&str> = l
            .throwers
            .iter()
            .filter_map(|id| stats.player(*id).map(|p| p.name.as_str()))
            .collect();
        println!(
            "  {}x {} ({}) from {} at ({:.0}, {:.0}), pitch {:.1} yaw {:.1}{} -> ({:.0}, {:.0})  by {}",
            l.count(),
            l.kind.label(),
            l.side.short_name(),
            l.throw_place.as_deref().unwrap_or("?"),
            l.throw_pos[0],
            l.throw_pos[1],
            l.throw_angles[0],
            l.throw_angles[1],
            if l.jump_throw { " (jump throw)" } else { "" },
            l.land_pos[0],
            l.land_pos[1],
            throwers.join(", "),
        );
    }
    Ok(())
}
