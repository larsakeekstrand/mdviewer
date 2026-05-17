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

fn paths_match(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a.file_name() == b.file_name() && a.parent() == b.parent(),
    }
}
