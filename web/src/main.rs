//! Builds the match viewer: compiles the analysis (`web/wasm`) to WebAssembly
//! and writes one HTML page with it inside, `web/dist/cs2-demo-viewer.html`
//! unless another path is given. Open that page in a browser (a double-click
//! is enough: no server) and drop a `.dem` file on it.
//!
//! cargo run -p cs2_demo_web [-- viewer.html]
//!
//! Needs the WebAssembly target once: `rustup target add wasm32-unknown-unknown`.

use std::error::Error;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const PAGE: &str = include_str!("../viewer.html");
const WASM_MARKER: &str = "/*__WASM__*/";
const TARGET: &str = "wasm32-unknown-unknown";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let out = match std::env::args_os().nth(1) {
        Some(path) => PathBuf::from(path),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("dist/cs2-demo-viewer.html"),
    };
    let wasm = std::fs::read(build_wasm()?)?;
    if !PAGE.contains(WASM_MARKER) {
        return Err(format!("viewer.html has no {WASM_MARKER} marker for the WebAssembly").into());
    }
    let page = PAGE.replacen(WASM_MARKER, &base64(&wasm), 1);
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&out, &page)?;
    println!(
        "wrote {} ({:.1} MB, of which {:.1} MB of WebAssembly)",
        out.display(),
        page.len() as f64 / 1e6,
        wasm.len() as f64 / 1e6
    );
    println!("open it in a browser and drop a .dem file on it");
    Ok(())
}

/// Builds the library for the browser and returns the path of the `.wasm` file.
fn build_wasm() -> Result<PathBuf, Box<dyn Error>> {
    eprintln!("building the WebAssembly module (about a minute the first time)");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut child = Command::new(cargo)
        .args([
            "build",
            "-p",
            "cs2_demo_wasm",
            "--lib",
            "--target",
            TARGET,
            "--profile",
            "wasm",
        ])
        // WebAssembly SIMD: the parse is 3.5 % faster (2.64 s against 2.74 s
        // on a 342 MB demo). Chrome 91, Firefox 89 and Safari 16.4 have it.
        .args([
            "--config",
            r#"target.wasm32-unknown-unknown.rustflags=["-C", "target-feature=+simd128"]"#,
        ])
        .arg("--message-format=json-render-diagnostics")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdout(Stdio::piped())
        .spawn()?;
    let mut wasm = None;
    let stdout = child.stdout.take().ok_or("no output from cargo")?;
    for line in BufReader::new(stdout).lines() {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line?) else {
            continue;
        };
        if msg["reason"] == "compiler-artifact" && msg["target"]["name"] == "cs2_demo_wasm" {
            let files = msg["filenames"].as_array().into_iter().flatten();
            if let Some(f) = files.filter_map(|f| f.as_str()).find(|f| f.ends_with(".wasm")) {
                wasm = Some(PathBuf::from(f));
            }
        }
    }
    if !child.wait()?.success() {
        return Err(format!(
            "the WebAssembly build failed. If the error above says the {TARGET} target may not be installed, run\n    rustup target add {TARGET}"
        )
        .into());
    }
    wasm.ok_or_else(|| "the build produced no .wasm file".into())
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
