# Install Claude Code Hook Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a **MDViewer ▸ Install Claude Code Hook…** menu item that writes a `PostToolUse` hook into the open project's `.claude/settings.local.json`, so markdown plan/spec/design files Claude Code writes there auto-open in MDViewer.

**Architecture:** A new `src-tauri/src/claude_hook.rs` holds pure, unit-tested logic (`is_plan_file`, `extract_file_path`, `merge_hook` + `HookOutcome`) plus the IO `run_hook`/`open_in_mdviewer`. The hook command MDViewer writes is its own binary + `--claude-hook`; `main.rs` intercepts that flag before the GUI starts, reads the PostToolUse JSON from stdin, and opens matching files via the OS opener (macOS `open -a` → the existing warm-open tab path). The installer is a thin `commands::install_claude_hook` that merges the hook via `merge_hook` and writes atomically with the shared `write_atomically`. Menu + frontend mirror the existing `install_cli` flow.

**Tech Stack:** Rust (Tauri 2, `serde_json`), `cargo test` for Rust unit tests, vanilla JS frontend (`window.__TAURI__.dialog`). No new crates (`serde_json` is already a dependency).

---

## File structure

- **Create `src-tauri/src/claude_hook.rs`** — pure helpers (`is_plan_file`, `extract_file_path`, `merge_hook`, `HookOutcome`) with `#[cfg(test)]` tests, plus `run_hook` + platform `open_in_mdviewer` (IO, untested).
- **Modify `src-tauri/src/main.rs`** — early `--claude-hook` branch before GUI launch.
- **Modify `src-tauri/src/lib.rs`** — `mod claude_hook;`, `pub fn run_claude_hook()`, register `commands::install_claude_hook` in the handler.
- **Modify `src-tauri/src/commands.rs`** — `install_claude_hook` command (uses `current_root`, `write_atomically`, `claude_hook::merge_hook`).
- **Modify `src-tauri/src/menu.rs`** — the menu item + `menu-install-claude-hook` event.
- **Modify `ui/app.js`** — listen for the event, call the command, show result dialogs.

## Conventions

- Rust unit tests live in `#[cfg(test)] mod tests` at the bottom of the file (like `fs_ops.rs`, `tasklist.rs`). Run with `cd src-tauri && cargo test claude_hook`.
- Lint gate before commit: `cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings`.
- Tauri commands return `Result<T, String>`; build error strings with `format!("…: {e}")`.
- Frontend + Rust changes both need `cargo build` to take effect (Tauri bundles `frontendDist` at compile time).
- No `Co-Authored-By` trailer. Commit after each task.

---

## Task 1: `is_plan_file` — match markdown plan/spec/design files

**Files:**
- Create: `src-tauri/src/claude_hook.rs`

- [ ] **Step 1: Write the failing test**

Create `src-tauri/src/claude_hook.rs` with only the test module and a stub:

```rust
//! Claude Code `PostToolUse` hook: matching, settings merge, and the
//! `--claude-hook` runtime that opens plan files in MDViewer. Pure helpers are
//! unit-tested; `run_hook`/`open_in_mdviewer` are IO and verified manually.

pub fn is_plan_file(_path: &str) -> bool {
    unimplemented!()
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
        assert!(!is_plan_file("plan.txt")); // wrong extension
        assert!(!is_plan_file("plans/notes.txt")); // wrong extension even under plans/
        assert!(!is_plan_file("templates/x.md")); // templates != plans/specs
        assert!(!is_plan_file("myplans/x.md")); // myplans != plans (exact component)
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test claude_hook 2>&1 | tail -20`
Expected: build fails or tests panic — `claude_hook` is not yet a module of the crate. (It becomes a module in Task 5; to run tests now, temporarily add `mod claude_hook;` to `src-tauri/src/lib.rs` near the other `mod` lines if needed — Task 5 makes it permanent. If you add it temporarily, keep it.)

To make the test runnable immediately, add this line to `src-tauri/src/lib.rs` among the `mod` declarations (lines 1-13):

```rust
mod claude_hook;
```

Re-run: `cd src-tauri && cargo test claude_hook 2>&1 | tail -20`
Expected: FAIL — `not yet implemented` panic from `unimplemented!()`.

- [ ] **Step 3: Implement**

Replace the `is_plan_file` stub:

```rust
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test claude_hook 2>&1 | tail -20`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/claude_hook.rs src-tauri/src/lib.rs
git commit -m "Add claude_hook::is_plan_file (match plan/spec/design markdown)"
```

---

## Task 2: `extract_file_path` — pull the written path from hook JSON

**Files:**
- Modify: `src-tauri/src/claude_hook.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `claude_hook.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test claude_hook 2>&1 | tail -20`
Expected: FAIL — `cannot find function extract_file_path`.

- [ ] **Step 3: Implement**

Add to `claude_hook.rs` (above the `tests` module):

```rust
/// Extract `tool_input.file_path` from a PostToolUse hook's stdin JSON.
/// Returns `None` for malformed JSON or a missing field.
pub fn extract_file_path(stdin_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(stdin_json).ok()?;
    v.get("tool_input")?
        .get("file_path")?
        .as_str()
        .map(|s| s.to_string())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test claude_hook 2>&1 | tail -20`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/claude_hook.rs
git commit -m "Add claude_hook::extract_file_path"
```

---

## Task 3: `merge_hook` + `HookOutcome` — idempotent settings merge

**Files:**
- Modify: `src-tauri/src/claude_hook.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module:

```rust
    use serde_json::json;

    #[test]
    fn merge_into_empty_installs_full_chain() {
        let (merged, outcome) = merge_hook(json!({}), "\"/x/mdviewer\" --claude-hook");
        assert_eq!(outcome, HookOutcome::Installed);
        let entry = &merged["hooks"]["PostToolUse"][0];
        assert_eq!(entry["matcher"], "Write");
        assert_eq!(entry["hooks"][0]["type"], "command");
        assert_eq!(entry["hooks"][0]["command"], "\"/x/mdviewer\" --claude-hook");
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
        // unrelated top-level key kept
        assert_eq!(merged["permissions"]["allow"][0], "Bash");
        // original hook kept, ours appended
        let arr = merged["hooks"]["PostToolUse"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["hooks"][0]["command"], "echo hi");
        assert_eq!(arr[1]["hooks"][0]["command"], "\"/x/mdviewer\" --claude-hook");
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
        assert_eq!(arr.len(), 1); // no duplicate
        assert_eq!(arr[0]["hooks"][0]["command"], "\"/new/mdviewer\" --claude-hook");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test claude_hook 2>&1 | tail -20`
Expected: FAIL — `cannot find type HookOutcome` / `cannot find function merge_hook`.

- [ ] **Step 3: Implement**

Add to `claude_hook.rs` (above the `tests` module). Note the `use` lines go at the top of the file with the others:

```rust
use serde::Serialize;
use serde_json::{json, Value};
```

```rust
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
    let post = hooks_obj
        .entry("PostToolUse")
        .or_insert_with(|| json!([]));
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
            if let Some(inner) = matcher_entry.get_mut("hooks").and_then(|h| h.as_array_mut()) {
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test claude_hook 2>&1 | tail -20`
Expected: PASS (8 tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/claude_hook.rs
git commit -m "Add claude_hook::merge_hook + HookOutcome (idempotent settings merge)"
```

---

## Task 4: `run_hook` + `open_in_mdviewer` — the `--claude-hook` runtime

**Files:**
- Modify: `src-tauri/src/claude_hook.rs`

No unit test (pure IO / process spawning); verified in the Task 9 smoke test. The logic it depends on (`extract_file_path`, `is_plan_file`) is already tested.

- [ ] **Step 1: Implement `run_hook` and the platform openers**

Add to `claude_hook.rs` (above the `tests` module):

```rust
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
    use std::process::Command;
    // current_exe is …/MDViewer.app/Contents/MacOS/mdviewer; the .app is 3 up.
    let bundle = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.ancestors().nth(3).map(|p| p.to_path_buf()))
        .filter(|p| p.extension().map(|e| e == "app").unwrap_or(false));
    let mut cmd = Command::new("open");
    match bundle {
        Some(app) => {
            cmd.arg("-a").arg(app);
        }
        None => {
            cmd.arg("-b").arg("com.mdviewer.app");
        }
    }
    cmd.arg(path);
    let _ = cmd.spawn();
}

#[cfg(target_os = "windows")]
fn open_in_mdviewer(path: &str) {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).arg(path).spawn();
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn open_in_mdviewer(_path: &str) {}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd src-tauri && cargo build 2>&1 | tail -5`
Expected: `Finished` (warnings about `run_hook`/`open_in_mdviewer` being unused are OK until Task 5 wires them).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/claude_hook.rs
git commit -m "Add claude_hook::run_hook + open_in_mdviewer (--claude-hook runtime)"
```

---

## Task 5: Wire `--claude-hook` into `main.rs` and expose it from `lib.rs`

**Files:**
- Modify: `src-tauri/src/lib.rs` (confirm `mod claude_hook;` from Task 1; add `pub fn run_claude_hook`)
- Modify: `src-tauri/src/main.rs:57-69` (the `main` fn)

- [ ] **Step 1: Expose the hook runner from `lib.rs`**

Confirm `mod claude_hook;` is present among the `mod` lines (added in Task 1). Then add this public wrapper near `pub fn run` (the existing top-level fn around `lib.rs:45`):

```rust
/// Run the `--claude-hook` PostToolUse handler and return (never launches the GUI).
pub fn run_claude_hook() {
    claude_hook::run_hook();
}
```

- [ ] **Step 2: Intercept the flag in `main`**

In `src-tauri/src/main.rs`, change the `main` function so it short-circuits on the flag **before** parsing args / launching the GUI. Replace:

```rust
fn main() -> ExitCode {
    let startup = match resolve_args() {
```

with:

```rust
fn main() -> ExitCode {
    if std::env::args().any(|a| a == "--claude-hook") {
        mdviewer_lib::run_claude_hook();
        return ExitCode::SUCCESS;
    }
    let startup = match resolve_args() {
```

- [ ] **Step 3: Verify it builds**

Run: `cd src-tauri && cargo build 2>&1 | tail -5`
Expected: `Finished` (the unused-warnings from Task 4 are gone now).

- [ ] **Step 4: Manually verify the hook runtime end-to-end (no GUI launch)**

Run (pipes a fake PostToolUse payload; on macOS this will try to `open -a` — that's fine, it may open MDViewer):

```bash
cd src-tauri
echo '{"tool_input":{"file_path":"/tmp/does-not-exist-plan.md"}}' | cargo run -q -- --claude-hook; echo "exit=$?"
```

Expected: `exit=0`, returns immediately, and does NOT open the main MDViewer tree window from this invocation (it only attempts to `open` the path). Then verify a non-match is silent:

```bash
echo '{"tool_input":{"file_path":"/tmp/README.txt"}}' | cargo run -q -- --claude-hook; echo "exit=$?"
```

Expected: `exit=0`, nothing opened.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/main.rs src-tauri/src/lib.rs
git commit -m "Intercept --claude-hook in main before GUI launch"
```

---

## Task 6: `install_claude_hook` command

**Files:**
- Modify: `src-tauri/src/commands.rs` (add the command; it uses the existing private `current_root` and `write_atomically`)
- Modify: `src-tauri/src/lib.rs:89` area (register in `generate_handler!`)

- [ ] **Step 1: Add the command**

In `src-tauri/src/commands.rs`, add (place it near `install_cli`; it can reference `crate::claude_hook`):

```rust
/// Merge the MDViewer `--claude-hook` PostToolUse hook into the open project's
/// `.claude/settings.local.json`. Idempotent: updates an existing entry's path
/// rather than duplicating. The target dir is the current sidebar root.
#[tauri::command]
pub fn install_claude_hook(state: State<'_, AppState>) -> Result<crate::claude_hook::HookOutcome, String> {
    let root = current_root(&state)?;
    let exe =
        std::env::current_exe().map_err(|e| format!("cannot resolve app binary path: {e}"))?;
    let command = format!("\"{}\" --claude-hook", exe.display());

    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir)
        .map_err(|e| format!("cannot create {}: {e}", claude_dir.display()))?;
    let settings_path = claude_dir.join("settings.local.json");

    let existing: serde_json::Value = match std::fs::read_to_string(&settings_path) {
        Ok(s) if !s.trim().is_empty() => serde_json::from_str(&s).map_err(|e| {
            format!(
                "{} is not valid JSON; not modified ({e})",
                settings_path.display()
            )
        })?,
        _ => serde_json::json!({}),
    };

    let (merged, outcome) = crate::claude_hook::merge_hook(existing, &command);
    let bytes = serde_json::to_vec_pretty(&merged)
        .map_err(|e| format!("cannot serialize settings: {e}"))?;
    write_atomically(&settings_path, &bytes)
        .map_err(|e| format!("cannot write {}: {e}", settings_path.display()))?;
    Ok(outcome)
}
```

- [ ] **Step 2: Register the command**

In `src-tauri/src/lib.rs`, in the `tauri::generate_handler![ … ]` list (near `commands::install_cli` at line ~89), add a line:

```rust
            commands::install_claude_hook,
```

- [ ] **Step 3: Verify it builds**

Run: `cd src-tauri && cargo build 2>&1 | tail -5`
Expected: `Finished`.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "Add install_claude_hook command (merge hook into settings.local.json)"
```

---

## Task 7: Menu item + event

**Files:**
- Modify: `src-tauri/src/menu.rs` (item build + add to app menu + event arm)

- [ ] **Step 1: Build the menu item and add it to the app menu**

In `src-tauri/src/menu.rs`, after the `install_cli` item block (around line 100-102), add an **un-gated** item (it works on both platforms):

```rust
    let install_claude_hook = MenuItemBuilder::with_id(
        "install-claude-hook",
        "Install Claude Code Hook…",
    )
    .build(app)?;
```

Then add it to the app menu builder. The current code (around line 110-112) is:

```rust
    let app_menu_builder = SubmenuBuilder::new(app, "MDViewer")
        .about(None)
        .item(&github_source)
        .item(&check_updates);
    #[cfg(target_os = "macos")]
    let app_menu_builder = app_menu_builder.item(&install_cli);
```

Add the new item right after the macOS-gated `install_cli` line:

```rust
    let app_menu_builder = app_menu_builder.item(&install_claude_hook);
```

(Unconditional — no `#[cfg]`.)

- [ ] **Step 2: Add the menu-event arm**

In the `on_menu_event` match (where `"install-cli" => { let _ = app.emit("menu-install-cli", ()); }` is, around line 31), add an arm:

```rust
            "install-claude-hook" => {
                let _ = app.emit("menu-install-claude-hook", ());
            }
```

- [ ] **Step 3: Verify it builds**

Run: `cd src-tauri && cargo build 2>&1 | tail -5`
Expected: `Finished`.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/menu.rs
git commit -m "Add Install Claude Code Hook menu item + event"
```

---

## Task 8: Frontend — handle the event, call the command, show dialogs

**Files:**
- Modify: `ui/app.js` (a `listen` near line 277 + an `installClaudeHook` function near `installCli` ~line 3174)

- [ ] **Step 1: Add the event listener**

In `ui/app.js`, just after the existing `menu-install-cli` listener (lines 277-280):

```js
  await listen("menu-install-cli", async () => {
    if (!IS_MAC) return;
    await installCli();
  });
```

add:

```js
  await listen("menu-install-claude-hook", async () => {
    await installClaudeHook();
  });
```

- [ ] **Step 2: Add the `installClaudeHook` function**

In `ui/app.js`, just after the existing `installCli` function (ends ~line 3191), add:

```js
async function installClaudeHook() {
  if (!treeRoot) {
    await dialogApi.message("Open a folder first to install the hook there.", {
      title: "MDViewer",
      kind: "info",
    });
    return;
  }
  let outcome;
  try {
    outcome = await invoke("install_claude_hook");
  } catch (e) {
    await dialogApi.message("Couldn't install the Claude Code hook.\n\n" + e, {
      title: "MDViewer",
      kind: "error",
    });
    return;
  }
  const msg =
    outcome === "updated"
      ? "Already installed — updated the MDViewer path."
      : "Installed. Plan, spec, and design files Claude Code writes in this project will now open in MDViewer.";
  await dialogApi.message(msg, { title: "MDViewer", kind: "info" });
}
```

- [ ] **Step 3: Verify build + existing JS tests**

Run: `node --test ui/*.test.js 2>&1 | grep -E "# (pass|fail)"`
Expected: PASS (no regressions; this feature adds no JS unit tests).
Run: `cd src-tauri && cargo build 2>&1 | tail -3`
Expected: `Finished`.

- [ ] **Step 4: Commit**

```bash
git add ui/app.js
git commit -m "Wire Install Claude Code Hook menu action in the frontend"
```

---

## Task 9: Build + manual smoke test

**Files:** none (verification only)

- [ ] **Step 1: Full Rust test + lint gate**

Run: `cd src-tauri && cargo test claude_hook 2>&1 | tail -5`
Expected: 8 tests pass.
Run: `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings 2>&1 | tail -3`
Expected: clean.

- [ ] **Step 2: Run the app on a folder**

Run: `cd src-tauri && cargo run -- ..`
(Opens MDViewer rooted at the repo.)

- [ ] **Step 3: Install the hook via the menu**

In the app: **MDViewer ▸ Install Claude Code Hook…**. Expect the dialog *"Installed. Plan, spec, and design files…"*. Then verify the file:

```bash
cat /Users/laek/source/mdviewer/.claude/settings.local.json
```

Expected: contains a `hooks.PostToolUse` entry with `matcher: "Write"` and a `command` like `"…/mdviewer" --claude-hook`. (If `.claude/settings.local.json` already existed, confirm its other contents are intact.)

- [ ] **Step 4: Verify idempotency**

Click **Install Claude Code Hook…** again. Expect *"Already installed — updated the MDViewer path."* Re-check the file: still exactly one `--claude-hook` entry (no duplicate).

- [ ] **Step 5: End-to-end — Claude writes a plan, it opens**

With MDViewer running and the hook installed in this repo, in a separate terminal run a Claude Code session in this repo and have it write a file like `scratch-plan.md` (or simulate the hook directly):

```bash
echo '{"tool_input":{"file_path":"'"$PWD"'/scratch-plan.md"}}' | "$(pwd)/src-tauri/target/debug/mdviewer" --claude-hook
```

(First create the file: `echo '# Test' > scratch-plan.md`.)
Expected: a new tab for `scratch-plan.md` appears in the already-running MDViewer window. Clean up: `rm scratch-plan.md` and (if desired) remove the test hook entry from `.claude/settings.local.json`.

- [ ] **Step 6: Commit any fixes**

```bash
git add -A
git commit -m "Polish Claude Code hook install after smoke test"
```

(Skip if no fixes were needed.)

---

## Self-review notes (for the implementer)

- **Spec coverage:** Decision table → Task 1 (match incl. plans//specs/ dirs), Task 6 (settings.local.json target, current_root), Task 5+4 (self-hook command + runtime), Task 7 (Write matcher via merge_hook in Task 3), Task 3 (idempotency), Task 7 (both-platform menu item). Part 2 Half A → Task 6; Half B → Tasks 4-5. Part 3 JSON/merge → Task 3. Part 4 cross-platform → Task 4 (`open_in_mdviewer` cfg branches). Part 5 tests → Tasks 1-3. Part 6 file layout → all. Out-of-scope items (uninstall, configurable patterns, committed settings) are correctly absent.
- **Type consistency:** `HookOutcome { Installed, Updated }` (snake_case serde → `"installed"`/`"updated"`) defined in Task 3, returned by `install_claude_hook` in Task 6, consumed by the frontend in Task 8 (checks `=== "updated"`). `merge_hook(Value, &str) -> (Value, HookOutcome)` signature consistent across Tasks 3 and 6. `is_plan_file(&str) -> bool` / `extract_file_path(&str) -> Option<String>` consistent across Tasks 1-2 and 4.
- **Known v1 limitations (accepted, per spec):** Windows opens a new window rather than a tab (no single-instance); a superpowers plan named without a keyword still matches only if under a `plans/`/`specs/` directory; no uninstall menu item.
