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
}
