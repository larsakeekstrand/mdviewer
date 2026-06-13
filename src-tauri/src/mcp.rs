//! MCP server: hand-rolled JSON-RPC 2.0 over stdio (the `--mcp` proxy) plus the
//! pure protocol helpers shared with the GUI-side socket listener
//! (`mcp_server.rs`). Pure helpers are unit-tested; `run_proxy` is IO and
//! covered by `tests/mcp_proxy.rs`.

use serde::{Deserialize, Serialize};
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
    let Some(method) = msg.get("method").and_then(|m| m.as_str()) else {
        // A message with an id but no method is a JSON-RPC *response* —
        // responses are never themselves responded to.
        return Dispatch::Ignore;
    };
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
        (Some(id), other) => Dispatch::Reply(rpc_error(
            id,
            -32601,
            &format!("method '{other}' not supported"),
        )),
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
const IMAGE_EXTS: [&str; 9] = [
    "png", "jpg", "jpeg", "gif", "webp", "avif", "bmp", "ico", "svg",
];

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
/// from a hostile client never reaches the frontend. `gui_id` is the
/// GUI-global pending-map key, distinct from `req.id` (the per-connection
/// socket id), because two concurrent proxy connections can both use socket
/// id 1.
pub fn event_payload(gui_id: u64, req: &GuiRequest) -> Value {
    let mut p = json!({ "requestId": gui_id });
    for key in ["path", "line", "instructions"] {
        if let Some(v) = req.args.get(key) {
            p[key] = v.clone();
        }
    }
    p
}

/// The local-socket name both sides agree on. Named pipe on Windows; a
/// filesystem path in the per-user `$TMPDIR` on Unix (macOS UDS paths cap at
/// ~104 bytes, so never `app_data_dir`). `MDVIEWER_MCP_SOCKET` overrides for
/// tests and dev.
///
/// Windows: the default pipe name embeds the username to avoid collisions
/// across local users. Pipe squatting by another user on the same machine
/// remains theoretically possible (documented trade-off; mitigated by the
/// per-user component).
pub fn socket_name() -> std::io::Result<interprocess::local_socket::Name<'static>> {
    #[cfg(windows)]
    {
        use interprocess::local_socket::{GenericNamespaced, ToNsName};
        let name = std::env::var("MDVIEWER_MCP_SOCKET").unwrap_or_else(|_| {
            let user = std::env::var("USERNAME")
                .or_else(|_| std::env::var("USER"))
                .unwrap_or_else(|_| "user".to_string());
            format!("mdviewer-mcp-{user}.sock")
        });
        name.to_ns_name::<GenericNamespaced>()
    }
    #[cfg(not(windows))]
    {
        use interprocess::local_socket::{GenericFilePath, ToFsName};
        socket_fs_path().unwrap().to_fs_name::<GenericFilePath>()
    }
}

/// The socket's filesystem path, when it has one (Unix). Used to probe for a
/// live server before binding. `None` on Windows (named pipes have no path).
pub fn socket_fs_path() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        None
    }
    #[cfg(not(windows))]
    {
        Some(
            std::env::var("MDVIEWER_MCP_SOCKET")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::env::temp_dir().join("mdviewer-mcp.sock")),
        )
    }
}

/// True if `config` already declares our `mdviewer` MCP server. Tolerates
/// missing/wrong-typed fields by returning false.
#[allow(dead_code)]
pub fn mcp_installed(config: &Value) -> bool {
    config
        .get("mcpServers")
        .and_then(|m| m.get("mdviewer"))
        .map(|v| !v.is_null())
        .unwrap_or(false)
}

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

use std::io::{BufRead, BufReader, Write};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use interprocess::local_socket::prelude::*;
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
            Dispatch::Reply(v) => {
                if write_json_line(&stdout, &v).is_err() {
                    break;
                }
            }
            Dispatch::Call {
                rpc_id,
                tool,
                args,
                progress_token,
            } => {
                let response = match forward_call(
                    &mut conn,
                    &stdout,
                    &mut next_gui_id,
                    &tool,
                    args,
                    &progress_token,
                ) {
                    Ok(reply) => match (reply.result, reply.error) {
                        (Some(text), _) => tool_text_result(rpc_id, &text, false),
                        (None, Some(e)) => tool_text_result(rpc_id, &e, true),
                        (None, None) => tool_text_result(rpc_id, "empty reply", true),
                    },
                    Err(e) => rpc_error(rpc_id, -32000, &e),
                };
                if write_json_line(&stdout, &response).is_err() {
                    break;
                }
            }
        }
    }
}

fn write_json_line(stdout: &Arc<Mutex<std::io::Stdout>>, v: &Value) -> std::io::Result<()> {
    let mut out = stdout.lock().unwrap();
    writeln!(out, "{v}")?;
    out.flush()
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
        if reply.id != req.id {
            return Err(format!(
                "mdviewer answered request {} while {} was pending",
                reply.id, req.id
            ));
        }
        if reply.error.as_deref() == Some(STARTING_ERR) {
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }
        return Ok(reply);
    }
    Err("mdviewer did not finish starting".to_string())
}

/// One reply per request is the protocol invariant, so a throwaway BufReader
/// can't buffer-steal bytes that belong to a later reply. A request that died
/// at read time is deliberately NOT resent — it may have already executed
/// GUI-side (e.g. opened a document), and re-sending risks double execution;
/// the caller surfaces the error instead.
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
    if let Ok(s) = Stream::connect(name.borrow()) {
        *conn = Some(s);
        return Ok(());
    }
    crate::claude_hook::launch_mdviewer(None);
    for _ in 0..LAUNCH_RETRIES {
        std::thread::sleep(Duration::from_millis(250));
        if let Ok(s) = Stream::connect(name.borrow()) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude_hook::HookOutcome;
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
                let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
                assert_eq!(
                    names,
                    vec!["open_document", "request_review", "get_viewer_state"]
                );
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
            Dispatch::Call {
                rpc_id,
                tool,
                args,
                progress_token,
            } => {
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

    #[test]
    fn response_objects_are_ignored() {
        assert_eq!(
            handle_message(r#"{"jsonrpc":"2.0","id":9,"result":{}}"#),
            Dispatch::Ignore
        );
        assert_eq!(
            handle_message(r#"{"jsonrpc":"2.0","id":9,"error":{"code":-1,"message":"x"}}"#),
            Dispatch::Ignore
        );
    }

    #[test]
    fn known_tools_matches_tool_defs() {
        let names: Vec<String> = tool_defs()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, KNOWN_TOOLS);
    }

    #[test]
    fn viewable_path_allowlists_markdown_and_images() {
        // Mirrors ui/app.js MD_EXT and ui/filetype.js IMAGE_EXT.
        for p in [
            "a.md",
            "B.MARKDOWN",
            "x.mdown",
            "x.mkd",
            "x.mkdn",
            "i.png",
            "i.jpg",
            "i.JPEG",
            "i.gif",
            "i.webp",
            "i.avif",
            "i.bmp",
            "i.ico",
            "i.svg",
        ] {
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
        let req = GuiRequest {
            id: 5,
            tool: "open_document".into(),
            args: json!({"path": "a.md"}),
        };
        let line = serde_json::to_string(&req).unwrap();
        let back: GuiRequest = serde_json::from_str(&line).unwrap();
        assert_eq!(back, req);

        let ok = GuiReply {
            id: 5,
            result: Some("done".into()),
            error: None,
        };
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
        assert_eq!(
            review_reply_text(Some("Review of plan.md\n…".into())),
            "Review of plan.md\n…"
        );
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

    #[test]
    fn mcp_installed_detects_our_server() {
        let cfg =
            json!({"mcpServers": {"mdviewer": {"command": "/x/mdviewer", "args": ["--mcp"]}}});
        assert!(mcp_installed(&cfg));
    }

    #[test]
    fn mcp_installed_false_for_absent_or_other() {
        assert!(!mcp_installed(&json!({})));
        assert!(!mcp_installed(&json!({"mcpServers": {}})));
        assert!(!mcp_installed(
            &json!({"mcpServers": {"other": {"command": "npx"}}})
        ));
    }

    #[test]
    fn mcp_installed_false_for_wrong_types() {
        assert!(!mcp_installed(&json!({"mcpServers": "oops"})));
        assert!(!mcp_installed(&json!([])));
    }
}
