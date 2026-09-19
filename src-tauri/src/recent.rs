use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const MAX_RECENT: usize = 10;
const MAX_SESSIONS: usize = 30;
const FILE_NAME: &str = "recent.json";

/// Which release stream the auto-updater follows. Persisted in `recent.json`;
/// read at update-check time to pick the manifest endpoint.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Beta,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfSettings {
    pub preset: String,
    pub base_size: f64,
    pub paper: String,
    pub margins: String,
    pub page_numbers: String,
    #[serde(default = "default_table_style")]
    pub table_style: String,
    #[serde(default = "default_table_fit")]
    pub table_fit: String,
    #[serde(default = "default_orientation")]
    pub orientation: String,
}

fn default_table_style() -> String {
    "editorial".into()
}
fn default_table_fit() -> String {
    "wrap".into()
}
fn default_orientation() -> String {
    "portrait".into()
}

impl Default for PdfSettings {
    fn default() -> Self {
        Self {
            preset: "clean".into(),
            base_size: 11.0,
            paper: "a4".into(),
            margins: "normal".into(),
            page_numbers: "bottom-center".into(),
            table_style: default_table_style(),
            table_fit: default_table_fit(),
            orientation: default_orientation(),
        }
    }
}

/// A single window's tab session for a project root. `touched` is a Unix
/// timestamp (seconds) used only to pick eviction order when the number of
/// stored sessions exceeds `MAX_SESSIONS`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub tabs: Vec<PathBuf>,
    pub active: Option<usize>,
    #[serde(default)]
    pub touched: u64,
}

/// A project window open at quit time: its (canonical) root and logical bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    pub root: PathBuf,
    pub bounds: Option<crate::windows::Bounds>,
}

#[derive(Default, Serialize, Deserialize)]
struct Store {
    folders: Vec<PathBuf>,
    #[serde(default)]
    last_folder: Option<PathBuf>,
    /// Legacy pre-multi-window session (single tab list next to `last_folder`).
    /// Read-only: `migrate_legacy` folds it into `sessions` on load and it is
    /// never written again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    open_tabs: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_tab: Option<usize>,
    #[serde(default)]
    channel: UpdateChannel,
    #[serde(default)]
    pdf_export: PdfSettings,
    #[serde(default)]
    sessions: BTreeMap<PathBuf, Session>,
    /// Project windows open at the last quit, most recently focused first.
    #[serde(default)]
    windows: Vec<SavedWindow>,
}

impl Store {
    /// Move `canonical` to the front of the recent list, deduplicating and
    /// capping at `MAX_RECENT`. Leaves `last_folder` untouched.
    fn push_folder(&mut self, canonical: PathBuf) {
        self.folders.retain(|p| p != &canonical);
        self.folders.insert(0, canonical);
        self.folders.truncate(MAX_RECENT);
    }

    /// Pre-multi-window stores kept one tab session next to last_folder.
    fn migrate_legacy(&mut self) {
        let tabs = std::mem::take(&mut self.open_tabs);
        let active = self.active_tab.take();
        if tabs.is_empty() {
            return;
        }
        if let Some(root) = self.last_folder.clone() {
            self.sessions.entry(root).or_insert(Session {
                tabs,
                active,
                touched: 0,
            });
        }
    }
}

/// Evicts the least-recently-touched sessions until `map.len() <= max`.
fn cap_sessions(map: &mut BTreeMap<PathBuf, Session>, max: usize) {
    while map.len() > max {
        let oldest = map
            .iter()
            .min_by_key(|(_, s)| s.touched)
            .map(|(k, _)| k.clone());
        match oldest {
            Some(k) => {
                map.remove(&k);
            }
            None => break,
        }
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
    let mut store: Store = serde_json::from_slice(&bytes).unwrap_or_default();
    store.migrate_legacy();
    store
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

/// Returns the persisted open-tab paths and the active index for `root`,
/// unfiltered. `root` is canonicalized before lookup so it matches however
/// `save_session` stored it.
pub fn load_session(app: &AppHandle, root: &Path) -> (Vec<PathBuf>, Option<usize>) {
    let store = load_store(app);
    store
        .sessions
        .get(&canonical_or_keep(root))
        .map(|s| (s.tabs.clone(), s.active))
        .unwrap_or_default()
}

/// Persists the open-tab paths and active index for `root`, preserving every
/// other root's session and every other store field. Evicts the
/// least-recently-touched sessions beyond `MAX_SESSIONS`.
pub fn save_session(app: &AppHandle, root: &Path, tabs: &[PathBuf], active: Option<usize>) {
    let mut store = load_store(app);
    let touched = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    store.sessions.insert(
        canonical_or_keep(root),
        Session {
            tabs: tabs.to_vec(),
            active,
            touched,
        },
    );
    cap_sessions(&mut store.sessions, MAX_SESSIONS);
    write_store(app, &store);
}

pub fn load_channel(app: &AppHandle) -> UpdateChannel {
    load_store(app).channel
}

/// Persists the update channel, preserving every other field.
pub fn save_channel(app: &AppHandle, channel: UpdateChannel) {
    let mut store = load_store(app);
    store.channel = channel;
    write_store(app, &store);
}

pub fn load_pdf_settings(app: &AppHandle) -> PdfSettings {
    load_store(app).pdf_export
}

pub fn save_pdf_settings(app: &AppHandle, settings: &PdfSettings) {
    let mut store = load_store(app);
    store.pdf_export = settings.clone();
    write_store(app, &store);
}

pub fn load_windows(app: &AppHandle) -> Vec<SavedWindow> {
    load_store(app).windows
}

/// Persists the open-window snapshot, preserving every other field.
pub fn save_windows(app: &AppHandle, windows: &[SavedWindow]) {
    let mut store = load_store(app);
    store.windows = windows.to_vec();
    write_store(app, &store);
}

/// Keeps saved windows whose root is still a directory, dropping later
/// duplicates of the same root. Order is preserved. Pure.
pub fn restore_windows(
    saved: Vec<SavedWindow>,
    is_dir: impl Fn(&Path) -> bool,
) -> Vec<SavedWindow> {
    let mut seen = std::collections::HashSet::new();
    saved
        .into_iter()
        .filter(|w| is_dir(&w.root) && seen.insert(w.root.clone()))
        .collect()
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
    fn restore_windows_drops_missing_roots_and_dedupes() {
        let w = |r: &str| SavedWindow {
            root: PathBuf::from(r),
            bounds: None,
        };
        let kept = restore_windows(vec![w("/a"), w("/gone"), w("/b"), w("/a")], |p| {
            p != Path::new("/gone")
        });
        assert_eq!(kept, vec![w("/a"), w("/b")]);
    }

    #[test]
    fn store_without_windows_field_loads() {
        let s: Store = serde_json::from_str(r#"{"folders":[]}"#).unwrap();
        assert!(s.windows.is_empty());
    }

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
        let mut sessions = BTreeMap::new();
        sessions.insert(
            PathBuf::from("/p"),
            Session {
                tabs: vec![PathBuf::from("/a"), PathBuf::from("/b")],
                active: Some(1),
                touched: 42,
            },
        );
        let s = Store {
            sessions,
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Store = serde_json::from_str(&json).unwrap();
        let session = &back.sessions[Path::new("/p")];
        assert_eq!(session.tabs, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
        assert_eq!(session.active, Some(1));
        assert_eq!(session.touched, 42);
    }

    #[test]
    fn deserializes_legacy_store_without_session_fields() {
        let back: Store = serde_json::from_str(r#"{"folders":["/a"],"last_folder":"/x"}"#).unwrap();
        assert!(back.open_tabs.is_empty());
        assert_eq!(back.active_tab, None);
        assert!(back.sessions.is_empty());
    }

    #[test]
    fn legacy_session_migrates_under_last_folder() {
        let mut s: Store = serde_json::from_str(
            r#"{"folders":[],"last_folder":"/p","open_tabs":["/p/a.md"],"active_tab":0}"#,
        )
        .unwrap();
        s.migrate_legacy();
        assert_eq!(
            s.sessions[Path::new("/p")].tabs,
            vec![PathBuf::from("/p/a.md")]
        );
        assert_eq!(s.sessions[Path::new("/p")].active, Some(0));
        assert!(s.open_tabs.is_empty());
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("open_tabs"));
    }

    #[test]
    fn legacy_session_without_last_folder_is_dropped() {
        let mut s: Store = serde_json::from_str(r#"{"folders":[],"open_tabs":["/x.md"]}"#).unwrap();
        s.migrate_legacy();
        assert!(s.sessions.is_empty());
        assert!(s.open_tabs.is_empty());
    }

    #[test]
    fn cap_sessions_evicts_least_recently_touched() {
        let mut m = BTreeMap::new();
        for i in 0..5u64 {
            m.insert(
                PathBuf::from(format!("/r{i}")),
                Session {
                    tabs: vec![],
                    active: None,
                    touched: i,
                },
            );
        }
        cap_sessions(&mut m, 3);
        let keys: Vec<_> = m.keys().cloned().collect();
        assert_eq!(
            keys,
            vec![
                PathBuf::from("/r2"),
                PathBuf::from("/r3"),
                PathBuf::from("/r4")
            ]
        );
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

    #[test]
    fn store_defaults_channel_to_stable() {
        let s = Store::default();
        assert_eq!(s.channel, UpdateChannel::Stable);
    }

    #[test]
    fn deserializes_legacy_store_without_channel() {
        let back: Store = serde_json::from_str(r#"{"folders":["/a"]}"#).unwrap();
        assert_eq!(back.channel, UpdateChannel::Stable);
    }

    #[test]
    fn channel_round_trips_as_lowercase() {
        let s = Store {
            channel: UpdateChannel::Beta,
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"channel\":\"beta\""), "got: {json}");
        let back: Store = serde_json::from_str(&json).unwrap();
        assert_eq!(back.channel, UpdateChannel::Beta);
    }

    #[test]
    fn pdf_settings_default_is_clean() {
        let s = PdfSettings::default();
        assert_eq!(s.preset, "clean");
        assert_eq!(s.paper, "a4");
        assert_eq!(s.margins, "normal");
        assert_eq!(s.page_numbers, "bottom-center");
        assert_eq!(s.base_size, 11.0);
    }

    #[test]
    fn pdf_settings_round_trip_camel_case() {
        let s = PdfSettings {
            preset: "report".into(),
            base_size: 12.5,
            paper: "letter".into(),
            margins: "wide".into(),
            page_numbers: "bottom-right".into(),
            table_style: "minimal".into(),
            table_fit: "fit".into(),
            orientation: "landscape".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"baseSize\":12.5"), "got: {json}");
        assert!(
            json.contains("\"pageNumbers\":\"bottom-right\""),
            "got: {json}"
        );
        let back: PdfSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.preset, "report");
        assert!(json.contains("\"tableStyle\":\"minimal\""), "got: {json}");
        assert_eq!(back.orientation, "landscape");
        assert_eq!(back.table_fit, "fit");
    }

    #[test]
    fn store_defaults_pdf_settings_when_absent() {
        let back: Store = serde_json::from_str(r#"{"folders":["/a"]}"#).unwrap();
        assert_eq!(back.pdf_export.preset, "clean");
    }

    #[test]
    fn pdf_settings_default_has_new_table_fields() {
        let s = PdfSettings::default();
        assert_eq!(s.table_style, "editorial");
        assert_eq!(s.table_fit, "wrap");
        assert_eq!(s.orientation, "portrait");
    }

    #[test]
    fn pdf_settings_old_json_gets_new_field_defaults() {
        // Settings persisted by a version without the new fields must still load.
        let json = r#"{"preset":"clean","baseSize":11.0,"paper":"a4","margins":"normal","pageNumbers":"bottom-center"}"#;
        let s: PdfSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.table_style, "editorial");
        assert_eq!(s.table_fit, "wrap");
        assert_eq!(s.orientation, "portrait");
    }
}
