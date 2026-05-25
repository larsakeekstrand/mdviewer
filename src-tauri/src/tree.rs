use std::path::Path;

use serde::Serialize;

#[derive(Serialize)]
pub struct TreeEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

/// Lists every immediate child of `dir`, unfiltered: hidden dotfiles,
/// git-ignored files, and heavy build directories (`node_modules`, `target`)
/// are all included.
/// Directories are returned first, then files; both sorted alphabetically.
pub fn list_directory(dir: &Path) -> Result<Vec<TreeEntry>, String> {
    if !dir.is_dir() {
        return Err(format!("not a directory: {}", dir.display()));
    }

    let read = std::fs::read_dir(dir).map_err(|e| format!("read dir {}: {e}", dir.display()))?;

    let mut entries: Vec<TreeEntry> = read
        .filter_map(|res| res.ok())
        .map(|entry| {
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            TreeEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry.path().to_string_lossy().into_owned(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn unique_temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "mdviewer_tree_test_{}_{nanos}_{n}",
            std::process::id()
        ));
        fs::create_dir(&dir).unwrap();
        dir
    }

    #[test]
    fn lists_everything_including_hidden_ignored_and_heavy_dirs() {
        let dir = unique_temp_dir();
        fs::write(dir.join("a.md"), "x").unwrap();
        fs::write(dir.join(".hidden"), "x").unwrap();
        fs::write(dir.join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(dir.join("ignored.txt"), "x").unwrap();
        fs::create_dir(dir.join("node_modules")).unwrap();
        fs::create_dir(dir.join("target")).unwrap();
        fs::create_dir(dir.join("sub")).unwrap();

        let names: HashSet<String> = list_directory(&dir)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();

        for expected in [
            "a.md",
            ".hidden",
            ".gitignore",
            "ignored.txt",
            "node_modules",
            "target",
            "sub",
        ] {
            assert!(names.contains(expected), "{expected} should be listed");
        }

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn directories_sort_before_files_alphabetically() {
        let dir = unique_temp_dir();
        fs::write(dir.join("zeta.md"), "x").unwrap();
        fs::write(dir.join("alpha.md"), "x").unwrap();
        fs::create_dir(dir.join("zed")).unwrap();
        fs::create_dir(dir.join("apex")).unwrap();

        let order: Vec<String> = list_directory(&dir)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(order, vec!["apex", "zed", "alpha.md", "zeta.md"]);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn errors_on_non_directory() {
        let dir = unique_temp_dir();
        let file = dir.join("file.md");
        fs::write(&file, "x").unwrap();
        assert!(list_directory(&file).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
