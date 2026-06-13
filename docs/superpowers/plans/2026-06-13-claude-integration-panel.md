# Claude Code Integration Panel + Nudge — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a dedicated **Claude Code Integration** window (explains the hook/MCP/Review features, shows per-project install state, Install/Update buttons) plus a conservative first-run nudge banner that surfaces it in git projects without the integration installed.

**Architecture:** Two read-only pure predicates (`hook_installed`, `mcp_installed`) feed a thin `integration_status` command. A new webview window (mirroring the Settings window) renders status + buttons against the current root and calls the existing install commands. Those commands gain an `integration-changed` event so the main window's nudge re-evaluates after any install. The nudge is gated on git-repo + not-installed + a global localStorage dismissal flag.

**Tech Stack:** Rust (Tauri 2.11, serde_json), vanilla JS frontend (no build step), `node --test` for JS units.

**Spec:** `docs/superpowers/specs/2026-06-13-claude-integration-panel-design.md`

---

## Conventions for every task

- **Worktree discipline:** this plan is executed in an isolated worktree on a feature branch. EVERY file path is under the worktree root. Before committing, confirm `git rev-parse --show-toplevel` is the worktree, not the main checkout, and `git branch --show-current` is the feature branch — never commit to `main`.
- Run Rust from `src-tauri/`: `cargo test`, and before each commit `cargo fmt` + `cargo clippy --all-targets -- -D warnings` (CI enforces both with `-D warnings`).
- Run JS tests from the repo root: `node --test ui/*.test.js`.
- Frontend changes require `cargo build` to re-bundle (`frontendDist` is embedded at compile time).
- Commit messages: imperative subject, **no** `Co-Authored-By` trailer. Comments only where the *why* is non-obvious.

## Status representation

`integration_status` returns booleans (`hook`, `mcp`) plus `root: Option<String>`. The frontend renders booleans as text via `statusLabel` ("Installed"/"Not installed") and buttons via `statusButtonLabel` ("Update"/"Install"). This refines the spec's illustrative `"installed"`/`"not_installed"` strings to plain booleans — simpler across the IPC boundary and in the pure helpers.

## File structure

```
src-tauri/src/
  claude_hook.rs  — ADD pure hook_installed(&Value) -> bool (+ tests)
  mcp.rs          — ADD pure mcp_installed(&Value) -> bool (+ tests)
  commands.rs     — ADD integration_status + show_integration_window commands;
                    ADD app: AppHandle + integration-changed emit to the two
                    install commands
  menu.rs         — ADD open_integration_window (pub fn, mirrors open_settings)
                    + "claude-integration" menu item & match arm
  lib.rs          — register integration_status + show_integration_window
  capabilities/default.json — add "claude-integration" to windows
ui/
  integration.js       — NEW pure helpers: statusButtonLabel, statusLabel, shouldNudge
  integration.test.js  — NEW node --test units
  claude-integration.html — NEW window markup (mirrors preferences.html)
  claude-integration.js   — NEW window logic (mirrors preferences.js)
  index.html       — ADD #integration-nudge banner (reuses .update-banner styling)
  app.js           — nudge: maybeShowIntegrationNudge + wiring + integration-changed listener
```

---

### Task 1: `claude_hook::hook_installed` (pure)

**Files:**
- Modify: `src-tauri/src/claude_hook.rs`

- [ ] **Step 1: Write the failing tests**

Append inside the existing `#[cfg(test)] mod tests` block in `src-tauri/src/claude_hook.rs`:

```rust
    #[test]
    fn hook_installed_detects_our_entry() {
        let settings = json!({
            "hooks": {"PostToolUse": [
                {"matcher": "Write", "hooks": [{"type": "command", "command": "'/x/mdviewer' --claude-hook"}]}
            ]}
        });
        assert!(hook_installed(&settings));
    }

    #[test]
    fn hook_installed_false_for_absent_or_unrelated() {
        assert!(!hook_installed(&json!({})));
        assert!(!hook_installed(&json!({"hooks": {"PostToolUse": []}})));
        let other = json!({
            "hooks": {"PostToolUse": [
                {"matcher": "Edit", "hooks": [{"type": "command", "command": "echo hi"}]}
            ]}
        });
        assert!(!hook_installed(&other));
    }

    #[test]
    fn hook_installed_false_for_wrong_types() {
        assert!(!hook_installed(&json!({"hooks": {"PostToolUse": "oops"}})));
        assert!(!hook_installed(&json!([])));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test claude_hook::tests::hook_installed`
Expected: compile error — `hook_installed` not found.

- [ ] **Step 3: Implement**

Add to `src-tauri/src/claude_hook.rs` (after `merge_hook`, before the tests module):

```rust
/// True if `settings` already contains our `--claude-hook` PostToolUse hook.
/// Read-only mirror of what `merge_hook` keys on; tolerates missing/wrong-typed
/// fields by returning false.
pub fn hook_installed(settings: &Value) -> bool {
    settings
        .get("hooks")
        .and_then(|h| h.get("PostToolUse"))
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter().any(|entry| {
                entry
                    .get("hooks")
                    .and_then(|h| h.as_array())
                    .map(|inner| {
                        inner.iter().any(|hook| {
                            hook.get("command")
                                .and_then(|c| c.as_str())
                                .map(|c| c.contains("--claude-hook"))
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test claude_hook::`
Expected: all pass (existing hook tests + 3 new).

- [ ] **Step 5: Lint and commit**

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings
git add src-tauri/src/claude_hook.rs
git commit -m "Add hook_installed: read-only detection of the Claude hook entry"
```

---

### Task 2: `mcp::mcp_installed` (pure)

**Files:**
- Modify: `src-tauri/src/mcp.rs`

- [ ] **Step 1: Write the failing tests**

Append inside the existing `#[cfg(test)] mod tests` block in `src-tauri/src/mcp.rs`:

```rust
    #[test]
    fn mcp_installed_detects_our_server() {
        let cfg = json!({"mcpServers": {"mdviewer": {"command": "/x/mdviewer", "args": ["--mcp"]}}});
        assert!(mcp_installed(&cfg));
    }

    #[test]
    fn mcp_installed_false_for_absent_or_other() {
        assert!(!mcp_installed(&json!({})));
        assert!(!mcp_installed(&json!({"mcpServers": {}})));
        assert!(!mcp_installed(&json!({"mcpServers": {"other": {"command": "npx"}}})));
    }

    #[test]
    fn mcp_installed_false_for_wrong_types() {
        assert!(!mcp_installed(&json!({"mcpServers": "oops"})));
        assert!(!mcp_installed(&json!([])));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test mcp::tests::mcp_installed`
Expected: compile error — `mcp_installed` not found.

- [ ] **Step 3: Implement**

Add to `src-tauri/src/mcp.rs` (next to `merge_mcp_config`, above the tests module):

```rust
/// True if `config` already declares our `mdviewer` MCP server. Tolerates
/// missing/wrong-typed fields by returning false.
pub fn mcp_installed(config: &Value) -> bool {
    config
        .get("mcpServers")
        .and_then(|m| m.get("mdviewer"))
        .map(|v| !v.is_null())
        .unwrap_or(false)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test mcp::`
Expected: all pass (existing mcp tests + 3 new).

- [ ] **Step 5: Lint and commit**

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings
git add src-tauri/src/mcp.rs
git commit -m "Add mcp_installed: read-only detection of the mdviewer MCP entry"
```

---

### Task 3: `integration_status` command

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs` (register)

- [ ] **Step 1: Implement the command**

Append to `src-tauri/src/commands.rs` (near `install_mcp_server`). Note: `Path`, `PathBuf`, `State`, `AppState`, and `serde` are already used in this file; add `use serde::Serialize;` at the top only if not already present (check the existing imports first).

```rust
/// Read-only install state for the current project root, for the Claude Code
/// Integration window and the first-run nudge. `root` is None when no folder is
/// open. Missing/empty/unparseable config files read as not-installed.
#[derive(Serialize)]
pub struct IntegrationStatus {
    pub hook: bool,
    pub mcp: bool,
    pub root: Option<String>,
}

#[tauri::command]
pub fn integration_status(state: State<'_, AppState>) -> IntegrationStatus {
    let root = state.current_root.lock().ok().and_then(|g| g.clone());
    let Some(root) = root else {
        return IntegrationStatus { hook: false, mcp: false, root: None };
    };
    let hook = read_json_file(&root.join(".claude").join("settings.local.json"))
        .map(|v| crate::claude_hook::hook_installed(&v))
        .unwrap_or(false);
    let mcp = read_json_file(&root.join(".mcp.json"))
        .map(|v| crate::mcp::mcp_installed(&v))
        .unwrap_or(false);
    IntegrationStatus {
        hook,
        mcp,
        root: Some(root.to_string_lossy().into_owned()),
    }
}

/// Read + parse a JSON file, returning None for missing/empty/unparseable.
fn read_json_file(path: &Path) -> Option<serde_json::Value> {
    let s = std::fs::read_to_string(path).ok()?;
    if s.trim().is_empty() {
        return None;
    }
    serde_json::from_str(&s).ok()
}
```

- [ ] **Step 2: Register in `lib.rs`**

In `src-tauri/src/lib.rs`, in the `tauri::generate_handler![...]` list, after `commands::install_mcp_server,` add:

```rust
            commands::integration_status,
```

- [ ] **Step 3: Build + lint**

Run: `cd src-tauri && cargo build && cargo test && cargo fmt && cargo clippy --all-targets -- -D warnings`
Expected: clean; all tests still pass (no new tests here — the logic is the pure predicates from Tasks 1–2; file reading is verified in the smoke test, consistent with the untested `install_*` commands).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "Add integration_status command (per-root hook/MCP install state)"
```

---

### Task 4: emit `integration-changed` from the install commands

**Files:**
- Modify: `src-tauri/src/commands.rs`

- [ ] **Step 1: Add the AppHandle param + emit to both install commands**

In `src-tauri/src/commands.rs`, add `use tauri::Emitter;` near the other `use tauri::...` imports if not already present (it's required for `AppHandle::emit`).

Change `install_claude_hook`'s signature and add the emit on success:

```rust
pub fn install_claude_hook(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<crate::claude_hook::HookOutcome, String> {
```

and immediately before the final `Ok(outcome)`:

```rust
    let _ = app.emit("integration-changed", ());
    Ok(outcome)
```

Do the same for `install_mcp_server`:

```rust
pub fn install_mcp_server(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<crate::claude_hook::HookOutcome, String> {
```

and before its final `Ok(outcome)`:

```rust
    let _ = app.emit("integration-changed", ());
    Ok(outcome)
```

The frontend invokes these with no positional args; Tauri injects `app` and `state`, so the JS `invoke("install_claude_hook")` / `invoke("install_mcp_server")` calls are unchanged.

- [ ] **Step 2: Build + test + lint**

Run: `cd src-tauri && cargo build && cargo test && cargo fmt && cargo clippy --all-targets -- -D warnings`
Expected: clean; all tests pass. (`AppHandle` is already imported in this file — used by `remember_folder`.)

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "Emit integration-changed after a successful hook/MCP install"
```

---

### Task 5: integration window — open helper, menu item, command

**Files:**
- Modify: `src-tauri/src/menu.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/capabilities/default.json`

- [ ] **Step 1: Window-open helper in `menu.rs`**

Add to `src-tauri/src/menu.rs` (next to `open_settings`; make it `pub` so the command can delegate):

```rust
pub fn open_integration_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("claude-integration") {
        let _ = win.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(
        app,
        "claude-integration",
        WebviewUrl::App("claude-integration.html".into()),
    )
    .title("Claude Code Integration")
    .inner_size(480.0, 360.0)
    .resizable(true)
    .build();
}
```

- [ ] **Step 2: Menu item + match arm in `menu.rs`**

In `rebuild()`, after the `install_mcp_server` item builder:

```rust
    let claude_integration =
        MenuItemBuilder::with_id("claude-integration", "Claude Code Integration…").build(app)?;
```

After `let app_menu_builder = app_menu_builder.item(&install_mcp_server);`:

```rust
    let app_menu_builder = app_menu_builder.item(&claude_integration);
```

In the `on_menu_event` match, after the `"install-mcp-server"` arm:

```rust
            "claude-integration" => open_integration_window(app),
```

(The closure binds `app` as `&AppHandle`, matching `open_settings(app)`.)

- [ ] **Step 3: Frontend-callable command in `commands.rs`**

Append to `src-tauri/src/commands.rs`:

```rust
/// Open (or focus) the Claude Code Integration window. Invoked by the nudge's
/// "Set up" button; the menu opens the same window directly.
#[tauri::command]
pub fn show_integration_window(app: AppHandle) {
    crate::menu::open_integration_window(&app);
}
```

- [ ] **Step 4: Register the command in `lib.rs`**

In the `generate_handler![...]` list, after `commands::integration_status,`:

```rust
            commands::show_integration_window,
```

- [ ] **Step 5: Allow the window in `capabilities/default.json`**

Change the `windows` array from `["main", "preferences"]` to:

```json
  "windows": ["main", "preferences", "claude-integration"],
```

- [ ] **Step 6: Build + lint**

Run: `cd src-tauri && cargo build && cargo test && cargo fmt && cargo clippy --all-targets -- -D warnings`
Expected: clean. (The window won't render until Task 7 adds the HTML, but the Rust compiles and the menu item appears.)

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/menu.rs src-tauri/src/commands.rs src-tauri/src/lib.rs src-tauri/capabilities/default.json
git commit -m "Add Claude Code Integration window: menu item + open command"
```

---

### Task 6: `ui/integration.js` pure helpers + tests

**Files:**
- Create: `ui/integration.js`
- Create: `ui/integration.test.js`

- [ ] **Step 1: Write the failing tests**

Create `ui/integration.test.js`:

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { statusButtonLabel, statusLabel, shouldNudge } from "./integration.js";

test("statusButtonLabel: install vs update", () => {
  assert.equal(statusButtonLabel(false), "Install");
  assert.equal(statusButtonLabel(true), "Update");
});

test("statusLabel: not installed vs installed", () => {
  assert.equal(statusLabel(false), "Not installed");
  assert.equal(statusLabel(true), "Installed");
});

test("shouldNudge: only in a git repo with nothing installed and not dismissed", () => {
  assert.equal(shouldNudge(true, false, false, false), true);
  // not a git repo
  assert.equal(shouldNudge(false, false, false, false), false);
  // hook already installed
  assert.equal(shouldNudge(true, true, false, false), false);
  // mcp already installed
  assert.equal(shouldNudge(true, false, true, false), false);
  // dismissed
  assert.equal(shouldNudge(true, false, false, true), false);
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `node --test ui/integration.test.js`
Expected: FAIL — cannot find module `./integration.js`.

- [ ] **Step 3: Implement**

Create `ui/integration.js`:

```js
// Pure helpers for the Claude Code integration window + first-run nudge.
// DOM-free + Tauri-free so they unit-test under `node --test`.

/** Button label for an install row: Update when present, else Install. */
export function statusButtonLabel(installed) {
  return installed ? "Update" : "Install";
}

/** Status text for an install row. */
export function statusLabel(installed) {
  return installed ? "Installed" : "Not installed";
}

/** Whether to show the first-run nudge: only in a git project where neither
 *  piece is installed and the user hasn't permanently dismissed it. */
export function shouldNudge(isGitRepo, hookInstalled, mcpInstalled, dismissed) {
  return isGitRepo && !hookInstalled && !mcpInstalled && !dismissed;
}
```

- [ ] **Step 4: Run all JS tests**

Run: `node --test ui/*.test.js`
Expected: all pass (existing + 3 new).

- [ ] **Step 5: Commit**

```bash
git add ui/integration.js ui/integration.test.js
git commit -m "Add ui/integration.js: pure helpers for the integration panel + nudge"
```

---

### Task 7: the integration window (`claude-integration.html` + `.js`)

**Files:**
- Create: `ui/claude-integration.html`
- Create: `ui/claude-integration.js`

- [ ] **Step 1: Create the HTML**

Create `ui/claude-integration.html` (mirrors `preferences.html`: inline styles, `color-scheme: light dark` so it follows the OS appearance, external module script per CSP):

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Claude Code Integration</title>
    <style>
      :root {
        color-scheme: light dark;
        font-family:
          -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
      }
      body { margin: 0; padding: 20px; font-size: 13px; line-height: 1.5; }
      h1 { font-size: 15px; margin: 0 0 4px; }
      .project { opacity: 0.7; margin: 0 0 16px; word-break: break-all; }
      .no-folder { opacity: 0.7; margin: 0 0 16px; }
      .feature { margin-bottom: 16px; }
      .feature-head {
        display: flex; align-items: center; justify-content: space-between; gap: 8px;
      }
      .feature-name { font-weight: 600; }
      .feature-desc { opacity: 0.8; margin: 2px 0 0; }
      .status { opacity: 0.7; margin-right: 8px; font-size: 12px; }
      button {
        font-size: 12px; padding: 3px 12px; border-radius: 6px;
        border: 1px solid currentColor; background: transparent; color: inherit;
        cursor: pointer;
      }
      button:disabled { opacity: 0.4; cursor: default; }
    </style>
  </head>
  <body>
    <h1>Claude Code Integration</h1>
    <div class="project" id="project" hidden></div>
    <div class="no-folder" id="no-folder" hidden>
      Open a folder to set up integration for a project.
    </div>

    <div class="feature">
      <div class="feature-head">
        <span class="feature-name">Hook</span>
        <span>
          <span class="status" id="hook-status"></span>
          <button id="hook-btn" type="button">Install</button>
        </span>
      </div>
      <p class="feature-desc">
        Plans, specs, and designs Claude Code writes here open automatically.
      </p>
    </div>

    <div class="feature">
      <div class="feature-head">
        <span class="feature-name">MCP server</span>
        <span>
          <span class="status" id="mcp-status"></span>
          <button id="mcp-btn" type="button">Install</button>
        </span>
      </div>
      <p class="feature-desc">
        Lets Claude open documents in MDViewer and request reviews you send back inline.
      </p>
    </div>

    <div class="feature">
      <div class="feature-head">
        <span class="feature-name">Review Mode</span>
        <span class="status">Always available</span>
      </div>
      <p class="feature-desc">
        Comment on any block, then copy your review or send it straight to a waiting Claude session.
      </p>
    </div>

    <script type="module" src="claude-integration.js"></script>
  </body>
</html>
```

- [ ] **Step 2: Create the window logic**

Create `ui/claude-integration.js`:

```js
// Claude Code Integration window. External module only (CSP script-src 'self').
import { statusButtonLabel, statusLabel } from "./integration.js";

const { invoke } = window.__TAURI__.core;

const projectEl = document.getElementById("project");
const noFolderEl = document.getElementById("no-folder");
const hookStatusEl = document.getElementById("hook-status");
const hookBtn = document.getElementById("hook-btn");
const mcpStatusEl = document.getElementById("mcp-status");
const mcpBtn = document.getElementById("mcp-btn");

function setRow(statusEl, btn, installed, disabled) {
  statusEl.textContent = statusLabel(installed);
  btn.textContent = statusButtonLabel(installed);
  btn.disabled = disabled;
}

async function load() {
  const s = await invoke("integration_status");
  if (!s.root) {
    projectEl.hidden = true;
    noFolderEl.hidden = false;
    setRow(hookStatusEl, hookBtn, false, true);
    setRow(mcpStatusEl, mcpBtn, false, true);
    return;
  }
  noFolderEl.hidden = true;
  projectEl.hidden = false;
  projectEl.textContent = `Project: ${s.root}`;
  setRow(hookStatusEl, hookBtn, s.hook, false);
  setRow(mcpStatusEl, mcpBtn, s.mcp, false);
}

async function runInstall(command, btn) {
  btn.disabled = true;
  try {
    await invoke(command);
  } catch (e) {
    console.error(command, "failed", e);
    btn.disabled = false;
    return;
  }
  // The Rust command emits integration-changed (the main window listens);
  // here we just refresh this window's own status + labels.
  await load();
}

hookBtn.addEventListener("click", () => runInstall("install_claude_hook", hookBtn));
mcpBtn.addEventListener("click", () => runInstall("install_mcp_server", mcpBtn));

load().catch((e) => console.error("integration status load failed", e));
```

- [ ] **Step 3: Build and verify the window opens**

Run: `cd src-tauri && cargo build`
Then manually: launch the dev binary on a folder, open **MDViewer ▸ Claude Code Integration…**, confirm the window shows the project path, three rows, and the correct Install/Update labels. (Full interaction is exercised in Task 10.)

- [ ] **Step 4: Commit**

```bash
git add ui/claude-integration.html ui/claude-integration.js
git commit -m "Add the Claude Code Integration window (status + install buttons)"
```

---

### Task 8: first-run nudge banner + app.js wiring

**Files:**
- Modify: `ui/index.html`
- Modify: `ui/app.js`

- [ ] **Step 1: Add the banner markup**

In `ui/index.html`, immediately after the closing `</div>` of `#update-banner` (around line 49), add (reuses the `.update-banner` / `.update-banner-btn` styling — no new CSS):

```html
    <div class="update-banner" id="integration-nudge" hidden role="status">
      <span class="update-banner-text" id="integration-nudge-text">
        💬 Using Claude Code in this project? Set up MDViewer integration.
      </span>
      <button id="integration-nudge-setup" class="update-banner-btn primary" type="button">
        Set up
      </button>
      <button id="integration-nudge-dismiss" class="update-banner-btn" type="button">
        Dismiss
      </button>
    </div>
```

- [ ] **Step 2: Import the helper + add the key/refs (top of `ui/app.js`)**

Add to the import block near the other `./*.js` imports:

```js
import { shouldNudge } from "./integration.js";
```

With the other element refs / constants near the top, add:

```js
const INTEGRATION_NUDGE_KEY = "mdviewer.integration.nudge_dismissed";
const integrationNudge = document.getElementById("integration-nudge");
const integrationNudgeSetup = document.getElementById("integration-nudge-setup");
const integrationNudgeDismiss = document.getElementById("integration-nudge-dismiss");
```

- [ ] **Step 3: Add the nudge logic**

Add these functions to `ui/app.js` (near `refreshGitStatus`):

```js
/** Show/hide the first-run integration nudge. Cheap and idempotent: a global
 *  localStorage dismissal or a non-git folder short-circuits before any IPC. */
async function maybeShowIntegrationNudge() {
  const dismissed = !!localStorage.getItem(INTEGRATION_NUDGE_KEY);
  const isGitRepo = !!gitRepoRoot;
  let hook = false;
  let mcp = false;
  if (isGitRepo && !dismissed) {
    try {
      const s = await invoke("integration_status");
      hook = s.hook;
      mcp = s.mcp;
    } catch (e) {
      console.debug("integration_status failed", e);
      return;
    }
  }
  integrationNudge.hidden = !shouldNudge(isGitRepo, hook, mcp, dismissed);
}
```

- [ ] **Step 4: Wire the buttons (in `init()`, with the other listeners/handlers)**

```js
  integrationNudgeSetup.addEventListener("click", () => {
    invoke("show_integration_window").catch((e) => console.error(e));
  });
  integrationNudgeDismiss.addEventListener("click", () => {
    try {
      localStorage.setItem(INTEGRATION_NUDGE_KEY, "1");
    } catch (_) {}
    integrationNudge.hidden = true;
  });

  await listen("integration-changed", () => {
    maybeShowIntegrationNudge();
  });
```

- [ ] **Step 5: Re-evaluate the nudge after each git-status refresh**

In `refreshGitStatus()`, at the very end (after `applyGitDecorations();`), add:

```js
  maybeShowIntegrationNudge();
```

This runs after `gitRepoRoot` is set, on both startup and folder change (both call `refreshGitStatus`), and self-heals if the integration is installed from the terminal.

- [ ] **Step 6: Build + run all JS tests**

Run: `node --test ui/*.test.js` → all pass.
Run: `cd src-tauri && cargo build` → clean (re-bundles the frontend).

- [ ] **Step 7: Commit**

```bash
git add ui/index.html ui/app.js
git commit -m "Add first-run integration nudge banner and wiring"
```

---

### Task 9: documentation

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`

- [ ] **Step 1: README**

Add a Features bullet (next to the existing Claude integration bullets):

```markdown
- **Claude Code Integration panel** — **MDViewer ▸ Claude Code Integration…**
  opens a window showing, for the current project, whether the hook and MCP
  server are installed, with one-click **Install**/**Update** buttons and a
  short explanation of each (plus Review Mode). When you open a git project
  that has neither set up, a one-time banner offers to set it up; dismissing it
  silences the prompt for good.
```

Add to the Menus section, after the Install MCP Server… entry:

```markdown
- **MDViewer ▸ Claude Code Integration…** — opens the integration window
  (install state + Install/Update buttons + explanations) for the open project.
```

- [ ] **Step 2: CLAUDE.md — file layout**

In the `ui/` layout block, after the `mcp.js` line:

```
  integration.js  — pure helpers: statusButtonLabel, statusLabel, shouldNudge
                    for the Claude Code Integration window + nudge (unit-tested)
  claude-integration.html/.js — the integration window (mirrors preferences.*)
```

- [ ] **Step 3: CLAUDE.md — architecture bullet**

After the **MCP server** architecture bullet, add:

```markdown
- **Claude Code Integration panel**: **MDViewer ▸ Claude Code Integration…**
  (`menu::open_integration_window`, mirrors `open_settings`) opens the
  `claude-integration` webview window (`ui/claude-integration.*`, listed in
  `capabilities/default.json`). It calls `integration_status` (read-only:
  `claude_hook::hook_installed` on `.claude/settings.local.json` +
  `mcp::mcp_installed` on `.mcp.json`, both pure/unit-tested; returns
  `{hook, mcp, root}`) to render per-row Install/Update buttons wired to the
  existing `install_claude_hook` / `install_mcp_server` commands. Those
  commands now emit `integration-changed` on success (from any entry point —
  window or the two standalone menu items), which the main window listens for
  to re-evaluate the **first-run nudge**: a banner (reusing `.update-banner`)
  shown when `shouldNudge(isGitRepo, hook, mcp, dismissed)` holds —
  `gitRepoRoot` set, neither installed, and the global
  `mdviewer.integration.nudge_dismissed` localStorage flag unset. Evaluated at
  the tail of `refreshGitStatus` (so `gitRepoRoot` is populated and it
  self-heals on terminal installs); **Dismiss** sets the flag permanently;
  **Set up** invokes `show_integration_window`.
```

- [ ] **Step 4: Commit**

```bash
git add README.md CLAUDE.md
git commit -m "Document the Claude Code Integration panel and nudge"
```

---

### Task 10: manual GUI smoke test (do not skip)

Per the standing project rule (automated work misses visual/theme bugs), verify the real app before merge.

- [ ] **Step 1: Build + scratch project**

```bash
cd src-tauri && cargo build
rm -rf /tmp/integ-smoke && mkdir /tmp/integ-smoke && cd /tmp/integ-smoke && git init -q && printf '# Notes\n' > notes.md
<worktree>/src-tauri/target/debug/mdviewer /tmp/integ-smoke
```

- [ ] **Step 2: Verify the nudge + window**

- On opening `/tmp/integ-smoke` (a git repo with nothing installed), the **nudge banner** appears at the top.
- Click **Set up** → the integration window opens; the **Project:** line shows `/private/tmp/integ-smoke` (or the real path); both rows read **Not installed** / **Install**; Review Mode row reads **Always available**.
- Click the hook's **Install** → its row flips to **Installed** / **Update**, and the **nudge banner in the main window disappears** (via `integration-changed`).
- Click the MCP **Install** → row flips to Installed/Update; `/tmp/integ-smoke/.mcp.json` and `.claude/settings.local.json` now exist with the right entries.
- Reopen the window → state persists (still Installed/Update).

- [ ] **Step 3: Verify dismissal + gating + no-folder + theme**

- Fresh scratch repo (uninstall: `rm -rf /tmp/integ-smoke2 && mkdir … && git init`), open it → nudge appears → click **Dismiss** → banner hides. Open another fresh git project → **nudge stays gone** (global flag). (To re-test, clear the key in devtools or use a fresh user dir.)
- Open a **non-git** folder with nothing installed → **no nudge**.
- With **no folder open**, open the window → rows disabled, "Open a folder…" note shown.
- Toggle OS appearance light/dark → the window (follows `color-scheme`) and the nudge banner both look correct.

- [ ] **Step 4: Fix anything found, re-run, commit fixes.**

---

## Final verification (after all tasks)

```bash
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
node --test ui/*.test.js
```

All green + smoke test passed → use superpowers:finishing-a-development-branch.
