# Multi-window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One project (tree root) per window, with incoming files, Claude hook launches and MCP calls routed to the window whose root contains them.

**Architecture:** One Tauri process, many webview windows. Per-window state (root, watchers, ready flag, pending opens) moves from `AppState` into a label-keyed `windows::Registry`. Commands learn their window from Tauri's injected `WebviewWindow`. Events become targeted (`emit_to`) and the frontend listens window-scoped. A pure `routing::route` picks the window for a path (longest containing root, MRU tie-break, else a new window at the git root / parent). Tab sessions are stored per root; the set of open windows is restored on launch.

**Tech Stack:** Rust / Tauri 2.11, `tauri-plugin-single-instance` 2, vanilla JS (no build step), `node --test`.

**Spec:** `docs/superpowers/specs/2026-09-19-multi-window-design.md`

## Global Constraints

- Tauri commands return `Result<T, String>`; errors use `format!("…: {e}")`.
- Per-window commands take `window: tauri::WebviewWindow` (Tauri-injected). Never accept a window label from the frontend as an argument.
- A command from a label with no project entry errors with exactly `"no project window"`. It never falls back to another window's root.
- Project window labels: `main` and `project-<n>`. Helper labels: `preferences`, `pdf-export`, `claude-integration`.
- **Frontend listeners must be window-scoped.** In Tauri 2, JS `event.listen()` defaults to target *Any* and receives `emit_to` events aimed at *other* windows. Use `getCurrentWebviewWindow().listen`.
- Lock order: take `AppState.windows` before `AppState.focus`, and never hold either across `emit`/`emit_to`, window creation or dialog calls.
- Per-root session LRU cap: `30`. Cascade offset for new windows: `24` px. New-window default size `1200×800`, min `600×400` (matches `tauri.conf.json`).
- Window title format: `"<root folder name> — MDViewer"` (em dash, U+2014).
- Commit messages: imperative subject, **no `Co-Authored-By` trailer** (project convention in CLAUDE.md).
- Gate for every task (run from `src-tauri/`): `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, and from the repo root `node --test ui/`. Any `ui/*` change needs `cargo build` before a manual check, because Tauri bundles `frontendDist` at compile time.
- **Dev gotcha (from Task 12 on):** single-instance keys on the bundle identifier `com.mdviewer.app`. Quit any installed MDViewer before `cargo run` or the smoke tests, or the dev process forwards its argv to the installed app and exits.

## File map

| File | Responsibility |
|---|---|
| `src-tauri/src/windows.rs` (new) | `WindowState`, `Registry` (label → state, helper owners, label counter), `window_title`; IO helpers `create_project_window`, `deliver_files`, `emit_to_front`, `on_destroyed` |
| `src-tauri/src/routing.rs` (new) | Pure `route`, `FocusOrder`, `open_folder_target`; IO `fallback_root` (git toplevel or parent) |
| `src-tauri/src/lib.rs` | `AppState` reshaped; setup registers `main`; `on_window_event` wiring; single-instance plugin; `ExitRequested` snapshot |
| `src-tauri/src/commands.rs` | Per-window commands; `helper_owner`; session commands per root |
| `src-tauri/src/watcher.rs` | Slots take a target label and `emit_to` it |
| `src-tauri/src/mcp_server.rs` | `McpPending` entries carry label + tool; routing; `abandon_window` |
| `src-tauri/src/mcp.rs` | `GuiRequest.cwd`; proxy sets it |
| `src-tauri/src/menu.rs` | New Window item; window-scoped emits; helper owners |
| `src-tauri/src/open_files.rs` | Finder opens → `routing` |
| `src-tauri/src/recent.rs` | `sessions` per root, `windows` snapshot, legacy migration |
| `src-tauri/src/main.rs` | `resolve_args` delegates to shared `launch::resolve_launch_path` |
| `src-tauri/src/launch.rs` (new) | Pure-ish `resolve_launch_path(raw, cwd)` shared by `main.rs` and the single-instance callback |
| `src-tauri/capabilities/default.json` | `project-*` glob; `core:window:allow-destroy` |
| `ui/app.js` | Scoped listeners; `emitTo` for PDF export; close guard; theme `storage` sync |
| `ui/pdf-export.js` | Owner-addressed `emitTo`; scoped listeners |
| `ui/claude-integration.js` | Refresh on `integration-changed` |
| `ui/windowscope.js` (new) + `.test.js` | Pure `anyDirty(tabs)` and `themeFromStorageEvent(e, key, isValidTheme)` |
| `src-tauri/tests/multi_window_smoke.rs` (new) | Two-root MCP routing smoke |

## Phases

The spec defines three phases; this plan follows them. Per-root sessions must land **before** a second window can exist; otherwise two windows would overwrite each other's single session.

- **Phase 1 — per-window state, no behavior change:** Tasks 1–4.
- **Phase 2 — multiple windows:** Tasks 5–9 (per-root sessions, lifecycle + New Window, frontend window hygiene, Open Folder semantics, restore all windows).
- **Phase 3 — routing:** Tasks 10–14 (route, Finder/Open File routing, single instance, MCP routing, smoke + docs).

---

## Phase 1 — per-window state, no behavior change

### Task 1: `windows::Registry` and `routing::FocusOrder` (pure)

**Files:**
- Create: `src-tauri/src/windows.rs`
- Create: `src-tauri/src/routing.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod windows; mod routing;`)

**Interfaces:**
- Produces:
  - `pub struct WindowState { pub root: PathBuf, pub watcher: WatcherSlot, pub tree_watcher: TreeWatcherSlot, pub ready: bool, pub pending_files: Vec<PathBuf> }` with `WindowState::new(root: PathBuf) -> Self`
  - `#[derive(Default)] pub struct Registry` with: `insert(&mut self, label: &str, root: PathBuf)`, `get(&self, label: &str) -> Option<&WindowState>`, `get_mut(&mut self, label: &str) -> Option<&mut WindowState>`, `remove(&mut self, label: &str) -> Option<WindowState>`, `root(&self, label: &str) -> Result<PathBuf, String>`, `set_root(&mut self, label: &str, root: PathBuf) -> Result<(), String>`, `roots(&self) -> Vec<(String, PathBuf)>`, `labels(&self) -> Vec<String>`, `label_with_root(&self, root: &Path) -> Option<String>`, `next_label(&mut self) -> String`, `set_owner(&mut self, helper: &str, owner: &str)`, `project_label_for(&self, caller: &str) -> Result<String, String>`
  - `pub fn window_title(root: &Path) -> String`
  - `#[derive(Default)] pub struct FocusOrder` with `touch(&mut self, label: &str)`, `remove(&mut self, label: &str)`, `front(&self) -> Option<&str>`, `as_slice(&self) -> &[String]`
  - `pub const NO_PROJECT_WINDOW: &str = "no project window";`

- [ ] **Step 1: Write the failing tests** in `src-tauri/src/windows.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn root_of_unknown_label_is_no_project_window() {
        let r = Registry::default();
        assert_eq!(r.root("main").unwrap_err(), NO_PROJECT_WINDOW);
    }

    #[test]
    fn insert_then_root_and_set_root() {
        let mut r = Registry::default();
        r.insert("main", PathBuf::from("/a"));
        assert_eq!(r.root("main").unwrap(), PathBuf::from("/a"));
        r.set_root("main", PathBuf::from("/b")).unwrap();
        assert_eq!(r.root("main").unwrap(), PathBuf::from("/b"));
        assert!(r.set_root("nope", PathBuf::from("/c")).is_err());
    }

    #[test]
    fn next_label_is_monotonic_and_skips_taken_labels() {
        let mut r = Registry::default();
        r.insert("project-1", PathBuf::from("/x"));
        assert_eq!(r.next_label(), "project-2");
        assert_eq!(r.next_label(), "project-3");
    }

    #[test]
    fn label_with_root_matches_exact_root_only() {
        let mut r = Registry::default();
        r.insert("main", PathBuf::from("/repo"));
        assert_eq!(r.label_with_root(Path::new("/repo")).as_deref(), Some("main"));
        assert_eq!(r.label_with_root(Path::new("/repo/docs")), None);
    }

    #[test]
    fn project_label_for_resolves_helpers_through_owner() {
        let mut r = Registry::default();
        r.insert("main", PathBuf::from("/a"));
        assert_eq!(r.project_label_for("main").unwrap(), "main");
        assert_eq!(r.project_label_for("pdf-export").unwrap_err(), NO_PROJECT_WINDOW);
        r.set_owner("pdf-export", "main");
        assert_eq!(r.project_label_for("pdf-export").unwrap(), "main");
    }

    #[test]
    fn removing_a_window_drops_owners_pointing_at_it() {
        let mut r = Registry::default();
        r.insert("main", PathBuf::from("/a"));
        r.set_owner("claude-integration", "main");
        r.remove("main");
        assert_eq!(
            r.project_label_for("claude-integration").unwrap_err(),
            NO_PROJECT_WINDOW
        );
    }

    #[test]
    fn window_title_uses_folder_name() {
        assert_eq!(window_title(Path::new("/Users/me/repo")), "repo — MDViewer");
        assert_eq!(window_title(Path::new("/")), "/ — MDViewer");
    }
}
```

In `src-tauri/src/routing.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_moves_label_to_front_without_duplicates() {
        let mut f = FocusOrder::default();
        f.touch("main");
        f.touch("project-1");
        f.touch("main");
        assert_eq!(f.as_slice(), ["main", "project-1"]);
        assert_eq!(f.front(), Some("main"));
    }

    #[test]
    fn remove_drops_label() {
        let mut f = FocusOrder::default();
        f.touch("main");
        f.touch("project-1");
        f.remove("project-1");
        assert_eq!(f.as_slice(), ["main"]);
        f.remove("main");
        assert_eq!(f.front(), None);
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd src-tauri && cargo test --lib windows:: routing::`
Expected: compile errors (`Registry`, `FocusOrder` not defined).

- [ ] **Step 3: Implement**

`src-tauri/src/windows.rs`:

```rust
//! Per-window state for project windows, keyed by Tauri window label. The
//! registry is pure (unit-tested); the IO helpers below it touch the AppHandle.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::watcher::{TreeWatcherSlot, WatcherSlot};

pub const NO_PROJECT_WINDOW: &str = "no project window";

pub struct WindowState {
    pub root: PathBuf,
    pub watcher: WatcherSlot,
    pub tree_watcher: TreeWatcherSlot,
    pub ready: bool,
    pub pending_files: Vec<PathBuf>,
}

impl WindowState {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            watcher: WatcherSlot::default(),
            tree_watcher: TreeWatcherSlot::default(),
            ready: false,
            pending_files: Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct Registry {
    windows: HashMap<String, WindowState>,
    /// helper window label → the project window it acts for.
    owners: HashMap<String, String>,
    next_id: u64,
}

impl Registry {
    pub fn insert(&mut self, label: &str, root: PathBuf) {
        self.windows.insert(label.to_string(), WindowState::new(root));
    }

    pub fn get(&self, label: &str) -> Option<&WindowState> {
        self.windows.get(label)
    }

    pub fn get_mut(&mut self, label: &str) -> Option<&mut WindowState> {
        self.windows.get_mut(label)
    }

    pub fn remove(&mut self, label: &str) -> Option<WindowState> {
        self.owners.retain(|_, owner| owner != label);
        self.windows.remove(label)
    }

    pub fn root(&self, label: &str) -> Result<PathBuf, String> {
        self.windows
            .get(label)
            .map(|w| w.root.clone())
            .ok_or_else(|| NO_PROJECT_WINDOW.to_string())
    }

    pub fn set_root(&mut self, label: &str, root: PathBuf) -> Result<(), String> {
        let w = self
            .windows
            .get_mut(label)
            .ok_or_else(|| NO_PROJECT_WINDOW.to_string())?;
        w.root = root;
        Ok(())
    }

    pub fn roots(&self) -> Vec<(String, PathBuf)> {
        self.windows
            .iter()
            .map(|(l, w)| (l.clone(), w.root.clone()))
            .collect()
    }

    pub fn labels(&self) -> Vec<String> {
        self.windows.keys().cloned().collect()
    }

    pub fn label_with_root(&self, root: &Path) -> Option<String> {
        self.windows
            .iter()
            .find(|(_, w)| w.root == root)
            .map(|(l, _)| l.clone())
    }

    pub fn next_label(&mut self) -> String {
        loop {
            self.next_id += 1;
            let label = format!("project-{}", self.next_id);
            if !self.windows.contains_key(&label) {
                return label;
            }
        }
    }

    pub fn set_owner(&mut self, helper: &str, owner: &str) {
        self.owners.insert(helper.to_string(), owner.to_string());
    }

    /// The project window a command caller acts for: itself if it is a project
    /// window, else the owner recorded when the helper window was opened.
    pub fn project_label_for(&self, caller: &str) -> Result<String, String> {
        if self.windows.contains_key(caller) {
            return Ok(caller.to_string());
        }
        self.owners
            .get(caller)
            .filter(|owner| self.windows.contains_key(owner.as_str()))
            .cloned()
            .ok_or_else(|| NO_PROJECT_WINDOW.to_string())
    }
}

pub fn window_title(root: &Path) -> String {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned());
    format!("{name} — MDViewer")
}
```

`src-tauri/src/routing.rs`:

```rust
//! Which window an incoming path belongs to. `route` and `FocusOrder` are pure
//! and unit-tested; `fallback_root` is the only IO.

/// Most-recently-focused project windows, front = most recent.
#[derive(Default)]
pub struct FocusOrder {
    order: Vec<String>,
}

impl FocusOrder {
    pub fn touch(&mut self, label: &str) {
        self.order.retain(|l| l != label);
        self.order.insert(0, label.to_string());
    }

    pub fn remove(&mut self, label: &str) {
        self.order.retain(|l| l != label);
    }

    pub fn front(&self) -> Option<&str> {
        self.order.first().map(String::as_str)
    }

    pub fn as_slice(&self) -> &[String] {
        &self.order
    }
}
```

Add `mod routing;` and `mod windows;` to `lib.rs`. Until Task 2 uses them, clippy flags dead code, so add `#![allow(dead_code)]` at the top of both new files with the comment `// Wired up in Task 2.`. Task 2 removes it.

- [ ] **Step 4: Run the tests**

Run: `cd src-tauri && cargo test --lib windows:: routing::`
Expected: 9 passed.

- [ ] **Step 5: Gate + commit**

```bash
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
git add src-tauri/src/windows.rs src-tauri/src/routing.rs src-tauri/src/lib.rs
git commit -m "Add per-window registry and focus order"
```

---

### Task 2: Move `AppState` to the registry; label-scoped commands and watchers

This is the risky refactor. With one window the app must behave exactly as before.

**Files:**
- Modify: `src-tauri/src/lib.rs` (`AppState`, `run`, setup)
- Modify: `src-tauri/src/commands.rs` (`get_initial_state`, `watch_tree`, `open_file`, `frontend_ready`, `remember_folder`, `create_file`, `create_folder`, `rename_path`, `duplicate_file`, `delete_to_trash`, remove `current_root` helper)
- Modify: `src-tauri/src/watcher.rs`
- Modify: `src-tauri/src/open_files.rs`
- Modify: `src-tauri/src/mcp_server.rs` (`prepare_request` reads root/ready from registry for `"main"`)
- Modify: `src-tauri/src/windows.rs` (remove the dead-code allow; add `deliver_files`)
- Modify: `ui/app.js` (window-scoped `listen`)

**Interfaces:**
- Consumes: `windows::Registry`, `routing::FocusOrder` (Task 1).
- Produces:
  - `pub struct AppState { pub tree_root: Option<PathBuf>, pub initial_file: Option<PathBuf>, pub windows: Mutex<windows::Registry>, pub focus: Mutex<routing::FocusOrder>, pub tasklist_lock: Mutex<()> }`
  - `WatcherSlot::watch_file(&mut self, app: &AppHandle, label: &str, file: &Path) -> Result<(), String>`
  - `TreeWatcherSlot::watch_dirs(&mut self, app: &AppHandle, label: &str, dirs: Vec<PathBuf>) -> Result<(), String>`
  - `pub fn windows::deliver_files(app: &AppHandle, label: &str, paths: Vec<PathBuf>)`
  - `pub fn commands::resolve_initial_root(app: &AppHandle, explicit: Option<&Path>) -> PathBuf`
  - `fn commands::window_root(state: &State<'_, AppState>, window: &tauri::WebviewWindow) -> Result<PathBuf, String>`

- [ ] **Step 1: Write the failing test for root resolution**

`resolve_initial_root`'s IO wrapper isn't unit-testable, so factor the decision into a pure helper in `commands.rs` and test that:

```rust
#[test]
fn initial_root_prefers_explicit_then_existing_last_then_cwd() {
    let is_dir = |p: &Path| p == Path::new("/last");
    assert_eq!(
        pick_initial_root(Some(Path::new("/arg")), Some(PathBuf::from("/last")), PathBuf::from("/cwd"), is_dir),
        (PathBuf::from("/arg"), true)
    );
    assert_eq!(
        pick_initial_root(None, Some(PathBuf::from("/last")), PathBuf::from("/cwd"), is_dir),
        (PathBuf::from("/last"), false)
    );
    assert_eq!(
        pick_initial_root(None, Some(PathBuf::from("/gone")), PathBuf::from("/cwd"), is_dir),
        (PathBuf::from("/cwd"), false)
    );
}
```

The bool is "persist as `last_folder`". Only an explicit argv root is persisted here, which matches today: the restored `last_folder` is already stored and the bare cwd is intentionally never stored.

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test --lib initial_root_prefers`
Expected: FAIL, `pick_initial_root` not found.

- [ ] **Step 3: Reshape `AppState` and setup** (`lib.rs`)

Delete `PendingOpens`, `current_root`, `watcher`, `tree_watcher` and `opens`. New struct:

```rust
pub struct AppState {
    pub tree_root: Option<PathBuf>,
    pub initial_file: Option<PathBuf>,
    pub windows: Mutex<windows::Registry>,
    pub focus: Mutex<routing::FocusOrder>,
    /// Serializes task-list write-backs. Held only for the read-verify-write
    /// critical section so two rapid clicks can't interleave reads.
    pub tasklist_lock: Mutex<()>,
}
```

In `run`, build it with `windows: Mutex::new(windows::Registry::default())` and `focus: Mutex::new(routing::FocusOrder::default())`. In `setup`, before `menu::install`, register the config window:

```rust
let root = commands::resolve_initial_root(&handle, state.tree_root.as_deref());
state.windows.lock().unwrap().insert("main", root.clone());
state.focus.lock().unwrap().touch("main");
if let Some(window) = app.get_webview_window("main") {
    let _ = window.set_title(&windows::window_title(&root));
    let _ = window.show();
}
```

(Remove the old standalone `get_webview_window("main")` show block; the snippet above replaces it.)

- [ ] **Step 4: Commands** (`commands.rs`)

```rust
fn pick_initial_root(
    explicit: Option<&Path>,
    last: Option<PathBuf>,
    cwd: PathBuf,
    is_dir: impl Fn(&Path) -> bool,
) -> (PathBuf, bool) {
    if let Some(p) = explicit {
        return (p.to_path_buf(), true);
    }
    match last.filter(|p| is_dir(p)) {
        Some(p) => (p, false),
        None => (cwd, false),
    }
}

/// The root the first window opens on: explicit argv → last_folder (if still a
/// directory) → cwd. Only an explicit root is persisted; the bare cwd never is.
pub fn resolve_initial_root(app: &AppHandle, explicit: Option<&Path>) -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let (root, persist) = pick_initial_root(explicit, recent::load_last(app), cwd, |p| p.is_dir());
    if persist {
        recent::save_last(app, &root);
    }
    root
}

fn window_root(
    state: &State<'_, AppState>,
    window: &tauri::WebviewWindow,
) -> Result<PathBuf, String> {
    state
        .windows
        .lock()
        .map_err(|_| "window registry poisoned".to_string())?
        .root(window.label())
}
```

`get_initial_state(window: tauri::WebviewWindow, app, state)`: `tree_root = window_root(&state, &window).unwrap_or_else(|_| PathBuf::from("/"))`. Drop the old resolution and the `current_root` seeding. Keep `initial_file` only when `window.label() == "main"`. Session loading is unchanged in this task (Task 5 makes it per root).

`watch_tree(app, window, state, dirs)`:

```rust
let mut reg = state.windows.lock().map_err(|_| "window registry poisoned".to_string())?;
let w = reg.get_mut(window.label()).ok_or_else(|| crate::windows::NO_PROJECT_WINDOW.to_string())?;
w.tree_watcher.watch_dirs(&app, window.label(), paths)
```

`open_file(app, window, state, path)`: same shape, calling `w.watcher.watch_file(&app, window.label(), &p)`.

`frontend_ready(window, state) -> Result<Vec<String>, String>`: under the registry lock, set `ready = true` on the entry and drain `pending_files`. Setting `ready` and draining happen under the **same** lock `deliver_files` takes (the CLAUDE.md cold-Finder rule).

`remember_folder(app, window, state, path)`: if `p.is_dir()`, `recent::save_last`, `set_root(window.label(), p.clone())`, and `window.set_title(&windows::window_title(&p))`.

The five file-op commands replace `state: State<'_, AppState>` + `current_root(&state)?` with `window: tauri::WebviewWindow, state: State<'_, AppState>` + `window_root(&state, &window)?`. Delete the old `current_root` helper. `install_claude_hook`, `install_mcp_server` and `integration_status` use it too; for now switch them to `window_root(&state, &window)` as well (Task 3 changes them to resolve through the helper's owner).

- [ ] **Step 5: Watchers target a label** (`watcher.rs`)

Add `label: &str` to both `watch_file` and `watch_dirs`. Capture `let target = label.to_string();` next to `app_handle`, and replace `app_handle.emit("file-changed", payload.clone())` with `app_handle.emit_to(target.as_str(), "file-changed", payload.clone())`. Same for `tree-changed`.

- [ ] **Step 6: `deliver_files` and Finder opens** (`windows.rs`, `open_files.rs`)

```rust
/// Hand files to a project window: emit `open-file` if its frontend is ready,
/// else buffer them for its `frontend_ready`. The ready check and the push
/// happen under one lock, so nothing is lost between them.
pub fn deliver_files(app: &tauri::AppHandle, label: &str, paths: Vec<PathBuf>) {
    use tauri::{Emitter, Manager};
    let state = app.state::<crate::AppState>();
    let ready = {
        let mut reg = state.windows.lock().unwrap();
        let Some(w) = reg.get_mut(label) else { return };
        if !w.ready {
            w.pending_files.extend(paths.iter().cloned());
        }
        w.ready
    };
    if ready {
        for p in &paths {
            let _ = app.emit_to(label, "open-file", p.to_string_lossy().into_owned());
        }
    }
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}
```

`open_files::handle_opened` becomes: compute `paths`; return if empty; `crate::windows::deliver_files(handle, "main", paths)`. (Task 11 replaces `"main"` with routing.)

- [ ] **Step 7: MCP reads the registry** (`mcp_server.rs::prepare_request`)

Replace the `opens.ready` and `current_root` reads with:

```rust
let label = "main";
let (ready, root) = {
    let reg = app.state::<crate::AppState>().windows.lock().unwrap();
    match reg.get(label) {
        Some(w) => (w.ready, Some(w.root.clone())),
        None => (false, None),
    }
};
if !ready {
    return Err(mcp::STARTING_ERR.to_string());
}
validate(req, root.as_deref())?;
```

Replace `app.emit(event, payload)` with `app.emit_to(label, event, payload)`. (Tasks 4 and 13 replace the hard-coded label.)

- [ ] **Step 8: Frontend listens window-scoped** (`ui/app.js`)

Replace line 74:

```js
const { emit, emitTo } = window.__TAURI__.event;
// Window-scoped: the global event.listen() defaults to target Any and would
// also receive events the backend emit_to's at OTHER windows.
const currentWindow = window.__TAURI__.webviewWindow.getCurrentWebviewWindow();
const listen = (event, handler) => currentWindow.listen(event, handler);
```

Every existing `listen(...)` call then becomes window-scoped with no further edits. Broadcast events (`menu-check-updates`, `channel-changed`, `integration-changed`) still arrive, because `app.emit` reaches every listener. The manual check in Step 10 confirms this.

- [ ] **Step 9: Run tests + gate**

Run: `cd src-tauri && cargo test --lib initial_root_prefers && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cd .. && node --test ui/`
Expected: all pass. Remove the `#![allow(dead_code)]` lines from `windows.rs`/`routing.rs`; if clippy still flags `FocusOrder::remove` or `as_slice` as unused, keep a narrowly scoped `#[allow(dead_code)] // used from Task 6` on just those items.

- [ ] **Step 10: Manual one-window regression check** (`cd src-tauri && cargo build && cargo run -- ../README.md`)

Confirm unchanged behavior: live reload on an external edit; tree refresh on an external file create; New File / Rename / Delete in the tree; ⌘F; Actions ▸ Toggle Raw (menu → `edit-action`); MDViewer ▸ Check for Updates… (broadcast event still arrives); Claude Code Integration window shows the root; MCP `get_viewer_state` via `cargo test --test launch_smoke -- --ignored` after a bundle build (or `./scripts/smoke-test.sh`).

- [ ] **Step 11: Commit**

```bash
git add -A src-tauri/src ui/app.js
git commit -m "Key window state by label and scope events to their window

With one window nothing changes. This moves root, watchers and the
ready/pending-open buffer into a per-label registry, gives commands their
window from Tauri, and makes the frontend listen window-scoped (global
listen() receives emit_to events aimed at other windows)."
```

---

### Task 3: Helper windows act for an owner project window

**Files:**
- Modify: `src-tauri/src/menu.rs` (`open_settings` unchanged; `open_pdf_export_window`, `open_integration_window` take `owner: &str`)
- Modify: `src-tauri/src/commands.rs` (`helper_owner` command; `install_claude_hook`, `install_mcp_server`, `integration_status`, `show_integration_window` resolve through `project_label_for`)
- Modify: `src-tauri/src/lib.rs` (register `commands::helper_owner`)
- Modify: `ui/pdf-export.js`, `ui/app.js` (PDF export `emitTo`), `ui/claude-integration.js`

**Interfaces:**
- Consumes: `Registry::set_owner`, `Registry::project_label_for` (Task 1).
- Produces:
  - `#[tauri::command] pub fn helper_owner(window: tauri::WebviewWindow, state: State<'_, AppState>) -> Result<String, String>`
  - `pub fn menu::open_pdf_export_window(app: &AppHandle, owner: &str)` (macOS)
  - `pub fn menu::open_integration_window(app: &AppHandle, owner: &str)`
  - Event `pdf-export-owner` (payload: owner label string), emitted to `pdf-export` when an already-open export window changes owner.

- [ ] **Step 1: Owner-aware root helper + test**

In `commands.rs`, add the pure helper used by the three integration commands and test it:

```rust
fn owner_root(reg: &crate::windows::Registry, caller: &str) -> Result<PathBuf, String> {
    let label = reg.project_label_for(caller)?;
    reg.root(&label)
}

#[test]
fn owner_root_resolves_helper_to_owner_root() {
    let mut reg = crate::windows::Registry::default();
    reg.insert("project-1", PathBuf::from("/b"));
    reg.set_owner("claude-integration", "project-1");
    assert_eq!(owner_root(&reg, "claude-integration").unwrap(), PathBuf::from("/b"));
    assert!(owner_root(&reg, "preferences").is_err());
}
```

Run: `cargo test --lib owner_root_resolves` → FAIL, then implement → PASS.

- [ ] **Step 2: Commands**

`install_claude_hook`, `install_mcp_server`: add `window: tauri::WebviewWindow`; `let root = owner_root(&state.windows.lock().map_err(|_| "window registry poisoned".to_string())?, window.label())?;`.

`integration_status(window, state)`: same resolution; on `Err` return the existing "no folder" shape (`root: None`).

```rust
#[tauri::command]
pub fn helper_owner(window: tauri::WebviewWindow, state: State<'_, AppState>) -> Result<String, String> {
    state
        .windows
        .lock()
        .map_err(|_| "window registry poisoned".to_string())?
        .project_label_for(window.label())
}

#[tauri::command]
pub fn show_integration_window(app: AppHandle, window: tauri::WebviewWindow) {
    crate::menu::open_integration_window(&app, window.label());
}
```

Register `commands::helper_owner` in `lib.rs`'s `generate_handler!`.

- [ ] **Step 3: Menu records owners**

In `menu.rs`, add

```rust
fn front_project(app: &AppHandle) -> Option<String> {
    let state = app.state::<crate::AppState>();
    let front = state.focus.lock().ok()?.front().map(str::to_string);
    front.or_else(|| state.windows.lock().ok()?.labels().into_iter().next())
}
```

`"export-pdf"` → `if let Some(o) = front_project(app) { open_pdf_export_window(app, &o) }`; the same for `"claude-integration"`.

```rust
#[cfg(target_os = "macos")]
pub fn open_pdf_export_window(app: &AppHandle, owner: &str) {
    app.state::<crate::AppState>().windows.lock().unwrap().set_owner("pdf-export", owner);
    if let Some(win) = app.get_webview_window("pdf-export") {
        let _ = app.emit_to("pdf-export", "pdf-export-owner", owner.to_string());
        let _ = win.set_focus();
        return;
    }
    // builder unchanged
}

pub fn open_integration_window(app: &AppHandle, owner: &str) {
    app.state::<crate::AppState>().windows.lock().unwrap().set_owner("claude-integration", owner);
    if let Some(win) = app.get_webview_window("claude-integration") {
        let _ = app.emit_to("claude-integration", "integration-changed", ());
        let _ = win.set_focus();
        return;
    }
    // builder unchanged
}
```

- [ ] **Step 4: PDF export window addresses its owner** (`ui/pdf-export.js`)

```js
const { emitTo } = window.__TAURI__.event;
const currentWindow = window.__TAURI__.webviewWindow.getCurrentWebviewWindow();
const listen = (event, handler) => currentWindow.listen(event, handler);
let owner = null;
```

In `init()`, first: `owner = await invoke("helper_owner").catch(() => null);` and `await listen("pdf-export-owner", (ev) => { owner = ev.payload; schedulePreview(); });`. Replace every `emit("pdf-export-request-preview", …)` / `emit("pdf-export-run", …)` with `if (owner) emitTo(owner, "pdf-export-request-preview", …)` (same for run). The `listen("pdf-export-…")` handlers are unchanged but now scoped.

In `ui/app.js`, replace every `emit("pdf-export-preview-html", …)`, `emit("pdf-export-active-name", …)` and `emit("pdf-export-done", …)` with `emitTo("pdf-export", …)` (lines ~459, 464, 477, 483, 506, 514, 516, 2257, 2260). Remove `emit` from the destructure if it is now unused.

- [ ] **Step 5: Integration window refreshes on owner change** (`ui/claude-integration.js`)

If it doesn't already listen for `integration-changed`, add after its first status load:

```js
const currentWindow = window.__TAURI__.webviewWindow.getCurrentWebviewWindow();
currentWindow.listen("integration-changed", () => refresh());
```

(`refresh` = the function that calls `invoke("integration_status")` and renders. Rename to match the file's existing function.)

- [ ] **Step 6: Gate, manual check, commit**

Gate as in Global Constraints. Manual: PDF export preview renders the active document and Export writes a file; the Integration window shows the root and Install works.

```bash
git add -A src-tauri/src ui
git commit -m "Route helper windows to the project window that opened them"
```

---

### Task 4: `McpPending` entries belong to a window

**Files:**
- Modify: `src-tauri/src/mcp_server.rs`
- Modify: `src-tauri/src/commands.rs` (`mcp_respond`, `mcp_review_result`)

**Interfaces:**
- Produces:
  - `McpPending::register(&self, label: &str, tool: &str) -> (u64, mpsc::Receiver<Handoff>)`
  - `McpPending::resolve(&self, id: u64, caller: &str, reply: Reply) -> Result<(), String>`
  - `McpPending::abandon_window(&self, label: &str)`

- [ ] **Step 1: Failing tests** (add to `mcp_server.rs` tests; update existing tests to pass `"main"` and a tool)

```rust
#[test]
fn resolve_from_another_window_is_rejected_and_keeps_entry() {
    let p = McpPending::default();
    let (id, _rx) = p.register("main", "open_document");
    let err = p.resolve(id, "project-1", Ok("x".into())).unwrap_err();
    assert!(err.contains("another window"), "got: {err}");
    // Still resolvable by its own window (rx alive, but no ack thread → times out
    // as "did not acknowledge", which proves the entry was not removed).
    assert!(p.resolve(id, "main", Ok("x".into())).unwrap_err().contains("acknowledge"));
}

#[test]
fn abandon_window_declines_reviews_and_errors_others() {
    let p = McpPending::default();
    let (_r, review_rx) = p.register("main", "request_review");
    let (_o, open_rx) = p.register("main", "open_document");
    let (_k, keep_rx) = p.register("project-1", "open_document");
    p.abandon_window("main");
    let (reply, _ack) = review_rx.recv().unwrap();
    assert_eq!(reply, Ok(crate::mcp::review_reply_text(None)));
    let (reply, _ack) = open_rx.recv().unwrap();
    assert_eq!(reply, Err("MDViewer window closed".to_string()));
    assert!(keep_rx.try_recv().is_err());
}
```

(The first test waits the 5 s ack timeout. That's acceptable, but if it's too slow, make the ack timeout a `const ACK_TIMEOUT` and add `#[cfg(test)]` shortening it to 200 ms.)

- [ ] **Step 2: Verify fail** — `cargo test --lib mcp_server::` → compile errors.

- [ ] **Step 3: Implement**

```rust
struct Entry {
    label: String,
    tool: String,
    tx: mpsc::Sender<Handoff>,
}

#[derive(Default)]
pub struct McpPending {
    next_id: AtomicU64,
    waiting: Mutex<HashMap<u64, Entry>>,
}

impl McpPending {
    pub fn register(&self, label: &str, tool: &str) -> (u64, mpsc::Receiver<Handoff>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = mpsc::channel();
        self.waiting.lock().unwrap().insert(
            id,
            Entry { label: label.to_string(), tool: tool.to_string(), tx },
        );
        (id, rx)
    }

    pub fn resolve(&self, id: u64, caller: &str, reply: Reply) -> Result<(), String> {
        let tx = {
            let mut waiting = self.waiting.lock().unwrap();
            match waiting.get(&id) {
                None => return Err(format!("unknown MCP request id {id}")),
                Some(e) if e.label != caller => {
                    return Err(format!("MCP request {id} belongs to another window"))
                }
                Some(_) => waiting.remove(&id).unwrap().tx,
            }
        };
        // ack wait unchanged
    }

    /// A window closed: answer everything it owed. A review counts as declined
    /// (a success, so Claude proceeds, as when the review tab is closed).
    pub fn abandon_window(&self, label: &str) {
        let gone: Vec<Entry> = {
            let mut waiting = self.waiting.lock().unwrap();
            let ids: Vec<u64> = waiting.iter().filter(|(_, e)| e.label == label).map(|(id, _)| *id).collect();
            ids.into_iter().filter_map(|id| waiting.remove(&id)).collect()
        };
        for e in gone {
            let reply = if e.tool == "request_review" {
                Ok(crate::mcp::review_reply_text(None))
            } else {
                Err("MDViewer window closed".to_string())
            };
            let (ack_tx, _ack_rx) = mpsc::channel();
            let _ = e.tx.send((reply, ack_tx));
        }
    }
}
```

`prepare_request`: `pending.register(label, &req.tool)`.

`commands.rs`: `mcp_respond(window: tauri::WebviewWindow, pending, request_id, text, is_error)` → `pending.resolve(request_id, window.label(), reply)`; same for `mcp_review_result`.

`abandon_window` is called from `on_destroyed` in Task 6.

- [ ] **Step 4: Tests pass** — `cargo test --lib mcp_server::`.

- [ ] **Step 5: Security review for Phase 1.** Dispatch the `security-reviewer` agent on the Phase 1 diff (`git diff main...HEAD`). Focus: label → state scoping, no fallback to another window's root, helper `owner` handling, `mcp_respond` cross-window rejection, window-scoped listeners. Fix CONFIRMED findings before committing.

- [ ] **Step 6: Gate + commit**

```bash
git add -A src-tauri/src
git commit -m "Tie in-flight MCP requests to the window they were sent to"
```

---

## Phase 2 — multiple windows

### Task 5: Per-root tab sessions (+ legacy migration)

**Files:**
- Modify: `src-tauri/src/recent.rs`
- Modify: `src-tauri/src/commands.rs` (`get_initial_state`, `save_session`)

**Interfaces:**
- Produces:
  - `#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)] pub struct Session { pub tabs: Vec<PathBuf>, pub active: Option<usize>, pub touched: u64 }`
  - `pub fn recent::load_session(app: &AppHandle, root: &Path) -> (Vec<PathBuf>, Option<usize>)`
  - `pub fn recent::save_session(app: &AppHandle, root: &Path, tabs: &[PathBuf], active: Option<usize>)`
  - `const MAX_SESSIONS: usize = 30;`
  - Private: `Store::migrate_legacy(&mut self)`, `fn cap_sessions(map: &mut BTreeMap<PathBuf, Session>, max: usize)`

- [ ] **Step 1: Failing tests** (`recent.rs` tests)

```rust
#[test]
fn legacy_session_migrates_under_last_folder() {
    let mut s: Store = serde_json::from_str(
        r#"{"folders":[],"last_folder":"/p","open_tabs":["/p/a.md"],"active_tab":0}"#,
    ).unwrap();
    s.migrate_legacy();
    assert_eq!(s.sessions[Path::new("/p")].tabs, vec![PathBuf::from("/p/a.md")]);
    assert_eq!(s.sessions[Path::new("/p")].active, Some(0));
    assert!(s.open_tabs.is_empty());
    let json = serde_json::to_string(&s).unwrap();
    assert!(!json.contains("open_tabs"));
}

#[test]
fn legacy_session_without_last_folder_is_dropped() {
    let mut s: Store = serde_json::from_str(r#"{"folders":[],"open_tabs":["/x.md"]}"#).unwrap();
    s.migrate_legacy();
    assert!(s.sessions.is_empty());
    assert!(s.open_tabs.is_empty());
}

#[test]
fn cap_sessions_evicts_least_recently_touched() {
    let mut m = BTreeMap::new();
    for i in 0..5u64 {
        m.insert(PathBuf::from(format!("/r{i}")), Session { tabs: vec![], active: None, touched: i });
    }
    cap_sessions(&mut m, 3);
    let keys: Vec<_> = m.keys().cloned().collect();
    assert_eq!(keys, vec![PathBuf::from("/r2"), PathBuf::from("/r3"), PathBuf::from("/r4")]);
}
```

Update the existing `store_round_trips_session_fields` test to round-trip `sessions` instead of `open_tabs`/`active_tab`, and keep `deserializes_legacy_store_without_session_fields` (it must still load).

- [ ] **Step 2: Verify fail** — `cargo test --lib recent::`.

- [ ] **Step 3: Implement**

```rust
const MAX_SESSIONS: usize = 30;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub tabs: Vec<PathBuf>,
    pub active: Option<usize>,
    #[serde(default)]
    pub touched: u64,
}
```

In `Store`: keep `open_tabs` / `active_tab` with `#[serde(default, skip_serializing_if = "Vec::is_empty")]` / `#[serde(default, skip_serializing_if = "Option::is_none")]` (read-only legacy), and add `#[serde(default)] sessions: BTreeMap<PathBuf, Session>`.

```rust
impl Store {
    /// Pre-multi-window stores kept one tab session next to last_folder.
    fn migrate_legacy(&mut self) {
        let tabs = std::mem::take(&mut self.open_tabs);
        let active = self.active_tab.take();
        if tabs.is_empty() {
            return;
        }
        if let Some(root) = self.last_folder.clone() {
            self.sessions.entry(root).or_insert(Session { tabs, active, touched: 0 });
        }
    }
}

fn cap_sessions(map: &mut BTreeMap<PathBuf, Session>, max: usize) {
    while map.len() > max {
        let oldest = map.iter().min_by_key(|(_, s)| s.touched).map(|(k, _)| k.clone());
        match oldest {
            Some(k) => { map.remove(&k); }
            None => break,
        }
    }
}
```

`load_store` calls `store.migrate_legacy()` after deserializing. New session functions (replacing the old two):

```rust
pub fn load_session(app: &AppHandle, root: &Path) -> (Vec<PathBuf>, Option<usize>) {
    let store = load_store(app);
    store
        .sessions
        .get(&canonical_or_keep(root))
        .map(|s| (s.tabs.clone(), s.active))
        .unwrap_or_default()
}

pub fn save_session(app: &AppHandle, root: &Path, tabs: &[PathBuf], active: Option<usize>) {
    let mut store = load_store(app);
    let touched = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    store.sessions.insert(
        canonical_or_keep(root),
        Session { tabs: tabs.to_vec(), active, touched },
    );
    cap_sessions(&mut store.sessions, MAX_SESSIONS);
    write_store(app, &store);
}
```

`commands.rs`: `save_session(app, window, state, tabs, active)` → `let root = window_root(&state, &window)?; recent::save_session(&app, &root, &paths, active); Ok(())` (return `Result<(), String>`). `get_initial_state` → `recent::load_session(&app, &tree_root)`.

**Frontend:** before `setTreeRoot` changes the root, the outgoing root's session must be saved *under the outgoing root*. `persistSession()` already runs on every tab change, so the stored session is current. The only gap is the backend switching root before the save. Order in `setTreeRoot` stays `rememberFolder(path)` last, and `persistSession()` is invoked **before** `treeRoot = path` is assigned:

```js
async function setTreeRoot(path) {
  if (isSearchModeOpen()) exitSearchMode();
  persistSession();
  treeRoot = path;
  // …rest unchanged
}
```

- [ ] **Step 4: Tests pass, gate, manual** (switch folders via Open Recent and back: tabs return), **commit**

```bash
git add -A src-tauri/src/recent.rs src-tauri/src/commands.rs ui/app.js
git commit -m "Remember tab sessions per project root"
```

---

### Task 6: Window lifecycle, File ▸ New Window, MRU-routed menu

**Files:**
- Modify: `src-tauri/src/windows.rs` (`create_project_window`, `emit_to_front`, `on_destroyed`, `open_folder_in_new_window`)
- Modify: `src-tauri/src/lib.rs` (`on_window_event`)
- Modify: `src-tauri/src/menu.rs` (New Window item; window-scoped emits)
- Modify: `src-tauri/capabilities/default.json`

**Interfaces:**
- Consumes: `Registry`, `FocusOrder`, `McpPending::abandon_window`.
- Produces:
  - `pub struct Bounds { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }` (serde, in `windows.rs`)
  - `pub fn create_project_window(app: &AppHandle, root: PathBuf, bounds: Option<Bounds>) -> Result<String, String>`
  - `pub fn emit_to_front<S: serde::Serialize + Clone>(app: &AppHandle, event: &str, payload: S)`
  - `pub fn front_label(app: &AppHandle) -> Option<String>`
  - `pub fn on_destroyed(app: &AppHandle, label: &str)`
  - `pub fn open_folder_in_new_window(app: &AppHandle, root: PathBuf)` (focus existing if that root is open)
  - `pub fn cascade(from: Option<(f64, f64)>) -> Option<(f64, f64)>` (pure: `+24` on both axes)

- [ ] **Step 1: Failing test for the one pure piece**

```rust
#[test]
fn cascade_offsets_from_focused_window() {
    assert_eq!(cascade(Some((100.0, 50.0))), Some((124.0, 74.0)));
    assert_eq!(cascade(None), None);
}
```

- [ ] **Step 2: Implement lifecycle** (`windows.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Bounds { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }

pub fn cascade(from: Option<(f64, f64)>) -> Option<(f64, f64)> {
    from.map(|(x, y)| (x + 24.0, y + 24.0))
}

pub fn front_label(app: &tauri::AppHandle) -> Option<String> {
    use tauri::Manager;
    let state = app.state::<crate::AppState>();
    let labels = state.windows.lock().ok()?.labels();
    let front = state.focus.lock().ok()?.front().map(str::to_string);
    front.filter(|l| labels.contains(l)).or_else(|| labels.into_iter().next())
}

pub fn emit_to_front<S: serde::Serialize + Clone>(app: &tauri::AppHandle, event: &str, payload: S) {
    use tauri::Emitter;
    if let Some(label) = front_label(app) {
        let _ = app.emit_to(label.as_str(), event, payload);
    }
}

/// Registers state BEFORE building so the new webview's first
/// get_initial_state finds its entry; removes it again if the build fails.
pub fn create_project_window(
    app: &tauri::AppHandle,
    root: PathBuf,
    bounds: Option<Bounds>,
) -> Result<String, String> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
    let state = app.state::<crate::AppState>();
    let label = {
        let mut reg = state.windows.lock().map_err(|_| "window registry poisoned".to_string())?;
        let label = reg.next_label();
        reg.insert(&label, root.clone());
        label
    };
    let from = front_label(app)
        .and_then(|l| app.get_webview_window(&l))
        .and_then(|w| Some((w.outer_position().ok()?, w.scale_factor().ok()?)))
        .map(|(p, s)| (p.x as f64 / s, p.y as f64 / s));
    let mut b = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title(window_title(&root))
        .min_inner_size(600.0, 400.0)
        .resizable(true);
    b = match bounds {
        Some(bb) => b.inner_size(bb.w, bb.h).position(bb.x, bb.y),
        None => {
            let b = b.inner_size(1200.0, 800.0);
            match cascade(from) {
                Some((x, y)) => b.position(x, y),
                None => b,
            }
        }
    };
    if let Err(e) = b.build() {
        state.windows.lock().unwrap().remove(&label);
        return Err(format!("cannot create window: {e}"));
    }
    Ok(label)
}

pub fn open_folder_in_new_window(app: &tauri::AppHandle, root: PathBuf) {
    use tauri::Manager;
    let root = root.canonicalize().unwrap_or(root);
    let existing = app.state::<crate::AppState>().windows.lock().unwrap().label_with_root(&root);
    match existing {
        Some(label) => {
            if let Some(w) = app.get_webview_window(&label) {
                let _ = w.set_focus();
            }
        }
        None => {
            if let Err(e) = create_project_window(app, root, None) {
                eprintln!("mdviewer: {e}");
            }
        }
    }
}

pub fn on_destroyed(app: &tauri::AppHandle, label: &str) {
    use tauri::Manager;
    let state = app.state::<crate::AppState>();
    let removed = state.windows.lock().unwrap().remove(label);
    if removed.is_none() {
        return; // helper window
    }
    state.focus.lock().unwrap().remove(label);
    app.state::<crate::mcp_server::McpPending>().abandon_window(label);
    // Dropping `removed` drops its WatcherSlot/TreeWatcherSlot, which stops them.
}
```

Before `create_project_window` compiles, check that `WebviewWindowBuilder::position` / `inner_size` take logical `f64` in Tauri 2.11 (they do in 2.x; adjust if the compiler disagrees).

- [ ] **Step 3: Wire window events** (`lib.rs`, on the builder before `.build`)

```rust
.on_window_event(|window, event| {
    let label = window.label().to_string();
    let app = window.app_handle();
    match event {
        tauri::WindowEvent::Focused(true) => {
            let state = app.state::<AppState>();
            let is_project = state.windows.lock().unwrap().get(&label).is_some();
            if is_project {
                state.focus.lock().unwrap().touch(&label);
            }
        }
        tauri::WindowEvent::Destroyed => windows::on_destroyed(app, &label),
        _ => {}
    }
})
```

- [ ] **Step 4: Menu** (`menu.rs`)

- Add `new-window` → `MenuItemBuilder::with_id("new-window", "New Window").accelerator("CmdOrCtrl+Shift+N")`, first item in the File submenu, followed by a separator.
- Handler `"new-window"` → `prompt_new_window(app.clone())`:

```rust
fn prompt_new_window(app: AppHandle) {
    app.dialog().file().pick_folder(move |chosen| {
        if let Some(p) = chosen.and_then(|f| f.as_path().map(PathBuf::from)) {
            recent::push(&app, &p);
            let _ = rebuild(&app);
            crate::windows::open_folder_in_new_window(&app, p);
        }
    });
}
```

- Replace every window-scoped `app.emit("edit-action" | "export" | "menu-install-cli" | "open-file" | "open-folder", …)` with `crate::windows::emit_to_front(app, …, …)`. Keep `menu-check-updates` as `app.emit` (app-wide). `front_project` from Task 3 is replaced by `crate::windows::front_label`.

- [ ] **Step 5: Capabilities** (`capabilities/default.json`)

```json
"windows": ["main", "project-*", "preferences", "claude-integration", "pdf-export"],
```

Add `"core:window:allow-destroy"` to `permissions` (the close guard in Task 7 needs it).

- [ ] **Step 6: Gate + manual**

`cargo build && cargo run -- ../README.md`. ⌘⇧N → pick another folder → second window titled `<folder> — MDViewer`. Then:
1. ⌘F / Actions ▸ Toggle Raw affect only the focused window.
2. Edit a file open in window A from another editor; window B doesn't flicker (DevTools console in B shows no `file-changed`).
3. ⌘⇧N on a folder that's already open focuses that window instead of opening a duplicate.
4. Closing B leaves A working; closing A quits as before.
5. The MRU fallback works: close the focused window, then a menu action goes to the remaining one.

- [ ] **Step 7: Commit**

```bash
git add -A src-tauri
git commit -m "Add File > New Window and route menu actions to the focused window"
```

---

### Task 7: Frontend window hygiene — close guard and cross-window theme sync

**Files:**
- Create: `ui/windowscope.js`, `ui/windowscope.test.js`
- Modify: `ui/app.js`

**Interfaces:**
- Produces: `export function anyDirty(tabs)` → boolean; `export function themeFromStorageEvent(e, key, isValidTheme)` → `"light" | "dark" | null`.

- [ ] **Step 1: Failing tests** (`ui/windowscope.test.js`)

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { anyDirty, themeFromStorageEvent } from "./windowscope.js";

const valid = (v) => v === "light" || v === "dark";

test("anyDirty is true only when some tab is dirty", () => {
  assert.equal(anyDirty([]), false);
  assert.equal(anyDirty([{ dirty: false }, { dirty: true }]), true);
  assert.equal(anyDirty([{ dirty: false }]), false);
});

test("themeFromStorageEvent returns the new theme for our key only", () => {
  assert.equal(themeFromStorageEvent({ key: "mdviewer.theme", newValue: "dark" }, "mdviewer.theme", valid), "dark");
  assert.equal(themeFromStorageEvent({ key: "other", newValue: "dark" }, "mdviewer.theme", valid), null);
  assert.equal(themeFromStorageEvent({ key: "mdviewer.theme", newValue: "purple" }, "mdviewer.theme", valid), null);
  assert.equal(themeFromStorageEvent({ key: "mdviewer.theme", newValue: null }, "mdviewer.theme", valid), null);
});
```

Run: `node --test ui/windowscope.test.js` → FAIL (module missing).

- [ ] **Step 2: Implement** (`ui/windowscope.js`)

```js
// Pure helpers for per-window behavior in a multi-window app (unit-tested).

export function anyDirty(tabs) {
  return tabs.some((t) => t.dirty);
}

// A `storage` event fires in every OTHER same-origin window when one window
// writes localStorage — that is how a theme toggle reaches the rest.
export function themeFromStorageEvent(e, key, isValidTheme) {
  if (e.key !== key || !isValidTheme(e.newValue)) return null;
  return e.newValue;
}
```

- [ ] **Step 3: Wire in `app.js`** (inside `init()`, after the listeners)

```js
import { anyDirty, themeFromStorageEvent } from "./windowscope.js";
// …
window.addEventListener("storage", (e) => {
  const theme = themeFromStorageEvent(e, THEME_KEY, isValidTheme);
  if (theme && theme !== currentTheme) applyTheme(theme);
});

currentWindow.onCloseRequested(async (event) => {
  if (anyDirty(tabs)) {
    const discard = await dialogApi.ask(
      "This window has unsaved changes. Close it and discard them?",
      { title: "MDViewer", kind: "warning", okLabel: "Discard", cancelLabel: "Cancel" },
    );
    if (!discard) {
      event.preventDefault();
      return;
    }
  }
  persistSession();
});
```

(Check `isValidTheme` is imported from `./theme.js` already; it is used by `hasThemePref`.)

- [ ] **Step 4: Gate, `cargo build`, manual** — toggle ☾/☀ in window A and window B follows; edit a file without saving in B, close B, and the prompt appears; Cancel keeps it open. **Commit**

```bash
git add ui/windowscope.js ui/windowscope.test.js ui/app.js
git commit -m "Guard window close on unsaved edits and sync theme across windows"
```

---

### Task 8: Open Folder replaces the root, or focuses the window that has it

**Files:**
- Modify: `src-tauri/src/routing.rs` (`open_folder_target`)
- Modify: `src-tauri/src/commands.rs` (`remember_folder` returns a result)
- Modify: `ui/app.js` (`setTreeRoot` respects it)

**Interfaces:**
- Produces:
  - `pub enum FolderTarget { Adopt, Focus(String) }`
  - `pub fn open_folder_target(caller: &str, root: &Path, roots: &[(String, PathBuf)]) -> FolderTarget`
  - `remember_folder` → `Result<Option<String>, String>`: `Ok(None)` = adopted; `Ok(Some(label))` = focused that window instead.

- [ ] **Step 1: Failing tests** (`routing.rs`)

```rust
#[test]
fn open_folder_adopts_when_no_other_window_has_it() {
    let roots = vec![("main".to_string(), PathBuf::from("/a"))];
    assert_eq!(open_folder_target("main", Path::new("/b"), &roots), FolderTarget::Adopt);
    assert_eq!(open_folder_target("main", Path::new("/a"), &roots), FolderTarget::Adopt);
}

#[test]
fn open_folder_focuses_other_window_with_same_root() {
    let roots = vec![
        ("main".to_string(), PathBuf::from("/a")),
        ("project-1".to_string(), PathBuf::from("/b")),
    ];
    assert_eq!(
        open_folder_target("main", Path::new("/b"), &roots),
        FolderTarget::Focus("project-1".into())
    );
}
```

- [ ] **Step 2: Implement**

```rust
#[derive(Debug, PartialEq)]
pub enum FolderTarget {
    Adopt,
    Focus(String),
}

pub fn open_folder_target(caller: &str, root: &Path, roots: &[(String, PathBuf)]) -> FolderTarget {
    roots
        .iter()
        .find(|(l, r)| l != caller && r == root)
        .map(|(l, _)| FolderTarget::Focus(l.clone()))
        .unwrap_or(FolderTarget::Adopt)
}
```

`remember_folder(app, window, state, path) -> Result<Option<String>, String>`: canonicalize `p` (fallback to itself); compute the target from `reg.roots()`; on `Focus(l)`, focus that window and return `Ok(Some(l))` **without** changing this window's root; on `Adopt`, do Task 2's adopt steps and return `Ok(None)`.

`ui/app.js`: `setTreeRoot` currently commits the new root locally before calling `rememberFolder`. Reorder so the backend decides first:

```js
async function setTreeRoot(path) {
  let focusedElsewhere = null;
  try {
    focusedElsewhere = await invoke("remember_folder", { path });
  } catch (e) {
    console.error("remember_folder failed", e);
  }
  if (focusedElsewhere) return;
  if (isSearchModeOpen()) exitSearchMode();
  persistSession();
  treeRoot = path;
  // …rest unchanged, minus the old trailing rememberFolder(path)
}
```

`persistSession()` must still save under the **old** root. `remember_folder` has already switched the backend root at this point, so move the save *before* the invoke:

```js
async function setTreeRoot(path) {
  persistSession(); // saved under the outgoing root, before the backend switches
  let focusedElsewhere = null;
  // …
```

The cold-Finder `rememberFolder(treeRoot)` in `init()` stays as a fire-and-forget; a brand-new window never has a duplicate.

- [ ] **Step 3: Tests, gate, manual** (two windows on /a and /b; Open Recent ▸ /b from window A focuses B and A is unchanged; Open Recent ▸ /c from A retargets A and its title updates), **commit**

```bash
git add -A src-tauri/src ui/app.js
git commit -m "Focus the window that already shows a folder instead of duplicating it"
```

---

### Task 9: Restore all windows on launch

**Files:**
- Modify: `src-tauri/src/recent.rs` (`windows` snapshot, `restore_windows`)
- Modify: `src-tauri/src/windows.rs` (`snapshot`)
- Modify: `src-tauri/src/lib.rs` (setup restore; `ExitRequested`; last-window snapshot)

**Interfaces:**
- Consumes: `windows::Bounds`, `create_project_window`.
- Produces:
  - `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)] pub struct SavedWindow { pub root: PathBuf, pub bounds: Option<windows::Bounds> }`
  - `pub fn recent::load_windows(app) -> Vec<SavedWindow>`, `pub fn recent::save_windows(app, &[SavedWindow])`
  - `pub fn recent::restore_windows(saved: Vec<SavedWindow>, is_dir: impl Fn(&Path) -> bool) -> Vec<SavedWindow>` (pure)
  - `pub fn windows::snapshot(app: &AppHandle) -> Vec<recent::SavedWindow>` (focus order first, so the most recent window becomes `main` on relaunch)

- [ ] **Step 1: Failing tests** (`recent.rs`)

```rust
#[test]
fn restore_windows_drops_missing_roots_and_dedupes() {
    let w = |r: &str| SavedWindow { root: PathBuf::from(r), bounds: None };
    let kept = restore_windows(vec![w("/a"), w("/gone"), w("/b"), w("/a")], |p| p != Path::new("/gone"));
    assert_eq!(kept, vec![w("/a"), w("/b")]);
}

#[test]
fn store_without_windows_field_loads() {
    let s: Store = serde_json::from_str(r#"{"folders":[]}"#).unwrap();
    assert!(s.windows.is_empty());
}
```

- [ ] **Step 2: Implement `recent.rs`**

`Store` gets `#[serde(default)] windows: Vec<SavedWindow>`.

```rust
pub fn restore_windows(saved: Vec<SavedWindow>, is_dir: impl Fn(&Path) -> bool) -> Vec<SavedWindow> {
    let mut seen = std::collections::HashSet::new();
    saved
        .into_iter()
        .filter(|w| is_dir(&w.root) && seen.insert(w.root.clone()))
        .collect()
}
```

`load_windows` / `save_windows` follow the `load_channel`/`save_channel` pattern.

- [ ] **Step 3: `windows::snapshot`**

```rust
pub fn snapshot(app: &tauri::AppHandle) -> Vec<crate::recent::SavedWindow> {
    use tauri::Manager;
    let state = app.state::<crate::AppState>();
    let roots: std::collections::HashMap<String, PathBuf> =
        state.windows.lock().unwrap().roots().into_iter().collect();
    let mut order: Vec<String> = state.focus.lock().unwrap().as_slice().to_vec();
    for l in roots.keys() {
        if !order.contains(l) {
            order.push(l.clone());
        }
    }
    order
        .into_iter()
        .filter_map(|l| {
            let root = roots.get(&l)?.clone();
            let bounds = app.get_webview_window(&l).and_then(|w| {
                let s = w.scale_factor().ok()?;
                let p = w.outer_position().ok()?.to_logical::<f64>(s);
                let z = w.inner_size().ok()?.to_logical::<f64>(s);
                Some(Bounds { x: p.x, y: p.y, w: z.width, h: z.height })
            });
            Some(crate::recent::SavedWindow { root, bounds })
        })
        .collect()
}
```

- [ ] **Step 4: Save points** (`lib.rs`, `windows::on_destroyed`)

- In `app.run(move |handle, event| …)`, add `tauri::RunEvent::ExitRequested { .. } => recent::save_windows(handle, &windows::snapshot(handle))`. This handles ⌘Q, which destroys windows *after* ExitRequested.
- In `on_destroyed`, **before** removing the label: if the registry has exactly one project window and it is `label`, `recent::save_windows(app, &snapshot(app))`. Closing a non-last window changes nothing.
- Guard against the ⌘Q teardown overwriting the snapshot as windows are destroyed one by one: add `pub quitting: AtomicBool` to `AppState`, set it in `ExitRequested`, and skip the last-window save in `on_destroyed` when it is set.

- [ ] **Step 5: Restore in setup** (`lib.rs`)

```rust
let saved = recent::restore_windows(recent::load_windows(&handle), |p| p.is_dir());
let main_root = match (&state.tree_root, saved.first()) {
    (Some(_), _) | (None, None) => commands::resolve_initial_root(&handle, state.tree_root.as_deref()),
    (None, Some(first)) => first.root.clone(),
};
// register + title "main" with main_root (Task 2 code), and apply saved bounds if
// main_root came from saved[0]: window.set_position / set_size (logical).
for w in saved.iter().skip(if state.tree_root.is_none() { 1 } else { 0 }) {
    if w.root != main_root {
        let _ = windows::create_project_window(&handle, w.root.clone(), w.bounds);
    }
}
```

With an explicit argv root, `main` shows that root and every saved window is restored alongside it (skipping a duplicate root). Task 12 later routes argv files into the right restored window.

- [ ] **Step 6: Tests, gate, manual** — open three windows, ⌘Q, relaunch: all three return with their tabs and positions; delete one root folder while quit, relaunch: two return; close windows until one is left, close it, relaunch: that one returns. **Commit**

```bash
git add -A src-tauri/src
git commit -m "Restore every open window on relaunch"
```

---

## Phase 3 — routing

### Task 10: `routing::route` (pure) + `fallback_root`

**Files:**
- Modify: `src-tauri/src/routing.rs`
- Modify: `src-tauri/src/git.rs` (make `git_toplevel` `pub(crate)`)

**Interfaces:**
- Produces:
  - `#[derive(Debug, PartialEq)] pub enum Route { Window(String), NewWindow(PathBuf) }`
  - `pub fn route(path: &Path, roots: &[(String, PathBuf)], mru: &[String], fallback_root: impl Fn(&Path) -> PathBuf) -> Route`
  - `pub fn fallback_root(path: &Path) -> PathBuf` (IO: `git::git_toplevel(parent)` else parent; for a directory, the directory itself)

- [ ] **Step 1: Failing tests**

```rust
fn roots(v: &[(&str, &str)]) -> Vec<(String, PathBuf)> {
    v.iter().map(|(l, r)| (l.to_string(), PathBuf::from(r))).collect()
}
fn mru(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}
fn fb(p: &Path) -> PathBuf {
    p.parent().unwrap().to_path_buf()
}

#[test]
fn routes_to_containing_window() {
    let r = roots(&[("main", "/a"), ("project-1", "/b")]);
    assert_eq!(route(Path::new("/b/x.md"), &r, &mru(&["main"]), fb), Route::Window("project-1".into()));
}

#[test]
fn longest_root_wins_for_nested_roots() {
    let r = roots(&[("main", "/repo"), ("project-1", "/repo/docs")]);
    assert_eq!(route(Path::new("/repo/docs/p.md"), &r, &mru(&["main"]), fb), Route::Window("project-1".into()));
    assert_eq!(route(Path::new("/repo/src/p.md"), &r, &mru(&["project-1"]), fb), Route::Window("main".into()));
}

#[test]
fn equal_roots_break_ties_by_mru_and_unfocused_rank_last() {
    let r = roots(&[("main", "/a"), ("project-1", "/a"), ("project-2", "/a")]);
    assert_eq!(route(Path::new("/a/x.md"), &r, &mru(&["project-1", "main"]), fb), Route::Window("project-1".into()));
    assert_eq!(route(Path::new("/a/x.md"), &r, &mru(&["project-2"]), fb), Route::Window("project-2".into()));
}

#[test]
fn prefix_that_is_not_a_path_component_does_not_match() {
    let r = roots(&[("main", "/repo")]);
    assert_eq!(route(Path::new("/repo2/x.md"), &r, &mru(&[]), fb), Route::NewWindow(PathBuf::from("/repo2")));
}

#[test]
fn no_match_opens_new_window_at_fallback_root() {
    assert_eq!(route(Path::new("/z/y/x.md"), &[], &mru(&[]), fb), Route::NewWindow(PathBuf::from("/z/y")));
}
```

- [ ] **Step 2: Verify fail**, then **implement**

```rust
#[derive(Debug, PartialEq)]
pub enum Route {
    Window(String),
    NewWindow(PathBuf),
}

/// Pick the project window for `path`: the longest containing root wins;
/// equal roots go to the most recently focused. Paths are compared component-
/// wise (Path::starts_with), so /repo does not contain /repo2. Callers pass
/// canonical paths.
pub fn route(
    path: &Path,
    roots: &[(String, PathBuf)],
    mru: &[String],
    fallback_root: impl Fn(&Path) -> PathBuf,
) -> Route {
    let rank = |label: &str| mru.iter().position(|l| l == label).unwrap_or(usize::MAX);
    roots
        .iter()
        .filter(|(_, root)| path.starts_with(root))
        .min_by(|(la, ra), (lb, rb)| {
            rb.components()
                .count()
                .cmp(&ra.components().count())
                .then_with(|| rank(la).cmp(&rank(lb)))
                .then_with(|| la.cmp(lb))
        })
        .map(|(l, _)| Route::Window(l.clone()))
        .unwrap_or_else(|| Route::NewWindow(fallback_root(path)))
}

pub fn fallback_root(path: &Path) -> PathBuf {
    let dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().map(Path::to_path_buf).unwrap_or_else(|| path.to_path_buf())
    };
    crate::git::git_toplevel(&dir).unwrap_or(dir)
}
```

- [ ] **Step 3: Tests pass, gate, commit**

```bash
git add src-tauri/src/routing.rs src-tauri/src/git.rs
git commit -m "Add path-to-window routing"
```

---

### Task 11: Route Finder opens and File ▸ Open File

**Files:**
- Modify: `src-tauri/src/windows.rs` (`deliver_path`)
- Modify: `src-tauri/src/open_files.rs`
- Modify: `src-tauri/src/menu.rs` (`prompt_open_file`)

**Interfaces:**
- Consumes: `routing::route`, `routing::fallback_root`, `create_project_window`, `deliver_files`.
- Produces: `pub fn windows::deliver_path(app: &AppHandle, path: PathBuf)`, which canonicalizes, routes, creates a window if needed, then calls `deliver_files`.

- [ ] **Step 1: Implement** (the routing decision is already unit-tested in Task 10; this is IO glue)

```rust
pub fn deliver_path(app: &tauri::AppHandle, path: PathBuf) {
    use tauri::Manager;
    let path = path.canonicalize().unwrap_or(path);
    let state = app.state::<crate::AppState>();
    let roots = state.windows.lock().unwrap().roots();
    let mru = state.focus.lock().unwrap().as_slice().to_vec();
    let label = match crate::routing::route(&path, &roots, &mru, crate::routing::fallback_root) {
        crate::routing::Route::Window(l) => l,
        crate::routing::Route::NewWindow(root) => match create_project_window(app, root, None) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("mdviewer: {e}");
                return;
            }
        },
    };
    deliver_files(app, &label, vec![path]);
}
```

Note that a new window's `WindowState` starts with `ready: false`, so the file is buffered and drained by that window's `frontend_ready`. Its frontend then opens it as a tab. **Check `init()`'s cold-Finder branch:** in a *new* window, `initial.initial_file` is null and `pending` is non-empty, so `coldFinder` would re-root the sidebar to the file's parent and override the git-root choice. Guard it by adding `is_new_window` to `InitialState` (`window.label() != "main"`) and using `const coldFinder = !initial.is_new_window && !initial.initial_file && pending.length > 0;`.

`open_files::handle_opened`: `for p in paths { crate::windows::deliver_path(handle, p) }`.

`menu::prompt_open_file`: replace the `emit("open-file", …)` with `crate::windows::deliver_path(&app, PathBuf::from(p))`.

- [ ] **Step 2: Gate, `cargo build` + bundle, manual (Finder)**

Per CLAUDE.md, Finder opens need the bundled `.app` registered with Launch Services. With windows on repo A and repo B: double-click a `.md` in B, and it opens as a tab in B's window. Double-click one in an unrelated folder `/tmp/x/` (not a git repo), and a new window opens rooted at `/tmp/x` (or its canonical `/private/tmp/x`). File ▸ Open File… on a file in A opens it in A even while B is focused.

- [ ] **Step 3: Commit**

```bash
git add -A src-tauri/src ui/app.js
git commit -m "Open files from Finder and Open File in the window that contains them"
```

---

### Task 12: Single instance (second launches forward to the running app)

**Files:**
- Create: `src-tauri/src/launch.rs`
- Modify: `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`

**Interfaces:**
- Produces:
  - `pub enum LaunchTarget { Folder(PathBuf), File(PathBuf) }`
  - `pub fn resolve_launch_path(raw: &str, cwd: &Path) -> Result<LaunchTarget, String>` (absolutize against `cwd`, canonicalize, stat; same errors as today's `resolve_args`)
  - `AppState.startup_queue: Mutex<Option<Vec<LaunchTarget>>>`: `Some(vec)` until setup completes, then `None`.

- [ ] **Step 1: Failing tests** (`launch.rs`, using a temp dir like `fs_ops` tests do)

```rust
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
    assert!(matches!(resolve_launch_path(".", &dir).unwrap(), LaunchTarget::Folder(_)));
}

#[test]
fn missing_path_errors() {
    let dir = tempdir_for("launch-missing");
    assert!(resolve_launch_path("nope.md", &dir).unwrap_err().contains("cannot open 'nope.md'"));
}
```

(`tempdir_for`: copy the helper pattern `fs_ops.rs` tests use; don't add a `tempfile` dependency if none exists.)

- [ ] **Step 2: Implement `launch.rs`**, then make `main.rs::resolve_args` call it: `Folder(p)` → `Startup { tree_root: Some(p), initial_file: None }`; `File(f)` → `tree_root: parent, initial_file: Some(f)`.

- [ ] **Step 3: Add the plugin**

`Cargo.toml`:

```toml
[target.'cfg(any(target_os = "macos", windows))'.dependencies]
tauri-plugin-single-instance = "2"
```

`lib.rs`: register it **first** on the builder:

```rust
let builder = tauri::Builder::default();
#[cfg(any(target_os = "macos", windows))]
let builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
    single_instance_open(app, argv, cwd);
}));
```

```rust
fn single_instance_open(app: &tauri::AppHandle, argv: Vec<String>, cwd: String) {
    let Some(raw) = argv.get(1) else {
        // Bare relaunch: bring the most recent window forward.
        if let Some(w) = windows::front_label(app).and_then(|l| app.get_webview_window(&l)) {
            let _ = w.set_focus();
        }
        return;
    };
    let Ok(target) = launch::resolve_launch_path(raw, std::path::Path::new(&cwd)) else { return };
    let state = app.state::<AppState>();
    if let Some(queue) = state.startup_queue.lock().unwrap().as_mut() {
        queue.push(target);
        return;
    }
    dispatch_launch(app, target);
}

fn dispatch_launch(app: &tauri::AppHandle, target: launch::LaunchTarget) {
    match target {
        launch::LaunchTarget::Folder(root) => windows::open_folder_in_new_window(app, root),
        launch::LaunchTarget::File(f) => windows::deliver_path(app, f),
    }
}
```

At the end of `setup`: `let queued = state.startup_queue.lock().unwrap().take().unwrap_or_default(); for t in queued { dispatch_launch(&handle, t) }`.

`--claude-hook` / `--mcp` never reach this: `main.rs` returns before `run`.

`argv[1]` is untrusted: it goes through `resolve_launch_path` (canonicalize + stat) exactly like a cold launch, and opening routes through the same viewer paths.

- [ ] **Step 4: Gate + manual**

- macOS, with MDViewer running from `cargo run` in `src-tauri/`: in another terminal, `./target/debug/mdviewer ../README.md`. The second process exits immediately and the file opens as a tab in the matching window.
- `./target/debug/mdviewer /some/other/folder` opens (or focuses) a window for it.
- The Claude hook path (`claude_hook::launch_mdviewer`) is unchanged, and now lands in the right window via routing.
- **Windows** (CI builds it; run it by hand if a Windows machine is available): a second `mdviewer.exe file.md` forwards instead of opening a second process. If nobody can check it, say so in the PR description.

- [ ] **Step 5: Commit**

```bash
git add -A src-tauri
git commit -m "Forward second launches to the running app and route their paths"
```

---

### Task 13: MCP routing by path and by the proxy's cwd

**Files:**
- Modify: `src-tauri/src/mcp.rs` (`GuiRequest.cwd`; proxy sets it)
- Modify: `src-tauri/src/mcp_server.rs` (`target_for`; focus routed window)
- Modify: `src-tauri/tests/launch_smoke.rs` (struct literal gets `cwd: None`)

**Interfaces:**
- Produces:
  - `pub struct GuiRequest { pub id: u64, pub tool: String, pub args: Value, #[serde(default, skip_serializing_if = "Option::is_none")] pub cwd: Option<String> }`
  - `pub fn mcp_server::pick_target(req: &GuiRequest, roots: &[(String, PathBuf)], mru: &[String]) -> Target` (pure), with `pub enum Target { Window(String), NewWindow(PathBuf), NoWindow }`

- [ ] **Step 1: Failing tests**

`mcp.rs`:

```rust
#[test]
fn gui_request_without_cwd_still_parses() {
    let r: GuiRequest = serde_json::from_str(r#"{"id":1,"tool":"get_viewer_state","args":{}}"#).unwrap();
    assert_eq!(r.cwd, None);
}
```

`mcp_server.rs`:

```rust
fn req(tool: &str, path: Option<&str>, cwd: Option<&str>) -> GuiRequest {
    GuiRequest {
        id: 1,
        tool: tool.into(),
        args: match path { Some(p) => serde_json::json!({"path": p}), None => serde_json::json!({}) },
        cwd: cwd.map(str::to_string),
    }
}
fn r2() -> Vec<(String, std::path::PathBuf)> {
    vec![("main".into(), "/a".into()), ("project-1".into(), "/b".into())]
}

#[test]
fn path_tools_route_by_path() {
    let t = pick_target(&req("open_document", Some("/b/x.md"), Some("/a")), &r2(), &["main".into()]);
    assert_eq!(t, Target::Window("project-1".into()));
}

#[test]
fn path_tool_outside_every_root_opens_new_window() {
    let t = pick_target(&req("request_review", Some("/c/p.md"), None), &r2(), &[]);
    assert!(matches!(t, Target::NewWindow(_)));
}

#[test]
fn viewer_state_routes_by_cwd_then_mru_never_new_window() {
    assert_eq!(pick_target(&req("get_viewer_state", None, Some("/b/sub")), &r2(), &["main".into()]), Target::Window("project-1".into()));
    assert_eq!(pick_target(&req("get_viewer_state", None, Some("/elsewhere")), &r2(), &["main".into()]), Target::Window("main".into()));
    assert_eq!(pick_target(&req("get_viewer_state", None, None), &[], &[]), Target::NoWindow);
}
```

Use a `fallback_root` stand-in inside `pick_target` for the pure test: `pick_target` takes no IO and calls `route` with `|p| p.parent().map(Path::to_path_buf).unwrap_or_default()`. The IO caller replaces a `NewWindow` result's root with `routing::fallback_root(path)` before creating the window. (This keeps `pick_target` pure while the real root still comes from git.)

- [ ] **Step 2: Implement**

`mcp.rs`: add the `cwd` field. In `forward_call`, build `GuiRequest { …, cwd: std::env::current_dir().ok().map(|d| d.to_string_lossy().into_owned()) }`. Update the test at `mcp.rs:801` and `tests/launch_smoke.rs` struct literals with `cwd: None`.

`mcp_server.rs`:

```rust
#[derive(Debug, PartialEq)]
pub enum Target {
    Window(String),
    NewWindow(std::path::PathBuf),
    NoWindow,
}

pub fn pick_target(req: &GuiRequest, roots: &[(String, std::path::PathBuf)], mru: &[String]) -> Target {
    use crate::routing::{route, Route};
    let parent = |p: &std::path::Path| p.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
    if req.tool == "get_viewer_state" {
        let by_cwd = req.cwd.as_deref().and_then(|c| match route(std::path::Path::new(c), roots, mru, parent) {
            Route::Window(l) => Some(l),
            Route::NewWindow(_) => None,
        });
        let fallback = mru.iter().find(|l| roots.iter().any(|(r, _)| r == *l)).cloned()
            .or_else(|| roots.first().map(|(l, _)| l.clone()));
        return by_cwd.or(fallback).map(Target::Window).unwrap_or(Target::NoWindow);
    }
    let Some(p) = req.args.get("path").and_then(Value::as_str) else {
        return Target::NoWindow;
    };
    match route(std::path::Path::new(p), roots, mru, parent) {
        Route::Window(l) => Target::Window(l),
        Route::NewWindow(r) => Target::NewWindow(r),
    }
}
```

In `prepare_request`, replace the hard-coded `"main"`:

```rust
let state = app.state::<crate::AppState>();
let canonical_req = canonicalize_path_arg(req); // clone with args.path canonicalized when it exists
let roots = state.windows.lock().unwrap().roots();
let mru = state.focus.lock().unwrap().as_slice().to_vec();
let label = match pick_target(&canonical_req, &roots, &mru) {
    Target::Window(l) => l,
    Target::NoWindow => return Err(mcp::STARTING_ERR.to_string()),
    Target::NewWindow(_) => {
        // Validate BEFORE creating a window, so a bad path never opens one. The
        // new window's own root is the containment boundary for generate_pdf.
        let p = canonical_req.args["path"].as_str().unwrap_or("");
        let root = crate::routing::fallback_root(std::path::Path::new(p));
        validate(&canonical_req, Some(&root))?;
        crate::windows::create_project_window(app, root, None)?;
        // Not ready yet → the proxy retries on STARTING_ERR (up to 15 s), and
        // the retry routes into the new window once its frontend is ready.
        return Err(mcp::STARTING_ERR.to_string());
    }
};
```

Then continue with Task 2's ready/root read for `label`, `validate(&canonical_req, root)`, `pending.register(&label, &req.tool)`, `app.emit_to(label.as_str(), event, payload)`, and for `request_review` focus `app.get_webview_window(&label)` (not `"main"`).

`canonicalize_path_arg`: when `args.path` exists and canonicalizes, replace it; otherwise leave the request unchanged so `validate` reports "file not found".

**Retry double-create guard:** on a retry the path now routes to the new window (its root contains the path), so no second window is created. The unit test `path_tools_route_by_path` covers the routing half. The manual check covers the timing.

- [ ] **Step 3: Tests pass, gate**

- [ ] **Step 4: Security review for Phase 3.** Dispatch `security-reviewer` on `git diff main...HEAD -- src-tauri/src/{routing,launch,mcp,mcp_server,windows,open_files}.rs src-tauri/src/lib.rs`. Focus: `generate_pdf` containment now uses the routed window's root; a crafted MCP path can open a window rooted at an arbitrary folder (equivalent to the user opening it, per the spec; confirm nothing else is gained); single-instance argv handling; `cwd` is only used for routing, never as a trust boundary.

- [ ] **Step 5: Commit**

```bash
git add -A src-tauri
git commit -m "Route MCP calls to the window that owns the path or the session's cwd"
```

---

### Task 14: Two-root smoke test, docs, final GUI check

**Files:**
- Create: `src-tauri/tests/multi_window_smoke.rs`
- Create: `src-tauri/tests/fixtures/multi/a/a.md`, `src-tauri/tests/fixtures/multi/b/b.md`
- Modify: `scripts/smoke-test.sh`, `CLAUDE.md`, `README.md`

- [ ] **Step 1: Smoke test** (`#![cfg(target_os = "macos")]`, `#[ignore]`, same harness shape as `launch_smoke.rs`: copy `bundle_path`, `inner_binary`, `test_socket_id`, and a `call(tool, args, cwd) -> Result<Value, String>` built from `try_once`)

Scenario:
1. Launch the inner binary with `fixtures/multi/a` as argv and `MDVIEWER_MCP_SOCKET` set; poll `get_viewer_state` (cwd `a`) until it answers.
2. `open_document { path: <abs b/b.md> }` → expect success (the first calls may return `STARTING_ERR` while window B boots; retry up to 30 × 500 ms like the proxy does).
3. `get_viewer_state` with `cwd: <abs b>` → `state.path` ends with `b.md`.
4. `get_viewer_state` with `cwd: <abs a>` → `state.path` is NOT `b.md` (a's window was untouched).

Point the app's data dir at a temp location if possible so the user's saved windows aren't restored into the test. If Tauri offers no override, note it in the test's doc comment: "restores the developer's saved windows; the assertions only look at windows a and b."

Append to `scripts/smoke-test.sh`: `( cd src-tauri && cargo test --test multi_window_smoke -- --ignored --nocapture )`.

- [ ] **Step 2: Docs**

`CLAUDE.md`:
- Stack/file layout: add `windows.rs`, `routing.rs`, `launch.rs`, `ui/windowscope.js`.
- Architecture: a "Multi-window" bullet covering registry-by-label, routing rule, per-root sessions, and restore.
- "Things that took hours": **JS `event.listen()` is target-Any in Tauri 2**. It receives `emit_to` events aimed at other windows, so frontend listeners must use `getCurrentWebviewWindow().listen`. Also note the single-instance dev gotcha (quit the installed app before `cargo run`).
- Update the Security containment bullet: `current_root` → per-window root from the calling window's label.

`README.md`: Features ("Open several projects side by side, one per window; files from Finder, the CLI and Claude Code open in the window of the project they belong to") and the File menu (New Window ⌘⇧N).

- [ ] **Step 3: Final GUI check** (the memory rule: automated checks miss visual bugs)

Build the bundle. Two windows side by side, one of them in dark mode. Check:
- a menu action hits only the focused window;
- a live reload in A leaves B untouched;
- PDF export from B previews B's document;
- the Integration window opened from B shows B's root;
- the Claude hook writing a plan in repo B opens it in B;
- MCP `request_review` from a Claude session in B opens in B, and Finish & Send returns to that session;
- ⌘Q + relaunch restores both windows with tabs and bounds.

- [ ] **Step 4: Full gate + commit**

```bash
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cd .. && node --test ui/
git add -A
git commit -m "Add multi-window smoke test and document the multi-window model"
```
