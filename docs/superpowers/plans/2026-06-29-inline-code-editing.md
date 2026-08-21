# In-place Code Editing + Tab Edit Indicators Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make code/text files editable in place (single-pane, syntax-highlighted CodeMirror replacing the read view) instead of the markdown split, and show a tab's editing/dirty state on inactive tabs.

**Architecture:** Reuse the single existing CodeMirror instance and `.editor-pane` element. A layout branch hides `#preview` and fills the pane for code tabs; markdown keeps its split + live preview. CodeMirror mode is chosen per file from a vendored mode set via a pure `modeForPath` helper. The tab gets an `editing` class that accent-tints its name.

**Tech Stack:** Vanilla JS/CSS frontend (no build step; bundled at compile time → changes need `cargo build`), CodeMirror 5.65.1 (vendored), `node --test` for JS units.

## Global Constraints

- Builds on the already-merged code-file-rendering work (`isCodeView`/`isMarkdownPath` in `ui/filetype.js`, the syntect read view). A code tab = `isCodeView(path)` (not markdown, not image).
- In-place editing applies ONLY to code tabs. Markdown editing keeps today's split + live preview, byte-for-byte.
- CodeMirror version is **5.65.1**; vendored mode files MUST match (cdnjs path `…/codemirror/5.65.1/mode/<name>/<name>.min.js`).
- Mode scripts load as classic `<script>`s respecting deps: `xml` before `htmlmixed`/`markdown`; `javascript` + `css` before `htmlmixed`.
- For code: `lineWrapping: false` (horizontal scroll); markdown: `lineWrapping: true`. Unknown extension → mode `null` (plain, still editable).
- Editing tab indicator uses accent `#5599ff` (the active-tab underline color); the `●` dirty dot is unchanged.
- Save/dirty/conflict machinery is reused unchanged.
- Frontend changes need `cargo build` to bundle; CSP is `script-src 'self'` so all scripts must be local (no CDN at runtime).

---

### Task 1: `editor-modes.js` extension→mode helper

**Files:**
- Create: `ui/editor-modes.js`, `ui/editor-modes.test.js`

**Interfaces:**
- Produces: `modeForPath(path) -> string | null` (CodeMirror mode name or null).

- [ ] **Step 1: Write the failing test**

Create `ui/editor-modes.test.js`:

```javascript
import { test } from "node:test";
import assert from "node:assert/strict";
import { modeForPath } from "./editor-modes.js";

test("modeForPath maps code extensions to CodeMirror modes", () => {
  assert.equal(modeForPath("main.rs"), "rust");
  assert.equal(modeForPath("app.tsx"), "javascript");
  assert.equal(modeForPath("data.json"), "javascript");
  assert.equal(modeForPath("script.py"), "python");
  assert.equal(modeForPath("style.css"), "css");
  assert.equal(modeForPath("page.html"), "htmlmixed");
  assert.equal(modeForPath("a.b.c.rs"), "rust");
  assert.equal(modeForPath("readme.md"), "markdown");
});

test("modeForPath returns null for unknown or extensionless paths", () => {
  assert.equal(modeForPath("notes.xyz"), null);
  assert.equal(modeForPath("Makefile"), null);
  assert.equal(modeForPath(""), null);
  assert.equal(modeForPath(null), null);
});
```

- [ ] **Step 2: Run the test (expect FAIL)**

Run: `node --test ui/editor-modes.test.js 2>&1 | tail -6`
Expected: FAIL — module/function not found.

- [ ] **Step 3: Implement `ui/editor-modes.js`**

```javascript
// Pure extension -> CodeMirror 5 mode-name mapping (no DOM), unit-testable.

const EXT_MODE = {
  md: "markdown", markdown: "markdown", mdown: "markdown", mkd: "markdown", mkdn: "markdown",
  js: "javascript", jsx: "javascript", mjs: "javascript", cjs: "javascript",
  ts: "javascript", tsx: "javascript", json: "javascript",
  py: "python",
  rs: "rust",
  c: "clike", h: "clike", cpp: "clike", hpp: "clike", cc: "clike", cxx: "clike",
  hxx: "clike", java: "clike", cs: "clike",
  css: "css", scss: "css", less: "css",
  html: "htmlmixed", htm: "htmlmixed", xhtml: "htmlmixed",
  xml: "xml", svg: "xml",
  sh: "shell", bash: "shell", zsh: "shell",
  yml: "yaml", yaml: "yaml",
  go: "go",
  sql: "sql",
};

// Returns the CodeMirror mode name for a file path, or null (plain text).
export function modeForPath(path) {
  const m = /\.([^.\/\\]+)$/.exec(path || "");
  if (!m) return null;
  return EXT_MODE[m[1].toLowerCase()] || null;
}
```

- [ ] **Step 4: Run the test (expect PASS)**

Run: `node --test ui/editor-modes.test.js 2>&1 | tail -6`
Expected: both tests pass.

- [ ] **Step 5: Commit**

```bash
git add ui/editor-modes.js ui/editor-modes.test.js
git commit -m "Add editor-modes: extension to CodeMirror mode map"
```

---

### Task 2: Vendor CodeMirror mode files + load them

**Files:**
- Create: `ui/codemirror/{javascript,css,htmlmixed,clike,python,rust,shell,yaml,go,sql}.min.js`
- Modify: `ui/index.html:308-310` (script tags)

**Interfaces:**
- Produces: the CodeMirror modes registered on `window.CodeMirror` at runtime, so `cm.setOption("mode", "<name>")` highlights.

- [ ] **Step 1: Download the mode files (matching CodeMirror 5.65.1)**

Run from the repo root:

```bash
cd ui/codemirror
for m in javascript css htmlmixed clike python rust shell yaml go sql; do
  curl -fsSL "https://cdnjs.cloudflare.com/ajax/libs/codemirror/5.65.1/mode/$m/$m.min.js" -o "$m.min.js"
done
cd ../..
ls -l ui/codemirror/*.min.js
```

Expected: 13 `.min.js` files total (the 3 pre-existing + 10 new), each non-empty (hundreds of bytes to a few KB). If any download is empty or 404s, STOP and report — do not commit placeholder files. (Fallback if cdnjs is unreachable: `npm pack codemirror@5.65.1`, extract, and copy `package/mode/<name>/<name>.min.js`.)

- [ ] **Step 2: Sanity-check the downloads**

Run: `for f in javascript css htmlmixed clike python rust shell yaml go sql; do test -s "ui/codemirror/$f.min.js" && head -c 40 "ui/codemirror/$f.min.js" | grep -q CodeMirror && echo "$f ok" || echo "$f BAD"; done`
Expected: every line `… ok`.

- [ ] **Step 3: Wire the script tags**

In `ui/index.html`, replace the three existing CodeMirror mode lines (currently lines 308-310):

```html
    <script src="codemirror/codemirror.min.js"></script>
    <script src="codemirror/xml.min.js"></script>
    <script src="codemirror/markdown.min.js"></script>
```

with (order matters — `xml`/`javascript`/`css` precede `htmlmixed`; `xml` precedes `markdown`):

```html
    <script src="codemirror/codemirror.min.js"></script>
    <script src="codemirror/xml.min.js"></script>
    <script src="codemirror/javascript.min.js"></script>
    <script src="codemirror/css.min.js"></script>
    <script src="codemirror/htmlmixed.min.js"></script>
    <script src="codemirror/markdown.min.js"></script>
    <script src="codemirror/clike.min.js"></script>
    <script src="codemirror/python.min.js"></script>
    <script src="codemirror/rust.min.js"></script>
    <script src="codemirror/shell.min.js"></script>
    <script src="codemirror/yaml.min.js"></script>
    <script src="codemirror/go.min.js"></script>
    <script src="codemirror/sql.min.js"></script>
```

- [ ] **Step 4: Build**

Run: `cd src-tauri && cargo build 2>&1 | tail -3`
Expected: clean (bundles the new `ui/` assets).

- [ ] **Step 5: Commit**

```bash
git add ui/codemirror/*.min.js ui/index.html
git commit -m "Vendor CodeMirror language modes for code editing"
```

---

### Task 3: In-place editing wiring + CSS

**Files:**
- Modify: `ui/app.js` (import ~line 30; `setActiveTab` ~1219-1230; `enterEditMode` ~1410-1431; `onEditorChange` ~1455-1474; `showEditorChrome` ~1449-1453)
- Modify: `ui/styles.css` (in-place layout + dark token classes)

**Interfaces:**
- Consumes: `modeForPath` (Task 1), `isCodeView` (already imported from `filetype.js`), modes registered by Task 2.

- [ ] **Step 1: Import `modeForPath`**

In `ui/app.js`, just after the existing `import { isImagePath, isMarkdownPath, isCodeView } from "./filetype.js";` (line 30), add:

```javascript
import { modeForPath } from "./editor-modes.js";
```

- [ ] **Step 2: Add the `inPlace` param to `showEditorChrome`**

Replace the whole function (currently ~1449-1453):

```javascript
function showEditorChrome(on, inPlace = false) {
  editorPane.hidden = !on;
  editorSplitter.hidden = !on || inPlace;
  paneBody.classList.toggle("editing", on);
  paneBody.classList.toggle("editing-inplace", on && inPlace);
}
```

- [ ] **Step 3: Set mode/wrap/layout in `enterEditMode`**

Replace `enterEditMode` (currently ~1410-1431):

```javascript
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
  t.editBuffer = src;
  ensureCm();
  const inPlace = isCodeView(t.path);
  cm.setOption("mode", modeForPath(t.path));
  cm.setOption("lineWrapping", !inPlace);
  cm.setValue(src);
  cm.clearHistory();
  showEditorChrome(true, inPlace);
  renderTabBar();
  cm.refresh();
  cm.focus();
  if (!inPlace) await renderFromEditor(t, { scrollLock: false });
}
```

- [ ] **Step 4: Mirror the same setup in `setActiveTab`'s editing branch**

Replace the `if (t.editing) { … } else { … }` block (currently ~1220-1230):

```javascript
  const t = tabs[idx];
  if (t.editing) {
    ensureCm();
    const inPlace = isCodeView(t.path);
    cm.setOption("mode", modeForPath(t.path));
    cm.setOption("lineWrapping", !inPlace);
    showEditorChrome(true, inPlace);
    cm.setValue(t.editBuffer != null ? t.editBuffer : t.savedContent);
    cm.clearHistory();
    cm.refresh();
    if (!inPlace) await renderFromEditor(t, { scrollLock: same && !forceRender });
  } else {
    showEditorChrome(false);
    await renderActive({ scrollLock: same && !forceRender });
  }
```

- [ ] **Step 5: Skip live preview for code in `onEditorChange`**

In `onEditorChange` (currently ~1455-1474), add an early return after the dirty-tracking block and before the `previewDebounce` logic:

```javascript
function onEditorChange() {
  const t = activeTab();
  if (!t || !t.editing) return;
  t.editBuffer = cm.getValue();
  const dirty = isDirty(t.editBuffer, t.savedContent);
  if (dirty !== t.dirty) {
    t.dirty = dirty;
    renderTabBar();
  }
  if (isCodeView(t.path)) return; // in-place editor has no live-preview pane
  if (previewDebounce) clearTimeout(previewDebounce);
  const path = t.path;
  previewDebounce = setTimeout(() => {
    previewDebounce = null;
    const current = activeTab();
    if (!current || !current.editing || current.path !== path) return;
    renderFromEditor(current, { scrollLock: true }).catch((e) =>
      console.error("live preview failed", e),
    );
  }, EDITOR_PREVIEW_DEBOUNCE_MS);
}
```

- [ ] **Step 6: Add the in-place layout + dark token CSS**

In `ui/styles.css`, after the `.editor-pane .CodeMirror { … }` rule (~line 1373), add the in-place layout:

```css
/* In-place code editing: the editor fills the pane; no preview, no splitter. */
.pane-body.editing-inplace .editor-pane {
  width: 100%;
  border-right: none;
}
.pane-body.editing-inplace .preview-scroll {
  display: none;
}
.editor-pane .CodeMirror.cm-s-default {
  white-space: pre;
}
```

Then, after the existing `[data-theme="dark"] .editor-pane .cm-variable-2, … .cm-def { color: #ffa657; }` rule (~line 1428), add coverage for the remaining code-mode token classes (`cm-variable` and `cm-operator` intentionally inherit the base editor fg):

```css
[data-theme="dark"] .editor-pane .cm-property,
[data-theme="dark"] .editor-pane .cm-builtin {
  color: #79c0ff;
}
[data-theme="dark"] .editor-pane .cm-variable-3 {
  color: #ffa657;
}
[data-theme="dark"] .editor-pane .cm-meta {
  color: #8b949e;
}
```

- [ ] **Step 7: Build**

Run: `cd src-tauri && cargo build 2>&1 | tail -3`
Expected: clean.

- [ ] **Step 8: Manual verification**

`cargo run -- ../README.md`, then:
- Open a `.rs` file → click **Edit** → editor fills the pane (no split, no preview), code is colored, line numbers present, no wrapping.
- Type → the tab name accent-tints and a `●` appears once changed; **⌘S** saves and clears the dot.
- **Done** → returns to the syntect read view.
- Open a `.md` file → **Edit** still opens the split with live preview.
- Toggle theme while editing a code file → editor recolors.

- [ ] **Step 9: Commit**

```bash
git add ui/app.js ui/styles.css
git commit -m "Edit code files in place with a single-pane editor"
```

---

### Task 4: Tab edit-state indicator

**Files:**
- Modify: `ui/app.js` (`makeTabEl` ~1315-1320), `ui/styles.css`

**Interfaces:**
- Consumes: `tab.editing` (set by the edit flow).

- [ ] **Step 1: Add the `editing` class in `makeTabEl`**

In `ui/app.js`, in `makeTabEl`, right after the line `if (idx === activeIdx) el.classList.add("active");`, add:

```javascript
  if (tab.editing) el.classList.add("editing");
```

- [ ] **Step 2: Accent-tint the editing tab's name**

In `ui/styles.css`, after the `.tab.preview .tab-name { font-style: italic; }` rule (~line 362), add:

```css
.tab.editing .tab-name {
  color: #5599ff;
}
```

- [ ] **Step 3: Build**

Run: `cd src-tauri && cargo build 2>&1 | tail -3`
Expected: clean.

- [ ] **Step 4: Manual verification**

`cargo run -- ../README.md`: open and **Edit** a file, switch to another tab — the edited tab's name shows the accent color (and the `●` if you made unsaved changes) while inactive.

- [ ] **Step 5: Commit**

```bash
git add ui/app.js ui/styles.css
git commit -m "Mark tabs in edit mode with an accent-colored name"
```

---

## Self-Review

**Spec coverage:**
- In-place single-pane layout, no live preview for code, markdown keeps split → Task 3 (Steps 2-6). ✓
- Mode-by-extension + wrap rules + plain fallback → Task 1 (`modeForPath`) + Task 3 (Steps 3-4). ✓
- Vendored mode set + load order → Task 2. ✓
- Theming: reuse `.editor-pane` CSS + extended dark token classes → Task 3 Step 6. ✓
- Save/dirty/conflict reused (no change) → unchanged code paths, called out in Task 3. ✓
- Tab editing indicator (accent name) + dirty dot unchanged → Task 4. ✓
- Binary edit naturally blocked (read_source fails) → no task needed (spec marks it out of scope). ✓
- Testing: `editor-modes` units + manual matrix → Task 1, Task 3 Step 8, Task 4 Step 4. ✓

**Placeholder scan:** No TBD/TODO; all code/CSS shown in full. The curl URLs and the npm fallback are concrete. ✓

**Type consistency:** `modeForPath(path) -> string|null` defined in Task 1, consumed in Task 3 (`cm.setOption("mode", modeForPath(t.path))`). `showEditorChrome(on, inPlace)` defined in Task 3 Step 2 and called with the second arg in Steps 3-4. `isCodeView` (already imported) gates inPlace consistently. CSS class `editing-inplace` set in Step 2 and targeted in Step 6; `.tab.editing` set in Task 4 Step 1 and targeted in Step 2. ✓
