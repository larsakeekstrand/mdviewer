#![allow(dead_code)]
// Wired up in Task 2.

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
    fn window_title_uses_folder_name() {
        assert_eq!(window_title(Path::new("/Users/me/repo")), "repo — MDViewer");
        assert_eq!(window_title(Path::new("/")), "/ — MDViewer");
    }
}
