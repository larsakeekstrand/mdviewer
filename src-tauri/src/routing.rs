//! Which window an incoming path belongs to. `route` and `FocusOrder` are pure
//! and unit-tested; `fallback_root` is the only IO.

use std::path::{Path, PathBuf};

/// Where an Open Folder request should land: adopt the root into the calling
/// window, or focus the window that already shows it.
#[derive(Debug, PartialEq)]
pub enum FolderTarget {
    Adopt,
    Focus(String),
}

/// Decide whether `caller` should adopt `root`, or defer to another window
/// that already has it open.
pub fn open_folder_target(caller: &str, root: &Path, roots: &[(String, PathBuf)]) -> FolderTarget {
    roots
        .iter()
        .find(|(l, r)| l != caller && r == root)
        .map(|(l, _)| FolderTarget::Focus(l.clone()))
        .unwrap_or(FolderTarget::Adopt)
}

/// Most-recently-focused project windows, front = most recent.
#[derive(Default)]
pub struct FocusOrder {
    order: Vec<String>,
}

impl FocusOrder {
    pub fn touch(&mut self, label: &str) {
        self.order.retain(|l| l != label);
        self.order.insert(0, label.to_string());
    }

    pub fn remove(&mut self, label: &str) {
        self.order.retain(|l| l != label);
    }

    pub fn front(&self) -> Option<&str> {
        self.order.first().map(String::as_str)
    }

    pub fn as_slice(&self) -> &[String] {
        &self.order
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn open_folder_adopts_when_no_other_window_has_it() {
        let roots = vec![("main".to_string(), PathBuf::from("/a"))];
        assert_eq!(
            open_folder_target("main", Path::new("/b"), &roots),
            FolderTarget::Adopt
        );
        assert_eq!(
            open_folder_target("main", Path::new("/a"), &roots),
            FolderTarget::Adopt
        );
    }

    #[test]
    fn open_folder_focuses_other_window_with_same_root() {
        let roots = vec![
            ("main".to_string(), PathBuf::from("/a")),
            ("project-1".to_string(), PathBuf::from("/b")),
        ];
        assert_eq!(
            open_folder_target("main", Path::new("/b"), &roots),
            FolderTarget::Focus("project-1".into())
        );
    }

    #[test]
    fn touch_moves_label_to_front_without_duplicates() {
        let mut f = FocusOrder::default();
        f.touch("main");
        f.touch("project-1");
        f.touch("main");
        assert_eq!(f.as_slice(), ["main", "project-1"]);
        assert_eq!(f.front(), Some("main"));
    }

    #[test]
    fn remove_drops_label() {
        let mut f = FocusOrder::default();
        f.touch("main");
        f.touch("project-1");
        f.remove("project-1");
        assert_eq!(f.as_slice(), ["main"]);
        f.remove("main");
        assert_eq!(f.front(), None);
    }
}
