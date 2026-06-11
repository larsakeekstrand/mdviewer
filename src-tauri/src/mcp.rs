//! MCP server: hand-rolled JSON-RPC 2.0 over stdio (the `--mcp` proxy) plus the
//! pure protocol helpers shared with the GUI-side socket listener
//! (`mcp_server.rs`). Pure helpers are unit-tested; `run_proxy` is IO and
//! covered by `tests/mcp_proxy.rs`.

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
}
