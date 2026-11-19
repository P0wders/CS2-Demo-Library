//! Your first `Visitor`: game events (kills, damage, bomb, purchases, ...)
//! as the parser reads them, with player names resolved.
//!
//! Without a filter, lists every event name in the demo with its count. With
//! one, prints those events and their fields:
//!
//! cargo run --release --example events -- match.dem
//! cargo run --release --example events -- match.dem player_death,bomb_planted

use cs2_demo::{Context, Demo, EventValue, GameEvent, ParseOptions, Visitor};
use std::collections::{BTreeMap, HashSet};

#[derive(Default)]
struct Events {
    filter: Option<HashSet<String>>,
    counts: BTreeMap<String, u32>,
}

// Every `Visitor` method has an empty default: implement only what you need.
impl Visitor for Events {
    fn on_game_event(&mut self, ctx: &Context, ev: &GameEvent) {
        *self.counts.entry(ev.name.to_string()).or_default() += 1;
        let Some(filter) = &self.filter else { return };
        if !filter.contains(&*ev.name) {
            return;
        }
        let mut line = format!("[{:>6}] {}", ev.tick, ev.name);
        for (key, value) in &ev.fields {
            // Player fields (`userid`, `attacker`, `assister`, ...) hold player
            // slots. `ctx.player_info(slot)` gives the name and SteamID from
            // the player table, which is there even in events-only mode.
            // (With entities decoded, `ctx.player_by_slot(slot)` gives the
            // full `Player`: team, position, health, money, ...)
            let player = match (&**key, value) {
                ("userid" | "attacker" | "assister", EventValue::Int(slot)) => ctx.player_info(*slot),
                _ => None,
            };
            match player {
                Some(p) => line.push_str(&format!(" {key}={}", p.name)),
                None => line.push_str(&format!(" {key}={value}")),
            }
        }
        println!("{line}");
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: cargo run --release --example events -- <match.dem> [event,event,...]")?;
    let mut events = Events {
        filter: args.next().map(|f| f.split(',').map(str::to_owned).collect()),
        ..Events::default()
    };

    // Events don't need entity state: `events_only()` skips it and parses
    // about 15 times faster.
    Demo::open(&path)?.parse(ParseOptions::events_only(), &mut events)?;

    if events.filter.is_none() {
        for (name, n) in &events.counts {
            println!("{n:>7}  {name}");
        }
    }

    // To read one field with its type: `ev.i32("dmg_health")`,
    // `ev.str("weapon")`, `ev.bool("headshot")`, `ev.player_slot("attacker")`,
    // or `ev.get(key)` for the raw `EventValue`.
    Ok(())
}
