# In-app Editing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make mdviewer a lightweight editor — edit a document's markdown/text content in a side-by-side source editor with live preview, and create / rename / duplicate / delete files and folders from the tree.

**Architecture:** The editor edits the **markdown source** and re-renders through the existing comrak→syntect→KaTeX→Mermaid pipeline (a new `render_preview` command renders the editor buffer instead of disk), so no rendering code changes. Explicit ⌘S saves via a read-verify-write `save_file` command (the `toggle_task` pattern). Tree operations live in a new `fs_ops.rs` module behind thin command wrappers, confined to the current sidebar root. CodeMirror 5 is vendored like Mermaid (classic `<script>`, no build step).

**Tech Stack:** Rust / Tauri 2.11, vanilla JS (no framework, no build step), CodeMirror 5 (vendored UMD), `trash` crate. Rust tests via `cargo test`; JS pure-helper tests via `node --test ui/*.test.js`.

**This plan is two independently-shippable milestones:**
- **Phase 1 (Tasks 1–10): the editor.** Ships a working split-view editor with save and conflict handling.
- **Phase 2 (Tasks 11–18): file operations.** Ships tree create/rename/duplicate/delete.

Commit after every task. Run `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` from `src-tauri/` before each backend commit; run `node --test ui/*.test.js` before each JS-helper commit. **Frontend changes require `cargo build` to appear** (Tauri bundles `ui/` at compile time) — note this when manually verifying, but the automated steps below rely on unit tests, not a running app.

---

## File Structure

**Phase 1 — editor**

- Create: `src-tauri/src/markdown.rs` — *(no, existing)*. New command added in `src-tauri/src/commands.rs`: `render_preview`, `save_file`.
- Modify: `src-tauri/src/commands.rs` — add `render_preview`, `save_file`; make `write_atomically` reusable (already module-private, fine).
- Modify: `src-tauri/src/lib.rs` — register the two new commands; add `current_root` to `AppState`.
- Create: `ui/editor.js` — pure helpers (`classifyFileChange`, dirty state) tested under `node --test`.
- Create: `ui/editor.test.js` — tests for `editor.js`.
- Vendor: `ui/codemirror/{codemirror.min.js,codemirror.min.css,xml.min.js,markdown.min.js}`.
- Modify: `ui/index.html` — CodeMirror scripts/CSS; editor pane + editor splitter; Edit/Save toolbar buttons.
- Modify: `ui/styles.css` — editor pane / split layout / dirty dot / conflict banner.
- Modify: `ui/app.js` — tab edit-state fields; enter/exit edit mode; CodeMirror wiring; live preview from buffer; save; conflict banner; dirty guard; `file-changed` integration.
- Modify: `src-tauri/src/menu.rs` — Actions ▸ Toggle Edit / Save menu items.

**Phase 2 — file operations**

- Create: `src-tauri/src/fs_ops.rs` — pure + IO helpers (`validate_name`, `duplicate_candidate`, `within_root`, `create_file`, `create_folder`, `rename_path`, `duplicate_file`) with unit tests.
- Modify: `src-tauri/Cargo.toml` — add `trash` dependency.
- Modify: `src-tauri/src/commands.rs` — thin command wrappers (`create_file`, `create_folder`, `rename_path`, `duplicate_file`, `delete_to_trash`) that read `current_root` and guard containment; extend `remember_folder` to set `current_root`.
- Modify: `src-tauri/src/lib.rs` — `mod fs_ops;` and register the five commands.
- Create: `ui/treeops.js` — `validateName` (mirrors Rust), tested.
- Create: `ui/treeops.test.js`.
- Modify: `ui/app.js` — context-menu items, inline rename widget, open-tabs-follow-rename, delete/duplicate flows.
- Modify: `ui/styles.css` — inline-rename input styling.

---

# PHASE 1 — EDITOR

## Task 1: `render_preview` command (render the editor buffer, not disk)

**Files:**
- Modify: `src-tauri/src/commands.rs` (add command near `render_file`, ~line 121)
- Modify: `src-tauri/src/lib.rs` (register, ~line 60)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module at the bottom of `src-tauri/src/commands.rs` (before the final closing `}`):

```rust
    #[test]
    fn render_preview_uses_markdown_for_md_paths() {
        let html = render_preview("# Hi".to_string(), "/x/note.md".to_string(), None);
        assert!(html.contains("<h1"), "expected markdown render, got: {html}");
    }

    #[test]
    fn render_preview_uses_plain_for_txt_paths() {
        let html = render_preview("# Hi".to_string(), "/x/note.txt".to_string(), None);
        assert!(!html.contains("<h1"), "plain text must not become an h1: {html}");
        assert!(html.contains("# Hi"), "plain text should be preserved: {html}");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test render_preview`
Expected: FAIL — `cannot find function render_preview`.

- [ ] **Step 3: Add the command**

Insert after `render_file` (after its closing `}` near line 121) in `src-tauri/src/commands.rs`:

```rust
/// Render an in-memory editor buffer (NOT disk) so the split editor's live
/// preview reflects unsaved text. Mirrors `render_file`'s markdown-vs-plain
/// choice by path extension; there is no raw mode (the editor itself is the
/// source view).
#[tauri::command]
pub fn render_preview(source: String, path: String, theme: Option<String>) -> String {
    let p = PathBuf::from(&path);
    let theme = theme.as_deref().unwrap_or("light");
    if markdown::is_markdown_path(&p) {
        markdown::render_markdown(&source, theme)
    } else {
        markdown::render_plain(&source)
    }
}
```

- [ ] **Step 4: Register the command**

In `src-tauri/src/lib.rs`, add to the `generate_handler!` list (after `commands::render_notes,` ~line 61):

```rust
            commands::render_preview,
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cd src-tauri && cargo test render_preview`
Expected: PASS (both tests).

- [ ] **Step 6: Lint + commit**

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "Add render_preview command for live editor preview"
```

---

## Task 2: `save_file` command (read-verify-write)

**Files:**
- Modify: `src-tauri/src/commands.rs` (add command + helper near `toggle_task`)
- Modify: `src-tauri/src/lib.rs` (register)

`save_file(path, contents, expected)`: when `expected` is `Some(s)`, the on-disk
content must equal `s` or the write is refused with `"file changed on disk"`
(the frontend then shows the conflict banner). `expected == None` forces the
write (the user's "Keep my version" choice). Writes go through the existing
`write_atomically`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src-tauri/src/commands.rs`:

```rust
    #[test]
    fn save_file_writes_when_expected_matches() {
        let dir = std::env::temp_dir().join(format!("mdv-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.md");
        std::fs::write(&f, b"old").unwrap();

        save_file_inner(&f, "new content", Some("old".to_string())).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "new content");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_file_refuses_when_disk_diverged() {
        let dir = std::env::temp_dir().join(format!("mdv-save2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.md");
        std::fs::write(&f, b"changed by someone else").unwrap();

        let err = save_file_inner(&f, "mine", Some("what I loaded".to_string())).unwrap_err();
        assert!(err.contains("changed on disk"), "got: {err}");
        // File is untouched.
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "changed by someone else");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_file_forces_when_expected_is_none() {
        let dir = std::env::temp_dir().join(format!("mdv-save3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.md");
        std::fs::write(&f, b"whatever").unwrap();

        save_file_inner(&f, "forced", None).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "forced");

        std::fs::remove_dir_all(&dir).unwrap();
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test save_file`
Expected: FAIL — `cannot find function save_file_inner`.

- [ ] **Step 3: Implement the command + testable inner fn**

Insert after `toggle_task` (after its closing `}` near line 438) in `src-tauri/src/commands.rs`:

```rust
/// Save an editor buffer to disk. `expected` is the content the editor believes
/// is currently on disk. `Some(_)` enforces read-verify-write: a divergence
/// means an external edit landed since load, and the write is refused so the
/// frontend can surface the conflict banner. `None` forces the write (the user
/// chose "Keep my version"). Writes atomically (temp-file + same-dir rename).
#[tauri::command]
pub fn save_file(path: String, contents: String, expected: Option<String>) -> Result<(), String> {
    save_file_inner(&PathBuf::from(&path), &contents, expected)
}

fn save_file_inner(path: &Path, contents: &str, expected: Option<String>) -> Result<(), String> {
    if let Some(expected) = expected {
        match std::fs::read_to_string(path) {
            Ok(disk) if disk != expected => return Err("file changed on disk".to_string()),
            Ok(_) => {}
            // A missing file is fine to (re)create — e.g. saving a brand-new file
            // whose on-disk bytes were just created empty.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("cannot read '{}': {e}", path.display())),
        }
    }
    write_atomically(path, contents.as_bytes())
        .map_err(|e| format!("cannot write '{}': {e}", path.display()))
}
```

- [ ] **Step 4: Register the command**

In `src-tauri/src/lib.rs` `generate_handler!`, after `commands::toggle_task,`:

```rust
            commands::save_file,
```

- [ ] **Step 5: Run to verify it passes**

Run: `cd src-tauri && cargo test save_file`
Expected: PASS (all three).

- [ ] **Step 6: Lint + commit**

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "Add save_file command with read-verify-write conflict guard"
```

---

## Task 3: `classifyFileChange` helper (the conflict policy, unit-tested)

**Files:**
- Create: `ui/editor.js`
- Create: `ui/editor.test.js`

This pure function encodes the whole external-change policy in one tested place.

- [ ] **Step 1: Write the failing test**

Create `ui/editor.test.js`:

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { classifyFileChange, isDirty } from "./editor.js";

test("not editing → always a plain reload", () => {
  assert.equal(
    classifyFileChange({ editing: false, dirty: false, diskContent: "x", savedContent: "y" }),
    "reload",
  );
});

test("editing, disk equals what we saved → our own write (ignore)", () => {
  assert.equal(
    classifyFileChange({ editing: true, dirty: true, diskContent: "v2", savedContent: "v2" }),
    "self",
  );
});

test("editing, external change, no unsaved edits → reload", () => {
  assert.equal(
    classifyFileChange({ editing: true, dirty: false, diskContent: "v2", savedContent: "v1" }),
    "reload",
  );
});

test("editing, external change, unsaved edits → conflict", () => {
  assert.equal(
    classifyFileChange({ editing: true, dirty: true, diskContent: "v2", savedContent: "v1" }),
    "conflict",
  );
});

test("isDirty compares buffer to saved", () => {
  assert.equal(isDirty("a", "a"), false);
  assert.equal(isDirty("a", "b"), true);
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `node --test ui/editor.test.js`
Expected: FAIL — cannot find module `./editor.js`.

- [ ] **Step 3: Implement `ui/editor.js`**

```js
// Pure helpers for the source editor. DOM-free so they unit-test under
// `node --test`; the CodeMirror wiring itself lives in app.js.

/** True when the editor buffer differs from the last loaded-or-saved content. */
export function isDirty(content, savedContent) {
  return content !== savedContent;
}

/** Decide what a `file-changed` event means for the active tab.
 *
 *  - "reload"   : adopt the disk content (not editing, or editing-but-clean).
 *  - "self"     : disk equals what we just saved — our own write; ignore.
 *  - "conflict" : disk diverged AND the editor has unsaved edits — warn, keep.
 */
export function classifyFileChange({ editing, dirty, diskContent, savedContent }) {
  if (!editing) return "reload";
  if (diskContent === savedContent) return "self";
  return dirty ? "conflict" : "reload";
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `node --test ui/editor.test.js`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add ui/editor.js ui/editor.test.js
git commit -m "Add editor.js conflict-classification helper with tests"
```

---

## Task 4: Vendor CodeMirror 5

**Files:**
- Create: `ui/codemirror/codemirror.min.js`, `ui/codemirror/codemirror.min.css`, `ui/codemirror/xml.min.js`, `ui/codemirror/markdown.min.js`

CodeMirror 5 ships UMD files that need no bundler. The markdown mode references
the `xml` mode for embedded HTML, so vendor both modes.

- [ ] **Step 1: Download the pinned files**

Run:

```bash
mkdir -p ui/codemirror
CM=https://cdnjs.cloudflare.com/ajax/libs/codemirror/5.65.16
curl -fsSL "$CM/codemirror.min.js"           -o ui/codemirror/codemirror.min.js
curl -fsSL "$CM/codemirror.min.css"          -o ui/codemirror/codemirror.min.css
curl -fsSL "$CM/mode/xml/xml.min.js"         -o ui/codemirror/xml.min.js
curl -fsSL "$CM/mode/markdown/markdown.min.js" -o ui/codemirror/markdown.min.js
```

- [ ] **Step 2: Verify the files are real (non-empty, look like JS/CSS)**

Run:

```bash
wc -c ui/codemirror/* && head -c 80 ui/codemirror/codemirror.min.js
```

Expected: each file is multiple KB; the JS head starts with a license comment / `(function`. If any file is tiny (a CDN error page), stop and retry.

- [ ] **Step 3: Commit**

```bash
git add ui/codemirror/
git commit -m "Vendor CodeMirror 5.65.16 (core + xml + markdown modes)"
```

---

## Task 5: Editor layout — index.html + CSS scaffolding

**Files:**
- Modify: `ui/index.html`
- Modify: `ui/styles.css`

Adds the (initially hidden) editor pane, the editor splitter, the Edit/Save
toolbar buttons, the conflict banner, and the CodeMirror scripts/CSS. No JS
behavior yet — this task just makes the DOM and styles exist.

- [ ] **Step 1: Add CodeMirror CSS to `<head>`**

In `ui/index.html`, after the `styles.css` link (line 9), add:

```html
    <link rel="stylesheet" href="codemirror/codemirror.min.css" />
```

- [ ] **Step 2: Add the Edit + Save toolbar buttons**

In `ui/index.html`, inside `<div class="toolbar">` (after the `toggle-raw` button, before `</div>` ~line 135), add:

```html
          <button
            id="save-file"
            class="toolbar-btn"
            type="button"
            title="Save (⌘S)"
            hidden
          >
            Save
          </button>
          <button
            id="toggle-edit"
            class="toolbar-btn"
            type="button"
            aria-pressed="false"
            title="Edit this document"
          >
            Edit
          </button>
```

- [ ] **Step 3: Wrap the preview in a flex pane-body with the editor pane + splitter**

In `ui/index.html`, replace the `preview-scroll` block (lines 137–142) with:

```html
      <div class="pane-body" id="pane-body">
        <div class="editor-pane" id="editor-pane" hidden></div>
        <div
          class="editor-splitter"
          id="editor-splitter"
          role="separator"
          aria-orientation="vertical"
          hidden
        ></div>
        <div class="preview-scroll" id="preview-scroll">
          <div class="preview-empty" id="preview-empty">
            Select a file from the tree to preview.
          </div>
          <article class="markdown-body" id="preview" hidden></article>
        </div>
      </div>
```

- [ ] **Step 4: Add the conflict banner** (just before the closing `</main>` ~line 202):

```html
      <div class="editor-conflict" id="editor-conflict" hidden role="alert">
        <span class="editor-conflict-text" id="editor-conflict-text">
          This file changed on disk.
        </span>
        <button id="editor-conflict-reload" class="update-banner-btn" type="button">
          Reload from disk
        </button>
        <button id="editor-conflict-keep" class="update-banner-btn primary" type="button">
          Keep my version
        </button>
      </div>
```

- [ ] **Step 5: Load the CodeMirror scripts** (classic, before `app.js`).

In `ui/index.html`, before `<script type="module" src="app.js"></script>` (line 246), add:

```html
    <!-- CodeMirror 5 (classic scripts, must run before the app.js module so
         window.CodeMirror + its modes exist at init). -->
    <script src="codemirror/codemirror.min.js"></script>
    <script src="codemirror/xml.min.js"></script>
    <script src="codemirror/markdown.min.js"></script>
```

- [ ] **Step 6: Add the CSS**

Append to `ui/styles.css`:

```css
/* ---- Source editor (split view) ---- */
.pane-body {
  display: flex;
  flex: 1;
  min-height: 0;
}
.preview-scroll {
  flex: 1;
  min-width: 0;
}
.editor-pane {
  width: var(--editor-width, 50%);
  min-width: 200px;
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--border, #d0d7de);
  overflow: hidden;
}
.editor-pane .CodeMirror {
  height: 100%;
  flex: 1;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 13px;
}
.editor-splitter {
  width: 5px;
  cursor: col-resize;
  background: var(--border, #d0d7de);
  flex: 0 0 auto;
}
.editor-splitter.dragging,
.editor-splitter:hover {
  background: var(--accent, #0969da);
}

/* Dirty (unsaved) indicator on a tab. */
.tab-dirty {
  margin-left: 4px;
  color: var(--fg-muted, #57606a);
}
.tab:hover .tab-dirty {
  display: none;
}
.tab .tab-dirty + .tab-close {
  display: none;
}
.tab:hover .tab-dirty + .tab-close {
  display: inline;
}

/* External-change conflict banner. */
.editor-conflict {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 12px;
  background: var(--banner-bg, #fff8c5);
  border-top: 1px solid var(--border, #d0d7de);
  font-size: 13px;
}
.editor-conflict-text {
  flex: 1;
}
[data-theme="dark"] .editor-conflict {
  background: #341a00;
}
```

- [ ] **Step 7: Build to verify the markup/CSS compile into the bundle**

Run: `cd src-tauri && cargo build`
Expected: builds with no errors (Tauri re-bundles `ui/`).

- [ ] **Step 8: Commit**

```bash
git add ui/index.html ui/styles.css
git commit -m "Add editor pane, splitter, Edit/Save buttons, conflict banner (markup + CSS)"
```

---

## Task 6: Tab edit-state fields + dirty dot in the tab bar

**Files:**
- Modify: `ui/app.js`

Adds `editing`, `dirty`, `savedContent` to every tab object and renders an
unsaved-dot. No editor yet — this isolates the model change.

- [ ] **Step 1: Update the tab-shape comment and all tab-creation sites**

In `ui/app.js`, change the model comment (line 85):

```js
const tabs = []; // [{ path, sticky, raw, editing, dirty, savedContent }]
```

Then add `editing: false, dirty: false, savedContent: null` to each `tabs.push({...})` / `tabs[previewIdx] = ...` site. The four sites:

`openPreview` (~line 684):
```js
  tabs.push({ path, sticky: false, raw: false, editing: false, dirty: false, savedContent: null });
```

`openSticky` (~line 695):
```js
  tabs.push({ path, sticky: true, raw: false, editing: false, dirty: false, savedContent: null });
```

`restoreSession` (~line 725):
```js
    tabs.push({ path: p, sticky: true, raw: false, editing: false, dirty: false, savedContent: null });
```

In `openPreview`, the reused-preview-tab branch (~line 678) also resets edit state — add after `tabs[previewIdx].raw = false;`:
```js
    tabs[previewIdx].editing = false;
    tabs[previewIdx].dirty = false;
    tabs[previewIdx].savedContent = null;
```

- [ ] **Step 2: Render the dirty dot in `makeTabEl`**

In `ui/app.js` `makeTabEl` (~line 800), after the `name` append and before the `close` span:

```js
  if (tab.dirty) {
    const dot = document.createElement("span");
    dot.className = "tab-dirty";
    dot.textContent = "●";
    dot.title = "Unsaved changes";
    el.appendChild(dot);
  }
```

- [ ] **Step 3: Verify the build**

Run: `cd src-tauri && cargo build`
Expected: builds clean.

- [ ] **Step 4: Commit**

```bash
git add ui/app.js
git commit -m "Add editing/dirty/savedContent to tab model and dirty dot"
```

---

## Task 7: Refactor — extract `paintHtml` from `renderActive`

**Files:**
- Modify: `ui/app.js`

So the editor's live preview and the disk renderer share one morphdom+postRender
path. Pure refactor; behavior must not change.

- [ ] **Step 1: Extract the helper**

In `ui/app.js`, replace the body of `renderActive` **from** the line
`previewEmpty.hidden = true;` (~line 914) **through** the end of the function
(the `if (findOpen()) runFind(...)` line ~966) with a call to a new helper, and
move that removed code into the helper. The result:

```js
async function renderActive({ scrollLock = true, forceMermaid = false } = {}) {
  const t = activeTab();
  if (!t) {
    showEmptyState();
    return;
  }
  if (isImagePath(t.path)) {
    renderImage(t, { scrollLock });
    return;
  }
  let result;
  try {
    result = await invoke("render_file", {
      path: t.path,
      theme: currentTheme,
      raw: t.raw,
    });
  } catch (e) {
    console.error("render_file failed", e);
    showError(String(e));
    return;
  }
  await paintHtml(t, result.html, result.raw, { scrollLock, forceMermaid });
}

/** Diff `html` into #preview and run the post-render pipeline. Shared by the
 *  disk renderer (renderActive) and the editor's live preview. */
async function paintHtml(t, html, raw, { scrollLock = true, forceMermaid = false } = {}) {
  previewEmpty.hidden = true;
  preview.hidden = false;
  preview.classList.toggle("raw-body", raw);

  const anchor = scrollLock ? captureAnchor() : null;

  const incoming = document.createElement("article");
  incoming.className = "markdown-body" + (raw ? " raw-body" : "");
  incoming.id = "preview";
  incoming.innerHTML = html;

  window.morphdom(preview, incoming, {
    onBeforeElUpdated: (fromEl, toEl) => {
      if (
        !forceMermaid &&
        fromEl.dataset.mvState &&
        fromEl.classList.contains("mermaid") &&
        fromEl.dataset.mermaidSrc === mermaidSource(toEl)
      ) {
        return false;
      }
      if (
        fromEl.dataset.mathState === "ok" &&
        toEl.hasAttribute &&
        toEl.hasAttribute("data-math-style") &&
        (toEl.textContent || "").trim() === fromEl.dataset.mathSrc
      ) {
        return false;
      }
      if (fromEl.tagName === "IMG" && toEl.tagName === "IMG") {
        const want = localImageUrl(parentDir(t.path), toEl.getAttribute("src"));
        if (want && fromEl.src === want) return false;
      }
      return !fromEl.isEqualNode(toEl);
    },
  });

  const hadPendingJump = t.pendingJumpLine != null;
  await postRender(t, { raw, forceMermaid });

  if (!hadPendingJump) {
    if (anchor) restoreAnchor(anchor);
    else previewScroll.scrollTop = 0;
  }

  if (findOpen()) runFind({ keepCurrent: true, scroll: false });
}
```

- [ ] **Step 2: Verify the build**

Run: `cd src-tauri && cargo build`
Expected: builds clean.

- [ ] **Step 3: Manual smoke (optional but recommended)**

Run: `cd src-tauri && cargo run -- ../README.md` — confirm the preview still renders, live reload and scroll anchoring still work, then quit. (No behavior should have changed.)

- [ ] **Step 4: Commit**

```bash
git add ui/app.js
git commit -m "Extract paintHtml from renderActive (no behavior change)"
```

---

## Task 8: Enter/exit edit mode + CodeMirror wiring + live preview

**Files:**
- Modify: `ui/app.js`

Wires the Edit button, instantiates CodeMirror in the editor pane, drives the
live preview from the buffer (debounced), and tracks dirty state.

- [ ] **Step 1: Import the editor helpers**

At the top of `ui/app.js`, after the `filetype.js` import (line 30):

```js
import { classifyFileChange, isDirty } from "./editor.js";
```

- [ ] **Step 2: Add editor module state + element refs**

After the existing element refs (~line 64, after `const splitter = ...`):

```js
const editBtn = document.getElementById("toggle-edit");
const saveBtn = document.getElementById("save-file");
const editorPane = document.getElementById("editor-pane");
const editorSplitter = document.getElementById("editor-splitter");
const paneBody = document.getElementById("pane-body");

let cm = null; // the single CodeMirror instance (created lazily, reused)
let previewDebounce = null;
const EDITOR_PREVIEW_DEBOUNCE_MS = 150;
```

- [ ] **Step 3: Wire the toolbar buttons in `init`**

In `init`, after `rawBtn.addEventListener("click", onToggleRaw);` (~line 273):

```js
  editBtn.addEventListener("click", onToggleEdit);
  saveBtn.addEventListener("click", () => saveActive());
```

- [ ] **Step 4: Add the edit-mode functions**

Add a new section (e.g. after `onToggleRaw`, ~line 843):

```js
/* ---- Source editor ---- */

function ensureCm() {
  if (cm) return cm;
  cm = window.CodeMirror(editorPane, {
    value: "",
    mode: "markdown",
    lineNumbers: true,
    lineWrapping: true,
    theme: "default",
  });
  cm.on("change", onEditorChange);
  // ⌘S / Ctrl-S from inside the editor.
  cm.setOption("extraKeys", {
    "Cmd-S": () => saveActive(),
    "Ctrl-S": () => saveActive(),
  });
  return cm;
}

async function onToggleEdit() {
  const t = activeTab();
  if (!t || isImagePath(t.path)) return;
  if (t.editing) {
    await exitEditMode(t);
  } else {
    await enterEditMode(t);
  }
}

async function enterEditMode(t) {
  let src;
  try {
    src = await invoke("read_source", { path: t.path });
  } catch (e) {
    showTransientError("Can't open this file for editing: " + e);
    return;
  }
  t.editing = true;
  t.raw = false;
  t.savedContent = src;
  t.dirty = false;
  ensureCm();
  cm.setValue(src);
  cm.clearHistory();
  showEditorChrome(true);
  renderTabBar();
  cm.refresh();
  cm.focus();
  await renderFromEditor(t, { scrollLock: false });
}

async function exitEditMode(t) {
  if (t.dirty) {
    const discard = await dialogApi.ask(
      `Discard unsaved changes to ${basename(t.path)}?`,
      { title: "MDViewer", kind: "warning" },
    );
    if (!discard) return;
  }
  t.editing = false;
  t.dirty = false;
  hideConflict();
  showEditorChrome(false);
  renderTabBar();
  await renderActive({ scrollLock: false });
}

function showEditorChrome(on) {
  editorPane.hidden = !on;
  editorSplitter.hidden = !on;
  paneBody.classList.toggle("editing", on);
  saveBtn.hidden = !on;
  editBtn.setAttribute("aria-pressed", on ? "true" : "false");
  editBtn.textContent = on ? "Done" : "Edit";
}

function onEditorChange() {
  const t = activeTab();
  if (!t || !t.editing) return;
  const dirty = isDirty(cm.getValue(), t.savedContent);
  if (dirty !== t.dirty) {
    t.dirty = dirty;
    renderTabBar();
  }
  if (previewDebounce) clearTimeout(previewDebounce);
  previewDebounce = setTimeout(() => {
    previewDebounce = null;
    renderFromEditor(t, { scrollLock: true }).catch((e) =>
      console.error("live preview failed", e),
    );
  }, EDITOR_PREVIEW_DEBOUNCE_MS);
}

/** Render the editor buffer (not disk) into the preview via render_preview. */
async function renderFromEditor(t, { scrollLock = true } = {}) {
  if (!cm) return;
  let html;
  try {
    html = await invoke("render_preview", {
      source: cm.getValue(),
      path: t.path,
      theme: currentTheme,
    });
  } catch (e) {
    console.error("render_preview failed", e);
    return;
  }
  await paintHtml(t, html, false, { scrollLock });
}
```

- [ ] **Step 5: Hide the Edit button for image tabs; reflect edit state in the toolbar**

In `renderTabBar` (~line 789), inside the `if (t)` block, after the `rawBtn.hidden = image;` line:

```js
    editBtn.hidden = image;
    if (!image) {
      editBtn.textContent = t.editing ? "Done" : "Edit";
      editBtn.setAttribute("aria-pressed", t.editing ? "true" : "false");
    }
    saveBtn.hidden = !t.editing;
    rawBtn.hidden = image || t.editing;
```

(Replace the existing `rawBtn.hidden = image;` line with the `rawBtn.hidden = image || t.editing;` above.)

- [ ] **Step 6: Add `saveActive` + the editor splitter drag** (place `saveActive` near the editor section; splitter drag near the existing sidebar splitter IIFE ~line 1994):

```js
async function saveActive() {
  const t = activeTab();
  if (!t || !t.editing || !cm) return;
  const content = cm.getValue();
  try {
    await invoke("save_file", {
      path: t.path,
      contents: content,
      expected: t.savedContent,
    });
    t.savedContent = content;
    t.dirty = false;
    hideConflict();
    renderTabBar();
  } catch (e) {
    if (String(e).includes("changed on disk")) {
      showConflict(t);
    } else {
      showTransientError("Save failed: " + e);
    }
  }
}
```

And the editor splitter drag (after the sidebar splitter IIFE):

```js
/* ---- Editor splitter ---- */
(() => {
  let dragging = false;
  editorSplitter.addEventListener("mousedown", (e) => {
    dragging = true;
    editorSplitter.classList.add("dragging");
    e.preventDefault();
  });
  window.addEventListener("mousemove", (e) => {
    if (!dragging) return;
    const rect = paneBody.getBoundingClientRect();
    const min = 200;
    const max = Math.max(min + 100, rect.width - 200);
    const w = Math.min(max, Math.max(min, e.clientX - rect.left));
    document.documentElement.style.setProperty("--editor-width", `${w}px`);
    if (cm) cm.refresh();
  });
  window.addEventListener("mouseup", () => {
    if (dragging) {
      dragging = false;
      editorSplitter.classList.remove("dragging");
    }
  });
})();
```

(`showConflict` / `hideConflict` are defined in Task 9; the build in this task's verify step will still compile because they're referenced inside functions, not at load time — but to avoid a `ReferenceError` at runtime before Task 9, add temporary no-op stubs now and replace them in Task 9:)

```js
function showConflict() {}
function hideConflict() {}
```

- [ ] **Step 7: Verify the build**

Run: `cd src-tauri && cargo build`
Expected: builds clean.

- [ ] **Step 8: Manual smoke**

Run: `cd src-tauri && cargo run -- ../README.md`. Click **Edit** → the split editor appears with the source; type a heading → preview updates after ~150 ms; the tab shows a ● dot; press ⌘S → dot clears and the file on disk changes; click **Done** → returns to full-width preview. Quit.

- [ ] **Step 9: Commit**

```bash
git add ui/app.js
git commit -m "Add split-view source editor (CodeMirror) with live preview and save"
```

---

## Task 9: Conflict banner + `file-changed` integration + dirty close guard

**Files:**
- Modify: `ui/app.js`

- [ ] **Step 1: Replace the temporary stubs with the real conflict banner**

Remove the `function showConflict() {}` / `function hideConflict() {}` stubs from Task 8 and add, in the editor section:

```js
const conflictBanner = document.getElementById("editor-conflict");
const conflictReload = document.getElementById("editor-conflict-reload");
const conflictKeep = document.getElementById("editor-conflict-keep");

function showConflict(t) {
  conflictReload.onclick = () => reloadFromDisk(t);
  conflictKeep.onclick = () => forceSave(t);
  conflictBanner.hidden = false;
}

function hideConflict() {
  conflictBanner.hidden = true;
}

async function reloadFromDisk(t) {
  let disk;
  try {
    disk = await invoke("read_source", { path: t.path });
  } catch (e) {
    showTransientError("Reload failed: " + e);
    return;
  }
  if (cm) cm.setValue(disk);
  t.savedContent = disk;
  t.dirty = false;
  hideConflict();
  renderTabBar();
  await renderFromEditor(t, { scrollLock: false });
}

async function forceSave(t) {
  if (!cm) return;
  const content = cm.getValue();
  try {
    await invoke("save_file", { path: t.path, contents: content, expected: null });
    t.savedContent = content;
    t.dirty = false;
    hideConflict();
    renderTabBar();
  } catch (e) {
    showTransientError("Save failed: " + e);
  }
}
```

- [ ] **Step 2: Route `file-changed` through the policy**

In `init`, replace the existing `file-changed` listener (~lines 216–225) with:

```js
  await listen("file-changed", async (ev) => {
    const tab = activeTab();
    if (tab && ev.payload === tab.path) {
      if (tab.editing) {
        await onEditingFileChanged(tab);
      } else {
        if (isImagePath(tab.path)) {
          imageVersions.set(tab.path, (imageVersions.get(tab.path) || 0) + 1);
        }
        await renderActive({ scrollLock: true });
      }
    }
    scheduleGitRefresh();
  });
```

And add the handler in the editor section:

```js
async function onEditingFileChanged(t) {
  let disk;
  try {
    disk = await invoke("read_source", { path: t.path });
  } catch (e) {
    // File may have been removed/renamed externally; leave the buffer intact.
    console.debug("editing file-changed read skipped:", e);
    return;
  }
  const cls = classifyFileChange({
    editing: t.editing,
    dirty: t.dirty,
    diskContent: disk,
    savedContent: t.savedContent,
  });
  if (cls === "self") return; // our own write; nothing to do
  if (cls === "reload") {
    await reloadFromDisk(t);
  } else {
    showConflict(t);
  }
}
```

- [ ] **Step 3: Dirty guard on close**

In `closeTab` (~line 760), at the very top of the function:

```js
function closeTab(idx) {
  if (idx < 0 || idx >= tabs.length) return;
  const t = tabs[idx];
  if (t.editing && t.dirty) {
    dialogApi
      .ask(`Discard unsaved changes to ${basename(t.path)}?`, {
        title: "MDViewer",
        kind: "warning",
      })
      .then((discard) => {
        if (discard) {
          t.dirty = false;
          closeTab(idx);
        }
      });
    return;
  }
  // ...existing body unchanged...
```

(Keep the rest of `closeTab` as-is below this guard.)

- [ ] **Step 4: Hide the conflict banner when switching tabs**

In `setActiveTab` (~line 733), after `activeIdx = idx;`:

```js
  if (typeof hideConflict === "function") hideConflict();
```

- [ ] **Step 5: Verify the build**

Run: `cd src-tauri && cargo build`
Expected: builds clean.

- [ ] **Step 6: Manual smoke (conflict path)**

Run `cargo run -- ../README.md`, click **Edit**, type an unsaved change, then in another terminal `echo "external" >> README.md`. The conflict banner appears; **Keep my version** overwrites (your text wins); repeat and use **Reload from disk** to adopt the external content. (Restore `README.md` with `git checkout README.md` afterward.)

- [ ] **Step 7: Commit**

```bash
git add ui/app.js
git commit -m "Add editor conflict banner, file-changed policy, dirty close guard"
```

---

## Task 10: Menu items — Actions ▸ Toggle Edit / Save

**Files:**
- Modify: `src-tauri/src/menu.rs`
- Modify: `ui/app.js`

- [ ] **Step 1: Add the menu items + emit ids**

In `src-tauri/src/menu.rs`, add the item builders near `edit_toggle_raw` (~line 138):

```rust
    let edit_toggle_edit =
        MenuItemBuilder::with_id("edit-toggle-edit", "Toggle Edit").build(app)?;
    let edit_save = MenuItemBuilder::with_id("edit-save", "Save")
        .accelerator("CmdOrCtrl+S")
        .build(app)?;
```

Add them to the `Actions` submenu (~line 143), after `.item(&edit_toggle_raw)`:

```rust
        .separator()
        .item(&edit_toggle_edit)
        .item(&edit_save)
```

Add the event arms near the other `edit-*` arms (~line 50):

```rust
            "edit-toggle-edit" => {
                let _ = app.emit("edit-action", "toggle-edit");
            }
            "edit-save" => {
                let _ = app.emit("edit-action", "save");
            }
```

- [ ] **Step 2: Handle the new actions in `runEditAction`**

In `ui/app.js` `runEditAction` (~line 1787), add cases (image-guard `toggle-edit`/`save` like `toggle-raw`):

```js
    case "toggle-edit":
      await onToggleEdit();
      break;
    case "save":
      await saveActive();
      break;
```

And extend the image-guard at the top of `runEditAction` to include the new actions:

```js
  if (
    t &&
    isImagePath(t.path) &&
    (name === "copy-source" || name === "toggle-raw" || name === "toggle-edit")
  ) {
    showTransientError("Not available for images.");
    return;
  }
```

- [ ] **Step 3: Verify the build**

Run: `cd src-tauri && cargo build`
Expected: builds clean.

- [ ] **Step 4: Lint + commit**

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings
git add src-tauri/src/menu.rs ui/app.js
git commit -m "Add Actions menu items for Toggle Edit and Save (⌘S)"
```

**Phase 1 complete — a working split-view editor.** Consider a checkpoint review (superpowers:requesting-code-review) before Phase 2.

---

# PHASE 2 — FILE OPERATIONS

## Task 11: `fs_ops.rs` pure helpers — name validation + duplicate naming

**Files:**
- Create: `src-tauri/src/fs_ops.rs`
- Modify: `src-tauri/src/lib.rs` (`mod fs_ops;`)

- [ ] **Step 1: Create the module with failing tests**

Create `src-tauri/src/fs_ops.rs`:

```rust
use std::path::{Path, PathBuf};

/// Validate a user-entered file or folder name (from the inline-rename input).
/// Rejects empties, path separators, and the `.`/`..` traversal names. The
/// frontend validates too (immediate feedback); this is the authoritative
/// backend guard.
pub fn validate_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if trimmed == "." || trimmed == ".." {
        return Err("invalid name".to_string());
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err("name cannot contain path separators".to_string());
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err("name cannot contain control characters".to_string());
    }
    Ok(())
}

/// Split a file name into (stem, extension). A leading-dot name with no other
/// dot (".gitignore") is all stem, no extension. "a.tar.gz" → ("a.tar", "gz").
fn split_ext(name: &str) -> (&str, Option<&str>) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], Some(&name[i + 1..])),
        _ => (name, None),
    }
}

/// The nth duplicate candidate name: n=1 → "note copy.md", n=2 → "note copy 2.md".
pub fn duplicate_candidate(name: &str, n: usize) -> String {
    let (stem, ext) = split_ext(name);
    let suffix = if n <= 1 {
        " copy".to_string()
    } else {
        format!(" copy {n}")
    };
    match ext {
        Some(e) => format!("{stem}{suffix}.{e}"),
        None => format!("{stem}{suffix}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_name_accepts_ordinary_names() {
        assert!(validate_name("notes.md").is_ok());
        assert!(validate_name(".gitignore").is_ok());
        assert!(validate_name("My File 2.txt").is_ok());
    }

    #[test]
    fn validate_name_rejects_bad_names() {
        assert!(validate_name("").is_err());
        assert!(validate_name("   ").is_err());
        assert!(validate_name(".").is_err());
        assert!(validate_name("..").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("a\\b").is_err());
        assert!(validate_name("a\nb").is_err());
    }

    #[test]
    fn duplicate_candidate_handles_extensions_and_dotfiles() {
        assert_eq!(duplicate_candidate("note.md", 1), "note copy.md");
        assert_eq!(duplicate_candidate("note.md", 2), "note copy 2.md");
        assert_eq!(duplicate_candidate("archive.tar.gz", 1), "archive.tar copy.gz");
        assert_eq!(duplicate_candidate("README", 1), "README copy");
        assert_eq!(duplicate_candidate(".gitignore", 1), ".gitignore copy");
    }
}
```

- [ ] **Step 2: Register the module**

In `src-tauri/src/lib.rs`, add to the `mod` list (after `mod export;` ~line 2, keep alphabetical-ish ordering):

```rust
mod fs_ops;
```

- [ ] **Step 3: Run the tests**

Run: `cd src-tauri && cargo test fs_ops`
Expected: PASS (3 tests). (You may see an `unused function` warning for the not-yet-used IO functions — that's fixed in Task 12.)

- [ ] **Step 4: Lint + commit**

```bash
cd src-tauri && cargo fmt && cargo test fs_ops
git add src-tauri/src/fs_ops.rs src-tauri/src/lib.rs
git commit -m "Add fs_ops name validation and duplicate-name helpers"
```

---

## Task 12: `fs_ops.rs` IO helpers — containment + create/rename/duplicate

**Files:**
- Modify: `src-tauri/src/fs_ops.rs`

- [ ] **Step 1: Add failing tests**

Append to the `tests` module in `src-tauri/src/fs_ops.rs`:

```rust
    fn tmp(label: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mdv-fsops-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn within_root_accepts_inside_and_rejects_outside() {
        let root = tmp("within");
        let inside = root.join("sub");
        std::fs::create_dir_all(&inside).unwrap();
        let new_child = inside.join("new.md"); // doesn't exist yet
        assert!(within_root(&new_child, &root)); // nearest existing ancestor is inside
        assert!(within_root(&inside, &root));
        assert!(!within_root(Path::new("/etc/passwd"), &root));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn create_file_rejects_existing() {
        let root = tmp("createfile");
        let p = create_file(&root, "a.md").unwrap();
        assert!(p.is_file());
        assert!(create_file(&root, "a.md").is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn create_folder_rejects_existing() {
        let root = tmp("createdir");
        let p = create_folder(&root, "sub").unwrap();
        assert!(p.is_dir());
        assert!(create_folder(&root, "sub").is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rename_rejects_existing_destination() {
        let root = tmp("rename");
        let a = create_file(&root, "a.md").unwrap();
        let _b = create_file(&root, "b.md").unwrap();
        assert!(rename_path(&a, &root.join("b.md")).is_err());
        assert!(rename_path(&a, &root.join("c.md")).is_ok());
        assert!(root.join("c.md").is_file());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn duplicate_picks_a_free_name() {
        let root = tmp("dup");
        let a = create_file(&root, "a.md").unwrap();
        std::fs::write(&a, b"hello").unwrap();
        let d1 = duplicate_file(&a).unwrap();
        assert_eq!(d1.file_name().unwrap(), "a copy.md");
        assert_eq!(std::fs::read_to_string(&d1).unwrap(), "hello");
        let d2 = duplicate_file(&a).unwrap();
        assert_eq!(d2.file_name().unwrap(), "a copy 2.md");
        std::fs::remove_dir_all(&root).unwrap();
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test fs_ops`
Expected: FAIL — `within_root` / `create_file` / `create_folder` / `rename_path` / `duplicate_file` not found.

- [ ] **Step 3: Implement the IO helpers**

Insert into `src-tauri/src/fs_ops.rs` (after `duplicate_candidate`, before `#[cfg(test)]`):

```rust
/// The nearest ancestor of `path` (including itself) that exists on disk.
fn nearest_existing(path: &Path) -> Option<PathBuf> {
    let mut p: &Path = path;
    loop {
        if p.exists() {
            return Some(p.to_path_buf());
        }
        p = p.parent()?;
    }
}

/// Whether `path` (or, for a not-yet-created path, its nearest existing
/// ancestor) resolves inside `root`. Canonicalizing resolves symlinks, so an
/// in-tree symlink pointing outside is rejected. `starts_with` is component-wise
/// (so `/work` never matches `/work-x`).
pub fn within_root(path: &Path, root: &Path) -> bool {
    match (
        nearest_existing(path).and_then(|p| std::fs::canonicalize(p).ok()),
        std::fs::canonicalize(root).ok(),
    ) {
        (Some(p), Some(r)) => p.starts_with(&r),
        _ => false,
    }
}

/// Create an empty file `dir/name`. Fails if it already exists (atomic via
/// create_new). `name` must already be validated by the caller.
pub fn create_file(dir: &Path, name: &str) -> Result<PathBuf, String> {
    validate_name(name)?;
    let target = dir.join(name);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|e| format!("cannot create '{}': {e}", target.display()))?;
    Ok(target)
}

/// Create a folder `dir/name`. Fails if it already exists.
pub fn create_folder(dir: &Path, name: &str) -> Result<PathBuf, String> {
    validate_name(name)?;
    let target = dir.join(name);
    if target.exists() {
        return Err(format!("'{}' already exists", target.display()));
    }
    std::fs::create_dir(&target)
        .map_err(|e| format!("cannot create folder '{}': {e}", target.display()))?;
    Ok(target)
}

/// Rename `from` → `to`. Refuses to overwrite an existing destination.
pub fn rename_path(from: &Path, to: &Path) -> Result<(), String> {
    if to.exists() {
        return Err(format!("'{}' already exists", to.display()));
    }
    std::fs::rename(from, to)
        .map_err(|e| format!("cannot rename '{}': {e}", from.display()))
}

/// Copy `path` to the first free "name copy"/"name copy N" sibling.
pub fn duplicate_file(path: &Path) -> Result<PathBuf, String> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "invalid file name".to_string())?;
    for n in 1..=10_000 {
        let candidate = dir.join(duplicate_candidate(name, n));
        if !candidate.exists() {
            std::fs::copy(path, &candidate)
                .map_err(|e| format!("cannot duplicate '{}': {e}", path.display()))?;
            return Ok(candidate);
        }
    }
    Err("too many copies".to_string())
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd src-tauri && cargo test fs_ops`
Expected: PASS (8 tests total).

- [ ] **Step 5: Lint + commit**

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
git add src-tauri/src/fs_ops.rs
git commit -m "Add fs_ops containment guard and create/rename/duplicate IO"
```

---

## Task 13: `current_root` in AppState; command wrappers (create/rename/duplicate)

**Files:**
- Modify: `src-tauri/src/lib.rs` (AppState field + init + register)
- Modify: `src-tauri/src/commands.rs` (extend `remember_folder`; add command wrappers)

- [ ] **Step 1: Add the `current_root` field**

In `src-tauri/src/lib.rs` `AppState` (~line 30), add:

```rust
    /// The folder the sidebar is currently showing. File operations are confined
    /// within it. Set by the frontend on every sidebar-root change.
    pub current_root: Mutex<Option<PathBuf>>,
```

And initialize it in `run` (~line 42), seeding from the startup root:

```rust
        current_root: Mutex::new(startup.tree_root.clone()),
```

(Place this before `watcher: ...`. Note `startup.tree_root` is moved into `tree_root` below — so set `current_root` first with `.clone()`, then keep `tree_root: startup.tree_root,` as-is.)

- [ ] **Step 2: Extend `remember_folder` to update `current_root`**

In `src-tauri/src/commands.rs`, replace `remember_folder` (~line 478) with:

```rust
#[tauri::command]
pub fn remember_folder(app: AppHandle, state: State<'_, AppState>, path: String) {
    let p = PathBuf::from(path);
    if p.is_dir() {
        recent::save_last(&app, &p);
        if let Ok(mut slot) = state.current_root.lock() {
            *slot = Some(p);
        }
    }
}
```

- [ ] **Step 3: Add a shared root-guard helper + the three command wrappers**

In `src-tauri/src/commands.rs`, add near the other commands (e.g. after `search_in_folder`):

```rust
/// The current sidebar root, or an error if no folder is open. File-op commands
/// confine their targets within it.
fn current_root(state: &State<'_, AppState>) -> Result<PathBuf, String> {
    state
        .current_root
        .lock()
        .map_err(|_| "current_root mutex poisoned".to_string())?
        .clone()
        .ok_or_else(|| "no folder is open".to_string())
}

#[tauri::command]
pub fn create_file(state: State<'_, AppState>, dir: String, name: String) -> Result<String, String> {
    let root = current_root(&state)?;
    let dir = PathBuf::from(dir);
    if !fs_ops::within_root(&dir, &root) {
        return Err("target is outside the open folder".to_string());
    }
    let p = fs_ops::create_file(&dir, &name)?;
    Ok(p.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn create_folder(
    state: State<'_, AppState>,
    dir: String,
    name: String,
) -> Result<String, String> {
    let root = current_root(&state)?;
    let dir = PathBuf::from(dir);
    if !fs_ops::within_root(&dir, &root) {
        return Err("target is outside the open folder".to_string());
    }
    let p = fs_ops::create_folder(&dir, &name)?;
    Ok(p.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn rename_path(state: State<'_, AppState>, from: String, to: String) -> Result<(), String> {
    let root = current_root(&state)?;
    let from = PathBuf::from(from);
    let to = PathBuf::from(to);
    if let Some(name) = to.file_name().and_then(|s| s.to_str()) {
        fs_ops::validate_name(name)?;
    } else {
        return Err("invalid destination name".to_string());
    }
    if !fs_ops::within_root(&from, &root) || !fs_ops::within_root(&to, &root) {
        return Err("target is outside the open folder".to_string());
    }
    fs_ops::rename_path(&from, &to)
}

#[tauri::command]
pub fn duplicate_file(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let root = current_root(&state)?;
    let path = PathBuf::from(path);
    if !fs_ops::within_root(&path, &root) {
        return Err("target is outside the open folder".to_string());
    }
    let p = fs_ops::duplicate_file(&path)?;
    Ok(p.to_string_lossy().into_owned())
}
```

Add `fs_ops` to the `use crate::{...}` import at the top of `commands.rs` (~line 6):

```rust
use crate::{fs_ops, git, markdown, recent, search, tasklist, tree, AppState};
```

- [ ] **Step 4: Register the commands**

In `src-tauri/src/lib.rs` `generate_handler!`, after `commands::remember_folder,`:

```rust
            commands::create_file,
            commands::create_folder,
            commands::rename_path,
            commands::duplicate_file,
```

- [ ] **Step 5: Build (verifies the `remember_folder` signature change compiles with its existing callers)**

Run: `cd src-tauri && cargo build`
Expected: builds clean. (Tauri's `generate_handler!` adapts to the added `State` arg automatically — no frontend call change needed for `remember_folder`.)

- [ ] **Step 6: Lint + commit**

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "Track current_root in AppState; add file-op command wrappers"
```

---

## Task 14: `delete_to_trash` command (the `trash` crate)

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add the dependency**

In `src-tauri/Cargo.toml`, under `[dependencies]` (after `opener = "0.7"` ~line 45):

```toml
# Cross-platform move-to-Trash / Recycle Bin for tree Delete.
trash = "5"
```

- [ ] **Step 2: Add the command**

In `src-tauri/src/commands.rs`, after `duplicate_file`:

```rust
#[tauri::command]
pub fn delete_to_trash(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let root = current_root(&state)?;
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(format!("not found: {path}"));
    }
    if !fs_ops::within_root(&p, &root) {
        return Err("target is outside the open folder".to_string());
    }
    trash::delete(&p).map_err(|e| format!("cannot move to Trash: {e}"))
}
```

- [ ] **Step 3: Register the command**

In `src-tauri/src/lib.rs` `generate_handler!`, after `commands::duplicate_file,`:

```rust
            commands::delete_to_trash,
```

- [ ] **Step 4: Build**

Run: `cd src-tauri && cargo build`
Expected: builds clean (downloads + compiles `trash`).

- [ ] **Step 5: Lint + commit**

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "Add delete_to_trash command using the trash crate"
```

---

## Task 15: `validateName` JS helper (mirrors Rust, for inline-rename feedback)

**Files:**
- Create: `ui/treeops.js`
- Create: `ui/treeops.test.js`

- [ ] **Step 1: Write the failing test**

Create `ui/treeops.test.js`:

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { validateName } from "./treeops.js";

test("validateName accepts ordinary names", () => {
  assert.equal(validateName("notes.md"), null);
  assert.equal(validateName(".gitignore"), null);
  assert.equal(validateName("My File 2.txt"), null);
});

test("validateName returns an error string for bad names", () => {
  assert.match(validateName(""), /empty/);
  assert.match(validateName("   "), /empty/);
  assert.match(validateName("."), /invalid/);
  assert.match(validateName(".."), /invalid/);
  assert.match(validateName("a/b"), /separator/);
  assert.match(validateName("a\\b"), /separator/);
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `node --test ui/treeops.test.js`
Expected: FAIL — cannot find module `./treeops.js`.

- [ ] **Step 3: Implement `ui/treeops.js`**

```js
// Pure helpers for tree file operations. Mirrors fs_ops.rs::validate_name so the
// inline-rename input gives immediate feedback; the backend re-validates.

/** Returns null when valid, or an error message string when not. */
export function validateName(name) {
  const trimmed = (name || "").trim();
  if (trimmed === "") return "Name cannot be empty";
  if (trimmed === "." || trimmed === "..") return "Invalid name";
  if (trimmed.includes("/") || trimmed.includes("\\")) {
    return "Name cannot contain path separators";
  }
  // eslint-disable-next-line no-control-regex
  if (/[ -]/.test(trimmed)) return "Name cannot contain control characters";
  return null;
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `node --test ui/treeops.test.js`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add ui/treeops.js ui/treeops.test.js
git commit -m "Add treeops.validateName helper mirroring fs_ops validation"
```

---

## Task 16: Inline rename / new-file / new-folder widget

**Files:**
- Modify: `ui/app.js`

Adds a VS Code–style in-place editable row, used by Rename and by New File / New
Folder (on a freshly-inserted placeholder row).

- [ ] **Step 1: Import `validateName`**

At the top of `ui/app.js`, after the `editor.js` import:

```js
import { validateName } from "./treeops.js";
```

- [ ] **Step 2: Add the inline-edit primitives**

Add a new section in `ui/app.js` (e.g. after the tree functions, before `/* ---- Tabs ---- */` ~line 668):

```js
/* ---- Tree file operations ---- */

/** Replace a tree row's name span with an <input> for editing. Resolves to the
 *  committed (validated) name, or null on cancel. Does not touch disk. */
function promptInlineName(row, initial) {
  return new Promise((resolve) => {
    const nameEl = row.querySelector(":scope > .name");
    const input = document.createElement("input");
    input.className = "tree-rename-input";
    input.type = "text";
    input.value = initial;
    input.spellcheck = false;
    if (nameEl) nameEl.replaceWith(input);
    else row.appendChild(input);
    input.focus();
    // Select the stem (before the extension) like VS Code.
    const dot = initial.lastIndexOf(".");
    input.setSelectionRange(0, dot > 0 ? dot : initial.length);

    let settled = false;
    const restore = () => {
      const span = document.createElement("span");
      span.className = "name";
      span.textContent = initial;
      input.replaceWith(span);
    };
    const commit = () => {
      if (settled) return;
      const value = input.value;
      const err = validateName(value);
      if (err) {
        input.classList.add("invalid");
        input.title = err;
        return; // keep editing
      }
      settled = true;
      resolve(value.trim());
    };
    const cancel = () => {
      if (settled) return;
      settled = true;
      restore();
      resolve(null);
    };
    input.addEventListener("keydown", (ev) => {
      ev.stopPropagation();
      if (ev.key === "Enter") {
        ev.preventDefault();
        commit();
      } else if (ev.key === "Escape") {
        ev.preventDefault();
        cancel();
      }
    });
    input.addEventListener("blur", cancel);
  });
}

async function renameTreeEntry(li) {
  const from = li.dataset.path;
  const row = li.querySelector(":scope > .row");
  if (!row) return;
  const newName = await promptInlineName(row, basename(from));
  if (newName == null || newName === basename(from)) {
    await refreshTree();
    return;
  }
  const to = parentDir(from) + "/" + newName;
  try {
    await invoke("rename_path", { from, to });
  } catch (e) {
    showTransientError("Rename failed: " + e);
    await refreshTree();
    return;
  }
  retargetTabsForRename(from, to);
  await refreshTree();
}

/** Insert a placeholder row into `container` and inline-edit its name to create
 *  a new file or folder via the backend. */
async function createTreeEntry(container, dir, depth, isDir) {
  const li = document.createElement("li");
  li.dataset.isDir = isDir ? "1" : "0";
  li.dataset.depth = String(depth);
  const row = document.createElement("div");
  row.className = "row " + (isDir ? "dir" : "file");
  row.style.setProperty("--row-indent", `${depth * 12 + 4}px`);
  const icon = document.createElement("span");
  icon.className = "icon";
  icon.textContent = isDir ? "\u{1F4C1}" : "·";
  row.appendChild(icon);
  const name = document.createElement("span");
  name.className = "name";
  row.appendChild(name);
  li.appendChild(row);
  container.prepend(li);

  const newName = await promptInlineName(row, isDir ? "untitled" : "untitled.md");
  if (newName == null) {
    li.remove();
    return;
  }
  try {
    const cmd = isDir ? "create_folder" : "create_file";
    const created = await invoke(cmd, { dir, name: newName });
    await refreshTree();
    if (!isDir) await openSticky(created);
    if (!isDir) {
      const t = activeTab();
      if (t && t.path === created) await enterEditMode(t);
    }
  } catch (e) {
    showTransientError("Create failed: " + e);
    li.remove();
    await refreshTree();
  }
}

/** A folder target's directory: the folder itself if `li` is a dir, else its
 *  parent. Used to decide where New File / New Folder land. */
function dirForNewEntry(li) {
  return li.dataset.isDir === "1" ? li.dataset.path : parentDir(li.dataset.path);
}

async function duplicateTreeEntry(li) {
  try {
    const created = await invoke("duplicate_file", { path: li.dataset.path });
    await refreshTree();
    await openPreview(created);
  } catch (e) {
    showTransientError("Duplicate failed: " + e);
  }
}

async function deleteTreeEntry(li) {
  const path = li.dataset.path;
  const ok = await dialogApi.ask(`Move "${basename(path)}" to Trash?`, {
    title: "MDViewer",
    kind: "warning",
  });
  if (!ok) return;
  try {
    await invoke("delete_to_trash", { path });
  } catch (e) {
    showTransientError("Delete failed: " + e);
    return;
  }
  closeTabsUnder(path);
  await refreshTree();
}
```

- [ ] **Step 3: Add the CSS for the rename input**

Append to `ui/styles.css`:

```css
.tree-rename-input {
  flex: 1;
  min-width: 0;
  font: inherit;
  padding: 0 2px;
  border: 1px solid var(--accent, #0969da);
  border-radius: 3px;
  background: var(--bg, #fff);
  color: inherit;
}
.tree-rename-input.invalid {
  border-color: #cf222e;
}
```

- [ ] **Step 4: Build**

Run: `cd src-tauri && cargo build`
Expected: builds clean. (`retargetTabsForRename` and `closeTabsUnder` are added in Task 17; this task only references them inside functions, so the bundle compiles. Do not exercise Rename/Delete in the app until Task 17 lands.)

- [ ] **Step 5: Commit**

```bash
git add ui/app.js ui/styles.css
git commit -m "Add inline rename / new-file / new-folder / duplicate / delete tree ops"
```

---

## Task 17: Open-tabs-follow-rename + close-tabs-on-delete

**Files:**
- Modify: `ui/app.js`

- [ ] **Step 1: Add the tab-retargeting helpers**

Add to the "Tree file operations" section in `ui/app.js`:

```js
/** After a rename, rewrite any open tab whose path is the renamed entry or
 *  nested under it (folder rename), and rewire the active tab's watcher. */
function retargetTabsForRename(from, to) {
  let activeChanged = false;
  for (let i = 0; i < tabs.length; i++) {
    const p = tabs[i].path;
    if (p === from) {
      tabs[i].path = to;
      if (i === activeIdx) activeChanged = true;
    } else if (p.startsWith(from + "/")) {
      tabs[i].path = to + p.slice(from.length);
      if (i === activeIdx) activeChanged = true;
    }
  }
  renderTabBar();
  if (activeChanged && activeIdx >= 0) {
    invoke("open_file", { path: tabs[activeIdx].path }).catch((e) =>
      console.warn("rewire watcher after rename failed", e),
    );
  }
}

/** Close any tab pointing at `path` or nested under it (folder delete). */
function closeTabsUnder(path) {
  for (let i = tabs.length - 1; i >= 0; i--) {
    const p = tabs[i].path;
    if (p === path || p.startsWith(path + "/")) {
      tabs[i].dirty = false; // deleted on disk — don't prompt to save
      closeTab(i);
    }
  }
}
```

- [ ] **Step 2: Build**

Run: `cd src-tauri && cargo build`
Expected: builds clean.

- [ ] **Step 3: Commit**

```bash
git add ui/app.js
git commit -m "Retarget open tabs on rename; close tabs on delete"
```

---

## Task 18: Wire the operations into the tree context menu

**Files:**
- Modify: `ui/app.js`

- [ ] **Step 1: Extend the tree-row context menu**

In `ui/app.js`, in the `contextmenu` handler's tree-row branch (~lines 1902–1923), replace the block that builds `items` for a tree row with:

```js
  if (treeRow && tree.contains(treeRow)) {
    const absolute = treeRow.dataset.path;
    const isDir = treeRow.dataset.isDir === "1";
    const relative = relativeToRoot(absolute, treeRoot);
    const dir = dirForNewEntry(treeRow);
    const container = isDir
      ? treeRow.querySelector(":scope > ul") || tree
      : treeRow.parentElement || tree;
    const depth = depthOf(treeRow) + (isDir ? 1 : 0);

    if (isDir) {
      items.push({
        label: "Search in Folder…",
        action: () => enterSearchMode(absolute, { treeRoot }),
      });
      items.push("---");
    }
    items.push({
      label: "New File…",
      action: async () => {
        if (isDir) await ensureExpanded(treeRow);
        await createTreeEntry(newEntryContainer(treeRow, isDir), dir, depth, false);
      },
    });
    items.push({
      label: "New Folder…",
      action: async () => {
        if (isDir) await ensureExpanded(treeRow);
        await createTreeEntry(newEntryContainer(treeRow, isDir), dir, depth, true);
      },
    });
    items.push("---");
    items.push({ label: "Rename…", action: () => renameTreeEntry(treeRow) });
    if (!isDir) {
      items.push({ label: "Duplicate", action: () => duplicateTreeEntry(treeRow) });
    }
    items.push({ label: "Delete", action: () => deleteTreeEntry(treeRow) });
    items.push("---");
    items.push({
      label: "Copy Relative Path",
      action: () => copyText(relative),
      disabled: !relative,
    });
    items.push({ label: "Copy Absolute Path", action: () => copyText(absolute) });
    buildContextMenu(items, ev.clientX, ev.clientY);
    return;
  }
```

- [ ] **Step 2: Add the small expand/container helpers**

In the "Tree file operations" section:

```js
/** Ensure a directory row is expanded so a new child is visible after creation. */
async function ensureExpanded(li) {
  const row = li.querySelector(":scope > .row");
  if (row && !row.classList.contains("open")) {
    const entry = { path: li.dataset.path, is_dir: true, name: basename(li.dataset.path) };
    await onDirClick(entry, li, row, depthOf(li));
  }
}

/** The <ul> a new entry should be inserted into for `li`: the folder's own list
 *  (creating it if missing) when `li` is a dir, else `li`'s parent list. */
function newEntryContainer(li, isDir) {
  if (isDir) {
    let ul = li.querySelector(":scope > ul");
    if (!ul) {
      ul = document.createElement("ul");
      li.appendChild(ul);
    }
    return ul;
  }
  return li.parentElement || tree;
}
```

- [ ] **Step 3: Add New File / New Folder to the sidebar-background menu**

In the sidebar-background branch (~lines 1928–1937), before the `Search in Folder…` push, add (so a right-click on empty sidebar space creates at the root):

```js
  if (sidebar && treeRoot) {
    items.push({
      label: "New File…",
      action: () => createTreeEntry(tree, treeRoot, 1, false),
    });
    items.push({
      label: "New Folder…",
      action: () => createTreeEntry(tree, treeRoot, 1, true),
    });
    items.push("---");
    items.push({
      label: "Search in Folder…",
      action: () => enterSearchMode(treeRoot, { treeRoot }),
    });
    buildContextMenu(items, ev.clientX, ev.clientY);
    return;
  }
```

- [ ] **Step 4: Build**

Run: `cd src-tauri && cargo build`
Expected: builds clean.

- [ ] **Step 5: Manual smoke (the whole Phase 2 flow)**

Run `cargo run -- ..` (open the repo root). Right-click a folder → **New File…**, type `scratch.md`, Enter → it's created, opens in the editor. Type content, ⌘S. Right-click it → **Duplicate** → `scratch copy.md` appears. **Rename…** it → the open tab's title follows. **Delete** → confirm → it goes to Trash (check `~/.Trash`) and its tab closes. Right-click empty sidebar space → **New Folder…** works at the root. Quit.

- [ ] **Step 6: Run all tests + commit**

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
cd .. && node --test ui/*.test.js
git add ui/app.js
git commit -m "Wire create/rename/duplicate/delete into the tree context menu"
```

**Phase 2 complete.**

---

## Task 19: Documentation

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update `README.md`**

Add to the Features list that mdviewer can now **edit** text/markdown files (split-view source editor with live preview, ⌘S to save) and **create / rename / duplicate / delete** files and folders from the tree (Delete moves to Trash). Update Usage and the Menus section (Actions ▸ Toggle Edit / Save; tree right-click operations).

- [ ] **Step 2: Update `CLAUDE.md`**

In the architecture quick-tour, document: the split-view editor (CodeMirror vendored under `ui/codemirror/`, `render_preview` renders the buffer, `save_file` read-verify-write, `classifyFileChange` policy in `ui/editor.js`, the editor↔watcher self-write distinction); the tab edit-state fields; `fs_ops.rs` + the five file-op commands; `AppState.current_root` set via `remember_folder`; `trash` dependency; `ui/treeops.js`. Add to the file-layout block (`fs_ops.rs`, `editor.js`, `treeops.js`, `codemirror/`).

- [ ] **Step 3: Commit**

```bash
git add README.md CLAUDE.md
git commit -m "Document in-app editing and file operations"
```

---

## Self-Review notes (for the implementer)

- **Spec coverage:** Editor (Tasks 1–10): split view ✓, CodeMirror ✓, live preview via `render_preview` ✓, explicit ⌘S via `save_file` ✓, conflict warn-and-keep ✓ (`classifyFileChange` + banner), clean-editor auto-reload ✓ (the `"reload"` branch), dirty dot + close guard ✓. File ops (Tasks 11–18): New File/Folder ✓, Rename (tabs follow) ✓, Duplicate ✓, Delete-to-Trash with confirm ✓, inline rename ✓, containment boundary ✓ (`within_root` + `current_root`). Tests: Rust `fs_ops`/`save_file`/`render_preview` ✓, JS `classifyFileChange`/`validateName` ✓.
- **Deviation from spec's test list:** the unique-"copy"-name logic lives in Rust (`duplicate_candidate`, where the operation runs) and is tested there, not in JS — the frontend never derives copy names. Noted intentionally.
- **Cross-task type consistency:** tab fields `editing/dirty/savedContent`; command names `render_preview`, `save_file`, `create_file`, `create_folder`, `rename_path`, `duplicate_file`, `delete_to_trash`; JS helpers `classifyFileChange`, `isDirty`, `validateName`; app.js functions `paintHtml`, `renderFromEditor`, `enterEditMode`, `retargetTabsForRename`, `closeTabsUnder` — all used consistently.
- **Forward references:** Task 8 stubs `showConflict`/`hideConflict` (real in Task 9); Task 16 references `retargetTabsForRename`/`closeTabsUnder` (added in Task 17). Both are inside functions, so each task's `cargo build` succeeds; the manual smoke for those flows is deferred to the task that completes them.
