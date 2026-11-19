//! Many demos at once: `match_stats_many` parses them in parallel, one per
//! CPU core, and returns one result per demo, in order.
//!
//! cargo run --release --example many_demos -- <demo or folder> [more ...]
//!
//! Folders are searched for `.dem` files, subfolders included.

use std::path::{Path, PathBuf};
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut paths = Vec::new();
    for arg in std::env::args().skip(1) {
        find_demos(Path::new(&arg), &mut paths);
    }
    if paths.is_empty() {
        return Err("usage: cargo run --release --example many_demos -- <demo or folder> [more ...]".into());
    }

    let start = Instant::now();
    let results = cs2_demo::match_stats_many(&paths);
    println!("{} demos parsed in {:.1?}\n", paths.len(), start.elapsed());

    // One `Result` per demo: a broken file doesn't stop the others.
    for (path, result) in paths.iter().zip(results) {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        match result {
            Ok(stats) => {
                let score: Vec<String> = stats
                    .teams
                    .iter()
                    .map(|t| format!("{} {}", t.name, t.score))
                    .collect();
                let best = stats
                    .players
                    .iter()
                    .max_by(|a, b| a.rating().total_cmp(&b.rating()));
                println!(
                    "{name}: {} | {} | {} rounds | best: {}",
                    stats.map,
                    score.join(" - "),
                    stats.rounds.len(),
                    best.map_or(String::new(), |p| format!("{} ({:.2})", p.name, p.rating())),
                );
            }
            Err(e) => println!("{name}: error: {e}"),
        }
    }
    Ok(())
}

fn find_demos(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        let mut entries: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for p in entries {
            find_demos(&p, out);
        }
    } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dem")) {
        out.push(path.to_path_buf());
    }
}
