//! Saving and loading results as JSON. With the `serde` feature, `MatchStats`
//! and everything in it (as well as replay frames, map overviews and scouting
//! profiles) implement `Serialize` and `Deserialize`: parse a demo once, keep
//! the JSON, and load it back later without the demo.
//!
//! cargo run --release --features serde --example json -- match.dem [stats.json]

use cs2_demo::{Demo, MatchStats};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: cargo run --release --features serde --example json -- <match.dem> [stats.json]")?;
    let out = args.next().unwrap_or_else(|| "stats.json".to_string());

    let stats = Demo::open(&path)?.match_stats()?;

    // Any serde format works; JSON here (`serde_json` in your Cargo.toml).
    let json = serde_json::to_string_pretty(&stats)?;
    std::fs::write(&out, &json)?;
    println!("wrote {out} ({} KB)", json.len() / 1024);

    // Loading it back gives the same `MatchStats`, ready for analysis or for
    // `scouting::Scouting::add`.
    let back: MatchStats = serde_json::from_str(&std::fs::read_to_string(&out)?)?;
    assert!(back == stats, "the JSON round trip changed the stats");
    println!(
        "read it back: {} on {}, {} players, {} rounds",
        back.teams
            .iter()
            .map(|t| format!("{} {}", t.name, t.score))
            .collect::<Vec<_>>()
            .join(" - "),
        back.map,
        back.players.len(),
        back.rounds.len()
    );

    // Parts serialize on their own too, e.g. only the kill feed:
    let kills = serde_json::to_string(&stats.kills)?;
    println!("the kill feed alone is {} KB", kills.len() / 1024);
    Ok(())
}
