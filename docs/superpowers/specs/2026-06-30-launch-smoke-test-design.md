# Launch smoke test — design

**Date:** 2026-06-30
**Status:** Approved, ready for implementation plan

## Problem

MDViewer has a wide pure-logic test base (`node --test` over pure JS helpers,
`cargo test` over pure Rust module logic) and one real subprocess integration
test (`tests/mcp_proxy.rs`, which binds a *fake* GUI to the socket). Nothing
exercises the actual built app: there is no test that proves the bundled
`MDViewer.app` boots, brings up its webview, runs `init()`, and renders the
initial document. Regressions of the "won't even launch / blank webview /
`init()` threw" class can only be caught by manually running the app — which is
exactly the gap noted in the project's working memory ("run the app before
merging UI features").

Tauri's official end-to-end tooling (`tauri-driver` + WebDriver) supports Linux
and Windows only — **not macOS**, the primary `.dmg` target — so "drive the real
UI like a user" is not available on the main platform.

## Goal

One command, run **before cutting a release**, that launches the bundled
`MDViewer.app` against a fixture markdown file and proves the frontend is alive
by round-tripping `get_viewer_state` over the app's existing MCP socket.

Non-goals: running in CI (deliberately out — avoids GUI-in-CI flakiness),
pixel-verifying rendered HTML, and driving arbitrary UI interactions.

## What it proves (and what it does not)

The test launches the real bundled app pointed at a fixture, then asks the
running app `get_viewer_state` over the MCP socket. The viewer-state tool returns
`{ path, reviewing }` where `path` is the active tab's file. A reply of
`{ path: "<…/smoke.md>", reviewing: false }` proves end-to-end:

- the Rust GUI process booted and bound the MCP socket
  (`mcp_server::start` runs unconditionally in the `setup` hook, `lib.rs:130`);
- the webview loaded and `init()` ran;
- `init()` parsed argv and opened the fixture as the active tab;
- the full `socket → GUI listener → webview → JS → back` round-trip works.

It does **not** pixel-verify that the markdown rendered to HTML —
`get_viewer_state` reports no render-success signal. "JS ran far enough to open
the correct document and answer a tool call" is the honest boundary of this
tier, and it is the "did it boot" signal we set out to catch.

## Approaches considered

| | Approach | Verdict |
|---|---|---|
| A | Rust `#[ignore]`d integration test in `src-tauri/tests/` that launches the bundle's inner binary and does the socket round-trip; a wrapper builds the bundle first | **Chosen** |
| B | Standalone shell script doing JSON socket I/O via `nc`/python | Rejected — hand-rolling framed-JSON socket reads in bash is fragile and can't reuse the `interprocess` patterns already in `mcp_proxy.rs` |
| C | Drive the real UI via WebDriver (`tauri-driver`) | Rejected — no macOS support, the primary target |

Approach A reuses machinery that already exists: `tests/mcp_proxy.rs` uses the
`interprocess` socket crate with newline-delimited JSON framing. This test is the
inversion of that one — the **real GUI** binds the socket and the **test**
connects as the proxy would. The wire protocol is trivial
(`{id, tool, args}` → `{id, result, error}`, defined as `mcp::GuiRequest` /
`mcp::GuiReply`). The bundle build stays *outside* the test (a Rust test
recursively invoking `cargo tauri build` is awkward and slow); orchestration
lives in a thin wrapper script.

## Components

### New files

1. **`src-tauri/tests/fixtures/smoke.md`** — a small committed fixture (a
   heading, a paragraph, a fenced code block, a task-list line) so the launched
   app opens something real.

2. **`src-tauri/tests/launch_smoke.rs`** — the test, marked `#[ignore]` so normal
   `cargo test` and CI never run it. Behavior:
   - Resolve the bundle path from the `MDVIEWER_SMOKE_APP` env var, defaulting to
     `target/release/bundle/macos/MDViewer.app`. If the bundle is absent, the
     test **skips with a clear message** ("build the bundle first") rather than
     failing confusingly.
   - Set `MDVIEWER_MCP_SOCKET` to an **isolated** temp path so the test never
     collides with a real running MDViewer or a live Claude MCP session.
   - Launch the **inner binary directly** —
     `MDViewer.app/Contents/MacOS/mdviewer <fixture>` — with those env vars.
     Running the inner binary (not `open`) is what lets the test inject the
     socket override and the file argument deterministically; a locally built
     `.app` is not quarantined, so Gatekeeper does not block it.
   - **Poll-connect** to the socket, retrying up to a generous timeout (~30 s;
     first launch includes webview spin-up, JS init, and markdown prewarm).
     Connection-refused during startup is expected and retried; only a *timeout*
     fails the test.
   - Send `{"id":1,"tool":"get_viewer_state","args":{}}\n`, read one reply line,
     parse it, and **assert** `error` is null and `result`'s `path` ends with
     `smoke.md`.
   - **Teardown on every path** (including assertion failure / panic): kill the
     child GUI process and remove the temp socket, via a drop guard so a panic
     still cleans up.

3. **`scripts/smoke-test.sh`** — the single pre-release command: `cargo tauri
   build` (skipped if `MDVIEWER_SMOKE_APP` is already set to a prebuilt bundle),
   then `cargo test --test launch_smoke -- --ignored --nocapture`.

### Touch-ups

- **`/cut-release` skill** — add a step that runs `scripts/smoke-test.sh` and
  aborts the release if it fails, making the gate part of the existing ritual.
- **`CLAUDE.md`** — one line under testing noting the smoke test exists and how
  to run it.

## Error handling / flakiness mitigations

- Timeout is **poll-based**, not a fixed `sleep` — as fast as the app allows,
  tolerant of a slow machine.
- Connection-refused during startup is retried; only a timeout is a failure.
- Isolated socket + isolated temp dir means re-runs and parallel local instances
  never interfere.
- Missing bundle is a clear **skip**, not a fail, so the signal is unambiguous.

## Platform scope

macOS only for v1 (the `.app` bundle path and inner-binary launch are
macOS-shaped). The socket mechanism is cross-platform, so a Windows variant
(launch the `.exe`, named-pipe socket) is a straightforward later extension if
wanted — explicitly out of scope here.

## Explicitly out of scope (v1)

- Running in CI.
- Pixel/HTML render verification.
- The "stretch" action round-trip (fire `open_document` on a second fixture and
  re-query state to prove an *action* tool mutates a tab). Left out to keep the
  first smoke test dead-simple; can be added later.
- Windows support.
