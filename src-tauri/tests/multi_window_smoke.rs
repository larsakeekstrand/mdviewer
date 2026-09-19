//! Two-root smoke test (macOS, `--ignored`). Launches the bundled MDViewer.app
//! rooted at `fixtures/multi/a`, then asks it (over the MCP socket) to open a
//! second file that lives outside that root, and proves the two roots are
//! routed to two independent windows. Run via `scripts/smoke-test.sh` or
//! `cargo test --test multi_window_smoke -- --ignored`.
//!
//! Routing note: `fixtures/multi/b/b.md` lives inside the mdviewer git repo
//! (same as `fixtures/multi/a`), so `routing::fallback_root` for it is the
//! REPO'S git toplevel, not `fixtures/multi/b` — and that toplevel also
//! contains `fixtures/multi/a`. That's fine: `route()` only ever compares
//! against *registered* window roots, and "main"'s registered root is exactly
//! `fixtures/multi/a` (not the repo root), so it does not claim b.md. Opening
//! b.md therefore creates a brand-new window rooted at the repo toplevel. The
//! assertions below are written around that: window "main" is asked via
//! `cwd: <abs a>`, the new window is asked via `cwd: <repo toplevel>`.
//!
//! Data-dir note: Tauri's `app_data_dir()` is derived from the bundle
//! identifier and OS conventions; there is no environment override in this
//! codebase to point it at a temp directory for a test run. So this test
//! *also* restores whatever windows/sessions are saved in the developer's own
//! `recent.json` (see `lib.rs`'s setup hook — restored windows are opened
//! regardless of an explicit argv root). That's harmless for the assertions
//! below, which only ever look at the window containing `fixtures/multi/a`
//! and the window containing `fixtures/multi/b/b.md`'s repo toplevel, by cwd.
#![cfg(target_os = "macos")]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{Name, Stream};
use serde_json::{json, Value};

const OVERALL_TIMEOUT: Duration = Duration::from_secs(60);
const STARTING_RETRIES: u32 = 30; // mirrors mcp::STARTING_RETRIES (x500ms = 15s)

#[test]
#[ignore = "launches the bundled GUI app; run pre-release via scripts/smoke-test.sh"]
fn two_roots_route_to_independent_windows() {
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
    let root_a = fixture_dir("a");
    let file_b = fixture_dir("b").join("b.md");
    assert!(root_a.join("a.md").is_file(), "fixture a/a.md missing");
    assert!(file_b.is_file(), "fixture b/b.md missing");

    let sock = test_socket_id();
    std::env::set_var("MDVIEWER_MCP_SOCKET", &sock);
    let _ = std::fs::remove_file(&sock);

    let mut child = Command::new(&inner)
        .arg(&root_a)
        .env("MDVIEWER_MCP_SOCKET", &sock)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("launch bundled inner binary");

    let outcome = (|| -> Result<(), String> {
        let name = mdviewer_lib::mcp::socket_name().map_err(|e| format!("socket_name: {e}"))?;
        let deadline = Instant::now() + OVERALL_TIMEOUT;
        let a_cwd = root_a.to_string_lossy().into_owned();

        // Step 1: poll until "main" (root = fixtures/multi/a) answers with a.md.
        wait_for(
            &name,
            deadline,
            "get_viewer_state",
            json!({}),
            Some(&a_cwd),
            |v| {
                v.get("path")
                    .and_then(Value::as_str)
                    .is_some_and(|p| p.ends_with("a.md"))
            },
        )
        .map_err(|e| format!("main window never opened a.md: {e}"))?;

        // Step 2: open_document(b.md) — outside a's root, so it routes to a
        // NEW window. The first calls may answer STARTING_ERR while that
        // window boots; retry up to STARTING_RETRIES x 500ms like the proxy
        // does (forward_call in mcp.rs).
        let b_path = file_b.to_string_lossy().into_owned();
        let mut opened = None;
        for _ in 0..STARTING_RETRIES {
            match call(&name, "open_document", json!({ "path": b_path }), None) {
                Ok(v) => {
                    opened = Some(v);
                    break;
                }
                Err(e) if e == mdviewer_lib::mcp::STARTING_ERR => {
                    thread::sleep(Duration::from_millis(500));
                }
                Err(e) => return Err(format!("open_document failed: {e}")),
            }
        }
        let opened = opened.ok_or("open_document never succeeded (new window never started)")?;
        let text = opened.as_str().unwrap_or("");
        if !text.ends_with("b.md") {
            return Err(format!("unexpected open_document reply: {text:?}"));
        }

        // Step 3: the new window's root is the repo's git toplevel (b.md's
        // fallback root). Ask by that cwd and expect it to answer with b.md.
        let repo_root = git_toplevel(file_b.parent().unwrap())?;
        let repo_cwd = repo_root.to_string_lossy().into_owned();
        wait_for(
            &name,
            deadline,
            "get_viewer_state",
            json!({}),
            Some(&repo_cwd),
            |v| {
                v.get("path")
                    .and_then(Value::as_str)
                    .is_some_and(|p| p.ends_with("b.md"))
            },
        )
        .map_err(|e| format!("new window never reported b.md open: {e}"))?;

        // Step 4: a's window (cwd = root_a) was untouched — still not b.md.
        let state_a = call(&name, "get_viewer_state", json!({}), Some(&a_cwd))
            .map_err(|e| format!("get_viewer_state(a) failed: {e}"))?;
        let path_a = state_a.get("path").and_then(Value::as_str).unwrap_or("");
        if path_a.ends_with("b.md") {
            return Err(format!(
                "a's window was affected by opening b.md (path: {path_a:?})"
            ));
        }

        Ok(())
    })();

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(&sock);

    outcome.expect("multi-window smoke test failed");
}

/// Poll `tool` until it succeeds and `ok` accepts the parsed result, or the
/// deadline passes.
fn wait_for(
    name: &Name<'static>,
    deadline: Instant,
    tool: &str,
    args: Value,
    cwd: Option<&str>,
    ok: impl Fn(&Value) -> bool,
) -> Result<Value, String> {
    let mut last = String::from("never called");
    while Instant::now() < deadline {
        match call(name, tool, args.clone(), cwd) {
            Ok(v) if ok(&v) => return Ok(v),
            Ok(v) => last = format!("got {v} but it did not match"),
            Err(e) => last = e,
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err(last)
}

/// One request/reply round-trip over the MCP socket, mirroring
/// `launch_smoke.rs::try_once`. `get_viewer_state`'s result is a JSON string
/// and parses to an object; `open_document`'s result is a plain "Opened
/// <path>" string and is returned as a JSON string value.
fn call(name: &Name<'static>, tool: &str, args: Value, cwd: Option<&str>) -> Result<Value, String> {
    let stream = Stream::connect(name.borrow()).map_err(|e| format!("connect: {e}"))?;
    let req = mdviewer_lib::mcp::GuiRequest {
        id: 1,
        tool: tool.to_string(),
        args,
        cwd: cwd.map(str::to_string),
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

    match (reply.result, reply.error) {
        (Some(r), _) => Ok(serde_json::from_str(&r).unwrap_or(Value::String(r))),
        (None, Some(e)) => Err(e),
        (None, None) => Err("empty reply".to_string()),
    }
}

fn git_toplevel(dir: &Path) -> Result<PathBuf, String> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(dir)
        .output()
        .map_err(|e| format!("git rev-parse: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git rev-parse failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    std::fs::canonicalize(&s).map_err(|e| format!("canonicalize {s}: {e}"))
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

fn fixture_dir(leaf: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/multi")
        .join(leaf)
        .canonicalize()
        .expect("fixture directory exists")
}

fn test_socket_id() -> String {
    let base = format!("mdviewer-multi-smoke-{}.sock", std::process::id());
    std::env::temp_dir()
        .join(base)
        .to_string_lossy()
        .into_owned()
}
