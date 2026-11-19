//! Your analysis and the built-in statistics in one parse: a tuple of
//! visitors gets every callback. Here, a visitor records the killer's health at
//! each kill (game state at the moment of the event), next to `MatchStats`.
//!
//! cargo run --release --example combine -- match.dem

use cs2_demo::{Context, Demo, GameEvent, ParseOptions, StatsCollector, Visitor};
use std::collections::HashMap;

/// Kills made with 20 health or less, per SteamID.
#[derive(Default)]
struct LowHealthKills(HashMap<u64, u32>);

impl Visitor for LowHealthKills {
    fn on_game_event(&mut self, ctx: &Context, ev: &GameEvent) {
        if &*ev.name != "player_death" {
            return;
        }
        let warmup = ctx.game_rules().is_some_and(|g| g.is_warmup());
        let killer = ev.player_slot("attacker").and_then(|s| ctx.player_by_slot(s));
        let victim = ev.player_slot("userid").and_then(|s| ctx.player_by_slot(s));
        if let (false, Some(k), Some(v)) = (warmup, killer, victim)
            && k.team() != v.team()
            && k.is_alive()
            && k.health() <= 20
        {
            *self.0.entry(k.steam_id()).or_default() += 1;
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: cargo run --release --example combine -- <match.dem>")?;
    let demo = Demo::open(&path)?;

    // Both visitors run in the same pass over the file. Nest tuples for more:
    // `((a, b), c)`.
    let mut both = (StatsCollector::new(), LowHealthKills::default());
    let ctx = demo.parse(ParseOptions::default(), &mut both)?;
    let (collector, low_health) = both;

    // `parse` returns the `Context` as it was at the end of the demo; the
    // collector needs it to finish the statistics.
    let stats = collector.into_stats(&ctx);

    for p in &stats.players {
        println!(
            "{:16} {:>2} kills, {:>2} with 20 hp or less, rating {:.2}",
            p.name,
            p.kills,
            low_health.0.get(&p.steam_id).copied().unwrap_or(0),
            p.rating()
        );
    }

    // The final `Context` also says whether every entity update decoded
    // exactly. Mismatches point to a game update the parser doesn't know yet.
    let d = ctx.entity_diagnostics();
    println!(
        "\n{} entity updates checked against the server's bit lengths, {} mismatches",
        d.verified_updates, d.size_mismatches
    );
    Ok(())
}
