# Restore tabs on relaunch — design

Date: 2026-05-24
Status: approved, ready for implementation plan

## Goal

On a plain launch, reopen the previous session's tabs (same file paths, same
order, same active tab). This is the natural companion to the existing
`last_folder` restore, and it closes the gap left by the auto-update feature,
whose post-install relaunch currently drops all open tabs.

## Behavior

- **Restore only on a plain launch**: when the app opens with no specific file
  to show — i.e. no CLI file argument AND no Finder-opened file (Dock/Spotlight
  launch, or the post-update relaunch via `app.restart()`).
- **A specific-file launch does not restore**: `mdviewer foo.md` or a Finder
  double-click shows just that file. That file then *becomes* the saved session
  (a specific-file open replaces the previously saved session going forward).
- **Restored tabs come back pinned (sticky), in rendered view, scrolled to
  top.** Persisted state is paths + order + which tab was active. Per-tab raw
  toggle and scroll position are intentionally out of scope (YAGNI).
- This rides on the exact rule `get_initial_state` already uses to restore
  `last_folder`: the frontend already computes plain-vs-specific launch as
  `!initial.initial_file && pending.length === 0`.

## Persistence (backend)

Extend the existing `recent.rs` `Store` rather than adding a new file — it
already pairs `last_folder` with the recents and uses the `#[serde(default)]`
backward-compat pattern:

```rust
struct Store {
    folders: Vec<PathBuf>,
    last_folder: Option<PathBuf>,
    #[serde(default)] open_tabs: Vec<PathBuf>,   // paths, in tab order
    #[serde(default)] active_tab: Option<usize>, // index into open_tabs
}
```

- New `recent::save_session(app, tabs: &[PathBuf], active: Option<usize>)`:
  writes `open_tabs` + `active_tab`, preserving `folders` and `last_folder`.
- New command `save_session(tabs: Vec<String>, active: Option<usize>)` →
  converts to `PathBuf` and calls `recent::save_session`. Best-effort; the
  frontend fire-and-forgets (mirrors `remember_folder`).
- Tab paths are stored **as-is, NOT canonicalized** (unlike folders). The live
  tab model compares tabs by string equality (`t.path === path`); canonicalizing
  on save could make a restored path mismatch a tree-click path and spawn a
  duplicate tab.
- `recent::clear` (Open Recent ▸ Clear) leaves `open_tabs`/`active_tab`
  untouched, exactly as it already leaves `last_folder`.

## Restore (backend + frontend)

- Pure, unit-tested helper:
  ```rust
  pub fn restore_session(
      tabs: Vec<PathBuf>,
      active: Option<usize>,
      exists: impl Fn(&Path) -> bool,
  ) -> (Vec<PathBuf>, Option<usize>)
  ```
  Keeps only paths satisfying `exists` (order preserved). Remaps `active` by
  tracking the active *path*: the returned active index is that path's position
  in the filtered list, or `None` if the active file is gone (or the list is
  empty).
- `get_initial_state` calls `restore_session(open_tabs, active_tab, |p| p.is_file())`
  and adds to `InitialState`:
  ```rust
  pub restore_tabs: Vec<String>,
  pub active_tab: Option<usize>,
  ```
  These are always returned; the frontend decides whether to use them.
- Frontend `restoreSession(paths, active)`: pushes every path as
  `{ path, sticky: true, raw: false }` in a single pass, then calls
  `setActiveTab` **once** (avoiding N renders and N `open_file` calls, each of
  which rewires the watcher). Defaults to index 0 when `active` is null and the
  list is non-empty.

## The persistence seam + restore guard

- Module-level `let restoring = true;` for the duration of `init()`.
  `persistSession()` early-returns while `restoring` is true.
- `persistSession()` invokes `save_session` with `tabs.map((t) => t.path)` and
  `activeIdx` (null when `-1`).
- Call sites:
  - `setActiveTab` — covers open-preview, open-sticky, switch, and
    close-to-neighbor (they all route through it).
  - `closeTab`'s last-tab-closed branch — which returns early without calling
    `setActiveTab`, so it needs its own `persistSession()` to save the now-empty
    session.
- `init()` ordering:
  1. `get_initial_state` (carries `restore_tabs` / `active_tab`).
  2. register listeners, `frontend_ready` → `pending`, resolve `treeRoot`,
     `renderRoot`, git.
  3. `plainLaunch = !initial.initial_file && pending.length === 0`.
  4. if `plainLaunch`: `restoreSession(initial.restore_tabs, initial.active_tab)`;
     else: open `initial.initial_file` and each `pending` file (as today).
  5. `restoring = false;` then `persistSession()` once — saves the final state
     (restored set, Finder-opened file, or empty) exactly once, and prevents the
     empty first paint from clobbering the saved session before it is read.

## Edge cases

- **Missing files**: filtered out on restore; if the active file is gone, the
  first surviving tab becomes active.
- **Empty / first run / legacy `recent.json`**: `restore_tabs` is empty →
  today's behavior, untouched.
- **Specific-file launch**: does not restore, and replaces the saved session
  with whatever ends up open (via the end-of-`init` `persistSession`).

## Testing

- Rust unit tests in `recent.rs`:
  - `Store` round-trips `open_tabs` + `active_tab`.
  - Legacy store (JSON without the two fields) deserializes to empty/None.
  - `restore_session`: all exist (active unchanged); a leading file missing
    (active index shifts down); the active file missing (→ None); empty input
    (→ empty, None).
- DOM/IPC restore code is not unit-tested, consistent with the rest of
  `app.js` — the testable logic lives in the Rust helper.

## Files touched

- `src-tauri/src/recent.rs` — `Store` fields, `save_session`, `restore_session`
  + unit tests.
- `src-tauri/src/commands.rs` — `save_session` command; `InitialState` fields;
  wire `restore_session` into `get_initial_state`.
- `src-tauri/src/lib.rs` — register the `save_session` command.
- `ui/app.js` — `persistSession`, `restoreSession`, the `restoring` guard, and
  the seam calls in `init` / `setActiveTab` / `closeTab`.
