# Multi-window: one project per window

**Date:** 2026-09-19
**Status:** Approved (brainstorming) — pending implementation plan

## Goal

Let MDViewer show several projects at once, one project (tree root) per window,
the way VS Code and JetBrains do.

The driving use case is running several Claude Code sessions in different repos
in parallel. Each session's plans, `open_document` calls and review requests
should land in *that repo's* window, not in whichever window happens to be in
front, and never in a window whose tree can't show the file.

Today the app is single-window by construction:

- `AppState` holds one `current_root`, one active-file `WatcherSlot`, one
  `TreeWatcherSlot`, and one `PendingOpens` ready flag.
- Every backend → frontend event is a broadcast `app.emit(...)` (`watcher.rs`,
  `menu.rs`, `mcp_server.rs`, `open_files.rs`). A second window would react to
  the first window's `file-changed`, menu actions and MCP calls.
- The PDF-export window talks to "the" preview window via frontend broadcast
  `emit(...)` (`pdf-export-request-preview` / `-run` / `-done`).
- `recent.json` stores one `last_folder` and one tab session.
- Windows has no single-instance handling: a second launch (CLI, or the Claude
  hook's re-spawn) is a second process that loses the MCP socket race. On macOS
  the CLI symlink runs the binary directly, so `mdviewer file.md` from a
  terminal also starts a second GUI process today.

## Decisions (locked during brainstorming)

| Question | Decision |
|---|---|
| Model | **One process, many webview windows**, per-window state keyed by window label. Not one process per window (the MCP socket and updater must stay single; cross-process routing would need a second IPC layer). |
| Routing an incoming file | **Longest containing root wins**; ties (same root in two windows) → most-recently-focused. |
| File with no containing window | **New window** rooted at the file's git repo root, else its parent folder. |
| Open Folder / Open Recent | **Replaces the focused window's root** (VS Code default). If another window already shows that root, focus it instead. |
| New window | **File ▸ New Window (⌘⇧N)** → folder picker; no window if cancelled. No rootless windows. |
| Relaunch | **Restore all windows** open at quit (root + bounds), each with its root's tabs. |
| Tab memory | **Per root**, not per window, so a root swap or window close keeps the project's tabs. |
| Menu target | **Front of the MRU focus list.** |
| `get_viewer_state` | Routed by the **MCP proxy's cwd**, falling back to MRU. |
| Second launches | **`tauri-plugin-single-instance`** forwards argv to the running process on both platforms. |
| Delivery | **Three phases**, each independently mergeable (see below). |

## Architecture

### Backend state

```rust
pub struct WindowState {
    pub root: PathBuf,                 // canonical; every project window has one
    pub watcher: WatcherSlot,          // active-file watcher
    pub tree_watcher: TreeWatcherSlot,
    pub ready: bool,                   // frontend_ready called
    pub pending_files: Vec<PathBuf>,   // opens routed here before ready
}

pub struct AppState {
    pub windows: Mutex<HashMap<String, WindowState>>, // key = window label
    pub focus: Mutex<FocusOrder>,                      // MRU of project-window labels
    pub startup_queue: Mutex<Vec<Vec<String>>>,        // single-instance argv before setup completes
    pub tasklist_lock: Mutex<()>,                      // unchanged, process-wide
    // tree_root / initial_file stay as launch inputs only
}
```

`current_root`, `watcher`, `tree_watcher` and `opens` leave `AppState`; their
per-window equivalents live in `WindowState`.

Project window labels: `main` (the `tauri.conf.json` window) and `project-<n>`
(monotonic counter). Helper windows (`preferences`, `pdf-export`,
`claude-integration`) are never entered in `windows` or `focus`.

### Commands learn their window from Tauri

Commands that act on per-window state take `window: tauri::WebviewWindow` and
look up `windows[window.label()]`. The label comes from Tauri, not from a
frontend argument, so a page cannot claim to be another window. Affected:
`get_initial_state`, `open_file`, `watch_tree`, `frontend_ready`,
`remember_folder`, `save_session`, `create_file`, `create_folder`,
`rename_path`, `duplicate_file`, `delete_to_trash`, `install_claude_hook`,
`install_mcp_server`, `integration_status`, `mcp_respond`, `mcp_review_result`.
`export_pdf` already prints the calling window's webview. `toggle_task`,
`save_file`, `git_status` and `search_in_folder` take explicit paths and are not
root-confined today; they stay as they are (out of scope here).

A missing entry (command from a helper window, or a race with close) is an
`Err("no project window")`, never a fallback to another window's root.

### Routing (`src-tauri/src/routing.rs`, pure, unit-tested)

```rust
pub enum Route { Window(String), NewWindow(PathBuf) }

pub fn route(
    path: &Path,
    roots: &[(String, PathBuf)],   // (label, canonical root)
    mru: &[String],
    fallback_root: impl Fn(&Path) -> PathBuf, // git root or parent
) -> Route
```

- Candidates are windows whose root contains `path` (component-wise
  `starts_with` on canonical paths, same semantics as `fs_ops::within_root`).
- The longest root wins, so `/repo/docs` beats `/repo`.
- Equal roots: earliest in `mru` wins; a label absent from `mru` ranks last.
- No candidate → `NewWindow(fallback_root(path))`. The git-root lookup is the
  IO wrapper around the pure core; the pure function takes it as a closure.

`FocusOrder` (same module): `touch(label)`, `remove(label)`, `front()`.
Updated from `WindowEvent::Focused(true)` and `Destroyed`.

Revised during implementation (Ruling R2 in the SDD progress ledger): a
first-ever Finder launch with no argv and no saved session roots `"main"` at a
bare-cwd fallback ("placeholder main"), which contains every path and would
otherwise swallow all routing. `routing.rs` adds `Delivery`
(`Existing(label)` / `RetargetMain(root)` / `NewWindow(root)`) and
`plan_delivery(path, roots, mru, placeholder_main, fallback_root)`, which
excludes a placeholder `"main"` from the routing candidates and, when nothing
else claims the path, returns `RetargetMain` instead of `NewWindow` — so the
first Finder open repoints `"main"` at the file's root rather than opening a
second, useless window at "/". The IO entry points are `windows::deliver_path`
(a single file: resolves `plan_delivery`, retargets/creates the window via
`windows::retarget_placeholder_main` / `create_project_window`, then hands the
file to `deliver_files`) and `windows::deliver_files` (emits `open-file` via
`emit_to(label, …)` if the target's `ready`, else buffers into its
`pending_files` under the same lock as the ready check). Finder opens,
single-instance argv and the hook all use `deliver_path`. MCP needs the label
for `McpPending`, so it doesn't call `plan_delivery` — `mcp_server.rs` reimplements
the same placeholder-exclusion + retarget-or-create shape locally
(`pick_target_for` filters a placeholder `"main"` out of the routing
candidates; `prepare_request`'s `Target::NewWindow` arm retargets it, or
creates a window, the same way `deliver_path` does) — see the MCP section
above.

### Events

| Event | Today | After |
|---|---|---|
| `file-changed`, `tree-changed` | broadcast | `emit_to(label)` of the watcher's window |
| menu `edit-action`, `export`, `open-file`, `open-folder`, `menu-install-cli` | broadcast | `emit_to(focus.front())` |
| `mcp-*` | broadcast | `emit_to(routed label)` |
| `open-file` (Finder / single-instance / hook) | broadcast | `routing::deliver` |
| `menu-check-updates`, `channel-changed`, `integration-changed` | broadcast | unchanged (app-wide) |

The watcher closures capture their window's label when `open_file` /
`watch_tree` install them.

### MCP

- The proxy (`mcp.rs::run_proxy`) adds `"cwd": <proxy cwd>` to every
  `GuiRequest`. Paths are already absolutized against it.
- `open_document`, `request_review`, `generate_pdf` route by their `path`
  argument. For `open_document`/`request_review`, a `NewWindow` route validates
  first, then creates the window (or repoints a still-placeholder `"main"`)
  and answers `STARTING_ERR`. The proxy already retries that for up to 15 s,
  and the retry routes into the new window once it is ready, so no new queue
  is needed. `get_viewer_state` routes by `cwd` (`route(cwd, …)`, but a
  `NewWindow` result falls back to `focus.front()` (or the lowest label)
  instead — asking for state must not open a window).
- **`generate_pdf` never opens or retargets a window.** Revised from the
  original design during implementation (phase-3 security review, SEC-1):
  writing a file must not widen trust to a root the caller merely named in
  `path`. `generate_pdf`'s `path_target` maps its own `Route::NewWindow` result
  straight to a refusal (`"source is outside every open workspace"`) instead of
  the create/retarget branch above — the source must already be inside a root
  some open window owns. `open_document`/`request_review` are unaffected and
  still auto-open.
- `validate` receives the **routed window's** root; `generate_pdf`'s
  source/output containment is checked against it (only ever an *existing*
  window's root, per the point above).
- `McpPending` stays global but each entry records its target label.
  `mcp_respond` / `mcp_review_result` reject an answer whose calling window
  label doesn't match.
- `request_review` focuses the routed window, not `main`.
- The "one review at a time" gate (`reviewBusy`) is already per window,
  because each window has its own `app.js`. Two Claude sessions in two repos
  can each have a review open.
- `WindowEvent::Destroyed` resolves that window's pending entries: a review →
  `{"declined": true}` (a success, as for tab close today); others → error
  `"MDViewer window closed"`.

### Single instance

`tauri-plugin-single-instance` is registered first in the builder. Its callback
receives `(argv, cwd)`. Argv goes through the same resolution as `main.rs`'s
`resolve_args` (factored into a shared pure-ish `resolve_launch_path(raw, cwd)`),
then:

- a directory → focus the window with that exact root, else a new window;
- a file → `routing::deliver`.

`--claude-hook` / `--mcp` invocations never reach the plugin: `main.rs`
dispatches them before `mdviewer_lib::run`. Argv that arrives before `setup`
completes waits in `startup_queue` and is drained at the end of `setup`.

With the plugin in place, `claude_hook::open_in_mdviewer` keeps its current
launch commands (`open -a` on macOS, exe re-spawn on Windows); the re-spawned
process forwards and exits.

## Frontend

Each window loads its own `index.html` / `app.js`, so the tab model,
`renderCache`, `domCache`, review state and editor are already per window.
Changes:

- `get_initial_state` returns this window's root and that root's restored
  tabs, and any `initial_file` routed to it.
- **Window title** `"<root folder name> — MDViewer"` (pure
  `windows::window_title`, unit-tested), set from Rust on window creation and
  on every root change, so no extra window permission is needed.
- **Open Folder / Open Recent** in the focused window: save the outgoing
  root's session, then set the new root. If another window already has that
  root, the backend focuses it and the current window is left unchanged
  (`remember_folder` returns `{focused: label}` instead of adopting the root; decision in pure
  `routing::open_folder_target`).
- **Close guard:** `onCloseRequested` → if any tab is dirty, the existing
  discard prompt; cancel → `event.preventDefault()`. Before closing, save the
  session.
- **Helper windows carry an owner, recorded in the backend.** Opening
  `pdf-export` or `claude-integration` records `helper label → project label`
  in the registry (menu: the MRU front window; nudge: the calling window). The
  frontend never supplies the owner. Integration commands resolve the root via
  that record; `pdf-export.js` asks `helper_owner` for the label and uses
  `emitTo(owner, …)`, and `app.js` replies with `emitTo("pdf-export", …)`.
- **Listeners are window-scoped.** In Tauri 2 the JS `event.listen()` defaults
  to target *Any* and would receive `emit_to` events aimed at other windows, so
  `app.js` and `pdf-export.js` listen via `getCurrentWebviewWindow().listen`.

## Window lifecycle

- **New Window (⌘⇧N)**, `menu.rs`: folder picker → `window::create(app, root,
  bounds: None)`. If a window already has that exact root, focus it.
- **`windows::create_project_window`**: insert the `WindowState` for label
  `project-<n>`, then build the `WebviewWindow` (cascade 24 px from the focused
  window, or saved bounds). Inserting first means the new webview's first
  `get_initial_state` always finds its entry; a failed build removes it again.
- **`Destroyed`**: stop watchers (drop the slots), resolve pending MCP entries,
  remove from `windows` and `focus`. If it was the last project window, write
  the `windows` snapshot first (see Persistence) and let the app exit as it
  does today.
- **macOS app reactivation with no windows** isn't reachable today (closing
  the last window quits). That stays as it is.

## Menu

- The app-global menu's `on_menu_event` sends window-scoped actions to
  `focus.front()`. On Windows, each window's menu bar gets the same handler;
  clicking a menu focuses its window first, so MRU is correct there too.
- New item **File ▸ New Window ⌘⇧N**.
- The macOS **Window** submenu (predefined items) lists open windows
  automatically once titles are set.
- **Actions** keeps its name (macOS `Edit`-submenu auto-insert trap).

## Persistence (`recent.json`)

```jsonc
{
  "folders": ["..."],                           // Open Recent — unchanged
  "sessions": {                                 // per root, LRU-capped at 30
    "/abs/root": { "tabs": ["..."], "active": 1, "touched": 1758290000 }
  },
  "windows": [                                  // snapshot at quit
    { "root": "/abs/root", "bounds": { "x": 0, "y": 0, "w": 1200, "h": 800 } }
  ],
  "last_folder": "/abs/root",                   // kept: argv-less fallback
  "channel": "stable", "pdf": { }               // unchanged
}
```

- `save_session` writes into `sessions[window root]`. Root swap, window close
  and quit all save first.
- `windows` is written on `RunEvent::Exit` (on macOS ⌘Q raises only `Exit`),
  on `ExitRequested` (last window closed, `app.exit()`), and on the last
  project window's `Destroyed`, snapshotting every project window. Closing a
  non-last window does not rewrite it, so relaunch restores what was open at
  quit.
- **Restore on launch** (`recent::restore_windows`, pure): drop entries whose
  root is no longer a directory; the first survivor reuses `main`, the rest
  become `project-*`. Tauri does not clamp bounds, so a saved position that
  overlaps no current monitor (≥ 50×50 logical px) is dropped, keeping the
  size; minimized/fullscreen geometry is never recorded. If none survive,
  fall back to today's argv → `last_folder` → cwd.
- **Argv plus saved windows:** an argv *file*: restore the saved windows (main =
  the first), then deliver the file through routing (so `mdviewer ~/repoB/plan.md` with repoB already
  restored lands in repoB's window). An argv *folder* becomes main's root, with
  the saved windows restored alongside (a saved window on the same root is
  skipped).
- **Migration:** a legacy store with `last_folder` + top-level `tabs`/`active`
  becomes `sessions[last_folder]`; no `windows` entry is synthesized — the
  `last_folder` fallback already yields the same single window. Serde
  defaults keep old files loading; covered by `deserializes_legacy_store_*`
  style tests.

## Error handling

| Situation | Behavior |
|---|---|
| Routed window not ready yet | Buffer in its `pending_files` / pending MCP handoff; drained by its `frontend_ready` under the same lock that sets `ready`. |
| Target window closes mid-MCP-call | `Destroyed` resolves pending: review → `{"declined": true}`; others → error. |
| Window creation fails (root vanished) | Error to the caller (MCP error; hook stays silent as today); the pre-inserted state is removed. |
| Command from a label with no `WindowState` | `Err("no project window")`; never borrows another window's root. |
| Saved window root missing at launch | Dropped from the restore list. |
| Single-instance argv before setup | Queued in `startup_queue`, drained at end of `setup`. |

## Security

- **Containment is per window.** `within_root` checks against the calling
  window's root, identified by Tauri's label. Project A's document cannot use
  file ops, task toggles, `generate_pdf` output or the hook/MCP install to
  reach project B's tree.
- **Routing doesn't widen trust.** Auto-creating a window rooted at a file's
  git root / parent is equivalent to the user opening that folder — true for
  `open_document`/`request_review`, which only ever read/display. `generate_pdf`
  writes a file, so it does NOT get this auto-open: its `path_target` refuses
  a `NewWindow` route outright (see the MCP section above and SEC-1 in the
  phase-3 security review) rather than treat a path the caller merely named as
  license to create a window and confine an output to it. `open_path`,
  `UNSAFE_OPEN_EXTS` and the exec-bit refusal are unchanged.
- **Capabilities:** `default.json` `windows` becomes
  `["main", "project-*", "preferences", "claude-integration", "pdf-export"]`.
  `project-*` gets exactly what `main` has; helper windows stay explicitly
  named.
- **Helper-window owner** is recorded by the backend when the helper opens and
  never accepted from the frontend.
- **Single-instance argv** is untrusted input: it is canonicalized and
  stat-checked exactly like a cold launch.
- **New IPC surface:** one command, `helper_owner` (returns the caller's owner
  label; no arguments). New Window is menu-only Rust. Every per-window command
  gains an implicit `WebviewWindow` argument; none gains a path-bearing
  argument. `core:window:allow-destroy` is added for the close guard.
- A `security-reviewer` pass is required before merging phase 1 (label →
  state scoping, capability glob) and phase 3 (routing, single-instance argv,
  MCP cwd).

## Cross-platform

- **macOS:** Finder opens (`RunEvent::Opened`) go through `routing::deliver`
  instead of broadcast. The CLI symlink path now forwards to the running
  instance via single-instance.
- **Windows:** single-instance fixes the multi-process behavior; argv opens
  and the hook's re-spawn forward to the running process. Per-window menu bars
  use the MRU rule.
- PDF export remains macOS-only; `export_pdf` targets the calling window's
  webview instead of `main`.

## Testing

- **Rust unit (pure):** `routing::route` (longest match, nested roots, tie →
  MRU, label missing from MRU, no match → fallback, Windows-style paths behind
  `cfg`), `FocusOrder`, `open_folder_target`, `resolve_launch_path`,
  `recent` migration, sessions LRU cap, `restore_windows` filtering,
  `McpPending` label mismatch rejection and resolve-on-destroy.
- **JS `node --test`:** `anyDirty`, `themeFromStorageEvent` (`ui/windowscope.js`).
- **Smoke:** extend `scripts/smoke-test.sh` to open two roots and verify over
  the MCP socket that `open_document` for a file in root B, and
  `get_viewer_state` with cwd B, answer from B's window.
- **Manual GUI check before merge** (visual regressions escape automated
  tests): two windows side by side; a menu action hits only the focused
  window; live reload in A doesn't touch B; PDF export from B previews B's
  document; ⌘Q + relaunch restores both windows with their tabs and bounds;
  dark mode in both.
- **Gate per task:** `cargo fmt --check`, `cargo clippy --all-targets -- -D
  warnings`, `cargo test`, `node --test ui/*.test.js`; `cargo build` after any
  `ui/*` change (Tauri bundles `frontendDist` at compile time).

## Delivery phases

Each phase is mergeable on its own and leaves the app shippable. Per-root
sessions come before a second window can exist, so two windows never overwrite
one shared session.

1. **Per-window state, no behavior change.** `WindowState` registry,
   label-scoped commands and watchers, `emit_to` + window-scoped listeners,
   backend-recorded helper owners, per-window `McpPending` entries.
   Acceptance: with one window, behavior is identical to today. Carries most
   of the regression risk.
2. **Multiple windows.** Per-root sessions + migration, window lifecycle + New
   Window + MRU menu routing, close guard + theme sync, Open Folder focus-or-
   adopt, restore all windows.
3. **Routing.** `routing::route`, Finder / Open File routing, single-instance
   plugin, MCP routing by path and cwd, two-root smoke test, docs.

## Out of scope

- Multi-root workspaces (several roots in one window).
- Dragging tabs between windows.
- Per-window theme. Theme stays one global localStorage preference. Nothing
  listens for cross-window changes today, so phase 2 adds a `storage` event
  listener in `app.js`: toggling ☾/☀ in one window calls `applyTheme` in the
  others (drops their theme-dependent caches exactly like a local toggle).
- A rootless "empty" window.
