//! Round by round: who won and how, each team's buy, the bomb, one player's
//! rounds, and the clutches. Also shows how to change where the buy types
//! (eco, force buy, ...) start, by running the `StatsCollector` yourself.
//!
//! cargo run --release --example rounds -- match.dem

use cs2_demo::stats::BuyThresholds;
use cs2_demo::{Demo, ParseOptions, StatsCollector, Team};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: cargo run --release --example rounds -- <match.dem>")?;
    let demo = Demo::open(&path)?;

    // `demo.match_stats()` is this, with `StatsCollector::new()`. Running the
    // collector yourself lets you configure it: here a force buy lasts up to
    // $4,500 per player instead of $4,000.
    let thresholds = BuyThresholds {
        force: 4_500,
        ..BuyThresholds::DEFAULT
    };
    let mut collector = StatsCollector::with_buy_thresholds(thresholds);
    let ctx = demo.parse(ParseOptions::default(), &mut collector)?;
    let stats = collector.into_stats(&ctx);

    // Every round: winner, how it ended, both teams' economy and the bomb.
    println!("round  score  winner                end           T buy      CT buy     bomb");
    for r in &stats.rounds {
        let buy = |side| r.economy(side).map_or("?", |e| e.buy.label());
        let bomb = match (r.bomb_site, r.bomb_defused, r.bomb_exploded) {
            (Some(site), true, _) => format!("{} (defused)", site.name()),
            (Some(site), _, true) => format!("{} (exploded)", site.name()),
            (Some(site), _, _) => site.name().to_string(),
            (None, ..) => String::new(),
        };
        println!(
            "{:>5}  {:>2}-{:<2}  {:20}  {:12}  {:9}  {:9}  {}",
            r.number,
            r.score_t,
            r.score_ct,
            r.winner_name.as_deref().unwrap_or("?"),
            r.reason.map_or("?".to_string(), |reason| format!("{reason:?}")),
            buy(Team::Terrorist),
            buy(Team::CounterTerrorist),
            bomb,
        );
    }

    // Every round of one player (here the top fragger): `RoundSummary::player`
    // gives their `PlayerRound`, with kills, damage, KAST and money.
    if let Some(top) = stats.players.iter().max_by_key(|p| p.kills) {
        println!("\n{}, round by round:", top.name);
        for r in &stats.rounds {
            let Some(pr) = r.player(top.steam_id) else {
                continue;
            };
            println!(
                "  round {:>2} as {:2}: {} kills, {:>3} damage, {}, start money ${}, equipment ${}{}",
                r.number,
                pr.side.map_or("?", |s| s.short_name()),
                pr.kills,
                pr.damage,
                if pr.died { "died" } else { "survived" },
                pr.money_start,
                pr.equipment_value,
                if pr.opening_kill { ", opening kill" } else { "" },
            );
        }
    }

    // Clutches: the last player alive on a side against 1 to 5 enemies.
    println!("\nclutches:");
    for c in &stats.clutches {
        println!(
            "  round {:>2}: {} ({}) 1v{}: {}, {} kills",
            c.round,
            c.name,
            c.side.short_name(),
            c.opponents,
            if c.won { "won" } else { "lost" },
            c.kills
        );
    }
    Ok(())
}
