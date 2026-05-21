# Remember the last selected directory across restarts — design

**Date:** 2026-05-21
**Status:** Approved, ready for planning

## Problem

A plain launch of MDViewer — from the Dock, the Finder app icon, or `mdviewer`
with no argument — always shows the process working directory in the sidebar
(`/` for a bundled `.app`). The app already persists a recent-folders list
(`recent.rs` → `recent.json` in `app_data_dir`), but it never restores where you
last were. Reopening the app drops you at `/` and you have to re-navigate.

The earlier file-associations spec explicitly left "changing the bare-launch
(no file) default tree root" out of scope; this feature closes that gap.

## Goal & scope

- On a **plain launch** (no file/folder argument, no Finder-opened file), open
  the directory the sidebar last showed instead of the process cwd.
- "Last selected directory" = **whatever the sidebar last displayed**, however
  the user got there: Open Folder, Open Recent, or opening a file (the sidebar
  follows the file's folder). Confirmed with the user.
- Explicit launch inputs still win for that session: an `argv` file/folder and a
  Finder-opened file take precedence over the remembered folder — and they
  update what's remembered for next time.
- Survive a remembered folder that no longer exists: fall back to cwd.

Out of scope: feature originally paired with this request — *select & copy text*
— is **already implemented and working** (`styles.css` `user-select: text`,
`app.js` copy actions, `menu.rs` Copy/⌘C, right-click Copy), confirmed with the
user; no work needed. Also out of scope: remembering open tabs, scroll position,
window size/position, or the expanded/collapsed state of the tree.

## Approach (chosen)

**B — a dedicated `last_folder` value inside the existing `recent.json`.** The
*frontend* is the single source of truth for which folder is shown, so it owns
persistence; the *backend* only reads the saved value to resolve a plain launch.

Rejected:

- **A — reuse the recent list (`recent[0]`).** Conflates two concepts: it would
  pollute "Open Recent" with folders merely passed through (a file's parent) and
  keep seeding `/` into the menu on plain launches.
- **C — frontend-only `localStorage`.** Would render `/` first (a slow listing
  of root) then re-render to the saved folder; the backend store is the
  established pattern for persistent state and resolves *before* the first tree
  render.

No new dependencies.

## Data flow

```
Launch:
  main.rs argv → Startup.tree_root: Option<PathBuf>   (None = plain launch)
  frontend init() → get_initial_state(app, state):
      Some(explicit)                         → use it,            persist last_folder
      None & load_last() exists as dir       → restore it,        persist last_folder
      None & no/dead last_folder             → cwd (fallback),    DO NOT persist
  → returns resolved tree_root to the frontend

Session (sidebar root changes):
  setTreeRoot(path)            (Open Folder / Open Recent) → invoke remember_folder(path)
  init() cold-Finder branch    (double-clicked file)       → invoke remember_folder(parent)
  → backend recent::save_last(path)
```

## Components

### 1. `src-tauri/src/recent.rs` — store both fields, never clobber

The store gains a second field; the file stays one JSON document and is
backward-compatible via `#[serde(default)]` (old files without the field load
fine):

```rust
#[derive(Default, Serialize, Deserialize)]
struct Store {
    folders: Vec<PathBuf>,
    #[serde(default)]
    last_folder: Option<PathBuf>,
}
```

Today's `write(list: &[PathBuf])` rebuilds `Store { folders }` from the list
alone — that path would **erase `last_folder`**. Refactor to read-modify-write
the whole `Store`, and split the pure mutation logic (unit-testable, no
`AppHandle`) from the file I/O:

- Pure, on `impl Store` (no filesystem, no `AppHandle`):
  - `fn push_folder(&mut self, canonical: PathBuf)` — dedup + insert-front + cap
    at `MAX_RECENT` (today's `push` body, minus the canonicalize call).
  - Setting `last_folder` is a one-line field assignment, done inline.
- File I/O (`AppHandle`-coupled): `fn load_store(app) -> Store` (missing/corrupt
  → `Store::default()`), `fn write_store(app, &Store)`.
- Public API (thin wrappers = load_store → mutate → write_store):
  - `pub fn load(app) -> Vec<PathBuf>` = `load_store(app).folders` (unchanged
    signature; `menu.rs` keeps calling it).
  - `pub fn push(app, folder) -> Vec<PathBuf>`: canonicalize (as today), then
    `store.push_folder(canonical)`, write, return `store.folders`.
  - `pub fn clear(app)`: clear `folders` only, write — **preserves
    `last_folder`** (clearing the Open Recent menu must not forget where you
    are).
  - `pub fn load_last(app) -> Option<PathBuf>` = `load_store(app).last_folder`.
  - `pub fn save_last(app, folder: &Path)`: canonicalize-or-keep (same fallback
    as `push`), set `store.last_folder`, write.

`display()` is unchanged. Canonicalization stays in the `AppHandle` wrappers so
`push_folder` takes an already-normalized path and is testable with arbitrary
`PathBuf`s.

### 2. `src-tauri/src/main.rs` — a plain launch yields `None`

`resolve_args()` returns `Startup.tree_root: Option<PathBuf>`:

- No argument → `tree_root: None` (was `cwd`). This is the only behavioral change
  here; the cwd fallback now lives in the backend resolver.
- Directory argument → `Some(canonical)`.
- File argument → `Some(parent)`, `initial_file: Some(canonical)` (unchanged).

### 3. `src-tauri/src/lib.rs` — `Option` tree root, guarded startup push

- `Startup.tree_root` and `AppState.tree_root` become `Option<PathBuf>`.
- Setup hook: the existing `recent::push(&handle, &state.tree_root)` becomes
  `if let Some(root) = &state.tree_root { recent::push(&handle, root); }` — stop
  seeding `/` (or any restored folder) into Open Recent on a plain launch; only
  an explicit dir/file launch contributes to the recent list, as before.
- Register `commands::remember_folder` in `invoke_handler!`.
- Verify no other reader of `state.tree_root` exists (`open_files.rs`,
  `watcher.rs` use `state.opens` / `state.watcher`, not `tree_root`); only
  `lib.rs` setup and `commands::get_initial_state` read it.

### 4. `src-tauri/src/commands.rs` — resolve on launch, persist on change

`get_initial_state` gains an `AppHandle` parameter and resolves the root:

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

The bare-cwd fallback is deliberately **not** persisted, so a virgin first
launch won't stickily remember `/`. Re-saving an explicit/restored root is a
harmless no-op-equivalent (idempotent write).

New command:

```rust
#[tauri::command]
pub fn remember_folder(app: AppHandle, path: String) {
    let p = PathBuf::from(path);
    if p.is_dir() {
        recent::save_last(&app, &p);
    }
}
```

Errorless (returns `()`): persistence is best-effort UI state; a failure must
never reject anything user-facing. (`recent::save_last` already swallows I/O
errors, consistent with the rest of the module.)

### 5. `ui/app.js` — persist whenever the sidebar root is set

Two call sites, both fire-and-forget (`invoke("remember_folder", { path })`
inside a `try/catch` that only `console.error`s, matching existing IPC error
handling):

- `setTreeRoot(path)` — the single chokepoint for **Open Folder** and
  **Open Recent** (both arrive as the `open-folder` event →
  `openExternalFolder` → `setTreeRoot`).
- `init()` cold-Finder branch — when `treeRoot` is set to
  `parentDir(pending[0])` (a file double-clicked in Finder while the app was
  closed), persist that parent.

`init()` does **not** call `remember_folder` for the non-Finder path: the
backend's `get_initial_state` already persisted the explicit/restored root, and
the bare cwd default must stay unpersisted.

## Error handling

- Missing/corrupt `recent.json` → `load_store` returns `Store::default()`
  (today's `unwrap_or_default` behavior), so `last_folder` is `None` and launch
  falls back to cwd.
- Remembered folder deleted/renamed → `.filter(|p| p.is_dir())` rejects it →
  cwd fallback.
- `remember_folder` with a non-directory or vanished path → no-op (guarded).
- All persistence I/O is best-effort and silent, matching `recent.rs` today.

## Testing

- **Rust unit tests** (`recent.rs`, run in CI on macos-14) — pure, no
  `AppHandle`, operating on `Store` and serde directly:
  - `push_folder` dedups, moves to front, and caps at `MAX_RECENT`.
  - Setting `last_folder` then `push_folder` leaves `last_folder` intact (the
    clobber regression — guaranteed because a single whole `Store` is written).
  - Clearing `folders` leaves `last_folder` intact.
  - `serde_json` round-trip of a `Store` preserves both fields; a JSON document
    written **without** the `last_folder` field deserializes with
    `last_folder == None` (serde-default / backward-compat).
- **Manual:**
  1. `cd src-tauri && cargo run -- ~/some/folder` → Open Recent / navigate so the
     sidebar shows folder X; quit.
  2. `cargo run` (no arg) → sidebar shows X, not the cwd.
  3. `cargo run -- ~/other` → shows `~/other` (explicit arg wins), and becomes
     the new remembered folder; quit; `cargo run` → shows `~/other`.
  4. Delete the remembered folder; `cargo run` → falls back to cwd, no crash.
  5. (Bundle) double-click a `.md` in Finder with the app closed → sidebar shows
     its folder; quit; relaunch from Dock → that folder is restored.

## Notes / risks

- **CWD of a bundled app is `/`.** That only matters as the first-ever-launch
  fallback; once any real folder is shown it's remembered.
- **`canonicalize` in `save_last`** mirrors `push`, so the stored path is
  normalized and comparisons/`is_dir` checks behave like the recent list.
- **Bundle identifier** stays `com.mdviewer.app`; `app_data_dir()` and therefore
  `recent.json` are keyed off it (see CLAUDE.md) — unchanged.
- **No new dependencies, no CSS/HTML changes.** Frontend edits require a
  `cargo build` to re-bundle (`frontendDist` is compiled in).
