//! GUI-side MCP socket listener: accepts connections from `mdviewer --mcp`
//! proxies, forwards tool calls to the webview as Tauri events, and routes
//! replies back through `McpPending`. The pending map is unit-tested; the
//! listener/connection runtime is IO, covered by the manual smoke test.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::Duration;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{ListenerOptions, Stream};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::mcp::{self, GuiReply, GuiRequest};
use crate::routing::{route, Route};

/// The webview's answer for one tool call: Ok(text) or Err(message).
pub type Reply = Result<String, String>;
/// Reply plus an ack channel the connection thread reports its write on.
type Handoff = (Reply, mpsc::Sender<std::io::Result<()>>);

/// How long `resolve` waits for the connection thread's socket-write ack
/// before reporting "did not acknowledge". Shortened under test so the
/// rejection-path tests (which never spawn an ack thread) stay fast.
#[cfg(not(test))]
const ACK_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const ACK_TIMEOUT: Duration = Duration::from_millis(200);

struct Entry {
    label: String,
    tool: String,
    tx: mpsc::Sender<Handoff>,
}

/// In-flight MCP requests, keyed by a GUI-generated id (NOT the proxy's
/// JSON-RPC id, which can collide across connections). Managed Tauri state.
#[derive(Default)]
pub struct McpPending {
    next_id: AtomicU64,
    waiting: Mutex<HashMap<u64, Entry>>,
}

// Used by the socket listener (Task 6) and commands (Task 7).
impl McpPending {
    /// Register a new in-flight request, tagged with the window it was sent
    /// to and the tool it's for (the latter drives `abandon_window`'s
    /// review-vs-error split). The connection thread blocks on the returned
    /// receiver until a command resolves it.
    pub fn register(&self, label: &str, tool: &str) -> (u64, mpsc::Receiver<Handoff>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = mpsc::channel();
        self.waiting.lock().unwrap().insert(
            id,
            Entry {
                label: label.to_string(),
                tool: tool.to_string(),
                tx,
            },
        );
        (id, rx)
    }

    /// Deliver a reply and wait for the connection thread's socket-write ack,
    /// so callers (e.g. mcp_review_result) know synchronously whether the
    /// proxy received it — the trigger for the frontend's clipboard fallback.
    /// Errors on an unknown id (already resolved, or fabricated by the
    /// webview), on an id that belongs to a different window than `caller`
    /// (leaving the entry in place for its rightful window), and on a dead
    /// or failing connection.
    pub fn resolve(&self, id: u64, caller: &str, reply: Reply) -> Result<(), String> {
        let tx = {
            let mut waiting = self.waiting.lock().unwrap();
            match waiting.get(&id) {
                None => return Err(format!("unknown MCP request id {id}")),
                Some(e) if e.label != caller => {
                    return Err(format!("MCP request {id} belongs to another window"))
                }
                Some(_) => waiting.remove(&id).unwrap().tx,
            }
        };
        let (ack_tx, ack_rx) = mpsc::channel();
        tx.send((reply, ack_tx))
            .map_err(|_| "the MCP connection is gone".to_string())?;
        match ack_rx.recv_timeout(ACK_TIMEOUT) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(format!("socket write failed: {e}")),
            Err(_) => Err("the MCP connection did not acknowledge".to_string()),
        }
    }

    /// Drop a request without replying (failed emit, app teardown).
    pub fn forget(&self, id: u64) {
        self.waiting.lock().unwrap().remove(&id);
    }

    /// A window closed: answer everything it owed instead of leaving the
    /// proxy blocked forever. A pending review counts as declined (a
    /// success, so Claude proceeds gracefully — same outcome as the user
    /// closing the review tab); every other tool gets an error.
    pub fn abandon_window(&self, label: &str) {
        let gone: Vec<Entry> = {
            let mut waiting = self.waiting.lock().unwrap();
            let ids: Vec<u64> = waiting
                .iter()
                .filter(|(_, e)| e.label == label)
                .map(|(id, _)| *id)
                .collect();
            ids.into_iter()
                .filter_map(|id| waiting.remove(&id))
                .collect()
        };
        for e in gone {
            let reply = if e.tool == "request_review" {
                Ok(crate::mcp::review_reply_text(None))
            } else {
                Err("MDViewer window closed".to_string())
            };
            let (ack_tx, _ack_rx) = mpsc::channel();
            let _ = e.tx.send((reply, ack_tx));
        }
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

/// Which window a tool call goes to.
#[derive(Debug, PartialEq)]
pub enum Target {
    Window(String),
    NewWindow(PathBuf),
    NoWindow,
    /// Refused outright (never opens or retargets a window).
    Refuse(String),
}

pub const PDF_OUTSIDE_WORKSPACE: &str =
    "source is outside every open workspace; open its folder in MDViewer first";

/// Pure routing for one tool call. Path tools go to the window whose root
/// contains the path (or a new window at the path's parent — the IO caller
/// substitutes the real git-aware root). `get_viewer_state` goes to the window
/// containing the proxy's cwd, else the most recently focused window; it never
/// opens a window.
pub fn pick_target(req: &GuiRequest, roots: &[(String, PathBuf)], mru: &[String]) -> Target {
    if req.tool == "get_viewer_state" {
        return viewer_target(req, roots, roots, mru);
    }
    path_target(req, roots, mru)
}

/// `pick_target` with a placeholder "main" (bare-cwd root, often "/") treated
/// as containing nothing: it never claims a path or a cwd, but is still a
/// fallback for `get_viewer_state`.
pub fn pick_target_for(
    req: &GuiRequest,
    roots: &[(String, PathBuf)],
    mru: &[String],
    placeholder_main: bool,
) -> Target {
    let routable: Vec<(String, PathBuf)> = roots
        .iter()
        .filter(|(l, _)| !(placeholder_main && l == "main"))
        .cloned()
        .collect();
    if req.tool == "get_viewer_state" {
        return viewer_target(req, &routable, roots, mru);
    }
    pick_target(req, &routable, mru)
}

fn parent_of(p: &Path) -> PathBuf {
    p.parent().map(Path::to_path_buf).unwrap_or_default()
}

fn viewer_target(
    req: &GuiRequest,
    cwd_roots: &[(String, PathBuf)],
    all_roots: &[(String, PathBuf)],
    mru: &[String],
) -> Target {
    let by_cwd =
        req.cwd
            .as_deref()
            .and_then(|c| match route(Path::new(c), cwd_roots, mru, parent_of) {
                Route::Window(l) => Some(l),
                Route::NewWindow(_) => None,
            });
    let fallback = || {
        mru.iter()
            .find(|l| all_roots.iter().any(|(r, _)| r == *l))
            .cloned()
            .or_else(|| all_roots.iter().map(|(l, _)| l).min().cloned())
    };
    by_cwd
        .or_else(fallback)
        .map(Target::Window)
        .unwrap_or(Target::NoWindow)
}

fn path_target(req: &GuiRequest, roots: &[(String, PathBuf)], mru: &[String]) -> Target {
    let Some(p) = req.args.get("path").and_then(Value::as_str) else {
        return Target::NoWindow;
    };
    match route(Path::new(p), roots, mru, parent_of) {
        Route::Window(l) => Target::Window(l),
        // generate_pdf writes a file, so its containment root must be one the
        // user opened; a root derived from the caller's own path is no boundary.
        Route::NewWindow(_) if req.tool == "generate_pdf" => {
            Target::Refuse(PDF_OUTSIDE_WORKSPACE.to_string())
        }
        Route::NewWindow(r) => Target::NewWindow(r),
    }
}

/// A copy of `req` with `args.path` canonicalized when it resolves, so routing
/// compares it against the registry's canonical roots. An unresolvable path is
/// left as-is for `validate` to report.
fn canonicalize_path_arg(req: &GuiRequest) -> GuiRequest {
    let mut args = req.args.clone();
    if let Some(c) = args
        .get("path")
        .and_then(Value::as_str)
        .and_then(|p| std::fs::canonicalize(p).ok())
    {
        args["path"] = Value::String(c.to_string_lossy().into_owned());
    }
    GuiRequest {
        id: req.id,
        tool: req.tool.clone(),
        args,
        cwd: req.cwd.clone(),
    }
}

/// Validate and forward one tool call to the window it routes to. The
/// returned receiver resolves when an mcp_respond / mcp_review_result command
/// answers it. Locks are taken one at a time (windows, then focus) and never
/// held across window creation, emits, or focusing.
fn prepare_request(
    app: &AppHandle,
    req: &GuiRequest,
) -> Result<std::sync::mpsc::Receiver<Handoff>, String> {
    let event = mcp::event_name(&req.tool).ok_or_else(|| format!("unknown tool '{}'", req.tool))?;
    let state = app.state::<crate::AppState>();
    let req = &canonicalize_path_arg(req);
    let roots = state.windows.lock().unwrap().roots();
    let mru = state.focus.lock().unwrap().as_slice().to_vec();
    let placeholder =
        state.main_placeholder.load(Ordering::SeqCst) && roots.iter().any(|(l, _)| l == "main");
    let label = match pick_target_for(req, &roots, &mru, placeholder) {
        Target::Window(l) => l,
        Target::Refuse(e) => return Err(e),
        Target::NoWindow => {
            // A path tool lands here only without a usable path: report that
            // instead of "starting", which the proxy would retry for 15 s.
            if req.tool != "get_viewer_state" {
                validate(req, None)?;
            }
            return Err(mcp::STARTING_ERR.to_string());
        }
        Target::NewWindow(_) => {
            // Validate against the would-be root BEFORE touching any window,
            // so a bad path never opens one. generate_pdf never reaches this
            // arm (path_target refuses it earlier as PDF_OUTSIDE_WORKSPACE) —
            // this root only ever becomes an open_document/request_review
            // window's containment boundary.
            let p = req.args["path"].as_str().unwrap_or("");
            let root = crate::routing::fallback_root(Path::new(p));
            validate(req, Some(&root))?;
            if placeholder {
                if !crate::windows::retarget_placeholder_main(app, root) {
                    return Err(mcp::STARTING_ERR.to_string());
                }
                "main".to_string()
            } else {
                crate::windows::create_project_window(app, root, None)?;
                // The new window isn't ready yet; the proxy retries on
                // STARTING_ERR and the retry routes into it (its root now
                // contains the path), so no second window is created.
                return Err(mcp::STARTING_ERR.to_string());
            }
        }
    };
    let (ready, root) = {
        let reg = state.windows.lock().unwrap();
        match reg.get(&label) {
            Some(w) => (w.ready, Some(w.root.clone())),
            None => (false, None),
        }
    };
    if !ready {
        return Err(mcp::STARTING_ERR.to_string());
    }
    validate(req, root.as_deref())?;

    let pending = app.state::<McpPending>();
    let (gui_id, rx) = pending.register(&label, &req.tool);
    let mut payload = mcp::event_payload(gui_id, req);
    if req.tool == "generate_pdf" {
        // Send the resolved output path the GUI side validated, so the frontend
        // never recomputes the default and can't drift from the boundary check.
        let source = req.args.get("path").and_then(Value::as_str).unwrap_or("");
        let out = req.args.get("output").and_then(Value::as_str);
        payload["output"] = serde_json::json!(mcp::pdf_output_path(source, out));
    }
    if app.emit_to(label.as_str(), event, payload).is_err() {
        pending.forget(gui_id);
        return Err("cannot reach the MDViewer window".to_string());
    }
    if req.tool == "request_review" {
        if let Some(w) = app.get_webview_window(&label) {
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
                return Err(format!(
                    "'{p}' is not a markdown, image, PDF, or spreadsheet file"
                ));
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

    fn req(tool: &str, path: Option<&str>, cwd: Option<&str>) -> GuiRequest {
        GuiRequest {
            id: 1,
            tool: tool.into(),
            args: match path {
                Some(p) => serde_json::json!({"path": p}),
                None => serde_json::json!({}),
            },
            cwd: cwd.map(str::to_string),
        }
    }

    fn r2() -> Vec<(String, std::path::PathBuf)> {
        vec![
            ("main".into(), "/a".into()),
            ("project-1".into(), "/b".into()),
        ]
    }

    #[test]
    fn path_tools_route_by_path() {
        let t = pick_target(
            &req("open_document", Some("/b/x.md"), Some("/a")),
            &r2(),
            &["main".into()],
        );
        assert_eq!(t, Target::Window("project-1".into()));
    }

    #[test]
    fn path_tool_outside_every_root_opens_new_window() {
        let t = pick_target(&req("request_review", Some("/c/p.md"), None), &r2(), &[]);
        assert!(matches!(t, Target::NewWindow(_)));
    }

    #[test]
    fn viewer_state_routes_by_cwd_then_mru_never_new_window() {
        assert_eq!(
            pick_target(
                &req("get_viewer_state", None, Some("/b/sub")),
                &r2(),
                &["main".into()]
            ),
            Target::Window("project-1".into())
        );
        assert_eq!(
            pick_target(
                &req("get_viewer_state", None, Some("/elsewhere")),
                &r2(),
                &["main".into()]
            ),
            Target::Window("main".into())
        );
        assert_eq!(
            pick_target(&req("get_viewer_state", None, None), &[], &[]),
            Target::NoWindow
        );
    }

    #[test]
    fn path_tool_without_path_is_no_window() {
        assert_eq!(
            pick_target(&req("open_document", None, Some("/b")), &r2(), &[]),
            Target::NoWindow
        );
    }

    #[test]
    fn placeholder_main_does_not_contain_every_path() {
        let roots = vec![("main".to_string(), std::path::PathBuf::from("/"))];
        let mru = ["main".to_string()];
        assert_eq!(
            pick_target_for(
                &req("open_document", Some("/z/y/x.md"), None),
                &roots,
                &mru,
                true
            ),
            Target::NewWindow("/z/y".into())
        );
        assert_eq!(
            pick_target_for(
                &req("open_document", Some("/z/y/x.md"), None),
                &roots,
                &mru,
                false
            ),
            Target::Window("main".into())
        );
    }

    #[test]
    fn placeholder_main_defers_to_a_containing_window() {
        let roots = vec![
            ("main".to_string(), std::path::PathBuf::from("/")),
            ("project-1".to_string(), std::path::PathBuf::from("/z")),
        ];
        let mru = ["main".to_string()];
        assert_eq!(
            pick_target_for(
                &req("open_document", Some("/z/y/x.md"), None),
                &roots,
                &mru,
                true
            ),
            Target::Window("project-1".into())
        );
    }

    #[test]
    fn viewer_state_with_placeholder_main_uses_mru_not_the_slash_root() {
        let roots = vec![
            ("main".to_string(), std::path::PathBuf::from("/")),
            ("project-1".to_string(), std::path::PathBuf::from("/b")),
        ];
        let mru = ["project-1".to_string(), "main".to_string()];
        assert_eq!(
            pick_target_for(
                &req("get_viewer_state", None, Some("/elsewhere")),
                &roots,
                &mru,
                true
            ),
            Target::Window("project-1".into())
        );
        let only_main = vec![("main".to_string(), std::path::PathBuf::from("/"))];
        assert_eq!(
            pick_target_for(
                &req("get_viewer_state", None, Some("/elsewhere")),
                &only_main,
                &[],
                true
            ),
            Target::Window("main".into())
        );
    }

    fn pdf_req(path: &str) -> GuiRequest {
        GuiRequest {
            id: 1,
            tool: "generate_pdf".into(),
            args: serde_json::json!({"path": path, "output": "/x/out.pdf"}),
            cwd: None,
        }
    }

    #[test]
    fn generate_pdf_outside_every_root_is_refused() {
        assert_eq!(
            pick_target_for(&pdf_req("/c/p.md"), &r2(), &[], false),
            Target::Refuse(PDF_OUTSIDE_WORKSPACE.to_string())
        );
        assert_eq!(
            pick_target(&pdf_req("/c/p.md"), &r2(), &[]),
            Target::Refuse(PDF_OUTSIDE_WORKSPACE.to_string())
        );
    }

    #[test]
    fn generate_pdf_never_retargets_a_placeholder_main() {
        let roots = vec![
            ("main".to_string(), std::path::PathBuf::from("/")),
            ("project-1".to_string(), std::path::PathBuf::from("/b")),
        ];
        assert_eq!(
            pick_target_for(&pdf_req("/z/y/x.md"), &roots, &["main".into()], true),
            Target::Refuse(PDF_OUTSIDE_WORKSPACE.to_string())
        );
    }

    #[test]
    fn generate_pdf_inside_an_open_root_routes_to_that_window() {
        assert_eq!(
            pick_target_for(&pdf_req("/b/x.md"), &r2(), &[], false),
            Target::Window("project-1".into())
        );
    }

    #[test]
    fn open_document_outside_every_root_still_opens_a_window() {
        assert_eq!(
            pick_target_for(
                &req("open_document", Some("/c/p.md"), None),
                &r2(),
                &[],
                false
            ),
            Target::NewWindow("/c".into())
        );
    }

    #[test]
    fn register_assigns_unique_ids() {
        let p = McpPending::default();
        let (a, _rx_a) = p.register("main", "open_document");
        let (b, _rx_b) = p.register("main", "open_document");
        assert_ne!(a, b);
    }

    #[test]
    fn resolve_round_trips_through_connection_thread() {
        let p = std::sync::Arc::new(McpPending::default());
        let (id, rx) = p.register("main", "open_document");
        // Fake connection thread: receive the reply, "write" it, ack success.
        let t = std::thread::spawn(move || {
            let (reply, ack) = rx.recv().unwrap();
            assert_eq!(reply, Ok("hello".to_string()));
            ack.send(Ok(())).unwrap();
        });
        assert!(p.resolve(id, "main", Ok("hello".to_string())).is_ok());
        t.join().unwrap();
    }

    #[test]
    fn resolve_unknown_id_errors_without_blocking() {
        let p = McpPending::default();
        assert!(p.resolve(999, "main", Ok("x".to_string())).is_err());
    }

    #[test]
    fn resolve_reports_a_dead_connection() {
        let p = McpPending::default();
        let (id, rx) = p.register("main", "open_document");
        drop(rx); // connection thread is gone (proxy died)
        assert!(p.resolve(id, "main", Ok("x".to_string())).is_err());
    }

    #[test]
    fn resolve_surfaces_write_failure() {
        let p = std::sync::Arc::new(McpPending::default());
        let (id, rx) = p.register("main", "open_document");
        let t = std::thread::spawn(move || {
            let (_reply, ack) = rx.recv().unwrap();
            ack.send(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "pipe closed",
            )))
            .unwrap();
        });
        let err = p.resolve(id, "main", Ok("x".to_string())).unwrap_err();
        assert!(err.contains("pipe closed"), "got: {err}");
        t.join().unwrap();
    }

    #[test]
    fn forget_removes_the_entry() {
        let p = McpPending::default();
        let (id, _rx) = p.register("main", "open_document");
        p.forget(id);
        assert!(p.resolve(id, "main", Ok("x".to_string())).is_err());
    }

    #[test]
    fn resolve_from_another_window_is_rejected_and_keeps_entry() {
        let p = McpPending::default();
        let (id, _rx) = p.register("main", "open_document");
        let err = p.resolve(id, "project-1", Ok("x".into())).unwrap_err();
        assert!(err.contains("another window"), "got: {err}");
        // Still resolvable by its own window (rx alive, but no ack thread → times out
        // as "did not acknowledge", which proves the entry was not removed).
        assert!(p
            .resolve(id, "main", Ok("x".into()))
            .unwrap_err()
            .contains("acknowledge"));
    }

    #[test]
    fn abandon_window_declines_reviews_and_errors_others() {
        let p = McpPending::default();
        let (_r, review_rx) = p.register("main", "request_review");
        let (_o, open_rx) = p.register("main", "open_document");
        let (_k, keep_rx) = p.register("project-1", "open_document");
        p.abandon_window("main");
        let (reply, _ack) = review_rx.recv().unwrap();
        assert_eq!(reply, Ok(crate::mcp::review_reply_text(None)));
        let (reply, _ack) = open_rx.recv().unwrap();
        assert_eq!(reply, Err("MDViewer window closed".to_string()));
        assert!(keep_rx.try_recv().is_err());
    }
}
