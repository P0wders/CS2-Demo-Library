//! Samples every living player's position as CSV (for heatmaps, 2D replays, ...).
//!
//! cargo run --release --example positions -- demo.dem [every_n_ticks] > positions.csv
//!
//! Columns: tick,round,name,steam_id,team,x,y,z,yaw,health,armor,money,weapon,place

use cs2_demo::{Context, Demo, GameEvent, ParseOptions, Visitor};
use std::io::{BufWriter, Write};
use std::ops::ControlFlow;

struct Positions<W: Write> {
    out: W,
    every: i32,
    round: u32,
    live: bool,
}

impl<W: Write> Visitor for Positions<W> {
    fn on_game_event(&mut self, ctx: &Context, ev: &GameEvent) {
        match &*ev.name {
            "begin_new_match" => self.round = 1,
            "round_start" if self.round > 0 && ctx.game_rules().is_some_and(|g| !g.is_warmup()) => {
                // begin_new_match already opened round 1 a tick earlier.
                if self.live {
                    self.round += 1;
                }
                self.live = true;
            }
            _ => {}
        }
    }

    fn on_tick_end(&mut self, ctx: &Context) -> ControlFlow<()> {
        if self.round == 0 || ctx.tick() % self.every != 0 {
            return ControlFlow::Continue(());
        }
        for p in ctx.active_players().filter(|p| p.is_alive()) {
            let Some([x, y, z]) = p.position() else { continue };
            let yaw = p.eye_angles().map_or(0.0, |a| a[1]);
            let weapon = p.active_weapon().map(|w| w.name()).unwrap_or_default();
            let _ = writeln!(
                self.out,
                "{},{},{},{},{},{:.1},{:.1},{:.1},{:.1},{},{},{},{},{}",
                ctx.tick(),
                self.round,
                p.name().replace(',', " "),
                p.steam_id(),
                p.team().short_name(),
                x,
                y,
                z,
                yaw,
                p.health(),
                p.armor(),
                p.money(),
                weapon,
                p.place().unwrap_or_default()
            );
        }
        ControlFlow::Continue(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: positions <demo.dem> [every_n_ticks]")?;
    let every: i32 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(64);
    let demo = Demo::open(&path)?;
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    writeln!(
        out,
        "tick,round,name,steam_id,team,x,y,z,yaw,health,armor,money,weapon,place"
    )?;
    let mut v = Positions {
        out,
        every: every.max(1),
        round: 0,
        live: false,
    };
    demo.parse(ParseOptions::default(), &mut v)?;
    v.out.flush()?;
    Ok(())
}
