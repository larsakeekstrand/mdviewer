# In-app editing: content editor + file operations

**Date:** 2026-06-02
**Status:** Approved (brainstorming) — pending implementation plan

## Goal

Turn mdviewer from a strict viewer into a lightweight editor: edit a document's
markdown/text content in-app, and create / rename / duplicate / delete files and
folders from the tree. The existing rendering pipeline (comrak → syntect → KaTeX
→ Mermaid, diffed with morphdom) and the project conventions (vanilla JS, no
build step, vendored libraries, hardened CSP, atomic writes) are preserved.

## Decisions (locked during brainstorming)

| Question | Decision |
|---|---|
| Editing model | Edit the **markdown source**, re-render with the existing pipeline. **No WYSIWYG** — it would bypass the render pipeline and require a bundler or a lossy HTML→markdown round-trip. |
| Layout | **Side-by-side split**: source editor left, live preview right, re-rendering as you type (debounced). |
| Editor widget | **CodeMirror 5** (markdown mode, line numbers), vendored as a UMD file — no build step. |
| Saving | **Explicit ⌘S** (and a Save button). A "modified" dot marks unsaved tabs. No autosave. |
| External-change conflict | **Warn and keep edits.** A clean editor still auto-reloads as today; a dirty editor shows a non-destructive banner and never auto-clobbers. |
| Delete | **Move to system Trash** (recoverable), after a confirm. |
| Tree operations | New File, New Folder, Rename, Duplicate, Delete. |
| Naming UX | **Inline tree rename** (VS Code style): the row becomes a text input; Enter commits, Esc cancels. |

**Out of scope (YAGNI):** WYSIWYG, autosave, multi-file / drag-drop moves,
undo-history beyond CodeMirror's built-in, find-replace within the editor.

---

## Part 1 — Content editor

### Tab model

Today a tab is `{ path, sticky, raw }`. Add three fields:

- `editing` — whether the active tab is in edit mode.
- `dirty` — editor content differs from `savedContent`.
- `savedContent` — the exact text last loaded-or-saved. Single anchor for both
  dirty-detection and conflict-detection. A plain string compare is sufficient
  at these file sizes; no hashing.

### CodeMirror (vendored)

- `ui/codemirror/cm.min.js` (CodeMirror 5 UMD), markdown mode, and its CSS,
  loaded as a classic `<script>` **before** the `app.js` module — same pattern
  as `mermaid.min.js` — so `window.CodeMirror` exists at `init()`.
- CSP unchanged: `script-src 'self'` covers the vendored file; the editor's
  injected styles are covered by the existing `style-src 'unsafe-inline'`.

### Layout

- An **Edit** (pencil) toolbar button toggles edit mode on the active tab.
  Hidden for image tabs (consistent with how Raw / Copy Source are guarded for
  images today).
- In edit mode, `#preview` becomes a right pane and a CodeMirror editor takes a
  new left pane, separated by a splitter that mirrors the existing sidebar
  splitter. Leaving edit mode restores the full-width preview.
- The `[hidden] { display: none !important }` rule must keep covering any new
  flex panes toggled via `.hidden` (see CLAUDE.md gotcha).

### Live preview

- New command `render_preview(source, path, theme)` renders the **editor's
  current text** (not disk), reusing `markdown.rs` and picking markdown vs plain
  by extension exactly like `render_file`. No new rendering code.
- CodeMirror's change event is debounced (~150 ms) → `render_preview` → the
  existing `postRender()` seam runs unchanged (mermaid / KaTeX / copy-buttons /
  images / sourcepos anchoring all work as-is).

### Save

- New command `save_file(path, contents, expected)`:
  - **read-verify-write**, mirroring `toggle_task`: read current disk content; if
    it ≠ `expected`, return `"file changed on disk"`; otherwise write via the
    existing atomic temp-file + same-directory rename helper.
  - A **"Keep my version"** override re-syncs `expected` to current disk content
    (or passes a force flag) so the next call writes unconditionally — this is
    the user's explicit consent to overwrite.
- On success the frontend sets `dirty=false`, `savedContent=contents`, clears the
  tab dot.
- Triggered by ⌘S and a Save button. ⌘S is a frontend key handler (no native
  menu accelerator required, but a **Actions ▸ Save** menu item may be added for
  discoverability).

### Watcher / self-write distinction

- Our own save fires `file-changed`. On each `file-changed` the frontend compares
  the incoming disk content to `savedContent`:
  - **equal** ⇒ our own write ⇒ ignore (only refresh git decoration).
  - **different** + editor **clean** ⇒ reload from disk (today's behavior).
  - **different** + editor **dirty** ⇒ show the conflict banner:
    *"This file changed on disk — [Reload from disk] [Keep my version]"*.
    Never auto-clobbers.

### Dirty guard

- Closing a tab or the window with unsaved edits prompts via Tauri's `ask`
  dialog before discarding.

---

## Part 2 — File operations

### New backend module `src-tauri/src/fs_ops.rs`

Keeps `commands.rs` (~840 lines) from growing further. Commands:

- `create_file(dir, name)` / `create_folder(dir, name)` — reject if the target
  already exists; return the new path.
- `rename_path(from, to)` — `std::fs::rename`; reject if `to` exists (no silent
  overwrite).
- `duplicate_file(path)` — copy to the first free `"<stem> copy<.ext>"`,
  `"<stem> copy 2<.ext>"`, … The unique-name picker is a **pure function**,
  unit-tested.
- `delete_to_trash(path)` — the `trash` crate (macOS Trash / Windows Recycle
  Bin), after a frontend confirm.

Reuse the existing atomic-write/rename helpers (generalized out of `commands.rs`)
where relevant.

### Safety boundary (containment)

- Every op canonicalizes its target and verifies it stays **within the current
  sidebar root**, reusing the `path_within_dir` logic (component-wise
  `starts_with`, symlinks resolved).
- To know the *current* root (the user can switch folders), `AppState` gains a
  `Mutex<PathBuf>` (`current_root`) updated whenever the frontend sets the
  sidebar root — extending the existing `remember_folder` call site.
- **No extension denylist on create/write**: making a `.sh` file as text is
  legitimate; the danger is only in *opening/executing*, which `UNSAFE_OPEN_EXTS`
  already guards. The editor only opens text — binary/image paths short-circuit
  before `read_to_string` as they do today.

### Frontend — tree context menu

- The existing custom tree context menu gains the five items. Target resolution:
  right-click a folder → New File / New Folder created **inside** it; right-click
  a file → created as a **sibling**.
- **Inline rename** (VS Code style): the tree row's label becomes an `<input>`;
  **Enter** commits, **Esc** cancels; blank names and path separators are
  rejected. New File / New Folder reuse the same widget on a freshly-inserted
  placeholder row. New files open in the editor ready to type.
- **Open tabs follow a rename**: any tab whose path is at/under the renamed path
  is rewritten and its watcher rewired.
- **Delete** (after a Tauri `ask` confirm) closes tabs pointing at the removed
  path, then the tree refreshes. The existing tree watcher keeps external changes
  in sync.

---

## Testing

- **Rust (`fs_ops` unit tests):**
  - create rejects an existing target;
  - rename rejects an existing destination;
  - containment rejects a `../` / symlink escape;
  - duplicate picks a unique name;
  - `save_file` rejects on disk-divergence and writes on match.
  - (Trash itself is not asserted — only the pure name/containment logic, the
    way `decide` / `classify_link` are tested apart from the privileged action.)
- **JS pure helpers (`node --test`, the `ui/export.js` pattern):**
  - dirty-state transitions;
  - conflict detection (disk vs `savedContent` compare);
  - unique-"copy"-name derivation;
  - inline-rename name validation.

## Phasing (for the implementation plan)

- **Phase 1 — editor:** split view, CodeMirror, `render_preview`, `save_file`,
  conflict handling, dirty guard. Independently shippable.
- **Phase 2 — file operations:** `fs_ops.rs`, containment boundary, tree context
  menu, inline rename, open-tab follow-through, delete-to-trash. Independently
  shippable.

## New dependencies

- **CodeMirror 5** — vendored UMD JS + CSS under `ui/codemirror/` (no Cargo
  dependency, no build step).
- **`trash`** crate — added to `src-tauri/Cargo.toml` for delete-to-Trash.

## Documentation impact

- Update `README.md` (Features / Usage / Menus) — mdviewer is now an editor.
- Update `CLAUDE.md` — new `fs_ops.rs`, `render_preview` / `save_file` commands,
  tab edit-state fields, CodeMirror vendoring note, `AppState.current_root`,
  and the editor↔watcher self-write distinction.
