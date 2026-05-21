use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const MAX_RECENT: usize = 10;
const FILE_NAME: &str = "recent.json";

#[derive(Default, Serialize, Deserialize)]
struct Store {
    folders: Vec<PathBuf>,
    #[serde(default)]
    last_folder: Option<PathBuf>,
}

impl Store {
    /// Move `canonical` to the front of the recent list, deduplicating and
    /// capping at `MAX_RECENT`. Leaves `last_folder` untouched.
    fn push_folder(&mut self, canonical: PathBuf) {
        self.folders.retain(|p| p != &canonical);
        self.folders.insert(0, canonical);
        self.folders.truncate(MAX_RECENT);
    }
}

fn store_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(FILE_NAME))
}

fn canonical_or_keep(folder: &Path) -> PathBuf {
    folder
        .canonicalize()
        .unwrap_or_else(|_| folder.to_path_buf())
}

fn load_store(app: &AppHandle) -> Store {
    let Some(path) = store_path(app) else {
        return Store::default();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return Store::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn write_store(app: &AppHandle, store: &Store) {
    let Some(path) = store_path(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(store) {
        let _ = std::fs::write(path, json);
    }
}

pub fn load(app: &AppHandle) -> Vec<PathBuf> {
    load_store(app).folders
}

/// Adds `folder` to the front of the recent list. Deduplicates and caps at
/// `MAX_RECENT`. Returns the new list.
pub fn push(app: &AppHandle, folder: &Path) -> Vec<PathBuf> {
    let mut store = load_store(app);
    store.push_folder(canonical_or_keep(folder));
    write_store(app, &store);
    store.folders
}

/// Empties the recent list. Preserves `last_folder` — clearing the Open Recent
/// menu must not forget where the sidebar was.
pub fn clear(app: &AppHandle) {
    let mut store = load_store(app);
    store.folders.clear();
    write_store(app, &store);
}

#[allow(dead_code)]
pub fn load_last(app: &AppHandle) -> Option<PathBuf> {
    load_store(app).last_folder
}

#[allow(dead_code)]
pub fn save_last(app: &AppHandle, folder: &Path) {
    let mut store = load_store(app);
    store.last_folder = Some(canonical_or_keep(folder));
    write_store(app, &store);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_folder_dedups_and_moves_to_front() {
        let mut s = Store::default();
        s.push_folder(PathBuf::from("/a"));
        s.push_folder(PathBuf::from("/b"));
        s.push_folder(PathBuf::from("/a"));
        assert_eq!(s.folders, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }

    #[test]
    fn push_folder_caps_at_max_recent() {
        let mut s = Store::default();
        for i in 0..(MAX_RECENT + 5) {
            s.push_folder(PathBuf::from(format!("/d{i}")));
        }
        assert_eq!(s.folders.len(), MAX_RECENT);
        assert_eq!(s.folders[0], PathBuf::from(format!("/d{}", MAX_RECENT + 4)));
    }

    #[test]
    fn push_folder_preserves_last_folder() {
        let mut s = Store {
            last_folder: Some(PathBuf::from("/keep")),
            ..Default::default()
        };
        s.push_folder(PathBuf::from("/a"));
        assert_eq!(s.last_folder, Some(PathBuf::from("/keep")));
    }

    #[test]
    fn store_round_trips_both_fields() {
        let mut s = Store::default();
        s.push_folder(PathBuf::from("/a"));
        s.last_folder = Some(PathBuf::from("/last"));
        let json = serde_json::to_string(&s).unwrap();
        let back: Store = serde_json::from_str(&json).unwrap();
        assert_eq!(back.folders, vec![PathBuf::from("/a")]);
        assert_eq!(back.last_folder, Some(PathBuf::from("/last")));
    }

    #[test]
    fn deserializes_legacy_store_without_last_folder() {
        let back: Store = serde_json::from_str(r#"{"folders":["/a"]}"#).unwrap();
        assert_eq!(back.folders, vec![PathBuf::from("/a")]);
        assert_eq!(back.last_folder, None);
    }
}
