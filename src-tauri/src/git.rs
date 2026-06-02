use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

#[derive(Serialize, Default)]
pub struct GitStatusReport {
    /// Absolute path of the repo root. `None` when `dir` isn't inside a working tree.
    pub repo_root: Option<String>,
    /// Map of absolute path → 2-char porcelain code (e.g. " M", "A ", "??", "MM").
    pub entries: HashMap<String, String>,
}

/// Runs `git status` in `dir` and returns absolute-path → status-code mapping.
/// Returns an empty report (no `repo_root`) when `dir` isn't a git working tree,
/// or when `git` isn't on PATH. Both are non-errors — a folder without git is
/// the normal case for a markdown viewer.
pub fn status(dir: &Path) -> Result<GitStatusReport, String> {
    if !dir.is_dir() {
        return Err(format!("not a directory: {}", dir.display()));
    }

    let toplevel = match git_toplevel(dir) {
        Some(p) => p,
        None => return Ok(GitStatusReport::default()),
    };

    let output = hardened_git(dir)
        .args([
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
        ])
        .output()
        .map_err(|e| format!("failed to run git status: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "git status exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let entries = parse_porcelain(&output.stdout, &toplevel);
    Ok(GitStatusReport {
        repo_root: Some(toplevel.to_string_lossy().into_owned()),
        entries,
    })
}

/// A `git` command pre-hardened against malicious repositories.
///
/// A folder opened in the viewer is untrusted: it may be a hostile git repo
/// whose config weaponizes git. The classic vector is `core.fsmonitor`, which
/// `git status` executes as an arbitrary command. We neutralize it by:
/// - suppressing system (`/etc/gitconfig`) and global (`~/.gitconfig`) config,
///   so only the explicit overrides below and repo-local config are read;
/// - forcing `core.fsmonitor` and `core.hooksPath` to inert values via
///   command-line `-c`, which is git's *highest-precedence* config source — it
///   overrides repo-local values AND anything a repo-local `include.path`
///   pulls in;
/// - `--no-optional-locks`, so a read-only status never writes the index (which
///   could otherwise trigger index-change hooks).
fn hardened_git(dir: &Path) -> Command {
    let null_device = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let mut cmd = Command::new("git");
    cmd.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", null_device)
        .arg("-c")
        .arg("core.fsmonitor=")
        .arg("-c")
        .arg(format!("core.hooksPath={null_device}"))
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(dir);
    cmd
}

/// Resolve `dir`'s git working-tree root. `None` means "not inside a repo"
/// (or git is unavailable) — caller treats it the same way.
fn git_toplevel(dir: &Path) -> Option<PathBuf> {
    let output = hardened_git(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8(output.stdout).ok()?;
    let trimmed = s.trim_end_matches('\n').trim_end_matches('\r');
    if trimmed.is_empty() {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

/// Parse `git status --porcelain=v1 -z` output into absolute-path → code pairs.
///
/// Each record is `XY SP path NUL`. With `--no-renames` we never need to consume
/// a paired original path, so the parse stays a straight forward NUL-split.
fn parse_porcelain(bytes: &[u8], repo_root: &Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for record in bytes.split(|b| *b == 0) {
        if record.len() < 4 {
            continue;
        }
        // Layout: byte 0 = staged code, 1 = worktree code, 2 = space, 3.. = path.
        let code = String::from_utf8_lossy(&record[..2]).into_owned();
        let path_bytes = &record[3..];
        let rel = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            // git emits raw bytes for non-UTF8 paths; we don't try to display them.
            Err(_) => continue,
        };
        let abs = repo_root.join(rel);
        out.insert(abs.to_string_lossy().into_owned(), code);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardened_git_neutralizes_config_exec_vectors() {
        let cmd = hardened_git(Path::new("/untrusted/repo"));

        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        // `core.fsmonitor` is the confirmed RCE vector that `git status` runs.
        assert!(
            args.windows(2)
                .any(|w| w[0] == "-c" && w[1] == "core.fsmonitor="),
            "fsmonitor must be disabled via -c; got {args:?}"
        );
        assert!(
            args.windows(2)
                .any(|w| w[0] == "-c" && w[1].starts_with("core.hooksPath=")),
            "hooksPath must be redirected via -c; got {args:?}"
        );
        assert!(
            args.iter().any(|a| a == "--no-optional-locks"),
            "must pass --no-optional-locks; got {args:?}"
        );

        let envs: std::collections::HashMap<String, Option<String>> = cmd
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|s| s.to_string_lossy().into_owned()),
                )
            })
            .collect();
        assert_eq!(
            envs.get("GIT_CONFIG_NOSYSTEM"),
            Some(&Some("1".to_string())),
            "system config must be suppressed"
        );
        assert!(
            envs.contains_key("GIT_CONFIG_GLOBAL"),
            "global config must be suppressed"
        );
    }

    #[test]
    fn parses_modified_added_untracked() {
        // Build the exact wire format git emits.
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(b" M src/foo.rs");
        bytes.push(0);
        bytes.extend_from_slice(b"A  src/bar.rs");
        bytes.push(0);
        bytes.extend_from_slice(b"?? new.md");
        bytes.push(0);
        bytes.extend_from_slice(b"MM both.rs");
        bytes.push(0);

        let repo = PathBuf::from("/repo");
        let map = parse_porcelain(&bytes, &repo);

        let key = |rel: &str| repo.join(rel).to_string_lossy().into_owned();
        assert_eq!(map.get(&key("src/foo.rs")).map(|s| s.as_str()), Some(" M"));
        assert_eq!(map.get(&key("src/bar.rs")).map(|s| s.as_str()), Some("A "));
        assert_eq!(map.get(&key("new.md")).map(|s| s.as_str()), Some("??"));
        assert_eq!(map.get(&key("both.rs")).map(|s| s.as_str()), Some("MM"));
    }

    #[test]
    fn ignores_truncated_records() {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(b"ab");
        bytes.push(0);
        bytes.extend_from_slice(b" M ok.rs");
        bytes.push(0);

        let repo = Path::new("/r");
        let map = parse_porcelain(&bytes, repo);
        assert_eq!(map.len(), 1);
        assert!(map.contains_key(repo.join("ok.rs").to_string_lossy().as_ref()));
    }

    #[test]
    fn handles_paths_with_spaces() {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(b" M docs/My Notes.md");
        bytes.push(0);
        let repo = Path::new("/r");
        let map = parse_porcelain(&bytes, repo);
        let key = repo.join("docs/My Notes.md").to_string_lossy().into_owned();
        assert_eq!(map.get(&key).map(|s| s.as_str()), Some(" M"));
    }
}
