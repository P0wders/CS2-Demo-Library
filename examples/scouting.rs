//! Scouting a team across many demos: per map, its record, T and CT rounds,
//! pistols, buys, bomb sites, setups, opening duels and lineups, then its
//! players.
//!
//! cargo run --release --example scouting -- "<team>" <demo or folder> [more ...]
//!
//! Without a team that played in the demos, lists the teams found.

use cs2_demo::scouting::{Frequency, Scouting};
use cs2_demo::stats::BuyType;
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let usage = "usage: cargo run --release --example scouting -- \"<team>\" <demo or folder> [more ...]";
    let mut args = std::env::args().skip(1);
    let team = args.next().ok_or(usage)?;
    let mut paths = Vec::new();
    for arg in args {
        find_demos(Path::new(&arg), &mut paths);
    }
    if paths.is_empty() {
        return Err(usage.into());
    }

    // Collect the `MatchStats` of every demo.
    let mut scouting = Scouting::new();
    for (path, stats) in paths.iter().zip(cs2_demo::match_stats_many(&paths)) {
        match stats {
            Ok(stats) => scouting.add(stats),
            Err(e) => eprintln!("skipped {}: {e}", path.display()),
        }
    }
    // Names are compared ignoring case, punctuation and a leading "Team".
    // For a renamed team: `scouting.alias("Old Name", "New Name")`.
    let Some(profile) = scouting.team(&team) else {
        println!("\"{team}\" is in none of these demos. Teams found:");
        for name in scouting.teams() {
            println!("  {name}");
        }
        return Ok(());
    };

    println!(
        "{}: {} matches, {} won\n",
        profile.name, profile.matches, profile.wins
    );
    // A `Record` is played / won, with `rate()` in [0, 1].
    for m in &profile.maps {
        println!("{}: won {} of {}", m.map, m.wins, m.matches);
        println!(
            "  rounds: T {}/{} ({:.0}%), CT {}/{} ({:.0}%), pistols {}/{}",
            m.t.won,
            m.t.played,
            m.t.rate() * 100.0,
            m.ct.won,
            m.ct.played,
            m.ct.rate() * 100.0,
            m.pistols.won,
            m.pistols.played
        );
        let buys: Vec<String> = BuyType::ALL
            .iter()
            .map(|&b| (b, m.buy(b)))
            .filter(|(_, r)| r.played > 0)
            .map(|(b, r)| format!("{} {}/{}", b.label(), r.won, r.played))
            .collect();
        println!("  rounds won by own buy: {}", buys.join(", "));
        println!(
            "  plants: A {}, B {}, on average {}",
            m.plants_a,
            m.plants_b,
            m.avg_plant_time
                .map_or("-".to_string(), |s| format!("{s:.0}s after freeze time"))
        );
        println!(
            "  opening duels won: T {}/{}, CT {}/{}",
            m.t_openings.won, m.t_openings.played, m.ct_openings.won, m.ct_openings.played
        );
        println!("  CT setups 20s in: {}", top(&m.ct_setups, 3));
        println!("  T positions 20s in: {}", top(&m.t_setups, 3));
        println!(
            "  {:.1} grenades per round, {} repeated lineups\n",
            m.grenades_per_round,
            m.lineups.len()
        );
    }

    // `PlayerProfile::stats` is a `PlayerStats` summed over the matches, so
    // every rate method works on it.
    println!("players:");
    for p in &profile.players {
        println!(
            "  {:16} {} matches, rating {:.2}, ADR {:.1}, KAST {:.0}%, usual T spots: {}, CT spots: {}",
            p.name,
            p.matches,
            p.stats.rating(),
            p.stats.adr(),
            p.stats.kast() * 100.0,
            top(&p.t_places, 2),
            top(&p.ct_places, 2),
        );
    }
    Ok(())
}

/// The `n` most frequent entries as "what (count)".
fn top(list: &[Frequency], n: usize) -> String {
    let v: Vec<String> = list
        .iter()
        .take(n)
        .map(|f| format!("{} ({}x)", f.what, f.count))
        .collect();
    if v.is_empty() {
        "-".to_string()
    } else {
        v.join("; ")
    }
}

fn find_demos(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        let mut entries: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for p in entries {
            find_demos(&p, out);
        }
    } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dem")) {
        out.push(path.to_path_buf());
    }
}
