//! Pre-release launch smoke test (macOS, `--ignored`). Launches the bundled
//! MDViewer.app against a fixture and round-trips `get_viewer_state` over the
//! MCP socket to prove the GUI boots and the frontend responds. Run via
//! `scripts/smoke-test.sh` or `cargo test --test launch_smoke -- --ignored`.
#![cfg(target_os = "macos")]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::Stream;
use serde_json::{json, Value};

const FIXTURE: &str = "smoke.md";
const OVERALL_TIMEOUT: Duration = Duration::from_secs(45);
const POLL_DEADLINE: Duration = Duration::from_secs(40);

#[test]
#[ignore = "launches the bundled GUI app; run pre-release via scripts/smoke-test.sh"]
fn launches_and_reports_open_document() {
    let app = bundle_path();
    if !app.exists() {
        println!(
            "SKIP: bundle not found at {}. Build it first \
             (scripts/smoke-test.sh, or `cd src-tauri && cargo tauri build`).",
            app.display()
        );
        return;
    }

    let inner = inner_binary(&app).expect("bundle has an executable in Contents/MacOS");
    let fixture = fixture_path();
    let sock = test_socket_id();
    std::env::set_var("MDVIEWER_MCP_SOCKET", &sock);
    let _ = std::fs::remove_file(&sock);

    let mut child = Command::new(&inner)
        .arg(&fixture)
        .env("MDVIEWER_MCP_SOCKET", &sock)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("launch bundled inner binary");

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(poll_for_state());
    });

    let outcome = rx.recv_timeout(OVERALL_TIMEOUT);

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(&sock);

    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(e)) => panic!("smoke test failed: {e}"),
        Err(_) => panic!("smoke test timed out after {OVERALL_TIMEOUT:?} (app never answered)"),
    }
}

fn poll_for_state() -> Result<(), String> {
    let name = mdviewer_lib::mcp::socket_name().map_err(|e| format!("socket_name: {e}"))?;
    let deadline = Instant::now() + POLL_DEADLINE;
    let mut last = String::from("never connected");
    while Instant::now() < deadline {
        match try_once(&name) {
            Ok(true) => return Ok(()),
            Ok(false) => last = "connected, but state.path did not match the fixture yet".into(),
            Err(e) => last = e,
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err(format!("never saw the fixture open; last attempt: {last}"))
}

fn try_once(name: &interprocess::local_socket::Name<'static>) -> Result<bool, String> {
    let stream = Stream::connect(name.borrow()).map_err(|e| format!("connect: {e}"))?;
    let req = mdviewer_lib::mcp::GuiRequest {
        id: 1,
        tool: "get_viewer_state".into(),
        args: json!({}),
    };
    let mut line = serde_json::to_string(&req).map_err(|e| format!("encode: {e}"))?;
    line.push('\n');
    (&stream)
        .write_all(line.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    let mut reader = BufReader::new(&stream);
    let mut resp = String::new();
    reader
        .read_line(&mut resp)
        .map_err(|e| format!("read: {e}"))?;
    let reply: mdviewer_lib::mcp::GuiReply =
        serde_json::from_str(resp.trim()).map_err(|e| format!("parse reply {resp:?}: {e}"))?;

    match reply.result {
        // result is a JSON string: {"path": <string|null>, "reviewing": <bool>}
        Some(result) => {
            let state: Value = serde_json::from_str(&result)
                .map_err(|e| format!("parse state {result:?}: {e}"))?;
            let path = state["path"].as_str().unwrap_or("");
            Ok(path.ends_with(FIXTURE))
        }
        // error (e.g. STARTING_ERR before frontend_ready) → not ready yet
        None => Ok(false),
    }
}

fn bundle_path() -> PathBuf {
    if let Ok(p) = std::env::var("MDVIEWER_SMOKE_APP") {
        return PathBuf::from(p);
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("target/release/bundle/macos/MDViewer.app")
}

fn inner_binary(app: &Path) -> Option<PathBuf> {
    let macos = app.join("Contents/MacOS");
    std::fs::read_dir(macos)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_file())
}

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(FIXTURE)
}

fn test_socket_id() -> String {
    let base = format!("mdviewer-smoke-{}.sock", std::process::id());
    std::env::temp_dir()
        .join(base)
        .to_string_lossy()
        .into_owned()
}
