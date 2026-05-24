use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::{git, markdown, recent, tasklist, tree, AppState};

#[derive(Serialize)]
pub struct InitialState {
    pub tree_root: String,
    pub initial_file: Option<String>,
    pub restore_tabs: Vec<String>,
    pub active_tab: Option<usize>,
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
    let (saved_tabs, saved_active) = recent::load_session(&app);
    let (tabs, active_tab) = recent::restore_session(saved_tabs, saved_active, |p| p.is_file());
    InitialState {
        tree_root: tree_root.to_string_lossy().into_owned(),
        initial_file: state
            .initial_file
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
        restore_tabs: tabs
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        active_tab,
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

#[tauri::command]
pub fn save_session(app: AppHandle, tabs: Vec<String>, active: Option<usize>) {
    let paths: Vec<PathBuf> = tabs.into_iter().map(PathBuf::from).collect();
    recent::save_session(&app, &paths, active);
}

/// Where the CLI symlink lives. `/usr/local/bin` is the first entry in macOS's
/// default `/etc/paths`, so it is already on `$PATH` for every login shell with
/// no profile edits. The directory is root-owned on a stock Mac, so creating
/// the link there usually needs admin rights (handled in `install_with_admin`).
const CLI_LINK_PATH: &str = "/usr/local/bin/mdviewer";

/// What is currently present at `CLI_LINK_PATH`.
#[derive(Debug, PartialEq)]
enum LinkState {
    Absent,
    SymlinkToTarget,
    SymlinkElsewhere,
    NonSymlink,
}

/// What `install_cli` should do given the current `LinkState`.
#[derive(Debug, PartialEq)]
enum InstallAction {
    Create,
    AlreadyInstalled,
    RefuseNonSymlink,
}

/// Pure decision: maps the on-disk state to the action. Unit-tested.
fn decide(state: LinkState) -> InstallAction {
    match state {
        LinkState::Absent | LinkState::SymlinkElsewhere => InstallAction::Create,
        LinkState::SymlinkToTarget => InstallAction::AlreadyInstalled,
        LinkState::NonSymlink => InstallAction::RefuseNonSymlink,
    }
}

/// The outcome reported back to the frontend. Serializes to snake_case strings
/// (`"installed"`, `"already_installed"`, `"cancelled"`) that `app.js` matches.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallOutcome {
    Installed,
    AlreadyInstalled,
    Cancelled,
}

/// Classifies what is at `link` relative to `target`. Uses `symlink_metadata`
/// so a symlink is inspected, not followed (a broken symlink still reads as a
/// symlink; a missing path reads as `Absent`).
fn classify_link(link: &Path, target: &Path) -> LinkState {
    match std::fs::symlink_metadata(link) {
        Err(_) => LinkState::Absent,
        Ok(meta) if meta.file_type().is_symlink() => match std::fs::read_link(link) {
            Ok(dest) if dest.as_path() == target => LinkState::SymlinkToTarget,
            _ => LinkState::SymlinkElsewhere,
        },
        Ok(_) => LinkState::NonSymlink,
    }
}

/// Creates (or replaces) the symlink. Tries unprivileged first so Macs where
/// `/usr/local/bin` is user-writable (e.g. Homebrew on Intel) never see a
/// password prompt; escalates only on a permission or missing-directory error.
fn create_cli_symlink(target: &Path, link: &Path) -> Result<InstallOutcome, String> {
    if link.is_symlink() {
        // Best-effort: on a root-owned dir this fails and we fall through to
        // the elevated `ln -sf`, which replaces the stale link itself.
        let _ = std::fs::remove_file(link);
    }
    match std::os::unix::fs::symlink(target, link) {
        Ok(()) => Ok(InstallOutcome::Installed),
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::NotFound
            ) =>
        {
            install_with_admin(target)
        }
        Err(e) => Err(format!("failed to create symlink: {e}")),
    }
}

/// Elevated path: an AppleScript admin prompt that runs `mkdir -p` + `ln -sf`.
/// The exe path is passed as an `argv` item and shell-quoted by AppleScript's
/// `quoted form of`, so it is never interpolated into a shell string by us — a
/// path containing spaces or quotes cannot break out. The destination is a
/// fixed literal.
fn install_with_admin(target: &Path) -> Result<InstallOutcome, String> {
    let script = format!(
        "do shell script \"mkdir -p /usr/local/bin && ln -sf \" & quoted form of (item 1 of argv) & \" {CLI_LINK_PATH}\" with administrator privileges"
    );
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg("on run argv")
        .arg("-e")
        .arg(&script)
        .arg("-e")
        .arg("end run")
        .arg("--")
        .arg(target)
        .output()
        .map_err(|e| format!("failed to launch osascript: {e}"))?;

    if output.status.success() {
        return Ok(InstallOutcome::Installed);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    // AppleScript reports a dismissed auth dialog as error -128 ("User canceled").
    if stderr.contains("-128") {
        return Ok(InstallOutcome::Cancelled);
    }
    Err(format!(
        "failed to create symlink with administrator privileges: {}",
        stderr.trim()
    ))
}

/// Symlinks the running binary into `/usr/local/bin` so `mdviewer` is runnable
/// from a terminal. The target is always our own `current_exe()`, never a
/// caller-supplied path.
#[tauri::command]
pub fn install_cli() -> Result<InstallOutcome, String> {
    let target =
        std::env::current_exe().map_err(|e| format!("cannot resolve app binary path: {e}"))?;
    let link = Path::new(CLI_LINK_PATH);
    match decide(classify_link(link, &target)) {
        InstallAction::AlreadyInstalled => Ok(InstallOutcome::AlreadyInstalled),
        InstallAction::RefuseNonSymlink => Err(format!(
            "{CLI_LINK_PATH} already exists and is not a symlink; refusing to overwrite it"
        )),
        InstallAction::Create => create_cli_symlink(&target, link),
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

    #[test]
    fn decide_creates_when_absent() {
        assert_eq!(decide(LinkState::Absent), InstallAction::Create);
    }

    #[test]
    fn decide_creates_when_symlink_points_elsewhere() {
        assert_eq!(decide(LinkState::SymlinkElsewhere), InstallAction::Create);
    }

    #[test]
    fn decide_already_installed_when_symlink_points_to_target() {
        assert_eq!(
            decide(LinkState::SymlinkToTarget),
            InstallAction::AlreadyInstalled
        );
    }

    #[test]
    fn decide_refuses_when_non_symlink_exists() {
        assert_eq!(
            decide(LinkState::NonSymlink),
            InstallAction::RefuseNonSymlink
        );
    }
}
