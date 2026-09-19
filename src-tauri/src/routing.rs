//! Which window an incoming path belongs to. Contains:
//! - `FolderTarget` and `open_folder_target` (which window an Open Folder targets)
//! - `FocusOrder` (MRU tracking for windows)
//! - `Route`, `route`, and `fallback_root` (path-to-window routing)

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

/// Where an incoming path should route: to an existing window or a new one.
#[derive(Debug, PartialEq)]
pub enum Route {
    Window(String),
    NewWindow(PathBuf),
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

/// Pick the project window for `path`: the longest containing root wins;
/// equal roots go to the most recently focused. Paths are compared component-
/// wise (Path::starts_with), so /repo does not contain /repo2. Callers pass
/// canonical paths.
pub fn route(
    path: &Path,
    roots: &[(String, PathBuf)],
    mru: &[String],
    fallback_root: impl Fn(&Path) -> PathBuf,
) -> Route {
    let rank = |label: &str| mru.iter().position(|l| l == label).unwrap_or(usize::MAX);
    roots
        .iter()
        .filter(|(_, root)| path.starts_with(root))
        .min_by(|(la, ra), (lb, rb)| {
            rb.components()
                .count()
                .cmp(&ra.components().count())
                .then_with(|| rank(la).cmp(&rank(lb)))
                .then_with(|| la.cmp(lb))
        })
        .map(|(l, _)| Route::Window(l.clone()))
        .unwrap_or_else(|| Route::NewWindow(fallback_root(path)))
}

/// How an incoming file is delivered: to a window that already contains it,
/// by repointing a placeholder "main", or to a new project window.
#[derive(Debug, PartialEq)]
pub enum Delivery {
    Existing(String),
    RetargetMain(PathBuf),
    NewWindow(PathBuf),
}

/// Route `path`, treating a placeholder "main" (bare-cwd root, often "/") as
/// containing nothing: it is left out of the candidates and, when no other
/// window takes the file, repointed at the file's root instead of opening a
/// second window.
pub fn plan_delivery(
    path: &Path,
    roots: &[(String, PathBuf)],
    mru: &[String],
    placeholder_main: bool,
    fallback_root: impl Fn(&Path) -> PathBuf,
) -> Delivery {
    let retarget = placeholder_main && roots.iter().any(|(l, _)| l == "main");
    let candidates: Vec<(String, PathBuf)> = roots
        .iter()
        .filter(|(l, _)| !(retarget && l == "main"))
        .cloned()
        .collect();
    match route(path, &candidates, mru, fallback_root) {
        Route::Window(l) => Delivery::Existing(l),
        Route::NewWindow(root) if retarget => Delivery::RetargetMain(root),
        Route::NewWindow(root) => Delivery::NewWindow(root),
    }
}

pub fn fallback_root(path: &Path) -> PathBuf {
    let dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path.to_path_buf())
    };
    let home = home_dir().map(|h| h.canonicalize().unwrap_or(h));
    let toplevel = crate::git::git_toplevel(&dir);
    project_root(dir, toplevel, home.as_deref())
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let var = std::env::var_os("USERPROFILE");
    #[cfg(not(windows))]
    let var = std::env::var_os("HOME");
    var.map(PathBuf::from)
}

/// The git toplevel above `dir`, unless it is the home directory or a
/// filesystem root (a dotfiles repo in $HOME would otherwise make every file
/// under it open with the whole home folder as its tree). `dir` itself is
/// always acceptable: it is the file's own folder.
pub fn project_root(dir: PathBuf, toplevel: Option<PathBuf>, home: Option<&Path>) -> PathBuf {
    match toplevel {
        Some(t) if t.parent().is_some() && Some(t.as_path()) != home => t,
        _ => dir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn roots(v: &[(&str, &str)]) -> Vec<(String, PathBuf)> {
        v.iter()
            .map(|(l, r)| (l.to_string(), PathBuf::from(r)))
            .collect()
    }

    fn mru(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn fb(p: &Path) -> PathBuf {
        p.parent().unwrap().to_path_buf()
    }

    #[test]
    fn routes_to_containing_window() {
        let r = roots(&[("main", "/a"), ("project-1", "/b")]);
        assert_eq!(
            route(Path::new("/b/x.md"), &r, &mru(&["main"]), fb),
            Route::Window("project-1".into())
        );
    }

    #[test]
    fn longest_root_wins_for_nested_roots() {
        let r = roots(&[("main", "/repo"), ("project-1", "/repo/docs")]);
        assert_eq!(
            route(Path::new("/repo/docs/p.md"), &r, &mru(&["main"]), fb),
            Route::Window("project-1".into())
        );
        assert_eq!(
            route(Path::new("/repo/src/p.md"), &r, &mru(&["project-1"]), fb),
            Route::Window("main".into())
        );
    }

    #[test]
    fn equal_roots_break_ties_by_mru_and_unfocused_rank_last() {
        let r = roots(&[("main", "/a"), ("project-1", "/a"), ("project-2", "/a")]);
        assert_eq!(
            route(Path::new("/a/x.md"), &r, &mru(&["project-1", "main"]), fb),
            Route::Window("project-1".into())
        );
        assert_eq!(
            route(Path::new("/a/x.md"), &r, &mru(&["project-2"]), fb),
            Route::Window("project-2".into())
        );
    }

    #[test]
    fn prefix_that_is_not_a_path_component_does_not_match() {
        let r = roots(&[("main", "/repo")]);
        assert_eq!(
            route(Path::new("/repo2/x.md"), &r, &mru(&[]), fb),
            Route::NewWindow(PathBuf::from("/repo2"))
        );
    }

    #[test]
    fn no_match_opens_new_window_at_fallback_root() {
        assert_eq!(
            route(Path::new("/z/y/x.md"), &[], &mru(&[]), fb),
            Route::NewWindow(PathBuf::from("/z/y"))
        );
    }

    #[test]
    fn placeholder_main_is_excluded_and_retargeted() {
        let r = roots(&[("main", "/")]);
        assert_eq!(
            plan_delivery(Path::new("/z/y/x.md"), &r, &mru(&["main"]), true, fb),
            Delivery::RetargetMain(PathBuf::from("/z/y"))
        );
    }

    #[test]
    fn non_placeholder_main_with_containing_root_is_existing() {
        let r = roots(&[("main", "/")]);
        assert_eq!(
            plan_delivery(Path::new("/z/y/x.md"), &r, &mru(&["main"]), false, fb),
            Delivery::Existing("main".into())
        );
    }

    #[test]
    fn no_match_without_placeholder_opens_new_window() {
        let r = roots(&[("main", "/a")]);
        assert_eq!(
            plan_delivery(Path::new("/z/y/x.md"), &r, &mru(&["main"]), false, fb),
            Delivery::NewWindow(PathBuf::from("/z/y"))
        );
    }

    #[test]
    fn placeholder_main_defers_to_another_containing_window() {
        let r = roots(&[("main", "/"), ("project-1", "/z")]);
        assert_eq!(
            plan_delivery(Path::new("/z/y/x.md"), &r, &mru(&["main"]), true, fb),
            Delivery::Existing("project-1".into())
        );
    }

    #[test]
    fn placeholder_flag_without_registered_main_opens_new_window() {
        let r = roots(&[("project-1", "/a")]);
        assert_eq!(
            plan_delivery(Path::new("/z/y/x.md"), &r, &mru(&[]), true, fb),
            Delivery::NewWindow(PathBuf::from("/z/y"))
        );
    }

    #[test]
    fn project_root_rejects_home_and_filesystem_root() {
        let home = Some(Path::new("/Users/me"));
        let dir = PathBuf::from("/Users/me/notes");
        assert_eq!(
            project_root(dir.clone(), Some(PathBuf::from("/Users/me/repo")), home),
            PathBuf::from("/Users/me/repo")
        );
        assert_eq!(
            project_root(dir.clone(), Some(PathBuf::from("/Users/me")), home),
            dir
        );
        assert_eq!(
            project_root(dir.clone(), Some(PathBuf::from("/")), home),
            dir
        );
        assert_eq!(project_root(dir.clone(), None, home), dir);
        assert_eq!(
            project_root(
                PathBuf::from("/Users/me"),
                Some(PathBuf::from("/Users/me")),
                home
            ),
            PathBuf::from("/Users/me")
        );
        assert_eq!(
            project_root(dir.clone(), Some(PathBuf::from("/Users/me")), None),
            PathBuf::from("/Users/me")
        );
    }

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
