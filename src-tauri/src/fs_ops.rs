use std::path::{Path, PathBuf};

/// Validate a user-entered file or folder name (from the inline-rename input).
/// Rejects empties, path separators, and the `.`/`..` traversal names. The
/// frontend validates too (immediate feedback); this is the authoritative
/// backend guard.
pub fn validate_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if trimmed == "." || trimmed == ".." {
        return Err("invalid name".to_string());
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err("name cannot contain path separators".to_string());
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err("name cannot contain control characters".to_string());
    }
    Ok(())
}

/// Split a file name into (stem, extension). A leading-dot name with no other
/// dot (".gitignore") is all stem, no extension. "a.tar.gz" -> ("a.tar", "gz").
fn split_ext(name: &str) -> (&str, Option<&str>) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], Some(&name[i + 1..])),
        _ => (name, None),
    }
}

/// The nth duplicate candidate name: n=1 -> "note copy.md", n=2 -> "note copy 2.md".
pub fn duplicate_candidate(name: &str, n: usize) -> String {
    let (stem, ext) = split_ext(name);
    let suffix = if n <= 1 {
        " copy".to_string()
    } else {
        format!(" copy {n}")
    };
    match ext {
        Some(e) => format!("{stem}{suffix}.{e}"),
        None => format!("{stem}{suffix}"),
    }
}

/// The nearest ancestor of `path` (including itself) that exists on disk.
fn nearest_existing(path: &Path) -> Option<PathBuf> {
    let mut p: &Path = path;
    loop {
        if p.exists() {
            return Some(p.to_path_buf());
        }
        p = p.parent()?;
    }
}

/// Whether `path` (or, for a not-yet-created path, its nearest existing
/// ancestor) resolves inside `root`. Canonicalizing resolves symlinks, so an
/// in-tree symlink pointing outside is rejected. `starts_with` is component-wise
/// (so `/work` never matches `/work-x`).
pub fn within_root(path: &Path, root: &Path) -> bool {
    match (
        nearest_existing(path).and_then(|p| std::fs::canonicalize(p).ok()),
        std::fs::canonicalize(root).ok(),
    ) {
        (Some(p), Some(r)) => p.starts_with(&r),
        _ => false,
    }
}

/// Create an empty file `dir/name`. Fails if it already exists (atomic via
/// create_new). `name` must already be validated by the caller.
pub fn create_file(dir: &Path, name: &str) -> Result<PathBuf, String> {
    validate_name(name)?;
    let target = dir.join(name);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|e| format!("cannot create '{}': {e}", target.display()))?;
    Ok(target)
}

/// Create a folder `dir/name`. Fails if it already exists.
pub fn create_folder(dir: &Path, name: &str) -> Result<PathBuf, String> {
    validate_name(name)?;
    let target = dir.join(name);
    if target.exists() {
        return Err(format!("'{}' already exists", target.display()));
    }
    std::fs::create_dir(&target)
        .map_err(|e| format!("cannot create folder '{}': {e}", target.display()))?;
    Ok(target)
}

/// Rename `from` -> `to`. Refuses to overwrite an existing destination.
pub fn rename_path(from: &Path, to: &Path) -> Result<(), String> {
    if to.exists() {
        return Err(format!("'{}' already exists", to.display()));
    }
    std::fs::rename(from, to).map_err(|e| format!("cannot rename '{}': {e}", from.display()))
}

/// Copy `path` to the first free "name copy"/"name copy N" sibling.
pub fn duplicate_file(path: &Path) -> Result<PathBuf, String> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "invalid file name".to_string())?;
    for n in 1..=10_000 {
        let candidate = dir.join(duplicate_candidate(name, n));
        if !candidate.exists() {
            std::fs::copy(path, &candidate)
                .map_err(|e| format!("cannot duplicate '{}': {e}", path.display()))?;
            return Ok(candidate);
        }
    }
    Err("too many copies".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_name_accepts_ordinary_names() {
        assert!(validate_name("notes.md").is_ok());
        assert!(validate_name(".gitignore").is_ok());
        assert!(validate_name("My File 2.txt").is_ok());
    }

    #[test]
    fn validate_name_rejects_bad_names() {
        assert!(validate_name("").is_err());
        assert!(validate_name("   ").is_err());
        assert!(validate_name(".").is_err());
        assert!(validate_name("..").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("a\\b").is_err());
        assert!(validate_name("a\nb").is_err());
    }

    #[test]
    fn duplicate_candidate_handles_extensions_and_dotfiles() {
        assert_eq!(duplicate_candidate("note.md", 1), "note copy.md");
        assert_eq!(duplicate_candidate("note.md", 2), "note copy 2.md");
        assert_eq!(
            duplicate_candidate("archive.tar.gz", 1),
            "archive.tar copy.gz"
        );
        assert_eq!(duplicate_candidate("README", 1), "README copy");
        assert_eq!(duplicate_candidate(".gitignore", 1), ".gitignore copy");
    }

    fn tmp(label: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mdv-fsops-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn within_root_accepts_inside_and_rejects_outside() {
        let root = tmp("within");
        let inside = root.join("sub");
        std::fs::create_dir_all(&inside).unwrap();
        let new_child = inside.join("new.md"); // doesn't exist yet
        assert!(within_root(&new_child, &root)); // nearest existing ancestor is inside
        assert!(within_root(&inside, &root));
        assert!(!within_root(Path::new("/etc/passwd"), &root));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn create_file_rejects_existing() {
        let root = tmp("createfile");
        let p = create_file(&root, "a.md").unwrap();
        assert!(p.is_file());
        assert!(create_file(&root, "a.md").is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn create_folder_rejects_existing() {
        let root = tmp("createdir");
        let p = create_folder(&root, "sub").unwrap();
        assert!(p.is_dir());
        assert!(create_folder(&root, "sub").is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rename_rejects_existing_destination() {
        let root = tmp("rename");
        let a = create_file(&root, "a.md").unwrap();
        let _b = create_file(&root, "b.md").unwrap();
        assert!(rename_path(&a, &root.join("b.md")).is_err());
        assert!(rename_path(&a, &root.join("c.md")).is_ok());
        assert!(root.join("c.md").is_file());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn duplicate_picks_a_free_name() {
        let root = tmp("dup");
        let a = create_file(&root, "a.md").unwrap();
        std::fs::write(&a, b"hello").unwrap();
        let d1 = duplicate_file(&a).unwrap();
        assert_eq!(d1.file_name().unwrap(), "a copy.md");
        assert_eq!(std::fs::read_to_string(&d1).unwrap(), "hello");
        let d2 = duplicate_file(&a).unwrap();
        assert_eq!(d2.file_name().unwrap(), "a copy 2.md");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
