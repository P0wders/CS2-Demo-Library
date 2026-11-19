//! Compares a player's per-round deaths/kills from game events with the
//! server's own per-round counters (`m_perRoundStats`).
//!
//! cargo run --release --example round_check -- demo.dem <player name>

use cs2_demo::{Demo, Entity};

fn per_round(c: &Entity, field: &str) -> Vec<i32> {
    let n = c
        .get::<u32>("m_pActionTrackingServices.m_perRoundStats")
        .unwrap_or(0);
    (0..n)
        .map(|i| {
            c.get::<i32>(&format!("m_pActionTrackingServices.m_perRoundStats.{i}.{field}"))
                .unwrap_or(-1)
        })
        .collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: round_check <demo.dem> <player>")?;
    let who = args.next().ok_or("usage: round_check <demo.dem> <player>")?;
    let demo = Demo::open(&path)?;
    let mut collector = cs2_demo::StatsCollector::new();
    let ctx = demo.parse(cs2_demo::ParseOptions::default(), &mut collector)?;
    let stats = collector.into_stats(&ctx);
    let p = ctx
        .players()
        .find(|p| p.name() == who)
        .ok_or("player not found")?;
    let server_deaths = per_round(p.controller(), "m_iDeaths");
    let server_kills = per_round(p.controller(), "m_iKills");
    println!("server per-round entries: {}", server_deaths.len());
    for (i, (sd, sk)) in server_deaths.iter().zip(&server_kills).enumerate() {
        let round = i as u32 + 1;
        let ours_d = stats
            .kills
            .iter()
            .filter(|k| k.round == round && k.victim == p.steam_id())
            .count();
        let ours_k = stats
            .kills
            .iter()
            .filter(|k| k.round == round && k.attacker == Some(p.steam_id()) && !k.team_kill)
            .count();
        let flag = if *sd as usize != ours_d || *sk as usize != ours_k {
            "  <-- differs"
        } else {
            ""
        };
        println!("round {round:>2}: server K/D {sk}/{sd}  events K/D {ours_k}/{ours_d}{flag}");
    }
    Ok(())
}
