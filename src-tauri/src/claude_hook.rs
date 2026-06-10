//! Claude Code `PostToolUse` hook: matching, settings merge, and the
//! `--claude-hook` runtime that opens plan files in MDViewer. Pure helpers are
//! unit-tested; `run_hook`/`open_in_mdviewer` are IO and verified manually.

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
        let json = r#"{"tool_name":"Write","tool_input":{"file_path":"/a/b/plan.md","file_text":"x"}}"#;
        assert_eq!(extract_file_path(json).as_deref(), Some("/a/b/plan.md"));
    }

    #[test]
    fn extract_file_path_handles_missing_and_malformed() {
        assert_eq!(extract_file_path("{}"), None);
        assert_eq!(extract_file_path(r#"{"tool_input":{}}"#), None);
        assert_eq!(extract_file_path("not json"), None);
    }
}
