use std::path::{Path, PathBuf};

/// What a launch argument names once resolved against the launching cwd.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchTarget {
    Folder(PathBuf),
    File(PathBuf),
}

/// Absolutizes `raw` against `cwd`, canonicalizes and stats it. `raw` is
/// untrusted (argv, or a second instance's argv forwarded to this one).
pub fn resolve_launch_path(raw: &str, cwd: &Path) -> Result<LaunchTarget, String> {
    let path = PathBuf::from(raw);
    let absolute = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    let canonical = absolute
        .canonicalize()
        .map_err(|e| format!("cannot open '{raw}': {e}"))?;
    let meta = std::fs::metadata(&canonical).map_err(|e| format!("cannot stat '{raw}': {e}"))?;
    if meta.is_dir() {
        Ok(LaunchTarget::Folder(canonical))
    } else {
        Ok(LaunchTarget::File(canonical))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir_for(label: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mdv-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn relative_file_resolves_against_cwd() {
        let dir = tempdir_for("launch-rel");
        std::fs::write(dir.join("a.md"), "x").unwrap();
        match resolve_launch_path("a.md", &dir).unwrap() {
            LaunchTarget::File(p) => assert_eq!(p, dir.join("a.md").canonicalize().unwrap()),
            _ => panic!("expected file"),
        }
    }

    #[test]
    fn directory_resolves_to_folder() {
        let dir = tempdir_for("launch-dir");
        assert!(matches!(
            resolve_launch_path(".", &dir).unwrap(),
            LaunchTarget::Folder(_)
        ));
    }

    #[test]
    fn missing_path_errors() {
        let dir = tempdir_for("launch-missing");
        assert!(resolve_launch_path("nope.md", &dir)
            .unwrap_err()
            .contains("cannot open 'nope.md'"));
    }
}
