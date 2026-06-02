use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::{git, markdown, recent, search, tasklist, tree, AppState};

/// Updater manifest endpoints. `STABLE_UPDATE_URL` mirrors the endpoint baked
/// into `tauri.conf.json`; `BETA_UPDATE_URL` is the rolling pre-release channel.
/// If you change either, update `tauri.conf.json` / the release workflow to match.
const STABLE_UPDATE_URL: &str =
    "https://github.com/larsakeekstrand/mdviewer/releases/latest/download/latest.json";
const BETA_UPDATE_URL: &str =
    "https://github.com/larsakeekstrand/mdviewer/releases/download/beta/latest.json";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMeta {
    rid: tauri::ResourceId,
    version: String,
    current_version: String,
    body: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    pub channel: recent::UpdateChannel,
    pub version: String,
}

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

/// Sets the directories whose listings the sidebar is currently showing (the
/// tree root plus every expanded folder) so changes made by other apps appear
/// live. The frontend re-sends the set whenever the visible folders change.
#[tauri::command]
pub fn watch_tree(
    app: AppHandle,
    state: State<'_, AppState>,
    dirs: Vec<String>,
) -> Result<(), String> {
    let paths: Vec<PathBuf> = dirs.into_iter().map(PathBuf::from).collect();
    let mut slot = state
        .tree_watcher
        .lock()
        .map_err(|_| "tree watcher mutex poisoned".to_string())?;
    slot.watch_dirs(&app, paths)
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

/// Render an in-memory editor buffer (NOT disk) so the split editor's live
/// preview reflects unsaved text. Mirrors `render_file`'s markdown-vs-plain
/// choice by path extension; there is no raw mode (the editor itself is the
/// source view).
#[tauri::command]
pub fn render_preview(source: String, path: String, theme: Option<String>) -> String {
    let p = PathBuf::from(&path);
    let theme = theme.as_deref().unwrap_or("light");
    if markdown::is_markdown_path(&p) {
        markdown::render_markdown(&source, theme)
    } else {
        markdown::render_plain(&source)
    }
}

#[tauri::command]
pub fn render_notes(source: String, theme: Option<String>) -> Result<String, String> {
    let theme = theme.as_deref().unwrap_or("light");
    Ok(markdown::render_markdown(&source, theme))
}

#[tauri::command]
pub fn read_source(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("cannot read '{path}': {e}"))
}

/// Returns the host operating system as the same string `std::env::consts::OS`
/// reports — "macos", "windows", "linux", etc. The frontend uses this to gate
/// macOS-only UI affordances (Install CLI menu, Export as PDF menu) so we have
/// one source of truth rather than sniffing `navigator.platform`.
#[tauri::command]
pub fn platform() -> &'static str {
    std::env::consts::OS
}

#[tauri::command]
pub fn restart(app: AppHandle) {
    app.restart();
}

#[tauri::command]
pub fn get_preferences(app: AppHandle) -> Preferences {
    Preferences {
        channel: recent::load_channel(&app),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// Persists the user's update channel and signals the UI to re-check on the new
/// channel immediately. Best-effort: like the rest of the `recent` store, an
/// underlying write failure is swallowed rather than surfaced to the caller.
#[tauri::command]
pub fn set_update_channel(app: AppHandle, channel: recent::UpdateChannel) {
    recent::save_channel(&app, channel);
    let _ = app.emit("channel-changed", ());
}

/// Checks for an update on the user's selected channel. Mirrors the updater
/// plugin's own `check`, but overrides the endpoint per the stored channel and
/// returns the resource id so the frontend can hand it to the plugin's
/// (unchanged) `download_and_install`.
#[tauri::command]
pub async fn check_update(
    app: AppHandle,
    webview: tauri::Webview,
) -> Result<Option<UpdateMeta>, String> {
    use tauri::Manager;
    use tauri_plugin_updater::UpdaterExt;

    let url = match recent::load_channel(&app) {
        recent::UpdateChannel::Beta => BETA_UPDATE_URL,
        recent::UpdateChannel::Stable => STABLE_UPDATE_URL,
    };
    let endpoint = tauri::Url::parse(url).map_err(|e| format!("bad updater endpoint: {e}"))?;

    let updater = webview
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|e| format!("updater endpoints: {e}"))?
        .build()
        .map_err(|e| format!("updater build: {e}"))?;

    let update = updater
        .check()
        .await
        .map_err(|e| format!("update check failed: {e}"))?;

    match update {
        Some(update) => {
            let version = update.version.clone();
            let current_version = update.current_version.clone();
            let body = update.body.clone();
            let rid = webview.resources_table().add(update);
            Ok(Some(UpdateMeta {
                rid,
                version,
                current_version,
                body,
            }))
        }
        None => Ok(None),
    }
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    // Restrict to http(s) so the command can't be abused to launch arbitrary
    // local files or schemes via the system opener.
    let lower = url.to_lowercase();
    if !lower.starts_with("https://") && !lower.starts_with("http://") {
        return Err("only http(s) URLs are allowed".to_string());
    }
    opener::open(&url).map_err(|e| format!("failed to open url: {e}"))
}

/// Extensions that the host OS shell would *execute* or use to redirect to an
/// arbitrary target, rather than passively display. A markdown document is
/// untrusted input, so a relative link pointing at one of these must not be
/// handed to the shell opener — otherwise a co-located payload Cmd/Right-
/// clicked from a deceptive link becomes local code execution.
///
/// This is a cross-platform union: on macOS the Windows entries are inert
/// (the OS doesn't auto-execute them), and vice versa. Denying both keeps
/// one source of truth and avoids cfg-conditional security policy.
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
    // Windows executables (PE32)
    "exe",
    "com",
    "scr",
    "pif",
    // Windows scripting hosts
    "bat",
    "cmd",
    "ps1",
    "psm1",
    "psc1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "msh",
    "msh1",
    "msh2",
    "mshxml",
    "msh1xml",
    "msh2xml",
    // Windows installer / control panel / registry
    "msi",
    "msp",
    "msc",
    "cpl",
    "reg",
    "inf",
    // Windows shortcut / link files (redirect `start` to an arbitrary target)
    "lnk",
    "scf",
    "appref-ms",
    // Windows app packages
    "appx",
    "appxbundle",
    "hta",
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
    if refuses_to_open(p) {
        return Err(format!("refusing to launch executable file type: {path}"));
    }
    opener::open(&path).map_err(|e| format!("failed to open path: {e}"))
}

/// Whether `path`, with symlinks resolved, is `dir` itself or nested inside it.
/// Export uses this to refuse inlining files that escape the opened workspace:
/// the canonicalize step closes the symlink-escape a textual path check misses
/// (e.g. an in-workspace `logo.png` that is a symlink to `~/.ssh/id_rsa`).
/// `Path::starts_with` is component-wise, so `/work` never matches `/work-x`.
#[tauri::command]
pub fn path_within_dir(path: String, dir: String) -> bool {
    match (std::fs::canonicalize(&path), std::fs::canonicalize(&dir)) {
        (Ok(p), Ok(d)) => p.starts_with(&d),
        _ => false,
    }
}

/// Whether the system opener must refuse `p`. Checks both the literal path and,
/// because `opener::open` follows symlinks, the symlink-resolved target — so a
/// link named `notes.txt` pointing at `/Applications/Evil.app` can't smuggle a
/// launchable type past the extension denylist.
fn refuses_to_open(p: &Path) -> bool {
    let resolved = p.canonicalize();
    is_unsafe_to_open(p) || resolved.as_deref().map(is_unsafe_to_open).unwrap_or(false)
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
#[cfg(target_os = "macos")]
const CLI_LINK_PATH: &str = "/usr/local/bin/mdviewer";

/// What is currently present at `CLI_LINK_PATH`.
#[cfg(target_os = "macos")]
#[derive(Debug, PartialEq)]
enum LinkState {
    Absent,
    SymlinkToTarget,
    SymlinkElsewhere,
    NonSymlink,
}

/// What `install_cli` should do given the current `LinkState`.
#[cfg(target_os = "macos")]
#[derive(Debug, PartialEq)]
enum InstallAction {
    Create,
    AlreadyInstalled,
    RefuseNonSymlink,
}

/// Pure decision: maps the on-disk state to the action.
#[cfg(target_os = "macos")]
fn decide(state: LinkState) -> InstallAction {
    match state {
        LinkState::Absent | LinkState::SymlinkElsewhere => InstallAction::Create,
        LinkState::SymlinkToTarget => InstallAction::AlreadyInstalled,
        LinkState::NonSymlink => InstallAction::RefuseNonSymlink,
    }
}

/// The outcome reported back to the frontend. Serializes to snake_case strings
/// (`"installed"`, `"already_installed"`, `"cancelled"`) that `app.js` matches.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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
#[cfg(target_os = "macos")]
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
#[cfg(target_os = "macos")]
fn create_cli_symlink(target: &Path, link: &Path) -> Result<InstallOutcome, String> {
    if link.is_symlink() {
        // Best-effort: a root-owned dir will reject this, and the following
        // symlink() call will also get PermissionDenied, triggering the admin
        // escalation path which replaces the stale link via `ln -sf`.
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
#[cfg(target_os = "macos")]
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
    // AppleScript reports a dismissed auth dialog as error -128, written
    // parenthesized (e.g. "User canceled. (-128)"); match the parens so a
    // code like -1280 can't false-positive.
    if stderr.contains("(-128)") {
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
#[cfg(target_os = "macos")]
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

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn install_cli() -> Result<InstallOutcome, String> {
    Err("CLI install is only supported on macOS".to_string())
}

#[tauri::command]
pub fn search_in_folder(
    root: String,
    query: String,
    case_sensitive: bool,
    whole_word: bool,
    respect_gitignore: bool,
) -> Result<search::SearchResults, String> {
    search::search_in_folder(
        Path::new(&root),
        &query,
        search::SearchOpts {
            case_sensitive,
            whole_word,
            respect_gitignore,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn path_within_dir_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!("mdviewer-within-{}", std::process::id()));
        let work = base.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let secret = base.join("secret.key"); // outside the workspace
        std::fs::write(&secret, b"top secret").unwrap();
        let inside = work.join("logo.png");
        std::fs::write(&inside, b"img").unwrap();
        let escaping = work.join("evil.png"); // inside workspace, points outside
        let _ = std::fs::remove_file(&escaping);
        symlink(&secret, &escaping).unwrap();

        let w = work.to_string_lossy().into_owned();
        assert!(path_within_dir(
            inside.to_string_lossy().into_owned(),
            w.clone()
        ));
        assert!(!path_within_dir(
            escaping.to_string_lossy().into_owned(),
            w.clone()
        ));
        assert!(!path_within_dir(secret.to_string_lossy().into_owned(), w));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    #[cfg(unix)]
    fn refuses_symlink_whose_target_is_launchable() {
        use std::os::unix::fs::symlink;

        let dir = std::env::temp_dir().join(format!("mdviewer-symlink-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("payload.command"); // .command is in UNSAFE_OPEN_EXTS
        std::fs::write(&target, b"#!/bin/sh\n").unwrap();
        let link = dir.join("notes.txt"); // innocuous-looking name
        let _ = std::fs::remove_file(&link);
        symlink(&target, &link).unwrap();

        // The link name alone (".txt") passes the extension denylist...
        assert!(!is_unsafe_to_open(&link));
        // ...but resolving the symlink reveals the .command target, so we refuse.
        assert!(refuses_to_open(&link));

        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_file(&target);
        let _ = std::fs::remove_dir(&dir);
    }

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

    #[cfg(target_os = "macos")]
    mod cli_install_tests {
        use super::super::{decide, InstallAction, LinkState};

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

    #[test]
    fn render_notes_renders_markdown_to_html() {
        let html = render_notes("# Hello".to_string(), None).unwrap();
        assert!(html.contains("<h1"), "expected an h1, got: {html}");
        assert!(html.contains("Hello"));
    }

    mod platform_safety_tests {
        use super::super::is_unsafe_to_open;
        use std::path::Path;

        fn unsafe_path(name: &str) -> bool {
            is_unsafe_to_open(Path::new(name))
        }

        #[test]
        fn macos_executables_are_unsafe() {
            assert!(unsafe_path("foo.app"));
            assert!(unsafe_path("foo.command"));
            assert!(unsafe_path("foo.scpt"));
        }

        #[test]
        fn windows_executables_are_unsafe() {
            assert!(unsafe_path("foo.exe"));
            assert!(unsafe_path("foo.bat"));
            assert!(unsafe_path("foo.cmd"));
            assert!(unsafe_path("foo.com"));
            assert!(unsafe_path("foo.ps1"));
            assert!(unsafe_path("foo.vbs"));
            assert!(unsafe_path("foo.lnk"));
            assert!(unsafe_path("foo.msi"));
            assert!(unsafe_path("foo.scr"));
            assert!(unsafe_path("foo.hta"));
            assert!(unsafe_path("foo.cpl"));
            assert!(unsafe_path("foo.reg"));
        }

        #[test]
        fn extension_match_is_case_insensitive() {
            assert!(unsafe_path("foo.EXE"));
            assert!(unsafe_path("foo.Bat"));
        }

        #[test]
        fn benign_extensions_are_safe() {
            assert!(!unsafe_path("foo.md"));
            assert!(!unsafe_path("foo.txt"));
            assert!(!unsafe_path("foo.png"));
            assert!(!unsafe_path("foo.pdf"));
        }
    }

    #[test]
    fn render_preview_uses_markdown_for_md_paths() {
        let html = render_preview("# Hi".to_string(), "/x/note.md".to_string(), None);
        assert!(
            html.contains("<h1"),
            "expected markdown render, got: {html}"
        );
    }

    #[test]
    fn render_preview_uses_plain_for_txt_paths() {
        let html = render_preview("# Hi".to_string(), "/x/note.txt".to_string(), None);
        assert!(
            !html.contains("<h1"),
            "plain text must not become an h1: {html}"
        );
        assert!(
            html.contains("# Hi"),
            "plain text should be preserved: {html}"
        );
    }
}
