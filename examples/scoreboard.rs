//! Prints an HLTV-style scoreboard and round history.
//!
//! cargo run --release --example scoreboard -- demo.dem

use cs2_demo::{Demo, RoundEndReason, Team};
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: scoreboard <demo.dem>")?;
    let demo = Demo::open(&path)?;
    let start = Instant::now();
    let stats = demo.match_stats()?;
    let elapsed = start.elapsed();

    let team_line: Vec<String> = stats
        .teams
        .iter()
        .map(|t| format!("{} ({}) {}", t.name, t.team.short_name(), t.score))
        .collect();
    println!("{} | {}", stats.map, team_line.join(" - "));
    println!(
        "{} rounds, {} kills, parsed in {:.2?}\n",
        stats.rounds.len(),
        stats.kills.len(),
        elapsed
    );

    let mut current_team = None;
    for p in &stats.players {
        if current_team != Some(p.team) {
            current_team = Some(p.team);
            println!(
                "{:<18} {:>3} {:>3} {:>3} {:>4} {:>6} {:>5} {:>5} {:>4} {:>4} {:>6}",
                p.team_name, "K", "D", "A", "+/-", "ADR", "HS%", "KAST", "FK", "FD", "Rating"
            );
        }
        println!(
            "{:<18} {:>3} {:>3} {:>3} {:>+4} {:>6.1} {:>4.0}% {:>4.0}% {:>4} {:>4} {:>6.2}",
            p.name,
            p.kills,
            p.deaths,
            p.assists,
            p.kills as i32 - p.deaths as i32,
            p.adr(),
            p.hs_ratio() * 100.0,
            p.kast() * 100.0,
            p.opening_kills,
            p.opening_deaths,
            p.rating()
        );
    }

    println!("\nRounds:");
    for r in &stats.rounds {
        let reason = match r.reason {
            Some(RoundEndReason::TargetBombed) => "bomb exploded",
            Some(RoundEndReason::BombDefused) => "bomb defused",
            Some(RoundEndReason::TWin) | Some(RoundEndReason::CtWin) => "elimination",
            Some(RoundEndReason::TargetSaved) => "time ran out",
            Some(_) => "other",
            None => "?",
        };
        println!(
            "  {:>2}. {:<3} {:<12} {:<14} {:>2}-{:<2} ({} kills)",
            r.number,
            r.winner.map_or("?", Team::short_name),
            r.winner_name.as_deref().unwrap_or(""),
            reason,
            r.score_t,
            r.score_ct,
            r.kills
        );
    }
    Ok(())
}
