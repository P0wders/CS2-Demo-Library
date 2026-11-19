//! # cs2_demo
//!
//! A fast, from-scratch parser for Counter-Strike 2 `.dem` files.
//!
//! ```no_run
//! use cs2_demo::{Demo, ParseOptions, Visitor, Context, GameEvent};
//!
//! struct Kills(u32);
//! impl Visitor for Kills {
//!     fn on_game_event(&mut self, _ctx: &Context, ev: &GameEvent) {
//!         if &*ev.name == "player_death" { self.0 += 1; }
//!     }
//! }
//!
//! let demo = Demo::open("match.dem")?;
//! println!("map: {}", demo.header().map_name);
//! let mut kills = Kills(0);
//! demo.parse(ParseOptions::events_only(), &mut kills)?;
//! println!("{} kills", kills.0);
//! # Ok::<(), cs2_demo::Error>(())
//! ```
//!
//! Layers, from low to high level:
//! * [`container`]: frames of the `.dem` file.
//! * [`parser`]: net messages, string tables, game events and entities,
//!   delivered to a [`Visitor`] (a tuple of visitors runs them all in one parse).
//! * [`model`]: typed views (players, teams, game rules) over entity state.
//! * [`stats`]: a ready-made analyzer producing [`stats::MatchStats`]: the
//!   scoreboard with T/CT splits, every player's round, team economy and buy
//!   types, clutches, every grenade with its lineup and effect, and aim.
//! * [`replay`]: positions over time, for 2D replays.
//! * [`maps`]: radar calibration, from world positions to radar pixels.
//! * [`scouting`]: team and player profiles over many matches
//!   (see [`match_stats_many`] to parse them in parallel).
//!
//! ## Performance
//! On a 342 MB, 48-minute pro demo (Ryzen 7 5800X): ready for the first tick in
//! 6 ms, events only 0.13 s (~2.6 GB/s), full entity state 1.95 s (~175 MB/s),
//! [`Demo::match_stats`] 2.1 s, single-threaded, with ~12 MB of memory besides
//! the memory-mapped file. Skip what you don't need with [`ParseOptions`];
//! parse several demos on several threads for throughput.
//!
//! ## Features
//! * `serde`: `serde::Serialize` and `Deserialize` for [`stats::MatchStats`]
//!   and its parts, the [`replay`] frames and [`maps::MapOverview`].
//! * `user-messages`: [`proto`] types for user messages, temp entities and CS
//!   game events, to decode what [`Visitor::on_net_message`] delivers.
//!
//! ## Format notes
//! Verified bit-exact on 32 demos from builds 10680, 10772, 10847, 10896,
//! 10924 and 10955 (patches 14134 to 14197), recorded by tournament (HLTV),
//! FACEIT and Valve matchmaking servers.
//! * `svc_PacketEntities.serialized_entities` lists the bit length of every
//!   entity's field data. The parser checks each decode against it and resyncs
//!   on mismatch; [`Context::entity_diagnostics`] reports any (expected: zero).
//! * Patch 14197 added a `fixed8` encoder (raw bytes instead of varints) for
//!   8-bit integers and enums; decoders are chosen from the schema each demo
//!   carries, so both encodings work.
//! * `round_start` / `round_end` are no longer recorded in demos; the parser
//!   synthesizes them from the game rules (fields include `synthetic = true`).
//! * Tournament demos may start after the match began, or be split across
//!   several files; [`stats`] opens the first round on its first event and
//!   numbers rounds from the game's own round counter.
//! * Set `CS2_DEMO_TRACE=1` to log every decoded entity field to stderr when
//!   investigating a new game build (see also `examples/dev/dump_schema.rs`).

pub mod bitreader;
pub mod container;
pub mod entities;
pub mod error;
pub mod events;
pub mod maps;
pub mod model;
pub mod parser;
pub mod replay;
pub mod scouting;
pub mod sendtables;
pub mod stats;
pub mod stringtables;

pub mod proto {
    //! Generated protobuf types (Valve's `.proto` files under `proto/`).
    //!
    //! By default only the messages the parser decodes itself. Enable the
    //! `user-messages` feature for user messages, temp entities and CS game
    //! events, to decode what [`crate::Visitor::on_net_message`] delivers.
    #![allow(clippy::all, non_camel_case_types, non_snake_case, missing_docs)]
    include!(concat!(env!("OUT_DIR"), "/_.rs"));
}

pub use container::DemoHeader;
pub use entities::{Entity, EntityOp, Value};
pub use error::{Error, Result};
pub use events::{EventValue, GameEvent};
pub use model::{GameRules, Player, PlayerInfo, RoundEndReason, Team, TeamInfo, Weapon};
pub use parser::{Context, DemoParser, ParseOptions, Visitor};
pub use stats::{MatchStats, StatsCollector};

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Reads one byte per page so the OS maps the whole file, stopping early when
/// the parse ends first (early stop, `end_tick`).
fn prefault(data: &[u8], stop: &AtomicBool) {
    const PAGE: usize = 4096;
    for (i, offset) in (0..data.len()).step_by(PAGE).enumerate() {
        if i % 256 == 0 && stop.load(Ordering::Relaxed) {
            return;
        }
        std::hint::black_box(data[offset]);
    }
}

enum Bytes {
    Mapped(memmap2::Mmap),
    Owned(Vec<u8>),
}

/// An opened demo file. The file is memory-mapped, so opening is instant and
/// parsing reads directly from the OS page cache.
pub struct Demo {
    bytes: Bytes,
    header: DemoHeader,
}

impl Demo {
    pub fn open(path: impl AsRef<Path>) -> Result<Demo> {
        let file = std::fs::File::open(path)?;
        // SAFETY: the map is read-only; modifying the demo file while it is being
        // parsed is unsupported (it would at worst produce parse errors).
        let map = unsafe { memmap2::Mmap::map(&file)? };
        let header = container::read_header(&map)?;
        Ok(Demo {
            bytes: Bytes::Mapped(map),
            header,
        })
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Demo> {
        let header = container::read_header(&bytes)?;
        Ok(Demo {
            bytes: Bytes::Owned(bytes),
            header,
        })
    }

    pub fn bytes(&self) -> &[u8] {
        match &self.bytes {
            Bytes::Mapped(m) => m,
            Bytes::Owned(v) => v,
        }
    }

    pub fn header(&self) -> &DemoHeader {
        &self.header
    }

    /// Parses the demo, driving `visitor`. Returns the final game state.
    pub fn parse<V: Visitor + ?Sized>(&self, opts: ParseOptions, visitor: &mut V) -> Result<Context> {
        let parser = DemoParser::new(self.bytes(), opts)?;
        match &self.bytes {
            Bytes::Owned(_) => parser.run(visitor),
            // The first touch of each page of a fresh mapping costs a page
            // fault (~0.1 s per 350 MB on Windows). Take them on another thread,
            // just ahead of the parser, instead of on the parsing thread.
            Bytes::Mapped(map) => {
                let stop = AtomicBool::new(false);
                std::thread::scope(|s| {
                    s.spawn(|| prefault(map, &stop));
                    let result = parser.run(visitor);
                    stop.store(true, Ordering::Relaxed);
                    result
                })
            }
        }
    }

    /// Parses the whole demo with the built-in [`StatsCollector`].
    pub fn match_stats(&self) -> Result<MatchStats> {
        let mut collector = StatsCollector::new();
        let ctx = self.parse(ParseOptions::default(), &mut collector)?;
        Ok(collector.into_stats(&ctx))
    }

    /// Iterates over raw top-level frames.
    pub fn frames(&self) -> container::FrameIter<'_> {
        container::FrameIter::new(self.bytes())
    }
}

/// Runs [`Demo::match_stats`] on several demos in parallel, one demo per CPU
/// core at a time. Results come back in the order of `paths`.
///
/// ```no_run
/// let results = cs2_demo::match_stats_many(&["a.dem", "b.dem", "c.dem"]);
/// for stats in results {
///     let stats = stats?;
///     println!("{}: {} rounds", stats.map, stats.rounds.len());
/// }
/// # Ok::<(), cs2_demo::Error>(())
/// ```
pub fn match_stats_many<P: AsRef<Path> + Sync>(paths: &[P]) -> Vec<Result<MatchStats>> {
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(paths.len());
    let next = AtomicUsize::new(0);
    let results: Vec<Mutex<Option<Result<MatchStats>>>> = paths.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(i) else { break };
                    let stats = Demo::open(path).and_then(|d| d.match_stats());
                    *results[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(stats);
                }
            });
        }
    });
    results
        .into_iter()
        .map(|r| {
            r.into_inner()
                .unwrap_or_else(|e| e.into_inner())
                .expect("every demo was parsed")
        })
        .collect()
}
