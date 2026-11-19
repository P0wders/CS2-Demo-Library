//! Compiles the vendored Valve protobuf definitions (proto/*.proto) into Rust
//! with prost. protox is a pure-Rust protobuf compiler, so no `protoc` binary
//! is required on the build machine.
//!
//! The upstream files (SteamDatabase/GameTracking-CS2, Protobufs/) are kept
//! byte-for-byte so they can be refreshed by re-downloading. They reference a
//! few Valve-internal options that are never defined (`additional_includes`,
//! `boxed_type`, `synthetic_default`); those are stripped into a sanitized copy
//! under OUT_DIR before compiling.

use std::{env, fs, path::PathBuf};

/// What the parser itself decodes (their imports are compiled too).
const CORE_PROTOS: &[&str] = &[
    "demo.proto",
    "netmessages.proto",
    "networkbasetypes.proto",
    "gameevents.proto",
];

/// With the `user-messages` feature: types for decoding the raw messages that
/// `Visitor::on_net_message` delivers. They triple the generated code, and
/// compile time with it, so they are opt-in.
const USER_MESSAGE_PROTOS: &[&str] = &[
    "network_connection.proto",
    "usermessages.proto",
    "cstrike15_usermessages.proto",
    "cs_gameevents.proto",
    "te.proto",
];

const UNDEFINED_FIELD_OPTIONS: &[&str] = &["boxed_type", "synthetic_default"];

fn main() {
    println!("cargo:rerun-if-changed=proto");
    println!("cargo:rerun-if-changed=build.rs");

    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let clean_dir = out.join("proto_clean");
    fs::create_dir_all(&clean_dir).unwrap();
    for entry in fs::read_dir("proto").expect("proto/ directory missing") {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "proto") {
            let src = fs::read_to_string(&path).unwrap();
            fs::write(clean_dir.join(path.file_name().unwrap()), sanitize(&src)).unwrap();
        }
    }

    let mut protos = CORE_PROTOS.to_vec();
    if env::var_os("CARGO_FEATURE_USER_MESSAGES").is_some() {
        protos.extend_from_slice(USER_MESSAGE_PROTOS);
    }
    let fds = protox::compile(&protos, [&clean_dir]).expect("failed to compile .proto files");
    prost_build::Config::new()
        .compile_fds(fds)
        .expect("failed to generate Rust code from protobufs");
}

fn sanitize(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        if line.trim_start().starts_with("option additional_includes") {
            continue;
        }
        out.push_str(&strip_field_options(line));
        out.push('\n');
    }
    out
}

/// Removes undefined entries from a single-line `[a = 1, b = "x"]` option list,
/// dropping the brackets entirely when nothing is left.
fn strip_field_options(line: &str) -> String {
    let (Some(open), Some(close)) = (line.find('['), line.rfind(']')) else {
        return line.to_string();
    };
    let kept: Vec<&str> = line[open + 1..close]
        .split(',')
        .map(str::trim)
        .filter(|opt| {
            let name = opt.split('=').next().unwrap_or("").trim();
            !UNDEFINED_FIELD_OPTIONS.contains(&name)
        })
        .collect();
    let head = line[..open].trim_end();
    let tail = &line[close + 1..];
    if kept.is_empty() {
        format!("{head}{tail}")
    } else {
        format!("{head} [{}]{tail}", kept.join(", "))
    }
}
