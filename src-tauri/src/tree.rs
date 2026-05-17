use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use serde::Serialize;

#[derive(Serialize)]
pub struct TreeEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

/// Lists immediate children of `dir`, gitignore-aware, hidden files filtered.
/// Dotfiles and common build directories (`node_modules`, `target`) are skipped.
/// Directories are returned first, then files; both sorted alphabetically.
pub fn list_directory(dir: &Path) -> Result<Vec<TreeEntry>, String> {
    if !dir.is_dir() {
        return Err(format!("not a directory: {}", dir.display()));
    }

    let walker = WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            if name == "node_modules" || name == "target" {
                return false;
            }
            true
        })
        .build();

    let mut entries: Vec<TreeEntry> = walker
        .filter_map(|res| res.ok())
        .filter(|entry| entry.path() != dir)
        .map(|entry| {
            let path = entry.path().to_path_buf();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            TreeEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: path_to_string(&path),
                is_dir,
            }
        })
        .collect();

    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    Ok(entries)
}

fn path_to_string(p: &PathBuf) -> String {
    p.to_string_lossy().into_owned()
}
