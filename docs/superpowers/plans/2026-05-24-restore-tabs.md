# Restore Tabs on Relaunch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On a plain launch (no CLI/Finder file), reopen the previous session's tabs — same paths, order, and active tab — restored as pinned, rendered, scrolled-to-top.

**Architecture:** Persist the open-tab paths + active index into the existing `recent.json` `Store` (alongside `last_folder`). A pure, unit-tested `restore_session` helper filters missing files and remaps the active index; `get_initial_state` returns the survivors. The frontend restores them only on a plain launch, behind a `restoring` guard so the empty first paint can't clobber the saved session.

**Tech Stack:** Tauri 2.11, Rust (serde/serde_json), vanilla JS (no build step), `cargo test` for the Rust helpers, `node --test` for the existing JS suite.

**Spec:** `docs/superpowers/specs/2026-05-24-restore-tabs-design.md`

---

## File Structure

- `src-tauri/src/recent.rs` — `Store` gains `open_tabs` + `active_tab`; new `save_session`, `load_session`, and the pure `restore_session` helper, plus unit tests.
- `src-tauri/src/commands.rs` — `InitialState` gains `restore_tabs` + `active_tab`; `get_initial_state` wires in `restore_session`; new `save_session` command.
- `src-tauri/src/lib.rs` — register the `save_session` command.
- `ui/app.js` — `restoring` flag, `persistSession`, `restoreSession`, and seam calls in `init` / `setActiveTab` / `closeTab`.

> **Why backend is one task, not two:** `recent` is a private module (`mod recent;`), so `pub fn`s in it are flagged `dead_code` by `clippy -D warnings` until something in the (non-test) crate calls them. The `recent.rs` helpers and their callers in `commands.rs`/`lib.rs` must therefore land in the same commit, or the lint gate fails on the intermediate commit. The TDD red→green for the `recent.rs` helpers still happens first within Task 1; clippy is only run after the wiring is in place.

---

## Task 1: Backend session support (recent.rs + commands.rs + lib.rs)

**Files:**
- Modify: `src-tauri/src/recent.rs` (the `Store` struct ~lines 9-14; add functions after `save_last` ~line 87; add tests in the existing `#[cfg(test)]` module)
- Modify: `src-tauri/src/commands.rs` (`InitialState` ~lines 8-12; `get_initial_state` ~lines 14-34; new `save_session` command after `remember_folder` ~line 297)
- Modify: `src-tauri/src/lib.rs` (`invoke_handler!` list)

- [ ] **Step 1: Write the failing tests (recent.rs)**

In `src-tauri/src/recent.rs`, inside the existing `#[cfg(test)] mod tests { ... }` block (after the last existing test, before its closing `}`), add:

```rust
    #[test]
    fn store_round_trips_session_fields() {
        let mut s = Store::default();
        s.open_tabs = vec![PathBuf::from("/a"), PathBuf::from("/b")];
        s.active_tab = Some(1);
        let json = serde_json::to_string(&s).unwrap();
        let back: Store = serde_json::from_str(&json).unwrap();
        assert_eq!(back.open_tabs, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
        assert_eq!(back.active_tab, Some(1));
    }

    #[test]
    fn deserializes_legacy_store_without_session_fields() {
        let back: Store =
            serde_json::from_str(r#"{"folders":["/a"],"last_folder":"/x"}"#).unwrap();
        assert!(back.open_tabs.is_empty());
        assert_eq!(back.active_tab, None);
    }

    #[test]
    fn restore_session_keeps_all_when_all_exist() {
        let (kept, active) = restore_session(
            vec![PathBuf::from("/a"), PathBuf::from("/b"), PathBuf::from("/c")],
            Some(1),
            |_| true,
        );
        assert_eq!(
            kept,
            vec![PathBuf::from("/a"), PathBuf::from("/b"), PathBuf::from("/c")]
        );
        assert_eq!(active, Some(1));
    }

    #[test]
    fn restore_session_drops_missing_and_shifts_active() {
        // "/a" is gone; active was index 1 ("/b"), which becomes index 0.
        let (kept, active) = restore_session(
            vec![PathBuf::from("/a"), PathBuf::from("/b"), PathBuf::from("/c")],
            Some(1),
            |p| p != Path::new("/a"),
        );
        assert_eq!(kept, vec![PathBuf::from("/b"), PathBuf::from("/c")]);
        assert_eq!(active, Some(0));
    }

    #[test]
    fn restore_session_active_file_missing_returns_none() {
        let (kept, active) = restore_session(
            vec![PathBuf::from("/a"), PathBuf::from("/b")],
            Some(1),
            |p| p != Path::new("/b"),
        );
        assert_eq!(kept, vec![PathBuf::from("/a")]);
        assert_eq!(active, None);
    }

    #[test]
    fn restore_session_empty_input_yields_none_active() {
        let (kept, active) = restore_session(vec![], Some(0), |_| true);
        assert!(kept.is_empty());
        assert_eq!(active, None);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test recent:: 2>&1 | tail -20`
Expected: compile error / FAIL — `Store` has no `open_tabs`/`active_tab` and `restore_session` is undefined.

- [ ] **Step 3: Add the two `Store` fields**

In `src-tauri/src/recent.rs`, change the `Store` struct (currently):

```rust
#[derive(Default, Serialize, Deserialize)]
struct Store {
    folders: Vec<PathBuf>,
    #[serde(default)]
    last_folder: Option<PathBuf>,
}
```

to:

```rust
#[derive(Default, Serialize, Deserialize)]
struct Store {
    folders: Vec<PathBuf>,
    #[serde(default)]
    last_folder: Option<PathBuf>,
    #[serde(default)]
    open_tabs: Vec<PathBuf>,
    #[serde(default)]
    active_tab: Option<usize>,
}
```

- [ ] **Step 4: Add the session load/save and the pure restore helper**

In `src-tauri/src/recent.rs`, after the existing `save_last` function (around line 87), add:

```rust
/// Returns the persisted open-tab paths and the active index, unfiltered.
pub fn load_session(app: &AppHandle) -> (Vec<PathBuf>, Option<usize>) {
    let store = load_store(app);
    (store.open_tabs, store.active_tab)
}

/// Persists the open-tab paths and active index, preserving `folders` and
/// `last_folder`. Paths are stored as-is (NOT canonicalized) so they keep
/// string-identity with the frontend's live tab model.
pub fn save_session(app: &AppHandle, tabs: &[PathBuf], active: Option<usize>) {
    let mut store = load_store(app);
    store.open_tabs = tabs.to_vec();
    store.active_tab = active;
    write_store(app, &store);
}

/// Filters `tabs` to the paths satisfying `exists` (order preserved) and remaps
/// `active` by tracking the active path: the result's active index is that
/// path's position in the filtered list, or `None` if the active file is gone
/// or the list is empty. Pure — no I/O, so it is unit-testable.
pub fn restore_session(
    tabs: Vec<PathBuf>,
    active: Option<usize>,
    exists: impl Fn(&Path) -> bool,
) -> (Vec<PathBuf>, Option<usize>) {
    let active_path = active.and_then(|i| tabs.get(i)).cloned();
    let kept: Vec<PathBuf> = tabs.into_iter().filter(|p| exists(p)).collect();
    let new_active = active_path.and_then(|ap| kept.iter().position(|p| *p == ap));
    (kept, new_active)
}
```

- [ ] **Step 5: Run the recent tests to verify they pass**

Run: `cd src-tauri && cargo test recent:: 2>&1 | tail -20`
Expected: all `recent::tests::*` pass (5 pre-existing + 6 new). (Do NOT run `clippy` yet — the new fns aren't called by non-test code until the wiring below, which would trip `dead_code`.)

- [ ] **Step 6: Extend `InitialState` (commands.rs)**

In `src-tauri/src/commands.rs`, change:

```rust
#[derive(Serialize)]
pub struct InitialState {
    pub tree_root: String,
    pub initial_file: Option<String>,
}
```

to:

```rust
#[derive(Serialize)]
pub struct InitialState {
    pub tree_root: String,
    pub initial_file: Option<String>,
    pub restore_tabs: Vec<String>,
    pub active_tab: Option<usize>,
}
```

- [ ] **Step 7: Wire `restore_session` into `get_initial_state`**

In `src-tauri/src/commands.rs`, replace the entire `get_initial_state` function (currently lines ~14-34) with:

```rust
#[tauri::command]
pub fn get_initial_state(app: AppHandle, state: State<'_, AppState>) -> InitialState {
    let tree_root = match &state.tree_root {
        Some(p) => {
            recent::save_last(&app, p);
            p.clone()
        }
        // A restored folder is already stored as last_folder; only fall back to
        // cwd (unpersisted) when there's nothing valid to restore.
        None => recent::load_last(&app)
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))),
    };
    let (saved_tabs, saved_active) = recent::load_session(&app);
    let (tabs, active_tab) = recent::restore_session(saved_tabs, saved_active, |p| p.is_file());
    InitialState {
        tree_root: tree_root.to_string_lossy().into_owned(),
        initial_file: state
            .initial_file
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
        restore_tabs: tabs
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        active_tab,
    }
}
```

- [ ] **Step 8: Add the `save_session` command**

In `src-tauri/src/commands.rs`, immediately after the `remember_folder` command (it ends around line 297), add:

```rust
#[tauri::command]
pub fn save_session(app: AppHandle, tabs: Vec<String>, active: Option<usize>) {
    let paths: Vec<PathBuf> = tabs.into_iter().map(PathBuf::from).collect();
    recent::save_session(&app, &paths, active);
}
```

(`PathBuf` and `recent` are already imported at the top of `commands.rs`.)

- [ ] **Step 9: Register the command (lib.rs)**

In `src-tauri/src/lib.rs`, in the `tauri::generate_handler![ ... ]` list, add `commands::save_session,` after `commands::remember_folder,`:

```rust
            commands::frontend_ready,
            commands::remember_folder,
            commands::save_session,
        ])
```

- [ ] **Step 10: Build, lint, test (everything now wired)**

Run from `src-tauri/`:

```bash
cargo build 2>&1 | tail -3
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
cargo fmt --check && echo FMT_OK
cargo test 2>&1 | grep 'test result'
```

Expected: build succeeds; clippy clean (the new `pub fn`s are now called by `get_initial_state`/`save_session`, so no `dead_code`); `FMT_OK`; tests pass. If fmt reports diffs, run `cargo fmt` and re-check.

- [ ] **Step 11: Commit**

```bash
git add src-tauri/src/recent.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "Persist and restore the open-tab session"
```

---

## Task 2: Frontend persist + restore (app.js)

**Files:**
- Modify: `ui/app.js` (module state ~line 51; `openSticky` end ~line 529; `setActiveTab` ~lines 531-548; `closeTab` ~lines 557-572; `init` tail ~lines 233-238)

- [ ] **Step 1: Add the `restoring` flag**

In `ui/app.js`, find:

```js
const tabs = []; // [{ path, sticky, raw }]
let activeIdx = -1;
```

Add a line right after `let activeIdx = -1;`:

```js
let restoring = true; // suppress session persistence until init() finishes restoring
```

- [ ] **Step 2: Add `persistSession` and `restoreSession` helpers**

In `ui/app.js`, directly after the `openSticky` function (it ends around line 529, just before `async function setActiveTab`), add:

```js
function persistSession() {
  if (restoring) return;
  invoke("save_session", {
    tabs: tabs.map((t) => t.path),
    active: activeIdx >= 0 ? activeIdx : null,
  }).catch((e) => console.error("save_session failed", e));
}

async function restoreSession(paths, active) {
  for (const p of paths) {
    tabs.push({ path: p, sticky: true, raw: false });
  }
  if (tabs.length === 0) return;
  const idx =
    active != null && active >= 0 && active < tabs.length ? active : 0;
  await setActiveTab(idx);
}
```

- [ ] **Step 3: Persist from `setActiveTab`**

In `ui/app.js`, in `setActiveTab`, the valid-index path currently reads:

```js
  const same = idx === activeIdx;
  activeIdx = idx;
  renderTabBar();
  highlightSelectedByPath(tabs[idx].path);
```

Insert `persistSession();` right after `renderTabBar();`:

```js
  const same = idx === activeIdx;
  activeIdx = idx;
  renderTabBar();
  persistSession();
  highlightSelectedByPath(tabs[idx].path);
```

- [ ] **Step 4: Persist from `closeTab`'s last-tab branch**

In `ui/app.js`, `closeTab` has an early-return branch when the last tab is closed:

```js
  if (tabs.length === 0) {
    activeIdx = -1;
    renderTabBar();
    showEmptyState();
    return;
  }
```

Insert `persistSession();` before the `return;`:

```js
  if (tabs.length === 0) {
    activeIdx = -1;
    renderTabBar();
    showEmptyState();
    persistSession();
    return;
  }
```

(The non-empty close path calls `setActiveTab(next)`, which already persists.)

- [ ] **Step 5: Restore on plain launch in `init`**

In `ui/app.js`, the tail of `init` currently reads:

```js
  await renderRoot();
  refreshGitStatus();

  if (initial.initial_file) await openSticky(initial.initial_file);
  for (const p of pending) await openSticky(p);
}
```

Replace the two `openSticky` statements with the plain-launch branch and the end-of-init save:

```js
  await renderRoot();
  refreshGitStatus();

  const plainLaunch = !initial.initial_file && pending.length === 0;
  if (plainLaunch) {
    await restoreSession(initial.restore_tabs, initial.active_tab);
  } else {
    if (initial.initial_file) await openSticky(initial.initial_file);
    for (const p of pending) await openSticky(p);
  }

  restoring = false;
  persistSession();
}
```

- [ ] **Step 6: Rebuild the bundle and verify the JS suite**

Frontend changes only take effect after a Rust rebuild (`tauri-codegen` bundles the UI at compile time). Run:

```bash
cd src-tauri && cargo build 2>&1 | tail -2
cd /Users/laek/source/mdviewer && node --test ui/*.test.js 2>&1 | tail -5
```

Expected: build succeeds; JS suite passes (32 tests — this task adds no new JS tests; the testable logic is the Rust helper from Task 1).

- [ ] **Step 7: Confirm the seam wiring**

Run: `grep -n "persistSession\|restoreSession\|restoring" ui/app.js`
Expected: `let restoring = true`; the two function definitions; the `persistSession()` calls in `setActiveTab` and `closeTab`; and `restoreSession(...)` + `restoring = false;` + `persistSession();` in `init`.

- [ ] **Step 8: Commit**

```bash
git add ui/app.js
git commit -m "Restore tabs on plain launch and persist the tab session"
```

---

## Notes for the executor

- **No version bump.** `main` is already at `1.5.0` and that release has not been cut/tagged yet, so this feature ships in the same unreleased `1.5.0` (the maintainer decides release grouping). Do NOT add a bump task.
- **Commit style** (per `CLAUDE.md`): imperative subject, **no** `Co-Authored-By` trailer.
- **Lint gate**: `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` from `src-tauri/` must be clean before each Rust commit.
- **Tauri arg/field naming:** the JS reads `initial.restore_tabs` / `initial.active_tab` (Tauri serializes the Rust struct as snake_case — matching the existing `initial.tree_root` / `initial.initial_file` usage). The `save_session` command args `tabs` / `active` are single words, so no camelCase translation issue.
- **Manual end-state check** (maintainer, optional): open a few files (mixing single-click preview and double-click pins), quit, relaunch from the Dock — the tabs and active selection should return as pinned tabs. Then `mdviewer somefile.md` should show only that file, and that becomes the new saved session.
- **Why the `restoring` guard matters:** persistence flows through `setActiveTab`, which also fires during restore and during the empty first paint. Without the guard, the empty initial state would save an empty session and erase what we are about to restore. `init` keeps `restoring = true` throughout and saves exactly once at the end.
