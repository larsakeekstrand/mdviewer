use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::{markdown, tree, updates, AppState};

#[derive(Serialize)]
pub struct InitialState {
    pub tree_root: String,
    pub initial_file: Option<String>,
}

#[tauri::command]
pub fn get_initial_state(state: State<'_, AppState>) -> InitialState {
    InitialState {
        tree_root: state.tree_root.to_string_lossy().into_owned(),
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
pub fn check_for_updates() -> Result<updates::UpdateInfo, String> {
    updates::check()
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

/// Open a local filesystem path in the default macOS application (Cmd+click
/// on non-markdown links in the preview).
#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err(format!("not found: {path}"));
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
