use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use tauri::{AppHandle, Emitter};

type DebouncerType = Debouncer<RecommendedWatcher, RecommendedCache>;

#[derive(Default)]
pub struct WatcherSlot {
    debouncer: Option<DebouncerType>,
    watched_file: Option<PathBuf>,
}

impl WatcherSlot {
    /// Replaces any active watcher with one watching the parent directory of `file`.
    /// Editor save patterns (atomic write-and-rename) can orphan path-level watchers,
    /// so we watch the directory and filter events down to the file we care about.
    pub fn watch_file(&mut self, app: &AppHandle, file: &Path) -> Result<(), String> {
        // Drop any existing watcher first; some platforms refuse to re-watch.
        self.debouncer.take();
        self.watched_file = None;

        let canonical = file
            .canonicalize()
            .map_err(|e| format!("canonicalize failed: {e}"))?;
        let parent = canonical
            .parent()
            .ok_or_else(|| "file has no parent directory".to_string())?
            .to_path_buf();

        let watched_file = canonical.clone();
        let app_handle = app.clone();

        let mut debouncer = new_debouncer(
            Duration::from_millis(200),
            None,
            move |result: DebounceEventResult| {
                let events = match result {
                    Ok(events) => events,
                    Err(_errors) => return,
                };
                let touches_target = events
                    .iter()
                    .flat_map(|ev| ev.paths.iter())
                    .any(|p| paths_match(p, &watched_file));
                if touches_target {
                    let payload = watched_file.to_string_lossy().into_owned();
                    let _ = app_handle.emit("file-changed", payload);
                }
            },
        )
        .map_err(|e| format!("watcher init failed: {e}"))?;

        debouncer
            .watch(&parent, RecursiveMode::NonRecursive)
            .map_err(|e| format!("watch failed: {e}"))?;

        self.debouncer = Some(debouncer);
        self.watched_file = Some(canonical);
        Ok(())
    }
}

#[derive(Default)]
pub struct TreeWatcherSlot {
    debouncer: Option<DebouncerType>,
}

impl TreeWatcherSlot {
    /// Replaces any active tree watcher with one watching each directory in
    /// `dirs` non-recursively. Any event under a watched directory emits a
    /// single `tree-changed`; the frontend re-reads and reconciles the affected
    /// listings. Watching is best-effort per directory — a vanished or
    /// unreadable directory is skipped rather than failing the whole call, so a
    /// folder deleted out from under us doesn't break refresh for the rest.
    pub fn watch_dirs(&mut self, app: &AppHandle, dirs: Vec<PathBuf>) -> Result<(), String> {
        // Drop the old watcher first; some platforms refuse to re-watch.
        self.debouncer.take();
        if dirs.is_empty() {
            return Ok(());
        }

        let app_handle = app.clone();
        let mut debouncer = new_debouncer(
            Duration::from_millis(200),
            None,
            move |result: DebounceEventResult| {
                if let Ok(events) = result {
                    if !events.is_empty() {
                        let _ = app_handle.emit("tree-changed", ());
                    }
                }
            },
        )
        .map_err(|e| format!("tree watcher init failed: {e}"))?;

        for dir in &dirs {
            let _ = debouncer.watch(dir, RecursiveMode::NonRecursive);
        }

        self.debouncer = Some(debouncer);
        Ok(())
    }
}

fn paths_match(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a.file_name() == b.file_name() && a.parent() == b.parent(),
    }
}
