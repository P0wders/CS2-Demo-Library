//! Reading the game state: a `Visitor` that looks at the `Context` every
//! N ticks and prints the score and every living player (position, callout,
//! health, armor, money, weapon). It stops the parse early after a few rounds.
//!
//! cargo run --release --example game_state -- match.dem [every_n_ticks] [rounds]
//!
//! Defaults: every 64 ticks (one second), the first 2 rounds.

use cs2_demo::{Context, Demo, ParseOptions, Team, Visitor};
use std::ops::ControlFlow;

struct Sampler {
    every: i32,
    rounds: i32,
}

impl Visitor for Sampler {
    // Called once per tick, after all of the tick's updates and events.
    fn on_tick_end(&mut self, ctx: &Context) -> ControlFlow<()> {
        // Match state comes from the game rules entity.
        let Some(rules) = ctx.game_rules() else {
            return ControlFlow::Continue(());
        };
        if rules.total_rounds_played() >= self.rounds {
            // Returning `Break` ends the parse here. (To stop at a known tick,
            // set `ParseOptions::end_tick` instead.)
            return ControlFlow::Break(());
        }
        if rules.is_warmup() || ctx.tick() % self.every != 0 {
            return ControlFlow::Continue(());
        }

        let score = |side| ctx.team(side).map_or(0, |t| t.score());
        println!(
            "tick {} ({:.0}s), round {}{}, T {} - {} CT",
            ctx.tick(),
            ctx.tick() as f32 / ctx.tick_rate(),
            rules.total_rounds_played() + 1,
            if rules.is_freeze_period() {
                " (freeze time)"
            } else {
                ""
            },
            score(Team::Terrorist),
            score(Team::CounterTerrorist),
        );

        // Typed views over the entities: `Player`, `TeamInfo`, `GameRules`.
        for p in ctx.active_players().filter(|p| p.is_alive()) {
            let [x, y, z] = p.position().unwrap_or_default();
            println!(
                "  {:2} {:16} ({:>6.0}, {:>6.0}, {:>5.0}) {:14} {:>3} hp {:>3} armor ${:<5} {}",
                p.team().short_name(),
                p.name(),
                x,
                y,
                z,
                p.place().unwrap_or_default(),
                p.health(),
                p.armor(),
                p.money(),
                p.active_weapon().map(|w| w.name()).unwrap_or_default(),
            );
        }

        // Any networked property is readable by name, for what the typed views
        // don't cover: on the controller (the player's slot: name, money,
        // scoreboard) or on the pawn (the body in the world). Nested fields use
        // dots. List them with `cargo run --example dump_schema -- match.dem
        // CCSPlayerPawn`.
        if let Some(p) = ctx.active_players().find(|p| p.is_alive()) {
            let cash_spent: Option<i32> = p.controller().get("m_pInGameMoneyServices.m_iCashSpentThisRound");
            let ducked: Option<bool> = p
                .pawn()
                .and_then(|pawn| pawn.get("m_pMovementServices.m_bDucked"));
            println!(
                "  raw properties of {}: cash spent this round {cash_spent:?}, ducked {ducked:?}",
                p.name()
            );
        }
        ControlFlow::Continue(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: cargo run --release --example game_state -- <match.dem> [every_n_ticks] [rounds]")?;
    let every: i32 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(64);
    let rounds: i32 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(2);

    let demo = Demo::open(&path)?;
    // The default options decode entities, which the game state needs.
    let mut sampler = Sampler {
        every: every.max(1),
        rounds,
    };
    let ctx = demo.parse(ParseOptions::default(), &mut sampler)?;
    println!("stopped at tick {}", ctx.tick());
    Ok(())
}
