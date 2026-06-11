//! GUI-side MCP socket listener: accepts connections from `mdviewer --mcp`
//! proxies, forwards tool calls to the webview as Tauri events, and routes
//! replies back through `McpPending`. The pending map is unit-tested; the
//! listener/connection runtime is IO, covered by the manual smoke test.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::Duration;

/// The webview's answer for one tool call: Ok(text) or Err(message).
#[cfg_attr(not(test), allow(dead_code))]
pub type Reply = Result<String, String>;
/// Reply plus an ack channel the connection thread reports its write on.
#[cfg_attr(not(test), allow(dead_code))]
type Handoff = (Reply, mpsc::Sender<std::io::Result<()>>);

/// In-flight MCP requests, keyed by a GUI-generated id (NOT the proxy's
/// JSON-RPC id, which can collide across connections). Managed Tauri state.
#[derive(Default)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct McpPending {
    next_id: AtomicU64,
    waiting: Mutex<HashMap<u64, mpsc::Sender<Handoff>>>,
}

// Used by the socket listener (Task 6) and commands (Task 7).
#[cfg_attr(not(test), allow(dead_code))]
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
