//! Per-window state for project windows, keyed by Tauri window label. The
//! registry is pure (unit-tested); the IO helpers below it touch the AppHandle.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::watcher::{TreeWatcherSlot, WatcherSlot};

pub const NO_PROJECT_WINDOW: &str = "no project window";

pub struct WindowState {
    pub root: PathBuf,
    pub watcher: WatcherSlot,
    pub tree_watcher: TreeWatcherSlot,
    pub ready: bool,
    pub pending_files: Vec<PathBuf>,
}

impl WindowState {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            watcher: WatcherSlot::default(),
            tree_watcher: TreeWatcherSlot::default(),
            ready: false,
            pending_files: Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct Registry {
    windows: HashMap<String, WindowState>,
    /// helper window label → the project window it acts for.
    owners: HashMap<String, String>,
    next_id: u64,
}

impl Registry {
    pub fn insert(&mut self, label: &str, root: PathBuf) {
        let root = root.canonicalize().unwrap_or(root);
        self.windows
            .insert(label.to_string(), WindowState::new(root));
    }

    pub fn get(&self, label: &str) -> Option<&WindowState> {
        self.windows.get(label)
    }

    pub fn get_mut(&mut self, label: &str) -> Option<&mut WindowState> {
        self.windows.get_mut(label)
    }

    pub fn remove(&mut self, label: &str) -> Option<WindowState> {
        self.owners.retain(|_, owner| owner != label);
        self.windows.remove(label)
    }

    pub fn root(&self, label: &str) -> Result<PathBuf, String> {
        self.windows
            .get(label)
            .map(|w| w.root.clone())
            .ok_or_else(|| NO_PROJECT_WINDOW.to_string())
    }

    pub fn set_root(&mut self, label: &str, root: PathBuf) -> Result<(), String> {
        let root = root.canonicalize().unwrap_or(root);
        let w = self
            .windows
            .get_mut(label)
            .ok_or_else(|| NO_PROJECT_WINDOW.to_string())?;
        w.root = root;
        Ok(())
    }

    pub fn roots(&self) -> Vec<(String, PathBuf)> {
        self.windows
            .iter()
            .map(|(l, w)| (l.clone(), w.root.clone()))
            .collect()
    }

    pub fn labels(&self) -> Vec<String> {
        self.windows.keys().cloned().collect()
    }

    pub fn label_with_root(&self, root: &Path) -> Option<String> {
        self.windows
            .iter()
            .find(|(_, w)| w.root == root)
            .map(|(l, _)| l.clone())
    }

    pub fn next_label(&mut self) -> String {
        loop {
            self.next_id += 1;
            let label = format!("project-{}", self.next_id);
            if !self.windows.contains_key(&label) {
                return label;
            }
        }
    }

    pub fn set_owner(&mut self, helper: &str, owner: &str) {
        self.owners.insert(helper.to_string(), owner.to_string());
    }

    /// The project window a command caller acts for: itself if it is a project
    /// window, else the owner recorded when the helper window was opened.
    pub fn project_label_for(&self, caller: &str) -> Result<String, String> {
        if self.windows.contains_key(caller) {
            return Ok(caller.to_string());
        }
        self.owners
            .get(caller)
            .filter(|owner| self.windows.contains_key(owner.as_str()))
            .cloned()
            .ok_or_else(|| NO_PROJECT_WINDOW.to_string())
    }
}

/// Buffer Finder opens that arrive before setup has registered any window.
/// Returns `None` when buffered, or `Some(paths)` when the buffer has already
/// been drained and the caller must deliver them itself.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn buffer_early(slot: &mut Option<Vec<PathBuf>>, paths: Vec<PathBuf>) -> Option<Vec<PathBuf>> {
    match slot {
        Some(buf) => {
            buf.extend(paths);
            None
        }
        None => Some(paths),
    }
}

/// Hand files to a project window: emit `open-file` if its frontend is ready,
/// else buffer them for its `frontend_ready`. The ready check and the push
/// happen under one lock, so nothing is lost between them.
pub fn deliver_files(app: &tauri::AppHandle, label: &str, paths: Vec<PathBuf>) {
    use tauri::{Emitter, Manager};
    let state = app.state::<crate::AppState>();
    let ready = {
        let mut reg = state.windows.lock().unwrap();
        let Some(w) = reg.get_mut(label) else { return };
        if !w.ready {
            w.pending_files.extend(paths.iter().cloned());
        }
        w.ready
    };
    if ready {
        for p in &paths {
            let _ = app.emit_to(label, "open-file", p.to_string_lossy().into_owned());
        }
    }
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub fn cascade(from: Option<(f64, f64)>) -> Option<(f64, f64)> {
    from.map(|(x, y)| (x + 24.0, y + 24.0))
}

pub fn front_label(app: &tauri::AppHandle) -> Option<String> {
    use tauri::Manager;
    let state = app.state::<crate::AppState>();
    let labels = state.windows.lock().ok()?.labels();
    let front = state.focus.lock().ok()?.front().map(str::to_string);
    front
        .filter(|l| labels.contains(l))
        .or_else(|| labels.into_iter().next())
}

pub fn emit_to_front<S: serde::Serialize + Clone>(app: &tauri::AppHandle, event: &str, payload: S) {
    use tauri::Emitter;
    if let Some(label) = front_label(app) {
        let _ = app.emit_to(label.as_str(), event, payload);
    }
}

/// Registers state BEFORE building so the new webview's first
/// get_initial_state finds its entry; removes it again if the build fails.
pub fn create_project_window(
    app: &tauri::AppHandle,
    root: PathBuf,
    bounds: Option<Bounds>,
) -> Result<String, String> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
    let state = app.state::<crate::AppState>();
    let from = front_label(app)
        .and_then(|l| app.get_webview_window(&l))
        .and_then(|w| Some((w.outer_position().ok()?, w.scale_factor().ok()?)))
        .map(|(p, s)| (p.x as f64 / s, p.y as f64 / s));
    let label = {
        let mut reg = state
            .windows
            .lock()
            .map_err(|_| "window registry poisoned".to_string())?;
        let label = reg.next_label();
        reg.insert(&label, root.clone());
        label
    };
    let mut b = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title(window_title(&root))
        .min_inner_size(600.0, 400.0)
        .resizable(true);
    b = match bounds {
        Some(bb) => b.inner_size(bb.w, bb.h).position(bb.x, bb.y),
        None => {
            let b = b.inner_size(1200.0, 800.0);
            match cascade(from) {
                Some((x, y)) => b.position(x, y),
                None => b,
            }
        }
    };
    if let Err(e) = b.build() {
        if let Ok(mut reg) = state.windows.lock() {
            reg.remove(&label);
        }
        return Err(format!("cannot create window: {e}"));
    }
    Ok(label)
}

/// Focus the window already showing `root`, or open a new one for it.
pub fn open_folder_in_new_window(app: &tauri::AppHandle, root: PathBuf) {
    use tauri::Manager;
    let root = root.canonicalize().unwrap_or(root);
    let existing = app
        .state::<crate::AppState>()
        .windows
        .lock()
        .unwrap()
        .label_with_root(&root);
    match existing {
        Some(label) => {
            if let Some(w) = app.get_webview_window(&label) {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
        None => {
            if let Err(e) = create_project_window(app, root, None) {
                eprintln!("mdviewer: {e}");
            }
        }
    }
}

/// Drops a closed project window's state; dropping its WatcherSlot and
/// TreeWatcherSlot stops their watchers. Helper windows are ignored.
pub fn on_destroyed(app: &tauri::AppHandle, label: &str) {
    use tauri::Manager;
    let state = app.state::<crate::AppState>();
    let is_last = {
        let reg = state.windows.lock().unwrap();
        reg.get(label).is_some() && reg.labels().len() == 1
    };
    if is_last && !state.quitting.load(std::sync::atomic::Ordering::SeqCst) {
        crate::recent::save_windows(app, &snapshot(app));
    }
    let removed = state.windows.lock().unwrap().remove(label);
    if removed.is_none() {
        return;
    }
    state.focus.lock().unwrap().remove(label);
    app.state::<crate::mcp_server::McpPending>()
        .abandon_window(label);
    drop(removed);
}

/// The open project windows, most recently focused first, so the front window
/// becomes `main` on relaunch. Locks the registry then focus, never nested.
pub fn snapshot(app: &tauri::AppHandle) -> Vec<crate::recent::SavedWindow> {
    use tauri::Manager;
    let state = app.state::<crate::AppState>();
    let roots: HashMap<String, PathBuf> =
        state.windows.lock().unwrap().roots().into_iter().collect();
    let mut order: Vec<String> = state.focus.lock().unwrap().as_slice().to_vec();
    for l in roots.keys() {
        if !order.contains(l) {
            order.push(l.clone());
        }
    }
    order
        .into_iter()
        .filter_map(|l| {
            let root = roots.get(&l)?.clone();
            let bounds = app.get_webview_window(&l).and_then(|w| {
                let s = w.scale_factor().ok()?;
                let p = w.outer_position().ok()?.to_logical::<f64>(s);
                let z = w.inner_size().ok()?.to_logical::<f64>(s);
                Some(Bounds {
                    x: p.x,
                    y: p.y,
                    w: z.width,
                    h: z.height,
                })
            });
            Some(crate::recent::SavedWindow { root, bounds })
        })
        .collect()
}

pub fn window_title(root: &Path) -> String {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned());
    format!("{name} — MDViewer")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn root_of_unknown_label_is_no_project_window() {
        let r = Registry::default();
        assert_eq!(r.root("main").unwrap_err(), NO_PROJECT_WINDOW);
    }

    #[test]
    fn insert_then_root_and_set_root() {
        let mut r = Registry::default();
        r.insert("main", PathBuf::from("/a"));
        assert_eq!(r.root("main").unwrap(), PathBuf::from("/a"));
        r.set_root("main", PathBuf::from("/b")).unwrap();
        assert_eq!(r.root("main").unwrap(), PathBuf::from("/b"));
        assert!(r.set_root("nope", PathBuf::from("/c")).is_err());
    }

    #[test]
    fn next_label_is_monotonic_and_skips_taken_labels() {
        let mut r = Registry::default();
        r.insert("project-1", PathBuf::from("/x"));
        assert_eq!(r.next_label(), "project-2");
        assert_eq!(r.next_label(), "project-3");
    }

    #[test]
    fn insert_and_set_root_canonicalize_a_real_path() {
        let dir = std::env::temp_dir().join(format!(
            "mdviewer-windows-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let canonical = dir.canonicalize().unwrap();
        let messy = sub.join("..");

        let mut r = Registry::default();
        r.insert("main", messy.clone());
        assert_eq!(r.root("main").unwrap(), canonical);

        r.set_root("main", messy).unwrap();
        assert_eq!(r.root("main").unwrap(), canonical);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn label_with_root_matches_exact_root_only() {
        let mut r = Registry::default();
        r.insert("main", PathBuf::from("/repo"));
        assert_eq!(
            r.label_with_root(Path::new("/repo")).as_deref(),
            Some("main")
        );
        assert_eq!(r.label_with_root(Path::new("/repo/docs")), None);
    }

    #[test]
    fn project_label_for_resolves_helpers_through_owner() {
        let mut r = Registry::default();
        r.insert("main", PathBuf::from("/a"));
        assert_eq!(r.project_label_for("main").unwrap(), "main");
        assert_eq!(
            r.project_label_for("pdf-export").unwrap_err(),
            NO_PROJECT_WINDOW
        );
        r.set_owner("pdf-export", "main");
        assert_eq!(r.project_label_for("pdf-export").unwrap(), "main");
    }

    #[test]
    fn removing_a_window_drops_owners_pointing_at_it() {
        let mut r = Registry::default();
        r.insert("main", PathBuf::from("/a"));
        r.set_owner("claude-integration", "main");
        r.remove("main");
        assert_eq!(
            r.project_label_for("claude-integration").unwrap_err(),
            NO_PROJECT_WINDOW
        );
    }

    #[test]
    fn buffer_early_buffers_until_drained_then_hands_back() {
        let mut slot = Some(vec![PathBuf::from("/a.md")]);
        assert_eq!(buffer_early(&mut slot, vec![PathBuf::from("/b.md")]), None);
        assert_eq!(
            slot,
            Some(vec![PathBuf::from("/a.md"), PathBuf::from("/b.md")])
        );
        let mut drained: Option<Vec<PathBuf>> = None;
        assert_eq!(
            buffer_early(&mut drained, vec![PathBuf::from("/c.md")]),
            Some(vec![PathBuf::from("/c.md")])
        );
        assert_eq!(drained, None);
    }

    #[test]
    fn cascade_offsets_from_focused_window() {
        assert_eq!(cascade(Some((100.0, 50.0))), Some((124.0, 74.0)));
        assert_eq!(cascade(None), None);
    }

    #[test]
    fn window_title_uses_folder_name() {
        assert_eq!(window_title(Path::new("/Users/me/repo")), "repo — MDViewer");
        assert_eq!(window_title(Path::new("/")), "/ — MDViewer");
    }
}
