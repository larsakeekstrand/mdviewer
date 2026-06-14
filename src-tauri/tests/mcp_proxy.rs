//! End-to-end test of `mdviewer --mcp`: a fake GUI binds the (overridden)
//! socket, the proxy is spawned as a real child process, and JSON-RPC flows
//! through stdin/stdout.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::ListenerOptions;
use serde_json::{json, Value};

// ONE test function on purpose: socket_name() reads MDVIEWER_MCP_SOCKET from
// the process environment, and `cargo test` runs test fns in parallel threads
// — two tests setting the var would race.
#[test]
fn proxy_round_trips_and_reports_eof_mid_request() {
    let sock = test_socket_id();
    std::env::set_var("MDVIEWER_MCP_SOCKET", &sock);

    // Remove any stale socket file left by a previous run (PID reuse in $TMPDIR
    // can make ListenerOptions::create_sync fail with "address already in use").
    #[cfg(not(windows))]
    let _ = std::fs::remove_file(&sock);

    let listener = ListenerOptions::new()
        .name(mdviewer_lib::mcp::socket_name().unwrap())
        .create_sync()
        .unwrap();

    // Fake GUI: answers open_document calls, drops the connection without
    // replying on request_review (the EOF-mid-request scenario). Detached —
    // it blocks on accept at the end and dies with the test process.
    std::thread::spawn(move || {
        loop {
            let Ok(conn) = listener.accept() else {
                continue;
            };
            let mut reader = BufReader::new(&conn);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let req: mdviewer_lib::mcp::GuiRequest = serde_json::from_str(line.trim()).unwrap();
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

    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"2025-06-18"}}),
    );
    let init = recv(&mut stdout);
    assert_eq!(init["result"]["serverInfo"]["name"], "mdviewer");

    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );

    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    );
    let list = recv(&mut stdout);
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 4);

    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"open_document","arguments":{"path":"plan.md"}}}),
    );
    let call = recv(&mut stdout);
    assert_eq!(call["id"], 3);
    assert_eq!(call["result"]["content"][0]["text"], "Opened plan.md");
    assert!(call["result"].get("isError").is_none());

    // EOF mid-request: the fake GUI hangs up instead of replying. The proxy
    // must answer with an error tool-result, not hang or crash.
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
            "params":{"name":"request_review","arguments":{"path":"plan.md"}}}),
    );
    let eof = recv(&mut stdout);
    assert_eq!(eof["id"], 4);
    let text = eof["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| eof["error"]["message"].as_str().unwrap());
    assert!(
        text.contains("closed") || text.contains("connection"),
        "got: {eof}"
    );

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
        std::env::temp_dir()
            .join(base)
            .to_string_lossy()
            .into_owned()
    }
}
