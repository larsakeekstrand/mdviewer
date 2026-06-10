//! Claude Code `PostToolUse` hook: matching, settings merge, and the
//! `--claude-hook` runtime that opens plan files in MDViewer. Pure helpers are
//! unit-tested; `run_hook`/`open_in_mdviewer` are IO and verified manually.

use serde::Serialize;
use serde_json::{json, Value};

/// True when `path` is a markdown file (`.md`/`.markdown`) that either has a
/// filename stem containing `plan`/`spec`/`design` (case-insensitive) or lives
/// under a directory component named exactly `plans` or `specs`.
pub fn is_plan_file(path: &str) -> bool {
    use std::path::{Component, Path};
    let p = Path::new(path);

    let ext_ok = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown"))
        .unwrap_or(false);
    if !ext_ok {
        return false;
    }

    let stem_match = p
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| {
            let s = s.to_lowercase();
            s.contains("plan") || s.contains("spec") || s.contains("design")
        })
        .unwrap_or(false);
    if stem_match {
        return true;
    }

    p.components().any(|c| match c {
        Component::Normal(os) => os
            .to_str()
            .map(|s| {
                let s = s.to_lowercase();
                s == "plans" || s == "specs"
            })
            .unwrap_or(false),
        _ => false,
    })
}

/// Result of merging the hook into a settings document.
#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookOutcome {
    Installed,
    Updated,
}

/// Navigate to `settings.hooks.PostToolUse`, creating missing intermediates as
/// empty object/array. Returns an error (rather than overwriting) if an existing
/// value has the wrong type, so we never clobber a user's settings file.
fn post_array_mut(settings: &mut Value) -> Result<&mut Vec<Value>, String> {
    if !settings.is_object() {
        return Err("settings root is not a JSON object".to_string());
    }
    let obj = settings.as_object_mut().unwrap();
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    if !hooks.is_object() {
        return Err("`hooks` is not a JSON object".to_string());
    }
    let hooks_obj = hooks.as_object_mut().unwrap();
    let post = hooks_obj.entry("PostToolUse").or_insert_with(|| json!([]));
    if !post.is_array() {
        return Err("`hooks.PostToolUse` is not a JSON array".to_string());
    }
    Ok(post.as_array_mut().unwrap())
}

/// Merge our `Write` PostToolUse hook into a Claude Code settings document.
/// If any command containing `--claude-hook` already exists, update every such
/// command's path (collapsing duplicates) and return `Updated`; otherwise append
/// a new entry and return `Installed`. Unrelated keys and hooks are preserved.
/// Errors (without modifying) if an existing `hooks`/`PostToolUse` value has an
/// unexpected type, so a user's settings file is never clobbered.
pub fn merge_hook(mut settings: Value, command: &str) -> Result<(Value, HookOutcome), String> {
    let outcome = {
        let arr = post_array_mut(&mut settings)?;
        let mut updated = false;
        for matcher_entry in arr.iter_mut() {
            if let Some(inner) = matcher_entry
                .get_mut("hooks")
                .and_then(|h| h.as_array_mut())
            {
                for hook in inner.iter_mut() {
                    let is_ours = hook
                        .get("command")
                        .and_then(|c| c.as_str())
                        .map(|c| c.contains("--claude-hook"))
                        .unwrap_or(false);
                    if is_ours {
                        hook["command"] = json!(command);
                        updated = true;
                    }
                }
            }
        }
        if updated {
            HookOutcome::Updated
        } else {
            arr.push(json!({
                "matcher": "Write",
                "hooks": [{ "type": "command", "command": command }]
            }));
            HookOutcome::Installed
        }
    };
    Ok((settings, outcome))
}

/// Extract `tool_input.file_path` from a PostToolUse hook's stdin JSON.
/// Returns `None` for malformed JSON or a missing field.
pub fn extract_file_path(stdin_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(stdin_json).ok()?;
    v.get("tool_input")?
        .get("file_path")?
        .as_str()
        .map(|s| s.to_string())
}

/// Entry point for `mdviewer --claude-hook`: read the PostToolUse JSON from
/// stdin, and if it announces a written plan/spec/design markdown file, open it
/// in MDViewer. Any error or non-match is swallowed (exit 0) so the hook never
/// disrupts Claude's tool call.
pub fn run_hook() {
    use std::io::Read as _;
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return;
    }
    let path = match extract_file_path(&input) {
        Some(p) => p,
        None => return,
    };
    if !is_plan_file(&path) {
        return;
    }
    open_in_mdviewer(&path);
}

#[cfg(target_os = "macos")]
fn open_in_mdviewer(path: &str) {
    use std::process::{Command, Stdio};
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(_) => return,
    };
    // …/MDViewer.app/Contents/MacOS/mdviewer → the .app bundle is 3 ancestors up.
    let bundle = exe
        .ancestors()
        .nth(3)
        .filter(|p| p.extension().map(|e| e == "app").unwrap_or(false));
    let mut cmd = match bundle {
        // Installed build: hand the file to the app bundle so the running
        // instance opens it (warm-open adds a tab).
        Some(app) => {
            let mut c = Command::new("open");
            c.arg("-a").arg(app).arg(path);
            c
        }
        // Dev build (target/debug/mdviewer has no .app): launch this binary
        // directly. `open -b com.mdviewer.app` would route to a stale installed
        // bundle, which is confusing during development.
        None => {
            let mut c = Command::new(&exe);
            c.arg(path);
            c
        }
    };
    let _ = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[cfg(target_os = "windows")]
fn open_in_mdviewer(path: &str) {
    use std::process::{Command, Stdio};
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(exe)
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn open_in_mdviewer(_path: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_stem_keywords() {
        assert!(is_plan_file("migration-plan.md"));
        assert!(is_plan_file("auth-design.md"));
        assert!(is_plan_file("api-spec.markdown"));
        assert!(is_plan_file("SPEC.MD"));
    }

    #[test]
    fn matches_plans_and_specs_directories() {
        assert!(is_plan_file("docs/superpowers/plans/2026-06-10-foo.md"));
        assert!(is_plan_file("docs/specs/x.md"));
    }

    #[test]
    fn rejects_non_matches() {
        assert!(!is_plan_file("README.md"));
        assert!(!is_plan_file("plan.txt"));
        assert!(!is_plan_file("plans/notes.txt"));
        assert!(!is_plan_file("templates/x.md"));
        assert!(!is_plan_file("myplans/x.md"));
    }

    #[test]
    fn extracts_file_path_from_post_tool_use_json() {
        let json =
            r#"{"tool_name":"Write","tool_input":{"file_path":"/a/b/plan.md","file_text":"x"}}"#;
        assert_eq!(extract_file_path(json).as_deref(), Some("/a/b/plan.md"));
    }

    #[test]
    fn extract_file_path_handles_missing_and_malformed() {
        assert_eq!(extract_file_path("{}"), None);
        assert_eq!(extract_file_path(r#"{"tool_input":{}}"#), None);
        assert_eq!(extract_file_path("not json"), None);
    }

    #[test]
    fn merge_into_empty_installs_full_chain() {
        let (merged, outcome) = merge_hook(json!({}), "\"/x/mdviewer\" --claude-hook").unwrap();
        assert_eq!(outcome, HookOutcome::Installed);
        let entry = &merged["hooks"]["PostToolUse"][0];
        assert_eq!(entry["matcher"], "Write");
        assert_eq!(entry["hooks"][0]["type"], "command");
        assert_eq!(
            entry["hooks"][0]["command"],
            "\"/x/mdviewer\" --claude-hook"
        );
    }

    #[test]
    fn merge_preserves_unrelated_keys_and_hooks() {
        let existing = json!({
            "permissions": {"allow": ["Bash"]},
            "hooks": {"PostToolUse": [
                {"matcher": "Edit", "hooks": [{"type": "command", "command": "echo hi"}]}
            ]}
        });
        let (merged, outcome) = merge_hook(existing, "\"/x/mdviewer\" --claude-hook").unwrap();
        assert_eq!(outcome, HookOutcome::Installed);
        assert_eq!(merged["permissions"]["allow"][0], "Bash");
        let arr = merged["hooks"]["PostToolUse"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["hooks"][0]["command"], "echo hi");
        assert_eq!(
            arr[1]["hooks"][0]["command"],
            "\"/x/mdviewer\" --claude-hook"
        );
    }

    #[test]
    fn merge_updates_existing_mdviewer_hook_in_place() {
        let existing = json!({
            "hooks": {"PostToolUse": [
                {"matcher": "Write", "hooks": [{"type": "command", "command": "\"/old/mdviewer\" --claude-hook"}]}
            ]}
        });
        let (merged, outcome) = merge_hook(existing, "\"/new/mdviewer\" --claude-hook").unwrap();
        assert_eq!(outcome, HookOutcome::Updated);
        let arr = merged["hooks"]["PostToolUse"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(
            arr[0]["hooks"][0]["command"],
            "\"/new/mdviewer\" --claude-hook"
        );
    }

    #[test]
    fn merge_creates_post_tool_use_when_only_other_hook_kinds_exist() {
        let existing = json!({ "hooks": { "PreToolUse": [] } });
        let (merged, outcome) = merge_hook(existing, "\"/x/mdviewer\" --claude-hook").unwrap();
        assert_eq!(outcome, HookOutcome::Installed);
        assert_eq!(merged["hooks"]["PreToolUse"], json!([])); // preserved
        assert!(merged["hooks"]["PostToolUse"].is_array());
        assert_eq!(merged["hooks"]["PostToolUse"][0]["matcher"], "Write");
    }

    #[test]
    fn merge_refuses_when_post_tool_use_is_wrong_type() {
        let existing = json!({ "hooks": { "PostToolUse": "oops-not-an-array" } });
        let result = merge_hook(existing, "\"/x/mdviewer\" --claude-hook");
        assert!(result.is_err());
    }
}
