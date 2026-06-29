# Launch Smoke Test Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A pre-release, local-only test that launches the bundled `MDViewer.app` against a fixture and round-trips `get_viewer_state` over the MCP socket to prove the app boots and the frontend responds.

**Architecture:** A `#[ignore]`d Rust integration test in `src-tauri/tests/launch_smoke.rs` launches the bundle's inner binary with an isolated MCP socket, then polls the socket — sending `{id,tool:"get_viewer_state",args:{}}` and reading `{id,result,error}` — until the active-tab `path` equals the launched fixture, or a timeout. A thin `scripts/smoke-test.sh` wrapper builds the bundle first and runs the test. It is wired into the `/cut-release` ritual.

**Tech Stack:** Rust, the `interprocess` local-socket crate (already a dependency), `serde_json`, the existing `mdviewer_lib::mcp::{socket_name, GuiRequest, GuiReply}` types.

## Global Constraints

- **macOS only** for v1. The test file is gated with `#![cfg(target_os = "macos")]` so it compiles to nothing on Windows CI.
- **Never runs in CI / normal `cargo test`** — the test is marked `#[ignore]`; it only runs via `cargo test --test launch_smoke -- --ignored`.
- **Reuse existing types**: `mdviewer_lib::mcp::socket_name()` (reads `MDVIEWER_MCP_SOCKET`), `mdviewer_lib::mcp::GuiRequest { id: u64, tool: String, args: Value }`, `mdviewer_lib::mcp::GuiReply { id: u64, result: Option<String>, error: Option<String> }`. `GuiReply.result` is a JSON **string** (the frontend does `JSON.stringify(viewerState(...))`), shaped `{ "path": <string|null>, "reviewing": <bool> }`.
- **Socket isolation**: set `MDVIEWER_MCP_SOCKET` to a unique temp path before launch so the test never collides with a real running MDViewer or a live Claude MCP session.
- **Default bundle path**: `MDVIEWER_SMOKE_APP` env var, defaulting to `<CARGO_MANIFEST_DIR>/target/release/bundle/macos/MDViewer.app` (CARGO_MANIFEST_DIR is `src-tauri`). Absent bundle → the test prints a SKIP message and passes (no false failure).
- **No comments in code unless the why is non-obvious** (project convention). Lint must be clean: `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`.

---

## Note on test style

This is a GUI-launch smoke test, not a unit test — there is no meaningful
red-green-refactor cycle. The test file **is** the deliverable. Verification
proceeds in three observable stages: (1) it compiles, (2) the no-bundle path
prints a clean SKIP and passes, (3) with a freshly built bundle it goes GREEN.
Do not invent a fake unit test to satisfy a TDD shape.

---

### Task 1: The smoke test and fixture

**Files:**
- Create: `src-tauri/tests/fixtures/smoke.md`
- Create: `src-tauri/tests/launch_smoke.rs`

**Interfaces:**
- Consumes: `mdviewer_lib::mcp::socket_name()`, `mdviewer_lib::mcp::GuiRequest`, `mdviewer_lib::mcp::GuiReply` (all already `pub`).
- Produces: a `#[ignore]`d test `launches_and_reports_open_document` runnable via `cargo test --test launch_smoke -- --ignored`.

- [ ] **Step 1: Create the fixture**

`src-tauri/tests/fixtures/smoke.md`:

```markdown
# Smoke fixture

A paragraph so the renderer has prose to lay out.

```rust
fn main() {
    println!("hello");
}
```

- [ ] a task-list item
```

- [ ] **Step 2: Write the test file**

`src-tauri/tests/launch_smoke.rs`:

```rust
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
    reader.read_line(&mut resp).map_err(|e| format!("read: {e}"))?;
    let reply: mdviewer_lib::mcp::GuiReply =
        serde_json::from_str(resp.trim()).map_err(|e| format!("parse reply {resp:?}: {e}"))?;

    match reply.result {
        // result is a JSON string: {"path": <string|null>, "reviewing": <bool>}
        Some(result) => {
            let state: Value =
                serde_json::from_str(&result).map_err(|e| format!("parse state {result:?}: {e}"))?;
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
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/release/bundle/macos/MDViewer.app")
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
    std::env::temp_dir().join(base).to_string_lossy().into_owned()
}
```

- [ ] **Step 3: Verify it compiles and the no-bundle path SKIPs cleanly**

First make sure no stale bundle is picked up for this check:

Run: `cd src-tauri && MDVIEWER_SMOKE_APP=/nonexistent/MDViewer.app cargo test --test launch_smoke -- --ignored --nocapture`
Expected: compiles, prints `SKIP: bundle not found at /nonexistent/MDViewer.app ...`, and the test result is `ok` (1 passed).

- [ ] **Step 4: Verify lint is clean**

Run: `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: no output, exit 0.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/tests/fixtures/smoke.md src-tauri/tests/launch_smoke.rs
git commit -m "Add launch smoke test (ignored, macOS)

Launches the bundled app against a fixture and round-trips
get_viewer_state over the MCP socket to prove it boots."
```

---

### Task 2: The pre-release wrapper script (full green run)

**Files:**
- Create: `scripts/smoke-test.sh`

**Interfaces:**
- Consumes: the `launches_and_reports_open_document` test from Task 1; `cargo tauri build` (requires `cargo-tauri`, see CLAUDE.md).
- Produces: `scripts/smoke-test.sh` — the one command run before a release.

- [ ] **Step 1: Write the script**

`scripts/smoke-test.sh`:

```bash
#!/usr/bin/env bash
# Pre-release launch smoke test: build the bundle, then launch it and round-trip
# get_viewer_state over the MCP socket. Set MDVIEWER_SMOKE_APP to a prebuilt
# .app to skip the (slow) bundle build.
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ -z "${MDVIEWER_SMOKE_APP:-}" ]]; then
  echo "Building the bundle (cargo tauri build)..."
  ( cd src-tauri && cargo tauri build )
fi

echo "Running the launch smoke test..."
( cd src-tauri && cargo test --test launch_smoke -- --ignored --nocapture )
```

- [ ] **Step 2: Make it executable**

Run: `chmod +x scripts/smoke-test.sh`
Expected: no output, exit 0.

- [ ] **Step 3: Run the full smoke test end-to-end (the real verification)**

Run: `./scripts/smoke-test.sh`
Expected: builds the bundle (slow, several minutes the first time), launches `MDViewer.app`, and the test prints `test launches_and_reports_open_document ... ok` (1 passed). A window may briefly appear and close — that is the launched app being torn down.

If it fails: read the panic message. `timed out (app never answered)` means the GUI never bound the socket or never became ready; `state.path did not match` means it answered but did not open the fixture. Re-run once to rule out a transient slow build/launch before investigating.

- [ ] **Step 4: Commit**

```bash
git add scripts/smoke-test.sh
git commit -m "Add scripts/smoke-test.sh pre-release wrapper"
```

---

### Task 3: Wire into the release ritual and document it

**Files:**
- Modify: `.claude/skills/cut-release/SKILL.md` (Verification section, around line 92-101)
- Modify: `CLAUDE.md` (Build / develop / release section)

**Interfaces:**
- Consumes: `scripts/smoke-test.sh` from Task 2.
- Produces: documentation only — no code.

- [ ] **Step 1: Add a smoke-test step to the cut-release Verification block**

In `.claude/skills/cut-release/SKILL.md`, after the existing verification fenced block (the line ending `node --test "$f"; done` / its closing ```), add:

```markdown

Before publishing the release (after step 8's build, against the artifact you
are about to ship), run the launch smoke test — it boots the bundled app and
confirms the frontend responds:

```sh
./scripts/smoke-test.sh   # builds MDViewer.app, launches it, round-trips get_viewer_state
```

If it times out or fails, do not publish — the bundle does not boot.
```

- [ ] **Step 2: Add a CLAUDE.md line under Build / develop / release**

In `CLAUDE.md`, in the `## Build / develop / release` section, after the release-build fenced block, add:

```markdown
# pre-release launch smoke test (macOS): builds the bundle, launches it,
# and round-trips get_viewer_state over the MCP socket to prove it boots
./scripts/smoke-test.sh
```

- [ ] **Step 3: Verify the docs reference real paths**

Run: `test -x scripts/smoke-test.sh && grep -q "smoke-test.sh" CLAUDE.md .claude/skills/cut-release/SKILL.md && echo OK`
Expected: `OK`.

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md .claude/skills/cut-release/SKILL.md
git commit -m "Document launch smoke test in CLAUDE.md and cut-release ritual"
```

---

## Self-Review

**Spec coverage:**
- Pre-release, local-only launch smoke test → Tasks 1+2 (`#[ignore]`d, wrapper script), no CI wiring. ✓
- Target bundled `.app` → `bundle_path()` defaults to the macOS bundle; `inner_binary()` launches `Contents/MacOS/<exe>`. ✓
- Liveness via `get_viewer_state` returning the fixture path → `try_once()`. ✓
- Isolated socket → `test_socket_id()` + `MDVIEWER_MCP_SOCKET`. ✓
- Poll-based timeout, connection-refused retried, only timeout fails → `poll_for_state()` loop + `rx.recv_timeout`. ✓
- Teardown on every path including panic → `child.kill()`/`wait()` + socket removal happen **before** the asserting `match`. ✓
- Missing bundle is a clear SKIP, not a fail → early `return` with message. ✓
- macOS-only, never in CI → `#![cfg(target_os = "macos")]` + `#[ignore]`. ✓
- Wire into `/cut-release` + CLAUDE.md → Task 3. ✓
- Stretch action round-trip / Windows / HTML render verification → explicitly out of scope, no task. ✓

**Placeholder scan:** No TBD/TODO; every code step shows complete code. ✓

**Type consistency:** `GuiRequest { id, tool, args }` and `GuiReply { id, result, error }` used exactly as defined in `mcp.rs`; `socket_name() -> io::Result<Name<'static>>` matches the signature read from source; `Stream::connect(name.borrow())` mirrors the working proxy code. ✓
