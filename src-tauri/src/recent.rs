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
    #[serde(default)]
    open_tabs: Vec<PathBuf>,
    #[serde(default)]
    active_tab: Option<usize>,
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

pub fn load_last(app: &AppHandle) -> Option<PathBuf> {
    load_store(app).last_folder
}

pub fn save_last(app: &AppHandle, folder: &Path) {
    let mut store = load_store(app);
    store.last_folder = Some(canonical_or_keep(folder));
    write_store(app, &store);
}

/// Returns the persisted open-tab paths and the active index, unfiltered.
pub fn load_session(app: &AppHandle) -> (Vec<PathBuf>, Option<usize>) {
    let store = load_store(app);
    (store.open_tabs, store.active_tab)
}

/// Persists the open-tab paths and active index, preserving `folders` and
/// `last_folder`. Paths are stored as-is (NOT canonicalized) so they keep
/// string-identity with the frontend's live tab model.
pub fn save_session(app: &AppHandle, tabs: &[PathBuf], active: Option<usize>) {
    let mut store = load_store(app);
    store.open_tabs = tabs.to_vec();
    store.active_tab = active;
    write_store(app, &store);
}

/// Filters `tabs` to the paths satisfying `exists` (order preserved) and remaps
/// `active` by tracking the active path: the result's active index is that
/// path's position in the filtered list, or `None` if the active file is gone
/// or the list is empty. Pure — no I/O, so it is unit-testable.
pub fn restore_session(
    tabs: Vec<PathBuf>,
    active: Option<usize>,
    exists: impl Fn(&Path) -> bool,
) -> (Vec<PathBuf>, Option<usize>) {
    let active_path = active.and_then(|i| tabs.get(i)).cloned();
    let kept: Vec<PathBuf> = tabs.into_iter().filter(|p| exists(p)).collect();
    let new_active = active_path.and_then(|ap| kept.iter().position(|p| *p == ap));
    (kept, new_active)
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

    #[test]
    fn store_round_trips_session_fields() {
        let s = Store {
            open_tabs: vec![PathBuf::from("/a"), PathBuf::from("/b")],
            active_tab: Some(1),
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Store = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.open_tabs,
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
        assert_eq!(back.active_tab, Some(1));
    }

    #[test]
    fn deserializes_legacy_store_without_session_fields() {
        let back: Store = serde_json::from_str(r#"{"folders":["/a"],"last_folder":"/x"}"#).unwrap();
        assert!(back.open_tabs.is_empty());
        assert_eq!(back.active_tab, None);
    }

    #[test]
    fn restore_session_keeps_all_when_all_exist() {
        let (kept, active) = restore_session(
            vec![
                PathBuf::from("/a"),
                PathBuf::from("/b"),
                PathBuf::from("/c"),
            ],
            Some(1),
            |_| true,
        );
        assert_eq!(
            kept,
            vec![
                PathBuf::from("/a"),
                PathBuf::from("/b"),
                PathBuf::from("/c")
            ]
        );
        assert_eq!(active, Some(1));
    }

    #[test]
    fn restore_session_drops_missing_and_shifts_active() {
        // "/a" is gone; active was index 1 ("/b"), which becomes index 0.
        let (kept, active) = restore_session(
            vec![
                PathBuf::from("/a"),
                PathBuf::from("/b"),
                PathBuf::from("/c"),
            ],
            Some(1),
            |p| p != Path::new("/a"),
        );
        assert_eq!(kept, vec![PathBuf::from("/b"), PathBuf::from("/c")]);
        assert_eq!(active, Some(0));
    }

    #[test]
    fn restore_session_active_file_missing_returns_none() {
        let (kept, active) = restore_session(
            vec![PathBuf::from("/a"), PathBuf::from("/b")],
            Some(1),
            |p| p != Path::new("/b"),
        );
        assert_eq!(kept, vec![PathBuf::from("/a")]);
        assert_eq!(active, None);
    }

    #[test]
    fn restore_session_empty_input_yields_none_active() {
        let (kept, active) = restore_session(vec![], Some(0), |_| true);
        assert!(kept.is_empty());
        assert_eq!(active, None);
    }

    #[test]
    fn restore_session_out_of_bounds_active_returns_none() {
        let (kept, active) = restore_session(
            vec![PathBuf::from("/a"), PathBuf::from("/b")],
            Some(5),
            |_| true,
        );
        assert_eq!(kept, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
        assert_eq!(active, None);
    }
}
