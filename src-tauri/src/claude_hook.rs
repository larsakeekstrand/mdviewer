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

/// Ensure `settings.hooks.PostToolUse` exists as an array and return it,
/// coercing non-object/non-array intermediates and preserving other keys.
fn ensure_post_array(settings: &mut Value) -> &mut Vec<Value> {
    if !settings.is_object() {
        *settings = json!({});
    }
    let obj = settings.as_object_mut().unwrap();
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
    }
    let hooks_obj = hooks.as_object_mut().unwrap();
    let post = hooks_obj.entry("PostToolUse").or_insert_with(|| json!([]));
    if !post.is_array() {
        *post = json!([]);
    }
    post.as_array_mut().unwrap()
}

/// Merge our `Write` PostToolUse hook into a Claude Code settings document.
/// If a command containing `--claude-hook` already exists, update its path
/// (`Updated`); otherwise append a new entry (`Installed`). Other keys and
/// hooks are preserved.
pub fn merge_hook(mut settings: Value, command: &str) -> (Value, HookOutcome) {
    let outcome = {
        let arr = ensure_post_array(&mut settings);
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
    (settings, outcome)
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
        let (merged, outcome) = merge_hook(json!({}), "\"/x/mdviewer\" --claude-hook");
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
        let (merged, outcome) = merge_hook(existing, "\"/x/mdviewer\" --claude-hook");
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
        let (merged, outcome) = merge_hook(existing, "\"/new/mdviewer\" --claude-hook");
        assert_eq!(outcome, HookOutcome::Updated);
        let arr = merged["hooks"]["PostToolUse"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(
            arr[0]["hooks"][0]["command"],
            "\"/new/mdviewer\" --claude-hook"
        );
    }
}
