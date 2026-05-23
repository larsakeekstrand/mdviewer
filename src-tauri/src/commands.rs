use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::{git, markdown, recent, tasklist, tree, AppState};

#[derive(Serialize)]
pub struct InitialState {
    pub tree_root: String,
    pub initial_file: Option<String>,
}

#[tauri::command]
pub fn get_initial_state(app: AppHandle, state: State<'_, AppState>) -> InitialState {
    let tree_root = match &state.tree_root {
        Some(p) => {
            recent::save_last(&app, p);
            p.clone()
        }
        // A restored folder is already stored as last_folder; only fall back to
        // cwd (unpersisted) when there's nothing valid to restore.
        None => recent::load_last(&app)
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))),
    };
    InitialState {
        tree_root: tree_root.to_string_lossy().into_owned(),
        initial_file: state
            .initial_file
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
    }
}

#[tauri::command]
pub fn list_dir(path: String) -> Result<Vec<tree::TreeEntry>, String> {
    let p = Path::new(&path);
    tree::list_directory(p)
}

#[tauri::command]
pub fn git_status(path: String) -> Result<git::GitStatusReport, String> {
    git::status(Path::new(&path))
}

#[derive(Serialize)]
pub struct RenderedFile {
    pub html: String,
    pub path: String,
    pub raw: bool,
}

#[tauri::command]
pub fn render_file(
    path: String,
    theme: Option<String>,
    raw: Option<bool>,
) -> Result<RenderedFile, String> {
    let p = PathBuf::from(&path);
    let contents =
        std::fs::read_to_string(&p).map_err(|e| format!("cannot read '{}': {}", p.display(), e))?;
    let theme = theme.as_deref().unwrap_or("light");
    let raw = raw.unwrap_or(false);
    let html = if !raw && markdown::is_markdown_path(&p) {
        markdown::render_markdown(&contents, theme)
    } else {
        markdown::render_plain(&contents)
    };
    Ok(RenderedFile { html, path, raw })
}

#[tauri::command]
pub fn read_source(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("cannot read '{path}': {e}"))
}

#[tauri::command]
pub fn restart(app: AppHandle) {
    app.restart();
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    // Restrict to http(s) so the command can't be abused to launch arbitrary
    // local files or schemes via the system opener.
    let lower = url.to_lowercase();
    if !lower.starts_with("https://") && !lower.starts_with("http://") {
        return Err("only http(s) URLs are allowed".to_string());
    }
    std::process::Command::new("open")
        .arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to open url: {e}"))
}

/// Extensions that macOS `open` (Launch Services) would *execute* or use to
/// redirect to an arbitrary target, rather than passively display. A markdown
/// document is untrusted input, so a relative link pointing at one of these
/// must not be handed to `open` — otherwise a co-located payload Cmd-clicked
/// from a deceptive link becomes local code execution.
const UNSAFE_OPEN_EXTS: &[&str] = &[
    // Executable bundles / things launched directly
    "app",
    "command",
    "terminal",
    "tool",
    "action",
    "workflow",
    "shortcut",
    // AppleScript
    "scpt",
    "scptd",
    "applescript",
    "osascript",
    // Shells / interpreters
    "sh",
    "bash",
    "zsh",
    "csh",
    "ksh",
    "fish",
    "py",
    "rb",
    "pl",
    "php",
    // Location files that redirect `open` to an arbitrary URL/path
    "webloc",
    "fileloc",
    "inetloc",
    "url",
    // Installers and loadable code bundles
    "pkg",
    "mpkg",
    "prefpane",
    "qlgenerator",
    "saver",
    "appex",
    "plugin",
    "kext",
    "bundle",
    "framework",
    "dylib",
    "so",
];

fn is_unsafe_to_open(path: &Path) -> bool {
    match path.extension().and_then(|s| s.to_str()) {
        Some(ext) => UNSAFE_OPEN_EXTS.contains(&ext.to_ascii_lowercase().as_str()),
        None => false,
    }
}

/// Open a local filesystem path in the default macOS application (Cmd+click
/// on non-markdown links in the preview).
#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err(format!("not found: {path}"));
    }
    if is_unsafe_to_open(p) {
        return Err(format!("refusing to launch executable file type: {path}"));
    }
    std::process::Command::new("open")
        .arg(&path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to open path: {e}"))
}

#[tauri::command]
pub fn open_file(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if !p.is_file() {
        return Err(format!("not a file: {path}"));
    }
    let mut slot = state
        .watcher
        .lock()
        .map_err(|_| "watcher mutex poisoned".to_string())?;
    slot.watch_file(&app, &p)
}

/// Writes export data (SVG text or base64-encoded PNG bytes) to a user-picked
/// path. Path is supplied by the frontend after going through the native save
/// dialog, so we trust it — the dialog is the consent boundary.
#[tauri::command]
pub fn save_export(path: String, data: String, base64_encoded: bool) -> Result<(), String> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;

    let bytes = if base64_encoded {
        STANDARD
            .decode(&data)
            .map_err(|e| format!("invalid base64 payload: {e}"))?
    } else {
        data.into_bytes()
    };
    std::fs::write(&path, bytes).map_err(|e| format!("failed to write '{path}': {e}"))
}

/// Toggle a GFM task-list checkbox at the given (1-indexed) line.
///
/// `expected_current` is the state the frontend believes the box is in
/// BEFORE the click. If the file's actual state diverges, the command
/// refuses to write — typically because the file changed on disk between
/// render and click. A soft "already in requested state" is reported as
/// success (covers stale watcher-driven re-renders racing a click).
#[tauri::command]
pub fn toggle_task(
    state: State<'_, AppState>,
    path: String,
    line: usize,
    new_state: bool,
    expected_current: bool,
) -> Result<(), String> {
    let _guard = state
        .tasklist_lock
        .lock()
        .map_err(|_| "tasklist mutex poisoned".to_string())?;

    // The toggle is well-defined: original_state == !new_state. If the
    // caller's expectation diverges from that invariant, something is
    // already inconsistent — refuse before touching disk.
    if expected_current == new_state {
        return Err("file changed on disk".to_string());
    }

    let p = PathBuf::from(&path);
    let content =
        std::fs::read_to_string(&p).map_err(|e| format!("cannot read '{}': {}", p.display(), e))?;

    let next = match tasklist::toggle_checkbox_at_line(&content, line, new_state) {
        Ok(s) => s,
        Err(tasklist::ToggleError::AlreadyInRequestedState) => {
            // Soft no-op: a stale watcher-driven re-render races with a
            // click on the same checkbox. Reporting success keeps the UI
            // calm; the file is already in the requested state.
            return Ok(());
        }
        Err(tasklist::ToggleError::LineOutOfRange) => {
            return Err("line out of range".to_string());
        }
        Err(tasklist::ToggleError::NotATaskListLine) => {
            return Err("file changed on disk".to_string());
        }
    };

    write_atomically(&p, next.as_bytes())
        .map_err(|e| format!("cannot write '{}': {}", p.display(), e))
}

/// Write `bytes` to `target` via a same-directory temp file + rename so a
/// crash mid-write can't truncate the user's file. Same-directory is load-
/// bearing on macOS — a cross-filesystem rename copies and isn't atomic.
fn write_atomically(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    let stem = target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("tasklist");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(".{stem}.tasklist-{nanos}.tmp"));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, target)
}

#[tauri::command]
pub fn frontend_ready(state: State<'_, AppState>) -> Vec<String> {
    let mut guard = state.opens.lock().unwrap();
    guard.ready = true;
    guard
        .files
        .drain(..)
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

/// Records the folder the sidebar is currently showing so the next plain
/// launch can restore it. Best-effort: a non-directory or vanished path is a
/// no-op, and persistence errors are swallowed (UI state, never user-facing).
#[tauri::command]
pub fn remember_folder(app: AppHandle, path: String) {
    let p = PathBuf::from(path);
    if p.is_dir() {
        recent::save_last(&app, &p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_launchable_extensions() {
        for name in [
            "setup.command",
            "Foo.app",
            "redirect.webloc",
            "thing.inetloc",
            "auto.workflow",
            "script.scpt",
            "go.pkg",
        ] {
            assert!(
                is_unsafe_to_open(Path::new(name)),
                "{name} should be refused"
            );
        }
    }

    #[test]
    fn flags_extensions_case_insensitively() {
        assert!(is_unsafe_to_open(Path::new("/x/RUN.SH")));
        assert!(is_unsafe_to_open(Path::new("/x/App.App")));
    }

    #[test]
    fn allows_viewable_files() {
        for name in [
            "photo.png",
            "scan.PDF",
            "notes.txt",
            "data.csv",
            "sheet.xlsx",
            "Makefile",
            "archive.zip",
        ] {
            assert!(
                !is_unsafe_to_open(Path::new(name)),
                "{name} should be allowed"
            );
        }
    }
}
