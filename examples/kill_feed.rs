//! The kill feed: every kill with the weapon, where both players stood and
//! the special cases (headshot, wallbang, through smoke, traded, ...), then
//! kills per weapon and each round's opening duel.
//!
//! cargo run --release --example kill_feed -- match.dem

use cs2_demo::Demo;
use std::collections::BTreeMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: cargo run --release --example kill_feed -- <match.dem>")?;
    let stats = Demo::open(&path)?.match_stats()?;

    // `stats.kills` is in game order. Player fields are SteamIDs and names
    // as they were at the time; `stats.player(steam_id)` finds the totals.
    for k in &stats.kills {
        let mut flags = Vec::new();
        for (on, name) in [
            (k.headshot, "headshot"),
            (k.wallbang, "wallbang"),
            (k.noscope, "no-scope"),
            (k.through_smoke, "through smoke"),
            (k.attacker_blind, "attacker blind"),
            (k.team_kill, "team kill"),
            (k.traded, "traded"),
        ] {
            if on {
                flags.push(name);
            }
        }
        println!(
            "round {:>2} {:>6}  {} [{}] -> {} [{}]  {} {}{}",
            k.round,
            k.round_time.map_or(String::new(), |t| format!("{t:.1}s")),
            k.attacker_name.as_deref().unwrap_or("world"),
            k.attacker_place.as_deref().unwrap_or("?"),
            k.victim_name,
            k.victim_place.as_deref().unwrap_or("?"),
            k.weapon,
            k.distance.map_or(String::new(), |d| format!("{d:.0}u ")),
            if flags.is_empty() {
                String::new()
            } else {
                format!("({})", flags.join(", "))
            },
        );
    }

    // Kills per weapon, team kills excluded.
    let mut per_weapon: BTreeMap<&str, u32> = BTreeMap::new();
    for k in stats
        .kills
        .iter()
        .filter(|k| !k.team_kill && k.attacker.is_some())
    {
        *per_weapon.entry(&k.weapon).or_default() += 1;
    }
    let mut per_weapon: Vec<_> = per_weapon.into_iter().collect();
    per_weapon.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
    println!("\nkills per weapon:");
    for (weapon, n) in per_weapon {
        println!("  {weapon:16} {n}");
    }

    // The opening duel of each round is its first kill.
    println!("\nopening duels:");
    for r in &stats.rounds {
        if let Some(k) = stats.kills.iter().find(|k| k.round == r.number) {
            println!(
                "  round {:>2}: {} ({}) killed {} ({}) at {}",
                r.number,
                k.attacker_name.as_deref().unwrap_or("world"),
                k.attacker_side.map_or("?", |s| s.short_name()),
                k.victim_name,
                k.victim_side.short_name(),
                k.victim_place.as_deref().unwrap_or("?"),
            );
        }
    }
    Ok(())
}
