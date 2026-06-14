//! GUI-side MCP socket listener: accepts connections from `mdviewer --mcp`
//! proxies, forwards tool calls to the webview as Tauri events, and routes
//! replies back through `McpPending`. The pending map is unit-tested; the
//! listener/connection runtime is IO, covered by the manual smoke test.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::Duration;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{ListenerOptions, Stream};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::mcp::{self, GuiReply, GuiRequest};

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

// Used by the socket listener (Task 6) and commands (Task 7).
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

/// Maximum bytes accepted for a single request line before dropping the connection.
const MAX_REQUEST_LINE: usize = 1024 * 1024;

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
    // file (crash) is handled by try_overwrite(true) on the bind below.
    if Stream::connect(name.borrow()).is_ok() {
        eprintln!("mdviewer: another instance owns the MCP socket; MCP disabled here");
        return Ok(());
    }
    let listener = ListenerOptions::new()
        .name(name)
        .try_overwrite(true)
        .create_sync()?;
    loop {
        match listener.accept() {
            Ok(stream) => {
                let app = app.clone();
                std::thread::spawn(move || handle_connection(app, stream));
            }
            Err(e) => {
                eprintln!("mdviewer: MCP accept error: {e}");
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}

/// One proxy connection. Strictly sequential — the proxy forwards one call
/// and waits for its reply before reading the next stdin line, so a
/// one-request-at-a-time loop is consistent end-to-end.
fn handle_connection(app: AppHandle, stream: Stream) {
    let mut reader = BufReader::new(&stream);
    loop {
        let mut line = String::new();
        // take(2*MAX) bounds buffering at ~2 MiB; the length check below
        // enforces the protocol limit without off-by-one gymnastics.
        match reader
            .by_ref()
            .take((MAX_REQUEST_LINE * 2) as u64)
            .read_line(&mut line)
        {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if line.len() > MAX_REQUEST_LINE {
            eprintln!("mdviewer: MCP request line too long; dropping connection");
            return;
        }
        let req: GuiRequest = match serde_json::from_str(line.trim()) {
            Ok(r) => r,
            Err(_) => {
                eprintln!("mdviewer: ignoring malformed MCP request line");
                continue;
            }
        };
        match prepare_request(&app, &req) {
            // Rejected before reaching the webview (validation, not ready).
            Err(e) => {
                let reply = GuiReply {
                    id: req.id,
                    result: None,
                    error: Some(e),
                };
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
                        Ok(text) => GuiReply {
                            id: req.id,
                            result: Some(text),
                            error: None,
                        },
                        Err(e) => GuiReply {
                            id: req.id,
                            result: None,
                            error: Some(e),
                        },
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
    let root = app
        .state::<crate::AppState>()
        .current_root
        .lock()
        .unwrap()
        .clone();
    validate(req, root.as_deref())?;
    let event = mcp::event_name(&req.tool).ok_or_else(|| format!("unknown tool '{}'", req.tool))?;

    let pending = app.state::<McpPending>();
    let (gui_id, rx) = pending.register();
    let mut payload = mcp::event_payload(gui_id, req);
    if req.tool == "generate_pdf" {
        // Send the resolved output path the GUI side validated, so the frontend
        // never recomputes the default and can't drift from the boundary check.
        let source = req.args.get("path").and_then(Value::as_str).unwrap_or("");
        let out = req.args.get("output").and_then(Value::as_str);
        payload["output"] = serde_json::json!(mcp::pdf_output_path(source, out));
    }
    if app.emit(event, payload).is_err() {
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

fn validate(req: &GuiRequest, root: Option<&std::path::Path>) -> Result<(), String> {
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
        "generate_pdf" => {
            let p = path()?;
            if !mcp::markdown_path(p) {
                return Err(format!("'{p}' is not a markdown file"));
            }
            if !std::path::Path::new(p).is_file() {
                return Err(format!("file not found: {p}"));
            }
            let out = mcp::pdf_output_path(p, req.args.get("output").and_then(Value::as_str));
            if !mcp::pdf_path(&out) {
                return Err(format!("output must be a .pdf file: {out}"));
            }
            // generate_pdf writes a file, so confine both source and output to
            // the open folder. No folder open → nothing to confine against, so
            // refuse rather than write somewhere arbitrary.
            let Some(root) = root else {
                return Err(
                    "no folder is open in MDViewer; generate_pdf needs a workspace".to_string(),
                );
            };
            if !crate::fs_ops::within_root(std::path::Path::new(p), root) {
                return Err(format!("source is outside the open workspace: {p}"));
            }
            if !crate::fs_ops::within_root(std::path::Path::new(&out), root) {
                return Err(format!("output is outside the open workspace: {out}"));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

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
