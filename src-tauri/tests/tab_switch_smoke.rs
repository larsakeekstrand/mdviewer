//! Tab-switching smoke test (macOS, `--ignored`). Drives a real GUI over the
//! MCP socket to prove that a revisited tab still paints real content, and that
//! a file edited while its tab was in the background is corrected by the
//! background revalidation. Run with:
//! `cargo test --test tab_switch_smoke -- --ignored --nocapture`
#![cfg(target_os = "macos")]

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::Stream;
use serde_json::{json, Value};

const READY_TIMEOUT: Duration = Duration::from_secs(40);
const CALL_TIMEOUT: Duration = Duration::from_secs(90);

struct Gui(Child);

impl Drop for Gui {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "launches the GUI; run manually via cargo test --test tab_switch_smoke -- --ignored"]
fn revisited_tab_paints_and_revalidates() {
    let bin = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/mdviewer");
    if !bin.exists() {
        println!("SKIP: {} not built (run `cargo build`)", bin.display());
        return;
    }

    let ws = std::env::temp_dir().join(format!("mdv-tabswitch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).unwrap();
    let a = ws.join("a.md");
    let b = ws.join("b.md");
    std::fs::write(
        &a,
        "# Alpha\n\nAlpha prose.\n\n```rust\nfn main() {}\n```\n",
    )
    .unwrap();
    std::fs::write(&b, "# Beta\n\nBeta prose.\n").unwrap();

    // AF_UNIX paths are capped near 104 bytes, so keep this short.
    let sock = format!("/tmp/mdv-tabswitch-{}.sock", std::process::id());
    let _ = std::fs::remove_file(&sock);
    std::env::set_var("MDVIEWER_MCP_SOCKET", &sock);

    let _gui = Gui(Command::new(&bin)
        .arg(&ws)
        .env("MDVIEWER_MCP_SOCKET", &sock)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("launch the debug binary"));

    wait_until_ready();

    call("open_document", json!({ "path": a.to_string_lossy() }));
    call("open_document", json!({ "path": b.to_string_lossy() }));
    call("open_document", json!({ "path": a.to_string_lossy() }));

    let first_pdf = ws.join("revisit.pdf");
    call(
        "generate_pdf",
        json!({ "path": a.to_string_lossy(), "output": first_pdf.to_string_lossy() }),
    );
    let first = std::fs::metadata(&first_pdf)
        .expect("first pdf written")
        .len();
    assert!(
        first > 3000,
        "a revisited tab produced a {first}-byte PDF — the reattached DOM is empty"
    );

    // Grow the file while its tab is active-but-retained, then revisit it. The
    // background revalidation must replace the retained render.
    let mut grown = std::fs::read_to_string(&a).unwrap();
    for _ in 0..30 {
        grown.push_str("\n\n## Appended\n\nMore prose to make the document longer.\n");
    }
    std::fs::write(&a, grown).unwrap();
    call("open_document", json!({ "path": b.to_string_lossy() }));
    call("open_document", json!({ "path": a.to_string_lossy() }));

    let second_pdf = ws.join("after-edit.pdf");
    call(
        "generate_pdf",
        json!({ "path": a.to_string_lossy(), "output": second_pdf.to_string_lossy() }),
    );
    let second = std::fs::metadata(&second_pdf)
        .expect("second pdf written")
        .len();
    assert!(
        second > first,
        "edited file still rendered at the old size ({first} -> {second}) — stale retained DOM"
    );

    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_file(&sock);
}

fn wait_until_ready() {
    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        if request("get_viewer_state", json!({})).is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(250));
    }
    panic!("the GUI never answered get_viewer_state within {READY_TIMEOUT:?}");
}

fn call(tool: &str, args: Value) -> String {
    let deadline = Instant::now() + CALL_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(result) = request(tool, args.clone()) {
            return result;
        }
        thread::sleep(Duration::from_millis(250));
    }
    panic!("{tool} never succeeded within {CALL_TIMEOUT:?}");
}

fn request(tool: &str, args: Value) -> Option<String> {
    let name = mdviewer_lib::mcp::socket_name().ok()?;
    let stream = Stream::connect(name.borrow()).ok()?;
    let req = mdviewer_lib::mcp::GuiRequest {
        id: 1,
        tool: tool.into(),
        args,
    };
    let mut line = serde_json::to_string(&req).ok()?;
    line.push('\n');
    (&stream).write_all(line.as_bytes()).ok()?;
    let mut reader = BufReader::new(&stream);
    let mut resp = String::new();
    reader.read_line(&mut resp).ok()?;
    let reply: mdviewer_lib::mcp::GuiReply = serde_json::from_str(resp.trim()).ok()?;
    reply.result
}
