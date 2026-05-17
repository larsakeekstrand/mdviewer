use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const MAX_RECENT: usize = 10;
const FILE_NAME: &str = "recent.json";

#[derive(Default, Serialize, Deserialize)]
struct Store {
    folders: Vec<PathBuf>,
}

fn store_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(FILE_NAME))
}

pub fn load(app: &AppHandle) -> Vec<PathBuf> {
    let Some(path) = store_path(app) else {
        return Vec::new();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return Vec::new();
    };
    let store: Store = serde_json::from_slice(&bytes).unwrap_or_default();
    store.folders
}

/// Adds `folder` to the front of the recent list. Deduplicates and caps at
/// `MAX_RECENT`. Returns the new list.
pub fn push(app: &AppHandle, folder: &Path) -> Vec<PathBuf> {
    let canonical = folder.canonicalize().unwrap_or_else(|_| folder.to_path_buf());
    let mut list = load(app);
    list.retain(|p| p != &canonical);
    list.insert(0, canonical);
    list.truncate(MAX_RECENT);
    write(app, &list);
    list
}

pub fn clear(app: &AppHandle) {
    write(app, &[]);
}

fn write(app: &AppHandle, list: &[PathBuf]) {
    let Some(path) = store_path(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let store = Store {
        folders: list.to_vec(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&store) {
        let _ = std::fs::write(path, json);
    }
}

/// Replaces `$HOME` with `~` for menu display.
pub fn display(p: &Path) -> String {
    let s = p.to_string_lossy().into_owned();
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() && s.starts_with(&home) {
            return format!("~{}", &s[home.len()..]);
        }
    }
    s
}
