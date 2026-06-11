# MCP Server Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose mdviewer to Claude Code as an MCP server (`mdviewer --mcp` stdio proxy → local socket → GUI) with three tools: `open_document`, blocking `request_review`, and `get_viewer_state`.

**Architecture:** A hand-rolled JSON-RPC 2.0 stdio proxy (same binary, `--mcp` flag, the `--claude-hook` precedent) relays tool calls over a Unix domain socket / Windows named pipe (via the `interprocess` crate) to a listener thread in the GUI. The listener emits Tauri events to the webview and parks replies in a pending-request map; new `mcp_respond` / `mcp_review_result` commands route the webview's answers back down the socket. A menu item merges the server into the project's `.mcp.json`.

**Tech Stack:** Rust (Tauri 2.11, serde_json, `interprocess` 2 for cross-platform local sockets), vanilla JS frontend, `node --test` for JS units.

**Spec:** `docs/superpowers/specs/2026-06-11-mcp-server-design.md`

---

## Conventions for every task

- Run Rust tests from `src-tauri/`: `cargo test`. Lint before each commit: `cargo fmt` then `cargo clippy --all-targets -- -D warnings` (CI enforces both).
- Run JS tests from the repo root: `node --test ui/*.test.js`.
- Commit messages: imperative subject, **no** `Co-Authored-By` trailer.
- Comments only where the *why* is non-obvious (project convention).

## File structure (what gets created/modified)

```
src-tauri/src/mcp.rs          — NEW: pure MCP protocol (JSON-RPC dispatch, tool defs,
                                arg validation, internal codec, .mcp.json merge,
                                socket naming) + run_proxy() runtime
src-tauri/src/mcp_server.rs   — NEW: GUI-side socket listener, McpPending map,
                                connection threads
src-tauri/tests/mcp_proxy.rs  — NEW: integration test, proxy ↔ fake GUI socket
src-tauri/src/claude_hook.rs  — MODIFY: extract launch_mdviewer(Option<&str>)
src-tauri/src/main.rs         — MODIFY: --mcp flag check
src-tauri/src/lib.rs          — MODIFY: modules, manage(McpPending), start listener,
                                register commands, run_mcp_proxy()
src-tauri/src/commands.rs     — MODIFY: mcp_respond, mcp_review_result,
                                install_mcp_server
src-tauri/src/menu.rs         — MODIFY: Install MCP Server… menu item
src-tauri/Cargo.toml          — MODIFY: add interprocess = "2"
ui/mcp.js                     — NEW: pure helpers (button label, hint text,
                                busy check, viewer state)
ui/mcp.test.js                — NEW: node --test units for ui/mcp.js
ui/app.js                     — MODIFY: event listeners, finishReview branch,
                                decline, closeTab, renderTabBar, renderReviewBar
ui/styles.css                 — MODIFY: MCP review-bar variant + decline button
README.md, CLAUDE.md          — MODIFY: document the feature
```

---

### Task 1: `mcp.rs` — JSON-RPC protocol core (pure)

The proxy's brain: parse one stdin line, decide whether to reply locally
(`initialize`, `tools/list`, `ping`, errors) or forward a `tools/call`.

**Files:**
- Create: `src-tauri/src/mcp.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod mcp;` — pub so the integration test can use it)

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/mcp.rs` with module doc, the types under test (empty/todo-free signatures are NOT allowed — write the real implementation in step 3; here, write the file with ONLY the test module first so the build fails on missing items):

```rust
//! MCP server: hand-rolled JSON-RPC 2.0 over stdio (the `--mcp` proxy) plus the
//! pure protocol helpers shared with the GUI-side socket listener
//! (`mcp_server.rs`). Pure helpers are unit-tested; `run_proxy` is IO and
//! covered by `tests/mcp_proxy.rs`.

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn initialize_echoes_protocol_version_and_advertises_tools() {
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{}}}"#;
        match handle_message(line) {
            Dispatch::Reply(v) => {
                assert_eq!(v["id"], 1);
                assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
                assert!(v["result"]["capabilities"]["tools"].is_object());
                assert_eq!(v["result"]["serverInfo"]["name"], "mdviewer");
            }
            other => panic!("expected Reply, got {other:?}"),
        }
    }

    #[test]
    fn notifications_are_ignored() {
        let line = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        assert_eq!(handle_message(line), Dispatch::Ignore);
        assert_eq!(handle_message("not json"), Dispatch::Ignore);
        assert_eq!(handle_message(""), Dispatch::Ignore);
    }

    #[test]
    fn ping_replies_empty_object() {
        let line = r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#;
        match handle_message(line) {
            Dispatch::Reply(v) => {
                assert_eq!(v["id"], 7);
                assert_eq!(v["result"], json!({}));
            }
            other => panic!("expected Reply, got {other:?}"),
        }
    }

    #[test]
    fn tools_list_returns_all_three_tools() {
        let line = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
        match handle_message(line) {
            Dispatch::Reply(v) => {
                let tools = v["result"]["tools"].as_array().unwrap();
                let names: Vec<&str> =
                    tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
                assert_eq!(names, vec!["open_document", "request_review", "get_viewer_state"]);
                for t in tools {
                    assert!(t["description"].as_str().unwrap().len() > 10);
                    assert_eq!(t["inputSchema"]["type"], "object");
                }
            }
            other => panic!("expected Reply, got {other:?}"),
        }
    }

    #[test]
    fn tools_call_forwards_known_tool_with_args_and_token() {
        let line = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"open_document","arguments":{"path":"docs/plan.md","line":42},"_meta":{"progressToken":"p1"}}}"#;
        match handle_message(line) {
            Dispatch::Call { rpc_id, tool, args, progress_token } => {
                assert_eq!(rpc_id, json!(3));
                assert_eq!(tool, "open_document");
                assert_eq!(args["path"], "docs/plan.md");
                assert_eq!(args["line"], 42);
                assert_eq!(progress_token, Some(json!("p1")));
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn tools_call_unknown_tool_is_an_error_reply() {
        let line = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"rm_rf","arguments":{}}}"#;
        match handle_message(line) {
            Dispatch::Reply(v) => {
                assert_eq!(v["error"]["code"], -32602);
            }
            other => panic!("expected Reply, got {other:?}"),
        }
    }

    #[test]
    fn unknown_method_with_id_is_method_not_found() {
        let line = r#"{"jsonrpc":"2.0","id":5,"method":"resources/list"}"#;
        match handle_message(line) {
            Dispatch::Reply(v) => assert_eq!(v["error"]["code"], -32601),
            other => panic!("expected Reply, got {other:?}"),
        }
    }

    #[test]
    fn tool_text_result_shapes_content_and_error_flag() {
        let ok = tool_text_result(json!(9), "hello", false);
        assert_eq!(ok["result"]["content"][0]["type"], "text");
        assert_eq!(ok["result"]["content"][0]["text"], "hello");
        assert!(ok["result"].get("isError").is_none());
        let err = tool_text_result(json!(9), "boom", true);
        assert_eq!(err["result"]["isError"], true);
    }

    #[test]
    fn progress_notification_carries_token_and_counter() {
        let n = progress_notification(&json!("p1"), 3);
        assert_eq!(n["method"], "notifications/progress");
        assert_eq!(n["params"]["progressToken"], "p1");
        assert_eq!(n["params"]["progress"], 3);
        assert!(n.get("id").is_none());
    }
}
```

Add `pub mod mcp;` to `src-tauri/src/lib.rs` (after `mod markdown;`, keeping the list alphabetical: between `mod markdown;` and `mod menu;`).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test mcp::`
Expected: compile error — `handle_message`, `Dispatch`, etc. not found.

- [ ] **Step 3: Write the implementation**

Insert above the `tests` module in `src-tauri/src/mcp.rs`:

```rust
use serde_json::{json, Value};

pub const KNOWN_TOOLS: [&str; 3] = ["open_document", "request_review", "get_viewer_state"];

/// What to do with one stdin line.
#[derive(Debug, PartialEq)]
pub enum Dispatch {
    /// A complete JSON-RPC response to write back on stdout.
    Reply(Value),
    /// A tools/call to forward to the GUI over the socket.
    Call {
        rpc_id: Value,
        tool: String,
        args: Value,
        progress_token: Option<Value>,
    },
    /// A notification or unparseable input — write nothing.
    Ignore,
}

/// Parse one JSON-RPC line and dispatch it. Pure: handles everything the proxy
/// can answer without the GUI; `tools/call` for a known tool becomes `Call`.
pub fn handle_message(line: &str) -> Dispatch {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return Dispatch::Ignore,
    };
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    match (id, method) {
        (None, _) => Dispatch::Ignore,
        (Some(id), "initialize") => {
            let requested = msg
                .pointer("/params/protocolVersion")
                .cloned()
                .unwrap_or_else(|| json!("2024-11-05"));
            Dispatch::Reply(rpc_result(
                id,
                json!({
                    "protocolVersion": requested,
                    "capabilities": { "tools": {} },
                    "serverInfo": {
                        "name": "mdviewer",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            ))
        }
        (Some(id), "ping") => Dispatch::Reply(rpc_result(id, json!({}))),
        (Some(id), "tools/list") => {
            Dispatch::Reply(rpc_result(id, json!({ "tools": tool_defs() })))
        }
        (Some(id), "tools/call") => {
            let name = msg.pointer("/params/name").and_then(|n| n.as_str());
            match name {
                Some(tool) if KNOWN_TOOLS.contains(&tool) => Dispatch::Call {
                    rpc_id: id,
                    tool: tool.to_string(),
                    args: msg
                        .pointer("/params/arguments")
                        .cloned()
                        .unwrap_or_else(|| json!({})),
                    progress_token: msg.pointer("/params/_meta/progressToken").cloned(),
                },
                Some(tool) => {
                    Dispatch::Reply(rpc_error(id, -32602, &format!("unknown tool '{tool}'")))
                }
                None => Dispatch::Reply(rpc_error(id, -32602, "tools/call missing params.name")),
            }
        }
        (Some(id), other) => {
            Dispatch::Reply(rpc_error(id, -32601, &format!("method '{other}' not supported")))
        }
    }
}

pub fn tool_defs() -> Value {
    json!([
        {
            "name": "open_document",
            "description": "Open a markdown or image file in the MDViewer window so the user can see it. Optionally scroll to a 1-based source line.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to a markdown or image file. Relative paths resolve against this MCP server's working directory (the project root)." },
                    "line": { "type": "integer", "description": "Optional 1-based line in the markdown source to scroll to and highlight." }
                },
                "required": ["path"]
            }
        },
        {
            "name": "request_review",
            "description": "Open a markdown document in MDViewer and ask the user to review it. Blocks until the user finishes (returns their comments as markdown) or declines (returns {\"declined\": true}). May take many minutes — the server emits progress to keep the call alive.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the markdown document to review." },
                    "instructions": { "type": "string", "description": "Optional short note shown to the user, e.g. what to focus on." }
                },
                "required": ["path"]
            }
        },
        {
            "name": "get_viewer_state",
            "description": "Report which document the user is currently viewing in MDViewer and whether a review is in progress.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

pub fn rpc_result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

pub fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Wrap plain text as an MCP tools/call result (the only content kind we emit).
pub fn tool_text_result(rpc_id: Value, text: &str, is_error: bool) -> Value {
    let mut result = json!({ "content": [{ "type": "text", "text": text }] });
    if is_error {
        result["isError"] = json!(true);
    }
    rpc_result(rpc_id, result)
}

pub fn progress_notification(token: &Value, progress: u64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/progress",
        "params": { "progressToken": token, "progress": progress }
    })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test mcp::`
Expected: 9 passed.

- [ ] **Step 5: Lint and commit**

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings
git add src-tauri/src/mcp.rs src-tauri/src/lib.rs
git commit -m "Add mcp module: pure JSON-RPC dispatch for the MCP stdio proxy"
```

---

### Task 2: `mcp.rs` — validation, internal codec, socket naming, reply text (pure)

**Files:**
- Modify: `src-tauri/src/mcp.rs`

- [ ] **Step 1: Write the failing tests**

Append inside the existing `tests` module:

```rust
    #[test]
    fn viewable_path_allowlists_markdown_and_images() {
        // Mirrors ui/app.js MD_EXT and ui/filetype.js IMAGE_EXT.
        for p in ["a.md", "B.MARKDOWN", "x.mdown", "x.mkd", "x.mkdn",
                  "i.png", "i.jpg", "i.JPEG", "i.gif", "i.webp", "i.avif",
                  "i.bmp", "i.ico", "i.svg"] {
            assert!(viewable_path(p), "{p} should be viewable");
        }
        for p in ["x.txt", "x.rs", "x.app", "x", "x.md.exe", "plan.md.sh"] {
            assert!(!viewable_path(p), "{p} should be rejected");
        }
    }

    #[test]
    fn markdown_path_rejects_images() {
        assert!(markdown_path("plan.md"));
        assert!(markdown_path("PLAN.MARKDOWN"));
        assert!(!markdown_path("shot.png"));
        assert!(!markdown_path("notes.txt"));
    }

    #[test]
    fn gui_codec_round_trips() {
        let req = GuiRequest { id: 5, tool: "open_document".into(), args: json!({"path": "a.md"}) };
        let line = serde_json::to_string(&req).unwrap();
        let back: GuiRequest = serde_json::from_str(&line).unwrap();
        assert_eq!(back, req);

        let ok = GuiReply { id: 5, result: Some("done".into()), error: None };
        let line = serde_json::to_string(&ok).unwrap();
        assert!(!line.contains("error")); // skip_serializing_if keeps lines lean
        let back: GuiReply = serde_json::from_str(&line).unwrap();
        assert_eq!(back, ok);

        let err: GuiReply = serde_json::from_str(r#"{"id":6,"error":"nope"}"#).unwrap();
        assert_eq!(err.error.as_deref(), Some("nope"));
        assert_eq!(err.result, None);
    }

    #[test]
    fn review_reply_text_distinguishes_decline() {
        assert_eq!(review_reply_text(Some("Review of plan.md\n…".into())), "Review of plan.md\n…");
        assert_eq!(review_reply_text(None), r#"{"declined": true}"#);
    }

    #[test]
    fn event_names_map_tools() {
        assert_eq!(event_name("open_document"), Some("mcp-open-document"));
        assert_eq!(event_name("request_review"), Some("mcp-request-review"));
        assert_eq!(event_name("get_viewer_state"), Some("mcp-get-state"));
        assert_eq!(event_name("bogus"), None);
    }

    #[test]
    fn event_payload_whitelists_args_and_adds_request_id() {
        let req = GuiRequest {
            id: 1,
            tool: "request_review".into(),
            args: json!({"path": "p.md", "instructions": "focus", "evil": "x"}),
        };
        let p = event_payload(42, &req);
        assert_eq!(p["requestId"], 42);
        assert_eq!(p["path"], "p.md");
        assert_eq!(p["instructions"], "focus");
        assert!(p.get("evil").is_none());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test mcp::`
Expected: compile error — `viewable_path` etc. not found.

- [ ] **Step 3: Write the implementation**

Append to `src-tauri/src/mcp.rs` (above `tests`):

```rust
use serde::{Deserialize, Serialize};

/// One forwarded tool call, proxy → GUI, as a single socket line.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct GuiRequest {
    pub id: u64,
    pub tool: String,
    pub args: Value,
}

/// One reply, GUI → proxy. Exactly one of result/error is set.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct GuiReply {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// GUI not ready yet (webview still booting); the proxy retries on this.
pub const STARTING_ERR: &str = "mdviewer is starting";

const MD_EXTS: [&str; 5] = ["md", "markdown", "mdown", "mkd", "mkdn"];
const IMAGE_EXTS: [&str; 9] = ["png", "jpg", "jpeg", "gif", "webp", "avif", "bmp", "ico", "svg"];

fn ext_of(path: &str) -> Option<String> {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

/// Allowlist of what `open_document` may open — mirrors the frontend's
/// MD_EXT (app.js) and IMAGE_EXT (filetype.js). Stricter than the
/// UNSAFE_OPEN_EXTS denylist: paths from Claude are untrusted input.
pub fn viewable_path(path: &str) -> bool {
    ext_of(path)
        .map(|e| MD_EXTS.contains(&e.as_str()) || IMAGE_EXTS.contains(&e.as_str()))
        .unwrap_or(false)
}

/// `request_review` targets must be markdown — reviewing an image is meaningless.
pub fn markdown_path(path: &str) -> bool {
    ext_of(path)
        .map(|e| MD_EXTS.contains(&e.as_str()))
        .unwrap_or(false)
}

/// The tool result text for a finished review: the review markdown, or the
/// spec'd decline marker (a successful result, not an error, so Claude
/// proceeds gracefully).
pub fn review_reply_text(review: Option<String>) -> String {
    review.unwrap_or_else(|| r#"{"declined": true}"#.to_string())
}

/// Tauri event each tool maps to. None for unknown tools (defense in depth —
/// the proxy already filters against KNOWN_TOOLS).
pub fn event_name(tool: &str) -> Option<&'static str> {
    match tool {
        "open_document" => Some("mcp-open-document"),
        "request_review" => Some("mcp-request-review"),
        "get_viewer_state" => Some("mcp-get-state"),
        _ => None,
    }
}

/// Webview event payload: requestId + whitelisted args only, so an odd field
/// from a hostile client never reaches the frontend.
pub fn event_payload(gui_id: u64, req: &GuiRequest) -> Value {
    let mut p = json!({ "requestId": gui_id });
    for key in ["path", "line", "instructions"] {
        if let Some(v) = req.args.get(key) {
            p[key] = v.clone();
        }
    }
    p
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test mcp::`
Expected: 15 passed.

- [ ] **Step 5: Lint and commit**

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings
git add src-tauri/src/mcp.rs
git commit -m "Add MCP tool validation, internal socket codec, and event mapping"
```

---

### Task 3: `mcp.rs` — `.mcp.json` merge (pure)

**Files:**
- Modify: `src-tauri/src/mcp.rs`

- [ ] **Step 1: Write the failing tests**

Append inside the `tests` module:

```rust
    use crate::claude_hook::HookOutcome;

    #[test]
    fn merge_into_empty_installs_server_entry() {
        let (merged, outcome) = merge_mcp_config(json!({}), "/x/mdviewer").unwrap();
        assert_eq!(outcome, HookOutcome::Installed);
        let entry = &merged["mcpServers"]["mdviewer"];
        assert_eq!(entry["command"], "/x/mdviewer");
        assert_eq!(entry["args"], json!(["--mcp"]));
    }

    #[test]
    fn merge_preserves_other_servers_and_updates_ours() {
        let existing = json!({
            "mcpServers": {
                "other": { "command": "npx", "args": ["x"] },
                "mdviewer": { "command": "/old/mdviewer", "args": ["--mcp"] }
            }
        });
        let (merged, outcome) = merge_mcp_config(existing, "/new/mdviewer").unwrap();
        assert_eq!(outcome, HookOutcome::Updated);
        assert_eq!(merged["mcpServers"]["other"]["command"], "npx");
        assert_eq!(merged["mcpServers"]["mdviewer"]["command"], "/new/mdviewer");
    }

    #[test]
    fn merge_refuses_wrong_types() {
        assert!(merge_mcp_config(json!([]), "/x").is_err());
        assert!(merge_mcp_config(json!({"mcpServers": "oops"}), "/x").is_err());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test mcp::merge`
Expected: compile error — `merge_mcp_config` not found.

- [ ] **Step 3: Write the implementation**

Append to `src-tauri/src/mcp.rs` (above `tests`):

```rust
/// Merge our MCP server into a `.mcp.json` document. The command is the raw
/// executable path — args are a JSON array, so no shell quoting is needed
/// (unlike claude_hook::hook_command). Refuses (without modifying) on
/// unexpected types so a user's config is never clobbered.
pub fn merge_mcp_config(
    mut config: Value,
    exe: &str,
) -> Result<(Value, crate::claude_hook::HookOutcome), String> {
    use crate::claude_hook::HookOutcome;
    if !config.is_object() {
        return Err("config root is not a JSON object".to_string());
    }
    let obj = config.as_object_mut().unwrap();
    let servers = obj.entry("mcpServers").or_insert_with(|| json!({}));
    if !servers.is_object() {
        return Err("`mcpServers` is not a JSON object".to_string());
    }
    let servers_obj = servers.as_object_mut().unwrap();
    let outcome = if servers_obj.contains_key("mdviewer") {
        HookOutcome::Updated
    } else {
        HookOutcome::Installed
    };
    servers_obj.insert(
        "mdviewer".to_string(),
        json!({ "command": exe, "args": ["--mcp"] }),
    );
    Ok((config, outcome))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test mcp::`
Expected: 18 passed.

- [ ] **Step 5: Lint and commit**

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings
git add src-tauri/src/mcp.rs
git commit -m "Add .mcp.json merge for the MCP server installer"
```

---

### Task 4: `claude_hook.rs` — extract `launch_mdviewer(Option<&str>)`

The proxy needs "launch the GUI **without** a file"; the hook launches it
**with** one. Extract the shared logic. Pure refactor — existing tests must
stay green.

**Files:**
- Modify: `src-tauri/src/claude_hook.rs`

- [ ] **Step 1: Refactor**

Replace the three `open_in_mdviewer` cfg variants with:

```rust
fn open_in_mdviewer(path: &str) {
    launch_mdviewer(Some(path));
}

/// Launch (or message) the MDViewer GUI, optionally handing it a file to open.
/// macOS: `open -a <bundle>` reaches the running instance (warm open adds a
/// tab); the dev binary (no .app ancestor) is spawned directly. Windows:
/// re-spawn the exe. Child stdio is detached so callers never block.
#[cfg(target_os = "macos")]
pub fn launch_mdviewer(path: Option<&str>) {
    use std::process::{Command, Stdio};
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(_) => return,
    };
    // …/MDViewer.app/Contents/MacOS/mdviewer → the .app bundle is 3 ancestors up.
    let bundle = exe
        .ancestors()
        .nth(3)
        .filter(|p| p.extension().map(|e| e == "app").unwrap_or(false));
    let mut cmd = match bundle {
        Some(app) => {
            let mut c = Command::new("open");
            c.arg("-a").arg(app);
            if let Some(p) = path {
                c.arg(p);
            }
            c
        }
        // Dev build: launch this binary directly. `open -b com.mdviewer.app`
        // would route to a stale installed bundle, confusing during development.
        None => {
            let mut c = Command::new(&exe);
            if let Some(p) = path {
                c.arg(p);
            }
            c
        }
    };
    let _ = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[cfg(target_os = "windows")]
pub fn launch_mdviewer(path: Option<&str>) {
    use std::process::{Command, Stdio};
    if let Ok(exe) = std::env::current_exe() {
        let mut c = Command::new(exe);
        if let Some(p) = path {
            c.arg(p);
        }
        let _ = c
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn launch_mdviewer(_path: Option<&str>) {}
```

(The doc comments on the old `open_in_mdviewer` variants move onto
`launch_mdviewer`; `open_in_mdviewer` itself needs no cfg anymore.)

- [ ] **Step 2: Run the full test suite**

Run: `cd src-tauri && cargo test`
Expected: all existing tests pass (113+ Rust tests), no new failures.

- [ ] **Step 3: Lint and commit**

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings
git add src-tauri/src/claude_hook.rs
git commit -m "Extract launch_mdviewer so the MCP proxy can start the GUI without a file"
```

---

### Task 5: `mcp_server.rs` — the pending-request map (pure-ish, std-only)

The map converts a blocking socket request into the app's event-driven world.
Key design point: `resolve` doesn't just hand the reply over — it **waits for
the connection thread's write acknowledgement**, so `mcp_review_result` learns
synchronously whether the proxy is still alive (the spec's clipboard-fallback
trigger).

**Files:**
- Create: `src-tauri/src/mcp_server.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod mcp_server;` between `mod mcp;` — added as `pub mod mcp;` in Task 1 — and `mod menu;`)

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/mcp_server.rs`:

```rust
//! GUI-side MCP socket listener: accepts connections from `mdviewer --mcp`
//! proxies, forwards tool calls to the webview as Tauri events, and routes
//! replies back through `McpPending`. The pending map is unit-tested; the
//! listener/connection runtime is IO, covered by the manual smoke test.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_assigns_unique_ids() {
        let p = McpPending::default();
        let (a, _rx_a) = p.register();
        let (b, _rx_b) = p.register();
        assert_ne!(a, b);
    }

    #[test]
    fn resolve_round_trips_through_connection_thread() {
        let p = std::sync::Arc::new(McpPending::default());
        let (id, rx) = p.register();
        // Fake connection thread: receive the reply, "write" it, ack success.
        let t = std::thread::spawn(move || {
            let (reply, ack) = rx.recv().unwrap();
            assert_eq!(reply, Ok("hello".to_string()));
            ack.send(Ok(())).unwrap();
        });
        assert!(p.resolve(id, Ok("hello".to_string())).is_ok());
        t.join().unwrap();
    }

    #[test]
    fn resolve_unknown_id_errors_without_blocking() {
        let p = McpPending::default();
        assert!(p.resolve(999, Ok("x".to_string())).is_err());
    }

    #[test]
    fn resolve_reports_a_dead_connection() {
        let p = McpPending::default();
        let (id, rx) = p.register();
        drop(rx); // connection thread is gone (proxy died)
        assert!(p.resolve(id, Ok("x".to_string())).is_err());
    }

    #[test]
    fn resolve_surfaces_write_failure() {
        let p = std::sync::Arc::new(McpPending::default());
        let (id, rx) = p.register();
        let t = std::thread::spawn(move || {
            let (_reply, ack) = rx.recv().unwrap();
            ack.send(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "pipe closed",
            )))
            .unwrap();
        });
        let err = p.resolve(id, Ok("x".to_string())).unwrap_err();
        assert!(err.contains("pipe closed"), "got: {err}");
        t.join().unwrap();
    }

    #[test]
    fn forget_removes_the_entry() {
        let p = McpPending::default();
        let (id, _rx) = p.register();
        p.forget(id);
        assert!(p.resolve(id, Ok("x".to_string())).is_err());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test mcp_server::`
Expected: compile error — `McpPending` not found.

- [ ] **Step 3: Write the implementation**

Insert above the `tests` module:

```rust
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::Duration;

/// The webview's answer for one tool call: Ok(text) or Err(message).
pub type Reply = Result<String, String>;
/// Reply plus an ack channel the connection thread reports its write on.
type Handoff = (Reply, mpsc::Sender<std::io::Result<()>>);

/// In-flight MCP requests, keyed by a GUI-generated id (NOT the proxy's
/// JSON-RPC id, which can collide across connections). Managed Tauri state.
#[derive(Default)]
pub struct McpPending {
    next_id: AtomicU64,
    waiting: Mutex<HashMap<u64, mpsc::Sender<Handoff>>>,
}

impl McpPending {
    /// Register a new in-flight request. The connection thread blocks on the
    /// returned receiver until a command resolves it.
    pub fn register(&self) -> (u64, mpsc::Receiver<Handoff>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = mpsc::channel();
        self.waiting.lock().unwrap().insert(id, tx);
        (id, rx)
    }

    /// Deliver a reply and wait for the connection thread's socket-write ack,
    /// so callers (e.g. mcp_review_result) know synchronously whether the
    /// proxy received it — the trigger for the frontend's clipboard fallback.
    /// Errors on an unknown id (already resolved, or fabricated by the
    /// webview) and on a dead or failing connection.
    pub fn resolve(&self, id: u64, reply: Reply) -> Result<(), String> {
        let tx = self
            .waiting
            .lock()
            .unwrap()
            .remove(&id)
            .ok_or_else(|| format!("unknown MCP request id {id}"))?;
        let (ack_tx, ack_rx) = mpsc::channel();
        tx.send((reply, ack_tx))
            .map_err(|_| "the MCP connection is gone".to_string())?;
        match ack_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(format!("socket write failed: {e}")),
            Err(_) => Err("the MCP connection did not acknowledge".to_string()),
        }
    }

    /// Drop a request without replying (failed emit, app teardown).
    pub fn forget(&self, id: u64) {
        self.waiting.lock().unwrap().remove(&id);
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test mcp_server::`
Expected: 6 passed.

- [ ] **Step 5: Lint and commit**

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings
git add src-tauri/src/mcp_server.rs src-tauri/src/lib.rs
git commit -m "Add McpPending: ack-confirmed pending-request map for MCP calls"
```

---

### Task 6: `mcp_server.rs` — socket listener runtime + `lib.rs` wiring

IO-heavy; verified by build + clippy here and end-to-end in Task 9/14.

**Files:**
- Modify: `src-tauri/Cargo.toml` (add `interprocess = "2"` to `[dependencies]`)
- Modify: `src-tauri/src/mcp.rs` (socket naming helpers — they need `interprocess` types)
- Modify: `src-tauri/src/mcp_server.rs` (listener + connection handler)
- Modify: `src-tauri/src/lib.rs` (manage `McpPending`, start the listener in setup)

- [ ] **Step 1: Add the dependency**

In `src-tauri/Cargo.toml` under `[dependencies]`, add:

```toml
interprocess = "2"
```

- [ ] **Step 2: Socket naming in `mcp.rs`**

Append to `src-tauri/src/mcp.rs` (above `tests`):

```rust
/// The local-socket name both sides agree on. Namespaced (a named pipe) on
/// Windows; a filesystem path in the per-user `$TMPDIR` elsewhere (macOS UDS
/// paths cap at ~104 bytes, so never app_data_dir). `MDVIEWER_MCP_SOCKET`
/// overrides for tests and dev.
pub fn socket_name() -> std::io::Result<interprocess::local_socket::Name<'static>> {
    use interprocess::local_socket::{GenericFilePath, GenericNamespaced, ToFsName, ToNsName};
    let custom = std::env::var("MDVIEWER_MCP_SOCKET").ok();
    if interprocess::local_socket::GenericNamespaced::is_supported() {
        custom
            .unwrap_or_else(|| "mdviewer-mcp.sock".to_string())
            .to_ns_name::<GenericNamespaced>()
    } else {
        socket_fs_path()
            .expect("fs path exists when namespaced is unsupported")
            .to_fs_name::<GenericFilePath>()
    }
}

/// The socket's filesystem path, when it has one (unix). Used to unlink a
/// stale socket file before binding. None on namespaced (Windows) transports.
pub fn socket_fs_path() -> Option<std::path::PathBuf> {
    use interprocess::local_socket::GenericNamespaced;
    if GenericNamespaced::is_supported() {
        return None;
    }
    Some(
        std::env::var("MDVIEWER_MCP_SOCKET")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir().join("mdviewer-mcp.sock")),
    )
}
```

> NOTE for the implementer: `interprocess` 2.x renamed its API between major
> versions. The shapes used in this plan (`Name`, `ToNsName::to_ns_name`,
> `ToFsName::to_fs_name`, `GenericNamespaced::is_supported()`,
> `ListenerOptions::new().name(n).create_sync()`, `Stream::connect(n)`,
> `io::Read/Write impls on &Stream`) are the 2.x surface. If `cargo build`
> disagrees on an item path, fix the import per `cargo doc -p interprocess`
> — the *behavior* in this plan is the contract, the import paths are not.

- [ ] **Step 3: Listener runtime in `mcp_server.rs`**

Append above the `tests` module:

```rust
use std::io::{BufRead, BufReader, Write};

use interprocess::local_socket::traits::{Listener as _, Stream as _};
use interprocess::local_socket::{ListenerOptions, Stream};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::mcp::{self, GuiReply, GuiRequest};

/// Spawn the MCP socket listener. Failure to bind only disables MCP — the
/// viewer itself must keep working — so errors are logged, never fatal.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        if let Err(e) = listen_loop(app) {
            eprintln!("mdviewer: MCP listener disabled: {e}");
        }
    });
}

fn listen_loop(app: AppHandle) -> std::io::Result<()> {
    let name = mcp::socket_name()?;
    // Stale-socket handling: probe before binding. A live instance answers
    // the connect — back off rather than steal its socket. A dead leftover
    // file (crash) is unlinked so the bind succeeds.
    if Stream::connect(name.clone()).is_ok() {
        eprintln!("mdviewer: another instance owns the MCP socket; MCP disabled here");
        return Ok(());
    }
    if let Some(p) = mcp::socket_fs_path() {
        let _ = std::fs::remove_file(&p);
    }
    let listener = ListenerOptions::new().name(name).create_sync()?;
    for conn in listener.incoming() {
        if let Ok(stream) = conn {
            let app = app.clone();
            std::thread::spawn(move || handle_connection(app, stream));
        }
    }
    Ok(())
}

/// One proxy connection. Strictly sequential — the proxy forwards one call
/// and waits for its reply before reading the next stdin line, so a
/// one-request-at-a-time loop is consistent end-to-end.
fn handle_connection(app: AppHandle, stream: Stream) {
    let mut reader = BufReader::new(&stream);
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let req: GuiRequest = match serde_json::from_str(line.trim()) {
            Ok(r) => r,
            Err(_) => continue,
        };
        match prepare_request(&app, &req) {
            // Rejected before reaching the webview (validation, not ready).
            Err(e) => {
                let reply = GuiReply { id: req.id, result: None, error: Some(e) };
                if write_line(&stream, &reply).is_err() {
                    return;
                }
            }
            // Parked: block until a command resolves it, write the reply, and
            // ack with the WRITE outcome. That ack is what resolve() returns
            // to mcp_review_result — the frontend learns synchronously whether
            // the proxy actually received the review (its clipboard-fallback
            // trigger). Acking before the write would report success for a
            // review the dead proxy never saw.
            Ok(rx) => match rx.recv() {
                Ok((reply, ack)) => {
                    let gui_reply = match reply {
                        Ok(text) => GuiReply { id: req.id, result: Some(text), error: None },
                        Err(e) => GuiReply { id: req.id, result: None, error: Some(e) },
                    };
                    let res = write_line(&stream, &gui_reply);
                    let failed = res.is_err();
                    let _ = ack.send(res);
                    if failed {
                        return;
                    }
                }
                Err(_) => return, // app shutting down
            },
        }
    }
}

fn write_line(mut stream: &Stream, reply: &GuiReply) -> std::io::Result<()> {
    let mut line = serde_json::to_string(reply).map_err(std::io::Error::other)?;
    line.push('\n');
    stream.write_all(line.as_bytes())
}

/// Validate and forward one tool call to the webview. The returned receiver
/// resolves when an mcp_respond / mcp_review_result command answers it.
fn prepare_request(
    app: &AppHandle,
    req: &GuiRequest,
) -> Result<std::sync::mpsc::Receiver<Handoff>, String> {
    if !app.state::<crate::AppState>().opens.lock().unwrap().ready {
        return Err(mcp::STARTING_ERR.to_string());
    }
    validate(req)?;
    let event = mcp::event_name(&req.tool).ok_or_else(|| format!("unknown tool '{}'", req.tool))?;

    let pending = app.state::<McpPending>();
    let (gui_id, rx) = pending.register();
    if app.emit(event, mcp::event_payload(gui_id, req)).is_err() {
        pending.forget(gui_id);
        return Err("cannot reach the MDViewer window".to_string());
    }
    if req.tool == "request_review" {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.set_focus();
        }
    }
    Ok(rx)
}

fn validate(req: &GuiRequest) -> Result<(), String> {
    let path = || -> Result<&str, String> {
        req.args
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required argument 'path'".to_string())
    };
    match req.tool.as_str() {
        "open_document" => {
            let p = path()?;
            if !mcp::viewable_path(p) {
                return Err(format!("'{p}' is not a markdown or image file"));
            }
            if !std::path::Path::new(p).is_file() {
                return Err(format!("file not found: {p}"));
            }
            Ok(())
        }
        "request_review" => {
            let p = path()?;
            if !mcp::markdown_path(p) {
                return Err(format!("'{p}' is not a markdown file"));
            }
            if !std::path::Path::new(p).is_file() {
                return Err(format!("file not found: {p}"));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
```

**Design note on paths:** relative paths in `validate` would resolve against
the GUI process's cwd, which is unrelated to the project. The **proxy**
absolutizes paths before forwarding (Task 8), since Claude Code spawns MCP
servers with cwd = project root; by the time `validate` runs, `path` is
absolute.

- [ ] **Step 4: Wire `lib.rs`**

In `run()`:
- after `.manage(state)` add `.manage(mcp_server::McpPending::default())`
- in the setup hook, after `menu::install(&handle)?;` add:

```rust
            mcp_server::start(handle.clone());
```

- [ ] **Step 5: Build, test, lint**

Run: `cd src-tauri && cargo build && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: builds clean; all tests pass. If `interprocess` item paths differ, fix imports per the Step 2 note (behavior unchanged).

- [ ] **Step 6: Commit**

```bash
cd src-tauri && cargo fmt
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/mcp.rs src-tauri/src/mcp_server.rs src-tauri/src/lib.rs
git commit -m "Add GUI-side MCP socket listener with stale-socket takeover"
```

---

### Task 7: commands — `mcp_respond` + `mcp_review_result`

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs` (register both commands)

- [ ] **Step 1: Add the commands**

Append to `src-tauri/src/commands.rs` (near `install_claude_hook`):

```rust
/// Generic reply for a pending MCP request (open_document, get_viewer_state,
/// and rejections like "a review is already in progress"). Only resolves ids
/// the pending map knows — the webview can't fabricate responses.
#[tauri::command]
pub fn mcp_respond(
    pending: State<'_, crate::mcp_server::McpPending>,
    request_id: u64,
    text: String,
    is_error: bool,
) -> Result<(), String> {
    let reply = if is_error { Err(text) } else { Ok(text) };
    pending.resolve(request_id, reply)
}

/// Finish or decline an MCP-requested review. `review: None` means declined —
/// a successful tool result, not an error, so Claude proceeds gracefully.
/// Errors when the proxy is gone, which the frontend turns into the
/// clipboard fallback.
#[tauri::command]
pub fn mcp_review_result(
    pending: State<'_, crate::mcp_server::McpPending>,
    request_id: u64,
    review: Option<String>,
) -> Result<(), String> {
    pending.resolve(request_id, Ok(crate::mcp::review_reply_text(review)))
}
```

- [ ] **Step 2: Register in `lib.rs`**

In the `generate_handler!` list, after `commands::install_claude_hook,` add:

```rust
            commands::mcp_respond,
            commands::mcp_review_result,
```

- [ ] **Step 3: Build, test, lint, commit**

Run: `cd src-tauri && cargo build && cargo test && cargo fmt && cargo clippy --all-targets -- -D warnings`
Expected: clean.

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "Add mcp_respond and mcp_review_result commands"
```

---

### Task 8: `mcp.rs` — proxy runtime (`run_proxy`) + `--mcp` flag

**Files:**
- Modify: `src-tauri/src/mcp.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub fn run_mcp_proxy()`)
- Modify: `src-tauri/src/main.rs` (`--mcp` check)

- [ ] **Step 1: Implement the proxy**

Append to `src-tauri/src/mcp.rs` (above `tests`):

```rust
use std::io::{BufRead, BufReader, Write};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use interprocess::local_socket::traits::Stream as _;
use interprocess::local_socket::Stream;

const KEEPALIVE_SECS: u64 = 10;
const STARTING_RETRIES: u32 = 30; // × 500 ms = wait up to 15 s for the webview
const LAUNCH_RETRIES: u32 = 40; // × 250 ms = wait up to 10 s for a cold GUI launch

/// Entry point for `mdviewer --mcp`: a stdio MCP server that relays tool
/// calls to the running GUI over the local socket, launching the GUI when it
/// isn't running. Exits when stdin closes (the MCP client ended the session).
pub fn run_proxy() {
    let stdout: Arc<Mutex<std::io::Stdout>> = Arc::new(Mutex::new(std::io::stdout()));
    let mut conn: Option<Stream> = None;
    let mut next_gui_id: u64 = 0;
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        match handle_message(&line) {
            Dispatch::Ignore => {}
            Dispatch::Reply(v) => write_json_line(&stdout, &v),
            Dispatch::Call { rpc_id, tool, args, progress_token } => {
                let response =
                    match forward_call(&mut conn, &stdout, &mut next_gui_id, &tool, args, &progress_token) {
                        Ok(reply) => match (reply.result, reply.error) {
                            (Some(text), _) => tool_text_result(rpc_id, &text, false),
                            (None, Some(e)) => tool_text_result(rpc_id, &e, true),
                            (None, None) => tool_text_result(rpc_id, "empty reply", true),
                        },
                        Err(e) => rpc_error(rpc_id, -32000, &e),
                    };
                write_json_line(&stdout, &response);
            }
        }
    }
}

fn write_json_line(stdout: &Arc<Mutex<std::io::Stdout>>, v: &Value) {
    let mut out = stdout.lock().unwrap();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

/// Make relative paths from the client absolute against OUR cwd (Claude Code
/// spawns MCP servers with cwd = project root). The GUI's cwd is unrelated.
fn absolutize_path_arg(args: &mut Value) {
    if let Some(p) = args.get("path").and_then(Value::as_str) {
        let pb = std::path::PathBuf::from(p);
        if pb.is_relative() {
            if let Ok(cwd) = std::env::current_dir() {
                args["path"] = json!(cwd.join(pb).to_string_lossy());
            }
        }
    }
}

fn forward_call(
    conn: &mut Option<Stream>,
    stdout: &Arc<Mutex<std::io::Stdout>>,
    next_gui_id: &mut u64,
    tool: &str,
    mut args: Value,
    progress_token: &Option<Value>,
) -> Result<GuiReply, String> {
    absolutize_path_arg(&mut args);
    for _ in 0..STARTING_RETRIES {
        ensure_connection(conn)?;
        *next_gui_id += 1;
        let req = GuiRequest {
            id: *next_gui_id,
            tool: tool.to_string(),
            args: args.clone(),
        };
        let stream = conn.as_mut().expect("ensure_connection populated conn");
        let mut line = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        line.push('\n');
        if stream.write_all(line.as_bytes()).is_err() {
            // The GUI restarted since the last call — reconnect once.
            *conn = None;
            ensure_connection(conn)?;
            let stream = conn.as_mut().expect("ensure_connection populated conn");
            stream
                .write_all(line.as_bytes())
                .map_err(|e| format!("cannot reach mdviewer: {e}"))?;
        }
        let reply = {
            let _ticker = ProgressTicker::start(stdout.clone(), progress_token.clone());
            read_reply(conn)?
        };
        if reply.error.as_deref() == Some(STARTING_ERR) {
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }
        return Ok(reply);
    }
    Err("mdviewer did not finish starting".to_string())
}

/// One reply per request is the protocol invariant, so a throwaway BufReader
/// can't buffer-steal bytes that belong to a later reply.
fn read_reply(conn: &mut Option<Stream>) -> Result<GuiReply, String> {
    let stream = conn.as_mut().expect("caller ensured connection");
    let mut reader = BufReader::new(&*stream);
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => {
            *conn = None;
            Err("mdviewer closed before the request finished".to_string())
        }
        Err(e) => {
            *conn = None;
            Err(format!("mdviewer connection failed: {e}"))
        }
        Ok(_) => serde_json::from_str(line.trim())
            .map_err(|e| format!("malformed reply from mdviewer: {e}")),
    }
}

fn ensure_connection(conn: &mut Option<Stream>) -> Result<(), String> {
    if conn.is_some() {
        return Ok(());
    }
    let name = socket_name().map_err(|e| format!("cannot build socket name: {e}"))?;
    if let Ok(s) = Stream::connect(name.clone()) {
        *conn = Some(s);
        return Ok(());
    }
    crate::claude_hook::launch_mdviewer(None);
    for _ in 0..LAUNCH_RETRIES {
        std::thread::sleep(Duration::from_millis(250));
        if let Ok(s) = Stream::connect(name.clone()) {
            *conn = Some(s);
            return Ok(());
        }
    }
    Err("cannot reach MDViewer (launch failed or timed out)".to_string())
}

/// Emits notifications/progress every KEEPALIVE_SECS while alive, so the MCP
/// client's tool timeout doesn't fire during a long review. Drop stops it.
struct ProgressTicker {
    stop: Option<mpsc::Sender<()>>,
}

impl ProgressTicker {
    fn start(stdout: Arc<Mutex<std::io::Stdout>>, token: Option<Value>) -> Self {
        let Some(token) = token else {
            return Self { stop: None };
        };
        let (tx, rx) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            let mut n: u64 = 0;
            while let Err(mpsc::RecvTimeoutError::Timeout) =
                rx.recv_timeout(Duration::from_secs(KEEPALIVE_SECS))
            {
                n += 1;
                let note = progress_notification(&token, n);
                let mut out = stdout.lock().unwrap();
                let _ = writeln!(out, "{note}");
                let _ = out.flush();
            }
        });
        Self { stop: Some(tx) }
    }
}

impl Drop for ProgressTicker {
    fn drop(&mut self) {
        // Dropping the sender disconnects recv_timeout and ends the thread.
        self.stop.take();
    }
}
```

- [ ] **Step 2: Wire `lib.rs` and `main.rs`**

`lib.rs`, next to `run_claude_hook`:

```rust
/// Run the `--mcp` stdio MCP proxy and return (never launches the GUI).
pub fn run_mcp_proxy() {
    mcp::run_proxy();
}
```

`main.rs`, immediately after the `--claude-hook` check:

```rust
    if std::env::args().nth(1).as_deref() == Some("--mcp") {
        mdviewer_lib::run_mcp_proxy();
        return ExitCode::SUCCESS;
    }
```

- [ ] **Step 3: Build, test, lint, commit**

Run: `cd src-tauri && cargo build && cargo test && cargo fmt && cargo clippy --all-targets -- -D warnings`
Expected: clean.

```bash
git add src-tauri/src/mcp.rs src-tauri/src/lib.rs src-tauri/src/main.rs
git commit -m "Add the --mcp stdio proxy: forward tool calls to the GUI socket"
```

---

### Task 9: integration test — proxy ↔ fake GUI over a real socket

**Files:**
- Create: `src-tauri/tests/mcp_proxy.rs`

- [ ] **Step 1: Write the test**

```rust
//! End-to-end test of `mdviewer --mcp`: a fake GUI binds the (overridden)
//! socket, the proxy is spawned as a real child process, and JSON-RPC flows
//! through stdin/stdout.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use interprocess::local_socket::traits::Listener as _;
use interprocess::local_socket::ListenerOptions;
use serde_json::{json, Value};

// ONE test function on purpose: socket_name() reads MDVIEWER_MCP_SOCKET from
// the process environment, and `cargo test` runs test fns in parallel threads
// — two tests setting the var would race.
#[test]
fn proxy_round_trips_and_reports_eof_mid_request() {
    let sock = test_socket_id();
    std::env::set_var("MDVIEWER_MCP_SOCKET", &sock);

    let listener = ListenerOptions::new()
        .name(mdviewer_lib::mcp::socket_name().unwrap())
        .create_sync()
        .unwrap();

    // Fake GUI: answers open_document calls, drops the connection without
    // replying on request_review (the EOF-mid-request scenario). Detached —
    // it blocks on accept at the end and dies with the test process.
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(conn) = conn else { continue };
            let mut reader = BufReader::new(&conn);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let req: mdviewer_lib::mcp::GuiRequest =
                    serde_json::from_str(line.trim()).unwrap();
                if req.tool == "request_review" {
                    break; // close without replying — simulates a dying GUI
                }
                // The proxy absolutizes relative paths against its own cwd.
                let path = req.args["path"].as_str().unwrap();
                assert!(path.ends_with("plan.md") && path != "plan.md");
                let reply = mdviewer_lib::mcp::GuiReply {
                    id: req.id,
                    result: Some("Opened plan.md".to_string()),
                    error: None,
                };
                let mut out = serde_json::to_string(&reply).unwrap();
                out.push('\n');
                if (&conn).write_all(out.as_bytes()).is_err() {
                    break;
                }
            }
        }
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_mdviewer"))
        .arg("--mcp")
        .env("MDVIEWER_MCP_SOCKET", &sock)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let send = |stdin: &mut std::process::ChildStdin, v: Value| {
        let mut s = v.to_string();
        s.push('\n');
        stdin.write_all(s.as_bytes()).unwrap();
    };
    let recv = |stdout: &mut BufReader<std::process::ChildStdout>| -> Value {
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        serde_json::from_str(line.trim()).unwrap()
    };

    send(&mut stdin, json!({"jsonrpc":"2.0","id":1,"method":"initialize",
        "params":{"protocolVersion":"2025-06-18"}}));
    let init = recv(&mut stdout);
    assert_eq!(init["result"]["serverInfo"]["name"], "mdviewer");

    send(&mut stdin, json!({"jsonrpc":"2.0","method":"notifications/initialized"}));

    send(&mut stdin, json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    let list = recv(&mut stdout);
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 3);

    send(&mut stdin, json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
        "params":{"name":"open_document","arguments":{"path":"plan.md"}}}));
    let call = recv(&mut stdout);
    assert_eq!(call["id"], 3);
    assert_eq!(call["result"]["content"][0]["text"], "Opened plan.md");
    assert!(call["result"].get("isError").is_none());

    // EOF mid-request: the fake GUI hangs up instead of replying. The proxy
    // must answer with an error tool-result, not hang or crash.
    send(&mut stdin, json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
        "params":{"name":"request_review","arguments":{"path":"plan.md"}}}));
    let eof = recv(&mut stdout);
    assert_eq!(eof["id"], 4);
    let text = eof["result"]["content"][0]["text"].as_str().unwrap_or_else(|| {
        eof["error"]["message"].as_str().unwrap()
    });
    assert!(text.contains("closed") || text.contains("connection"), "got: {eof}");

    drop(stdin); // EOF ends the proxy loop
    let status = child.wait().unwrap();
    assert!(status.success());
}

/// Unique per test process: a namespaced pipe name on Windows, a short
/// temp-dir path elsewhere.
fn test_socket_id() -> String {
    let base = format!("mdviewer-mcp-test-{}.sock", std::process::id());
    if cfg!(windows) {
        base
    } else {
        std::env::temp_dir().join(base).to_string_lossy().into_owned()
    }
}
```

- [ ] **Step 2: Run it**

Run: `cd src-tauri && cargo test --test mcp_proxy`
Expected: PASS (the spawned proxy connects to the pre-bound fake GUI, so the
GUI-launch path never triggers).

- [ ] **Step 3: Lint and commit**

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings
git add src-tauri/tests/mcp_proxy.rs
git commit -m "Add integration test: --mcp proxy round-trips against a fake GUI socket"
```

---

### Task 10: `ui/mcp.js` — pure frontend helpers + tests

**Files:**
- Create: `ui/mcp.js`
- Create: `ui/mcp.test.js`

- [ ] **Step 1: Write the failing tests**

Create `ui/mcp.test.js`:

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  reviewButtonLabel,
  mcpHintText,
  reviewBusy,
  viewerState,
} from "./mcp.js";

test("reviewButtonLabel: off, manual, and MCP states", () => {
  assert.equal(reviewButtonLabel(false, null), "💬 Review");
  assert.equal(reviewButtonLabel(true, null), "✓ Finish & Copy");
  assert.equal(reviewButtonLabel(true, 7), "✓ Finish & Send");
});

test("mcpHintText: with and without instructions", () => {
  assert.equal(
    mcpHintText("plan.md", ""),
    "Claude is waiting for your review of plan.md.",
  );
  assert.equal(
    mcpHintText("plan.md", "  focus on Phase 2  "),
    "Claude is waiting for your review of plan.md — “focus on Phase 2”",
  );
});

test("reviewBusy: any reviewing or MCP-pending tab blocks", () => {
  assert.equal(reviewBusy([]), false);
  assert.equal(reviewBusy([{ reviewMode: false }]), false);
  assert.equal(reviewBusy([{ reviewMode: true }]), true);
  assert.equal(reviewBusy([{ reviewMode: false, mcpRequestId: 3 }]), true);
});

test("viewerState: reports active path and any review in progress", () => {
  assert.deepEqual(viewerState([], -1), { path: null, reviewing: false });
  const tabs = [{ path: "/a.md", reviewMode: false }, { path: "/b.md", reviewMode: true }];
  assert.deepEqual(viewerState(tabs, 0), { path: "/a.md", reviewing: true });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `node --test ui/mcp.test.js`
Expected: FAIL — cannot find module `./mcp.js`.

- [ ] **Step 3: Write the implementation**

Create `ui/mcp.js`:

```js
// Pure helpers for the MCP review loop. DOM-free + Tauri-free so they
// unit-test under `node --test`; event wiring lives in app.js.

/** Toolbar label: Send for an MCP-initiated review, Copy for a manual one. */
export function reviewButtonLabel(reviewMode, mcpRequestId) {
  if (!reviewMode) return "💬 Review";
  return mcpRequestId != null ? "✓ Finish & Send" : "✓ Finish & Copy";
}

/** Review-bar hint while Claude waits, with its optional instructions. */
export function mcpHintText(fileName, instructions) {
  const base = `Claude is waiting for your review of ${fileName}`;
  const extra = (instructions || "").trim();
  return extra ? `${base} — “${extra}”` : `${base}.`;
}

/** True when an incoming request_review must be rejected: some tab is already
 *  reviewing (manual outranks the agent) or another MCP review is pending. */
export function reviewBusy(tabs) {
  return tabs.some((t) => !!t.reviewMode || t.mcpRequestId != null);
}

/** What get_viewer_state reports. */
export function viewerState(tabs, activeIdx) {
  const t = activeIdx >= 0 && activeIdx < tabs.length ? tabs[activeIdx] : null;
  return {
    path: t ? t.path : null,
    reviewing: tabs.some((x) => !!x.reviewMode),
  };
}
```

- [ ] **Step 4: Run all JS tests**

Run: `node --test ui/*.test.js`
Expected: all pass (4 new).

- [ ] **Step 5: Commit**

```bash
git add ui/mcp.js ui/mcp.test.js
git commit -m "Add ui/mcp.js: pure helpers for the MCP review loop"
```

---

### Task 11: `app.js` wiring + `styles.css`

**Files:**
- Modify: `ui/app.js`
- Modify: `ui/styles.css`

- [ ] **Step 1: Import the helpers**

After the `review.js` import at the top of `ui/app.js`:

```js
import {
  reviewButtonLabel,
  mcpHintText,
  reviewBusy,
  viewerState,
} from "./mcp.js";
```

- [ ] **Step 2: Tab fields**

Tabs gain two ephemeral fields, `mcpRequestId` (number | null) and
`mcpInstructions` (string) — like the review fields, never serialized.
Update the model comment at `const tabs = []` to include them. In
`openPreview`'s tab-reuse branch (after `tabs[previewIdx].orphanedReviews = [];`) add:

```js
    if (tabs[previewIdx].mcpRequestId != null) {
      // Reusing a tab with a parked MCP review orphans Claude's request —
      // decline it so the agent isn't left hanging. (Shouldn't happen: MCP
      // reviews open sticky tabs. Defense in depth.)
      invoke("mcp_review_result", {
        requestId: tabs[previewIdx].mcpRequestId,
        review: null,
      }).catch(() => {});
    }
    tabs[previewIdx].mcpRequestId = null;
    tabs[previewIdx].mcpInstructions = "";
```

- [ ] **Step 3: Event listeners**

In `init()`, after the `menu-install-claude-hook` listener:

```js
  await listen("mcp-open-document", async (ev) => {
    const { requestId, path, line } = ev.payload;
    try {
      if (line != null) await openTabAtLine(path, line);
      else await openSticky(path);
      await invoke("mcp_respond", { requestId, text: `Opened ${path}`, isError: false });
    } catch (e) {
      await invoke("mcp_respond", { requestId, text: String(e), isError: true }).catch(() => {});
    }
  });

  await listen("mcp-request-review", async (ev) => {
    const { requestId, path, instructions } = ev.payload;
    if (reviewBusy(tabs)) {
      await invoke("mcp_respond", {
        requestId,
        text: "a review is already in progress",
        isError: true,
      }).catch(() => {});
      return;
    }
    try {
      await openSticky(path);
    } catch (e) {
      await invoke("mcp_respond", { requestId, text: String(e), isError: true }).catch(() => {});
      return;
    }
    const t = activeTab();
    t.reviewMode = true;
    t.mcpRequestId = requestId;
    t.mcpInstructions = instructions || "";
    renderTabBar();
    renderReviewMarkers(t);
  });

  await listen("mcp-get-state", async (ev) => {
    const { requestId } = ev.payload;
    await invoke("mcp_respond", {
      requestId,
      text: JSON.stringify(viewerState(tabs, activeIdx)),
      isError: false,
    }).catch(() => {});
  });
```

- [ ] **Step 4: Toolbar label via the helper**

In `renderTabBar`, replace:

```js
      reviewBtn.textContent = t.reviewMode ? "✓ Finish & Copy" : "💬 Review";
      reviewBtn.title = t.reviewMode
        ? "Copy your review to the clipboard and exit review mode"
        : "Comment on this document and copy your review for Claude Code";
```

with:

```js
      reviewBtn.textContent = reviewButtonLabel(t.reviewMode, t.mcpRequestId);
      reviewBtn.title = !t.reviewMode
        ? "Comment on this document and copy your review for Claude Code"
        : t.mcpRequestId != null
          ? "Send your review to the waiting Claude Code session"
          : "Copy your review to the clipboard and exit review mode";
```

- [ ] **Step 5: MCP variant of the review bar + decline**

Replace `renderReviewBar` with:

```js
function renderReviewBar(t) {
  const bar = document.createElement("div");
  bar.className = "review-bar";

  const hint = document.createElement("div");
  hint.className = "review-hint";

  if (t.mcpRequestId != null) {
    bar.classList.add("mcp");
    hint.textContent = "💬 " + mcpHintText(basename(t.path), t.mcpInstructions);
    const decline = document.createElement("button");
    decline.type = "button";
    decline.className = "review-decline-btn";
    decline.textContent = "Decline";
    decline.title = "Tell Claude you're skipping this review";
    decline.addEventListener("click", (ev) => {
      ev.preventDefault();
      declineMcpReview(t);
    });
    const row = document.createElement("div");
    row.className = "review-mcp-row";
    row.append(hint, decline);
    bar.appendChild(row);
  } else {
    hint.textContent =
      "Comment on any block (hover for the +), then Finish & Copy to paste your review into Claude Code.";
    bar.appendChild(hint);
  }

  const note = document.createElement("textarea");
  note.className = "review-general-note";
  note.rows = 2;
  note.placeholder = "General note about this document (optional)";
  note.value = t.generalNote || "";
  note.addEventListener("input", () => {
    t.generalNote = note.value;
  });

  bar.appendChild(note);
  preview.prepend(bar);
}

/** Decline Claude's pending review request and exit review mode. The reply is
 *  best-effort — a dead proxy just means nobody is listening anymore. */
function declineMcpReview(t) {
  const requestId = t.mcpRequestId;
  t.reviews = [];
  t.orphanedReviews = [];
  t.generalNote = "";
  t.reviewMode = false;
  t.mcpRequestId = null;
  t.mcpInstructions = "";
  renderTabBar();
  renderReviewMarkers(t);
  invoke("mcp_review_result", { requestId, review: null }).catch(() => {});
}
```

- [ ] **Step 6: `finishReview` branch**

Replace `finishReview` with:

```js
/** Finish a review: commit any open comment box, then deliver it — to the
 *  waiting MCP request when Claude asked for the review, else to the
 *  clipboard — clear the annotations, exit review mode, and confirm with a
 *  toast. On delivery failure the review is kept (clipboard path) or falls
 *  back to the clipboard (MCP path) so the user's work is never lost. */
async function finishReview(t) {
  const pendingSave = preview.querySelector(".review-input .review-save-btn");
  if (pendingSave) pendingSave.click();

  const hasContent =
    (t.reviews && t.reviews.length > 0) ||
    (t.orphanedReviews && t.orphanedReviews.length > 0) ||
    (t.generalNote || "").trim() !== "";
  const rel = relativeToRoot(t.path, treeRoot) || basename(t.path);
  const text = hasContent
    ? formatReview(t.reviews || [], t.generalNote || "", rel, t.orphanedReviews || [])
    : "";

  if (t.mcpRequestId != null) {
    const requestId = t.mcpRequestId;
    const review = hasContent
      ? text
      : `Review of ${rel}\n\nThe user finished the review without comments.\n`;
    try {
      await invoke("mcp_review_result", { requestId, review });
      showTransientMessage("Review sent to Claude Code");
    } catch (e) {
      console.error("mcp_review_result failed", e);
      // Claude's session is gone — preserve the user's work on the clipboard.
      const copied = hasContent ? await copyText(text) : true;
      if (!copied) {
        showTransientError("Couldn't deliver or copy the review");
        return; // keep the review intact so the user can retry
      }
      showTransientError("Claude is gone — review copied to clipboard instead");
    }
    t.mcpRequestId = null;
    t.mcpInstructions = "";
  } else if (hasContent) {
    const copied = await copyText(text);
    if (!copied) {
      showTransientError("Couldn't copy the review to the clipboard");
      return; // keep the review intact so the user can retry
    }
    showTransientMessage("Review copied — paste into Claude Code");
  }
  t.reviews = [];
  t.orphanedReviews = [];
  t.generalNote = "";
  t.reviewMode = false;
  renderTabBar();
  renderReviewMarkers(t);
}
```

- [ ] **Step 7: closeTab declines**

In `closeTab`, right before `tabs.splice(idx, 1);`:

```js
  if (t.mcpRequestId != null) {
    // Closing the reviewed tab is an unambiguous "not now".
    invoke("mcp_review_result", { requestId: t.mcpRequestId, review: null }).catch(() => {});
  }
```

- [ ] **Step 8: CSS**

Append to `ui/styles.css` next to the existing `.review-bar` rules:

```css
.review-bar.mcp {
  border-left: 3px solid var(--accent, #0969da);
}

.review-mcp-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.review-decline-btn {
  flex: none;
  padding: 2px 10px;
  font-size: 12px;
  border: 1px solid #d0d7de;
  border-radius: 6px;
  background: transparent;
  color: inherit;
  cursor: pointer;
}

.review-decline-btn:hover {
  background: rgba(175, 184, 193, 0.2);
}

[data-theme="dark"] .review-decline-btn {
  border-color: #444c56;
}
```

(`var(--accent, #0969da)` matches the existing accent usages in `styles.css`,
e.g. the reveal-in-tree selected-row bar at line ~182.)

- [ ] **Step 9: Build and run all tests**

Run: `node --test ui/*.test.js` — all pass.
Run: `cd src-tauri && cargo build` (frontend changes need a rebuild — `frontendDist` is bundled at compile time).

- [ ] **Step 10: Commit**

```bash
git add ui/app.js ui/styles.css
git commit -m "Wire MCP events into the frontend: open, review request, decline, send"
```

---

### Task 12: installer — command, menu item, frontend handler

**Files:**
- Modify: `src-tauri/src/commands.rs` (add `install_mcp_server`)
- Modify: `src-tauri/src/lib.rs` (register it)
- Modify: `src-tauri/src/menu.rs` (menu item + event)
- Modify: `ui/app.js` (listener + `installMcpServer`)

- [ ] **Step 1: The command**

Append to `src-tauri/src/commands.rs`, after `install_claude_hook`:

```rust
/// Merge the MDViewer MCP server into the open project's `.mcp.json` so
/// Claude Code sessions in that project can drive the viewer. Idempotent,
/// mirrors install_claude_hook.
#[tauri::command]
pub fn install_mcp_server(
    state: State<'_, AppState>,
) -> Result<crate::claude_hook::HookOutcome, String> {
    let root = current_root(&state)?;
    let exe =
        std::env::current_exe().map_err(|e| format!("cannot resolve app binary path: {e}"))?;
    let config_path = root.join(".mcp.json");

    let existing: serde_json::Value = match std::fs::read_to_string(&config_path) {
        Ok(s) if !s.trim().is_empty() => serde_json::from_str(&s).map_err(|e| {
            format!(
                "{} is not valid JSON; not modified ({e})",
                config_path.display()
            )
        })?,
        _ => serde_json::json!({}),
    };

    let (merged, outcome) = crate::mcp::merge_mcp_config(existing, &exe.to_string_lossy())?;
    let bytes = serde_json::to_vec_pretty(&merged)
        .map_err(|e| format!("cannot serialize config: {e}"))?;
    write_atomically(&config_path, &bytes)
        .map_err(|e| format!("cannot write {}: {e}", config_path.display()))?;
    Ok(outcome)
}
```

Register in `lib.rs` after `commands::install_claude_hook,`:

```rust
            commands::install_mcp_server,
```

- [ ] **Step 2: Menu item**

In `src-tauri/src/menu.rs` `rebuild()`, after the `install_claude_hook` item builder:

```rust
    let install_mcp_server =
        MenuItemBuilder::with_id("install-mcp-server", "Install MCP Server…").build(app)?;
```

and after `.item(&install_claude_hook)`:

```rust
    let app_menu_builder = app_menu_builder.item(&install_mcp_server);
```

In the `on_menu_event` match, after the `"install-claude-hook"` arm:

```rust
            "install-mcp-server" => {
                let _ = app.emit("menu-install-mcp-server", ());
            }
```

- [ ] **Step 3: Frontend handler**

In `ui/app.js` `init()`, after the `menu-install-claude-hook` listener:

```js
  await listen("menu-install-mcp-server", async () => {
    await installMcpServer();
  });
```

And next to `installClaudeHook`:

```js
async function installMcpServer() {
  if (!treeRoot) {
    await dialogApi.message("Open a folder first to install the MCP server there.", {
      title: "MDViewer",
      kind: "info",
    });
    return;
  }
  let outcome;
  try {
    outcome = await invoke("install_mcp_server");
  } catch (e) {
    await dialogApi.message("Couldn't install the MCP server.\n\n" + e, {
      title: "MDViewer",
      kind: "error",
    });
    return;
  }
  const msg =
    outcome === "updated"
      ? "Already installed — updated the MDViewer path in .mcp.json."
      : "Installed. Claude Code sessions in this project can now open documents in MDViewer and request reviews (approve the 'mdviewer' MCP server when Claude Code asks).";
  await dialogApi.message(msg, { title: "MDViewer", kind: "info" });
}
```

- [ ] **Step 4: Build, test, lint, commit**

Run: `cd src-tauri && cargo build && cargo test && cargo fmt && cargo clippy --all-targets -- -D warnings && node --test ../ui/*.test.js`
Expected: clean.

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src-tauri/src/menu.rs ui/app.js
git commit -m "Add MDViewer ▸ Install MCP Server… (.mcp.json merge)"
```

---

### Task 13: documentation

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`

- [ ] **Step 1: README**

Add to the Features list:

```markdown
- **MCP server for Claude Code** — install with **MDViewer ▸ Install MCP
  Server…**; Claude can then open documents in MDViewer (`open_document`),
  check what you're reading (`get_viewer_state`), and request a review
  (`request_review`): the document opens in Review Mode with a banner, and
  **✓ Finish & Send** delivers your comments straight back to the waiting
  Claude session — no clipboard step. **Decline** (or closing the tab) tells
  Claude you're skipping it. Reviews can take as long as you need; if a
  review runs into a client-side tool timeout, raise `MCP_TOOL_TIMEOUT` in
  the Claude Code environment.
```

Add the menu entry to the Menus section (`MDViewer ▸ Install MCP Server…`),
mirroring the Install Claude Code Hook entry.

- [ ] **Step 2: CLAUDE.md**

In the file-layout block, after the `claude_hook.rs` line:

```
    mcp.rs        — MCP server: pure JSON-RPC dispatch/tool defs/validation/
                    .mcp.json merge (unit-tested) + run_proxy runtime for
                    `--mcp` (stdio ↔ local socket relay, launches GUI)
    mcp_server.rs — GUI-side socket listener; McpPending ack-confirmed map
                    routes tool calls webview-ward and replies socket-ward
```

And in `ui/`:

```
  mcp.js          — pure helpers: reviewButtonLabel, mcpHintText, reviewBusy,
                    viewerState for the MCP review loop (unit-tested)
```

Add an architecture bullet after the **Claude Code hook install** bullet (keep it tight, ~15 lines):

```markdown
- **MCP server**: **MDViewer ▸ Install MCP Server…** merges
  `{"mcpServers":{"mdviewer":{"command":<exe>,"args":["--mcp"]}}}` into
  `<root>/.mcp.json` (`mcp::merge_mcp_config`, idempotent like `merge_hook`).
  Claude Code spawns `mdviewer --mcp` (checked in `main.rs` next to
  `--claude-hook`): a hand-rolled stdio JSON-RPC proxy (`mcp.rs`, no SDK) that
  relays `tools/call` over a local socket (`interprocess` crate: named pipe on
  Windows, `$TMPDIR/mdviewer-mcp.sock` on macOS; `MDVIEWER_MCP_SOCKET`
  overrides for tests) to the GUI's listener thread (`mcp_server.rs`),
  launching the GUI via `claude_hook::launch_mdviewer(None)` if the connect
  fails. Tools: `open_document(path, line?)` → `openTabAtLine`/`openSticky`;
  `get_viewer_state`; blocking `request_review(path, instructions?)` → tab
  opens in Review Mode with an MCP banner (instructions + Decline), toolbar
  reads **✓ Finish & Send**, and `finishReview` routes the `formatReview`
  markdown to `mcp_review_result` instead of the clipboard (proxy dead →
  clipboard fallback toast; decline/tab-close → `{"declined": true}`, a
  success so Claude proceeds). The proxy emits `notifications/progress` every
  10 s to hold client timeouts open. Validation is GUI-side
  (`mcp_server::validate`): extension allowlist (markdown+images; markdown
  only for reviews) + existence; the proxy absolutizes relative paths against
  its cwd (= Claude's project root). One review at a time (`reviewBusy`);
  concurrent requests get an error reply. Stale socket on startup: probe,
  back off if a live instance answers, else unlink and bind.
```

- [ ] **Step 3: Commit**

```bash
git add README.md CLAUDE.md
git commit -m "Document the MCP server (README + CLAUDE.md)"
```

---

### Task 14: manual end-to-end smoke test (GUI — do not skip)

Per the standing project rule: automated/subagent work misses visual and
theme bugs; run the real app before merging.

- [ ] **Step 1: Build and install the dev binary's MCP entry**

```bash
cd src-tauri && cargo build
mkdir -p /tmp/mcp-smoke && printf '# Plan\n\nFirst point.\n\nSecond point.\n' > /tmp/mcp-smoke/plan.md
cd /tmp/mcp-smoke && /Users/laek/source/mdviewer/src-tauri/target/debug/mdviewer .
```

In the app: **MDViewer ▸ Install MCP Server…** → expect the "Installed." dialog
→ verify `/tmp/mcp-smoke/.mcp.json` contains the dev binary path and
`["--mcp"]`.

- [ ] **Step 2: Drive the loop from a real Claude Code session**

```bash
cd /tmp/mcp-smoke && claude
```

Approve the `mdviewer` MCP server when prompted, then ask Claude:
*"Use the mdviewer request_review tool on plan.md with instructions 'check the second point'."*

Verify, in both light and dark theme:
- the app window focuses; `plan.md` opens as a **sticky** tab already in Review Mode;
- the review bar shows the 💬 waiting hint **with the instructions** and a **Decline** button, accent border visible, dark-mode styling sane;
- toolbar reads **✓ Finish & Send**;
- add a comment on a block + a general note → **✓ Finish & Send** → toast "Review sent to Claude Code" → Claude's session receives the structured review text;
- run it again and click **Decline** → Claude receives `{"declined": true}` and continues gracefully;
- run it again, quit the Claude session mid-review, then hit **✓ Finish & Send** → toast "Claude is gone — review copied to clipboard instead" and the clipboard holds the review;
- `open_document` with a line number scrolls and pulses the target block;
- `get_viewer_state` reports the active path;
- with the app **not running**: asking Claude to `open_document` launches the GUI and opens the file.

- [ ] **Step 3: Fix anything found, re-run, commit fixes**

---

## Final verification (after all tasks)

```bash
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
node --test ui/*.test.js
```

All green + smoke test passed → use superpowers:finishing-a-development-branch.
