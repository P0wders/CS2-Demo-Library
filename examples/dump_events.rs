//! Prints game events as they happen, with player names resolved.
//!
//! cargo run --release --example dump_events -- demo.dem [event_name,event_name,...]
//!
//! Example: `dump_events match.dem round_start,round_end,player_death`

use cs2_demo::{Context, Demo, EventValue, GameEvent, ParseOptions, Visitor};
use std::collections::HashSet;

struct Dump {
    filter: Option<HashSet<String>>,
}

impl Visitor for Dump {
    fn on_game_event(&mut self, ctx: &Context, ev: &GameEvent) {
        if let Some(f) = &self.filter
            && !f.contains(&*ev.name)
        {
            return;
        }
        let rules = ctx.game_rules();
        let mut line = format!(
            "[{:>6}] {}{}",
            ev.tick,
            if rules.is_some_and(|g| g.is_warmup()) {
                "(warmup) "
            } else {
                ""
            },
            ev.name
        );
        for (k, v) in &ev.fields {
            let resolved = match (&**k, v) {
                ("userid" | "attacker" | "assister", EventValue::Int(slot)) => ctx
                    .player_by_slot(*slot)
                    .map(|p| format!("{}({})", p.name(), p.team().short_name())),
                _ => None,
            };
            match resolved {
                Some(r) => line.push_str(&format!(" {k}={r}")),
                None => line.push_str(&format!(" {k}={v}")),
            }
        }
        println!("{line}");
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: dump_events <demo.dem> [event,event,...]")?;
    let filter = args.next().map(|f| f.split(',').map(str::to_owned).collect());
    let demo = Demo::open(&path)?;
    demo.parse(ParseOptions::default(), &mut Dump { filter })?;
    Ok(())
}
