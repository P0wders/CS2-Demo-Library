//! Raw net messages: everything the parser doesn't decode itself (user
//! messages, temp entities, ...) can be delivered to `on_net_message` as
//! protobuf bytes. With the `user-messages` feature, `cs2_demo::proto` has the
//! types to decode them. This example prints the in-game chat.
//!
//! cargo run --release --features user-messages --example chat -- match.dem

use cs2_demo::proto::{CUserMessageSayText2, EBaseUserMessages};
use cs2_demo::{Context, Demo, ParseOptions, Visitor};
use prost::Message;

struct Chat;

impl Visitor for Chat {
    // `msg_type` is the message's id; `data` its protobuf encoding.
    fn on_net_message(&mut self, ctx: &Context, msg_type: u32, data: &[u8]) {
        if msg_type != EBaseUserMessages::UmSayText2 as u32 {
            return;
        }
        let Ok(msg) = CUserMessageSayText2::decode(data) else {
            return;
        };
        // `messagename` is the chat channel, e.g. "Cstrike_Chat_All" or
        // "Cstrike_Chat_CT"; `param1` the player's name, `param2` the text.
        let seconds = ctx.tick() as f32 / ctx.tick_rate();
        println!(
            "[{:>2}:{:02}] {} {}: {}",
            (seconds / 60.0) as u32,
            (seconds % 60.0) as u32,
            msg.messagename().trim_start_matches("Cstrike_Chat_"),
            msg.param1(),
            msg.param2()
        );
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: cargo run --release --features user-messages --example chat -- <match.dem>")?;

    // Raw messages are off by default (the parser skips them without copying).
    // Chat needs no entity state, so start from `events_only()`.
    let opts = ParseOptions {
        raw_net_messages: true,
        ..ParseOptions::events_only()
    };
    Demo::open(&path)?.parse(opts, &mut Chat)?;
    Ok(())
}
