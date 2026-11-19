//! Start here: open a demo, read its header, and get every statistic of the
//! match with one call.
//!
//! cargo run --release --example quick_start -- match.dem

use cs2_demo::Demo;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: cargo run --release --example quick_start -- <match.dem>")?;

    // Opening is instant: the file is memory-mapped and only the header (and
    // footer) are read.
    let demo = Demo::open(&path)?;
    let h = demo.header();
    println!("map:     {}", h.map_name);
    println!("server:  {}", h.server_name);
    println!("build:   {} (network patch {})", h.build_num, h.patch_version);
    if let Some(seconds) = h.playback_time {
        println!("length:  {:.0} min", seconds / 60.0);
    }

    // One full parse, with every statistic the crate computes. The result is a
    // plain struct (`MatchStats`) you can keep, filter or serialize.
    let stats = demo.match_stats()?;

    println!();
    for t in &stats.teams {
        println!(
            "{:20} {:>2}  (first half {}, second half {}, overtime {})",
            t.name, t.score, t.score_first_half, t.score_second_half, t.score_overtime
        );
    }
    if let Some(w) = stats.winner() {
        println!("winner: {}", w.name);
    }

    // One `PlayerStats` per player: raw counters (kills, damage, opening
    // kills, ...) and methods for the usual rates. `t` and `ct` hold the same
    // numbers for each side.
    println!(
        "\n{:16} {:>3} {:>3} {:>3}  {:>5}  {:>4}  {:>4}  {:>6}  {:>5}  {:>5}",
        "player", "K", "D", "A", "ADR", "HS%", "KAST", "rating", "T", "CT"
    );
    for p in &stats.players {
        println!(
            "{:16} {:>3} {:>3} {:>3}  {:>5.1}  {:>3.0}%  {:>3.0}%  {:>6.2}  {:>5.2}  {:>5.2}",
            p.name,
            p.kills,
            p.deaths,
            p.assists,
            p.adr(),
            p.hs_ratio() * 100.0,
            p.kast() * 100.0,
            p.rating(),
            p.t.rating(),
            p.ct.rating(),
        );
    }

    println!(
        "\n{} rounds, {} kills, {} grenades, {} clutches",
        stats.rounds.len(),
        stats.kills.len(),
        stats.grenades.len(),
        stats.clutches.len()
    );
    Ok(())
}
