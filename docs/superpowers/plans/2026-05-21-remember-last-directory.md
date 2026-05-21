# Remember the Last Directory Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On a plain launch (no file/folder argument, no Finder-opened file), MDViewer reopens the directory the sidebar last showed instead of the process working directory.

**Architecture:** Persist a `last_folder` value inside the existing `recent.json` store. The frontend — the single source of truth for which folder the sidebar shows — calls a `remember_folder` command whenever it sets the root. On launch, the backend's `get_initial_state` resolves the tree root as: explicit `argv` → restored `last_folder` (if it still exists) → `cwd` fallback.

**Tech Stack:** Rust / Tauri 2.11 backend (`serde`, `serde_json`), vanilla JS frontend (`window.__TAURI__` IPC). No new dependencies.

---

## File structure

- **Modify `src-tauri/src/recent.rs`** — add `last_folder: Option<PathBuf>` to `Store`; refactor file I/O to read-modify-write the whole `Store` so `push`/`clear` never clobber `last_folder`; split the pure list-mutation into `Store::push_folder`; add `load_last`/`save_last`; add unit tests.
- **Modify `src-tauri/src/main.rs`** — a plain launch yields `tree_root: None`.
- **Modify `src-tauri/src/lib.rs`** — `Startup.tree_root` and `AppState.tree_root` become `Option<PathBuf>`; guard the startup `recent::push`; register `remember_folder`.
- **Modify `src-tauri/src/commands.rs`** — `get_initial_state` takes `AppHandle` and resolves the root; add the `remember_folder` command.
- **Modify `ui/app.js`** — call `remember_folder` from `setTreeRoot` and the cold-Finder branch of `init`.
- **Modify `CLAUDE.md`** — document the behavior.

All commands run from `src-tauri/` unless noted. Lint gates (CI enforces both with `-D warnings`): `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`.

---

## Task 1: `recent.rs` — `last_folder` field, whole-Store I/O, pure logic + tests

**Files:**
- Modify/Test: `src-tauri/src/recent.rs`

- [ ] **Step 1: Write the failing tests**

Append this test module to the end of `src-tauri/src/recent.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_folder_dedups_and_moves_to_front() {
        let mut s = Store::default();
        s.push_folder(PathBuf::from("/a"));
        s.push_folder(PathBuf::from("/b"));
        s.push_folder(PathBuf::from("/a"));
        assert_eq!(s.folders, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }

    #[test]
    fn push_folder_caps_at_max_recent() {
        let mut s = Store::default();
        for i in 0..(MAX_RECENT + 5) {
            s.push_folder(PathBuf::from(format!("/d{i}")));
        }
        assert_eq!(s.folders.len(), MAX_RECENT);
        assert_eq!(s.folders[0], PathBuf::from(format!("/d{}", MAX_RECENT + 4)));
    }

    #[test]
    fn push_folder_preserves_last_folder() {
        let mut s = Store::default();
        s.last_folder = Some(PathBuf::from("/keep"));
        s.push_folder(PathBuf::from("/a"));
        assert_eq!(s.last_folder, Some(PathBuf::from("/keep")));
    }

    #[test]
    fn store_round_trips_both_fields() {
        let mut s = Store::default();
        s.push_folder(PathBuf::from("/a"));
        s.last_folder = Some(PathBuf::from("/last"));
        let json = serde_json::to_string(&s).unwrap();
        let back: Store = serde_json::from_str(&json).unwrap();
        assert_eq!(back.folders, vec![PathBuf::from("/a")]);
        assert_eq!(back.last_folder, Some(PathBuf::from("/last")));
    }

    #[test]
    fn deserializes_legacy_store_without_last_folder() {
        let back: Store = serde_json::from_str(r#"{"folders":["/a"]}"#).unwrap();
        assert_eq!(back.folders, vec![PathBuf::from("/a")]);
        assert_eq!(back.last_folder, None);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib recent`
Expected: FAIL — compile errors (`no function or associated item named push_folder`, `no field last_folder on type Store`).

- [ ] **Step 3: Implement the `recent.rs` changes**

Replace everything in `src-tauri/src/recent.rs` **above** the `display` function (i.e. lines 1–60, the `use`s through the `write` function) with:

```rust
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const MAX_RECENT: usize = 10;
const FILE_NAME: &str = "recent.json";

#[derive(Default, Serialize, Deserialize)]
struct Store {
    folders: Vec<PathBuf>,
    #[serde(default)]
    last_folder: Option<PathBuf>,
}

impl Store {
    /// Move `canonical` to the front of the recent list, deduplicating and
    /// capping at `MAX_RECENT`. Leaves `last_folder` untouched.
    fn push_folder(&mut self, canonical: PathBuf) {
        self.folders.retain(|p| p != &canonical);
        self.folders.insert(0, canonical);
        self.folders.truncate(MAX_RECENT);
    }
}

fn store_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(FILE_NAME))
}

fn canonical_or_keep(folder: &Path) -> PathBuf {
    folder
        .canonicalize()
        .unwrap_or_else(|_| folder.to_path_buf())
}

fn load_store(app: &AppHandle) -> Store {
    let Some(path) = store_path(app) else {
        return Store::default();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return Store::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn write_store(app: &AppHandle, store: &Store) {
    let Some(path) = store_path(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(store) {
        let _ = std::fs::write(path, json);
    }
}

pub fn load(app: &AppHandle) -> Vec<PathBuf> {
    load_store(app).folders
}

/// Adds `folder` to the front of the recent list. Deduplicates and caps at
/// `MAX_RECENT`. Returns the new list.
pub fn push(app: &AppHandle, folder: &Path) -> Vec<PathBuf> {
    let mut store = load_store(app);
    store.push_folder(canonical_or_keep(folder));
    write_store(app, &store);
    store.folders
}

/// Empties the recent list. Preserves `last_folder` — clearing the Open Recent
/// menu must not forget where the sidebar was.
pub fn clear(app: &AppHandle) {
    let mut store = load_store(app);
    store.folders.clear();
    write_store(app, &store);
}

pub fn load_last(app: &AppHandle) -> Option<PathBuf> {
    load_store(app).last_folder
}

pub fn save_last(app: &AppHandle, folder: &Path) {
    let mut store = load_store(app);
    store.last_folder = Some(canonical_or_keep(folder));
    write_store(app, &store);
}
```

Leave the existing `display` function (currently lines 62–71) unchanged.

- [ ] **Step 4: Run the tests, fmt, and clippy**

Run: `cargo test --lib recent`
Expected: PASS — 5 tests pass.

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: no output, exit 0.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/recent.rs
git commit -m "Persist a last_folder value in the recent store"
```

---

## Task 2: Optional tree root + launch resolution (backend wiring)

This change spans three files that must compile together, so it is one commit. The resolution logic is `AppHandle`-coupled (needs the managed `AppState` and `app_data_dir`), so it has no isolated unit test; verification is a clean build, clippy, and the existing test suite staying green. Manual end-to-end checks are in Task 5.

**Files:**
- Modify: `src-tauri/src/main.rs:20-55`
- Modify: `src-tauri/src/lib.rs:15-18`, `:26-31`, `:35`, `:44-54`, `:60-63`
- Modify: `src-tauri/src/commands.rs:6`, `:14-23`, and append a new command

- [ ] **Step 1: `main.rs` — a plain launch yields `None`**

In `src-tauri/src/main.rs`, change the `None` arm of the `match arg` block. Replace:

```rust
        None => Ok(mdviewer_lib::Startup {
            tree_root: cwd,
            initial_file: None,
        }),
```

with:

```rust
        None => Ok(mdviewer_lib::Startup {
            tree_root: None,
            initial_file: None,
        }),
```

In the same file, the directory and file arms must now wrap their root in `Some`. Change `tree_root: canonical,` (the directory arm) to `tree_root: Some(canonical),`, and change `tree_root: parent,` (the file arm) to `tree_root: Some(parent),`. Leave `cwd` as-is — it is still used by the file/dir arm for relative-path resolution and the parent fallback.

- [ ] **Step 2: `lib.rs` — `Option` tree root, guarded push, register command**

In `src-tauri/src/lib.rs`:

Change the `Startup` struct field (around line 16) from `pub tree_root: PathBuf,` to `pub tree_root: Option<PathBuf>,`.

Change the `AppState` struct field (around line 27) from `pub tree_root: PathBuf,` to `pub tree_root: Option<PathBuf>,`.

The `AppState` initializer `tree_root: startup.tree_root,` (around line 35) needs no change — both sides are now `Option<PathBuf>`.

Add `commands::remember_folder,` to the `invoke_handler!` list (after `commands::frontend_ready,`):

```rust
        .invoke_handler(tauri::generate_handler![
            commands::get_initial_state,
            commands::list_dir,
            commands::render_file,
            commands::open_file,
            commands::read_source,
            commands::check_for_updates,
            commands::open_url,
            commands::open_path,
            commands::frontend_ready,
            commands::remember_folder,
        ])
```

In the setup hook, replace the unconditional push (around line 62):

```rust
            recent::push(&handle, &state.tree_root);
```

with a guarded push that only records an explicit launch folder into Open Recent:

```rust
            if let Some(root) = &state.tree_root {
                recent::push(&handle, root);
            }
```

- [ ] **Step 3: `commands.rs` — resolve on launch + new command**

In `src-tauri/src/commands.rs`, add `recent` to the crate import (line 6). Change:

```rust
use crate::{markdown, tree, updates, AppState};
```

to:

```rust
use crate::{markdown, recent, tree, updates, AppState};
```

Replace the whole `get_initial_state` function (lines 14–23) with:

```rust
#[tauri::command]
pub fn get_initial_state(app: AppHandle, state: State<'_, AppState>) -> InitialState {
    let tree_root = match &state.tree_root {
        Some(p) => {
            recent::save_last(&app, p);
            p.clone()
        }
        None => match recent::load_last(&app).filter(|p| p.is_dir()) {
            Some(p) => {
                recent::save_last(&app, &p);
                p
            }
            None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
        },
    };
    InitialState {
        tree_root: tree_root.to_string_lossy().into_owned(),
        initial_file: state
            .initial_file
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
    }
}
```

Append a new command at the end of `src-tauri/src/commands.rs`:

```rust
/// Records the folder the sidebar is currently showing so the next plain
/// launch can restore it. Best-effort: a non-directory or vanished path is a
/// no-op, and persistence errors are swallowed (UI state, never user-facing).
#[tauri::command]
pub fn remember_folder(app: AppHandle, path: String) {
    let p = PathBuf::from(path);
    if p.is_dir() {
        recent::save_last(&app, &p);
    }
}
```

(`AppHandle`, `State`, `Path`, and `PathBuf` are already imported in this file.)

- [ ] **Step 4: Build, test, fmt, clippy**

Run: `cargo build`
Expected: compiles cleanly (no errors, no warnings).

Run: `cargo test`
Expected: PASS — all tests, including the Task 1 `recent` tests and existing `open_files`/`markdown` tests.

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: no output, exit 0.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/main.rs src-tauri/src/lib.rs src-tauri/src/commands.rs
git commit -m "Restore the last folder on a plain launch"
```

---

## Task 3: Frontend persists the sidebar root

This project has no JS test harness (vanilla JS, no build step), so verification is a clean `cargo build` (which re-bundles `frontendDist`) plus the manual checks in Task 5.

**Files:**
- Modify: `ui/app.js:218-226` (`setTreeRoot`), `ui/app.js:194-206` (`init` cold-Finder branch)

- [ ] **Step 1: Add a `rememberFolder` helper and call it in `setTreeRoot`**

In `ui/app.js`, replace the `setTreeRoot` function (lines 218–226):

```js
async function setTreeRoot(path) {
  treeRoot = path;
  treeTitle.textContent = basename(path) || path;
  treeTitle.title = path;
  childCache.clear();
  await renderRoot();
  const tab = activeTab();
  if (tab) highlightSelectedByPath(tab.path);
}
```

with:

```js
function rememberFolder(path) {
  invoke("remember_folder", { path }).catch((e) =>
    console.error("remember_folder failed", e),
  );
}

async function setTreeRoot(path) {
  treeRoot = path;
  treeTitle.textContent = basename(path) || path;
  treeTitle.title = path;
  childCache.clear();
  await renderRoot();
  rememberFolder(path);
  const tab = activeTab();
  if (tab) highlightSelectedByPath(tab.path);
}
```

- [ ] **Step 2: Persist the cold-Finder folder in `init`**

In `ui/app.js`, find this block in `init()` (lines 194–199):

```js
  // A cold Finder launch (no argv file) starts the sidebar at the file's folder.
  treeRoot =
    !initial.initial_file && pending.length
      ? parentDir(pending[0])
      : initial.tree_root;
  treeTitle.textContent = basename(treeRoot) || treeRoot;
  treeTitle.title = treeRoot;
```

Replace it with:

```js
  // A cold Finder launch (no argv file) starts the sidebar at the file's folder.
  const coldFinder = !initial.initial_file && pending.length > 0;
  treeRoot = coldFinder ? parentDir(pending[0]) : initial.tree_root;
  treeTitle.textContent = basename(treeRoot) || treeRoot;
  treeTitle.title = treeRoot;
  // Explicit-arg and restored roots are already persisted by get_initial_state;
  // only the cold-Finder folder needs persisting here. The bare cwd default is
  // intentionally not persisted.
  if (coldFinder) rememberFolder(treeRoot);
```

- [ ] **Step 3: Rebuild so the new UI is bundled**

Run: `cargo build`
Expected: compiles cleanly. (Tauri bundles `ui/*` at compile time; editing without rebuilding shows stale UI.)

- [ ] **Step 4: Commit**

```bash
git add ui/app.js
git commit -m "Frontend: remember the sidebar folder on every root change"
```

---

## Task 4: Document the behavior in CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update the `recent.rs` one-line description**

In `CLAUDE.md`, in the "File layout" block, change the `recent.rs` line from:

```
    recent.rs     — JSON-persisted recent-folders list (app_data_dir)
```

to:

```
    recent.rs     — JSON-persisted recent-folders list + last_folder (app_data_dir)
```

- [ ] **Step 2: Add an architecture note**

In `CLAUDE.md`, in the "Architecture quick-tour" section, add this bullet after the "File associations / open from Finder" bullet:

```
- **Last directory restore**: `recent.json` carries a `last_folder` alongside
  the recent list. The frontend calls the `remember_folder` command whenever it
  sets the sidebar root (`setTreeRoot`, and the cold-Finder branch of `init`).
  On a plain launch (`Startup.tree_root == None`), `get_initial_state` resolves
  the root as explicit argv → `last_folder` (if still a dir) → cwd, persisting
  all but the bare cwd fallback. `recent::clear` keeps `last_folder`.
```

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "Document last-directory restore in CLAUDE.md"
```

---

## Task 5: Manual end-to-end verification

No code changes — confirm the behavior matches the spec. Run from `src-tauri/`.

- [ ] **Step 1: Restore after navigating (CLI)**

Run: `cargo run -- ~` then, in the app, Open Folder (⌘⇧O) and pick some folder X; quit the app.
Run: `cargo run` (no argument).
Expected: the sidebar shows folder X, not the working directory.

- [ ] **Step 2: Explicit argument wins and updates the memory**

Run: `cargo run -- ../` (an explicit directory).
Expected: sidebar shows that directory. Quit, then run `cargo run` with no argument.
Expected: sidebar shows that same directory (it became the remembered folder).

- [ ] **Step 3: Deleted remembered folder falls back to cwd**

Create and remember a throwaway dir: `mkdir /tmp/mdv-test && cargo run -- /tmp/mdv-test`; quit; `rmdir /tmp/mdv-test`.
Run: `cargo run` (no argument).
Expected: the app launches without crashing and shows the working directory (the dead folder is rejected).

- [ ] **Step 4: Open Recent updates the memory**

Run: `cargo run`, use File ▸ Open Recent to pick a folder; quit; `cargo run`.
Expected: sidebar shows the recent folder you picked.

- [ ] **Step 5 (bundle, optional): cold Finder open is remembered**

Build and install the bundle per CLAUDE.md, double-click a `.md` in Finder with the app closed (sidebar shows its folder), quit, relaunch from the Dock.
Expected: that folder is restored.

---

## Self-review notes

- **Spec coverage:** `last_folder` field + non-clobbering I/O (Task 1) ✓; plain-launch `None` + resolution explicit→restored→cwd (Task 2) ✓; `remember_folder` command (Task 2) ✓; frontend persistence at both chokepoints (Task 3) ✓; `clear` preserves `last_folder` (Task 1, tested indirectly via `push_folder_preserves_last_folder` and the non-clobbering `clear` body) ✓; backward-compat with old `recent.json` (Task 1 legacy test) ✓; dead-folder fallback (Task 2 `is_dir` filter, Task 5 step 3) ✓; select & copy explicitly out of scope ✓.
- **Type consistency:** `Store.last_folder: Option<PathBuf>`, `recent::load_last(&AppHandle) -> Option<PathBuf>`, `recent::save_last(&AppHandle, &Path)`, `remember_folder(AppHandle, String)`, frontend `rememberFolder(path)` / `invoke("remember_folder", { path })`, `Startup.tree_root`/`AppState.tree_root: Option<PathBuf>` — consistent across tasks.
