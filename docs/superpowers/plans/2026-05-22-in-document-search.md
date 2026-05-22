# In-document Search Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a find bar that searches the active document with match highlighting, next/previous navigation, a match count, and case-sensitive and whole-word toggles.

**Architecture:** Pure text-matching lives in a dependency-free ES module (`ui/search.js`) that is unit-tested with Node's built-in test runner. The frontend (`ui/app.js`) walks the rendered preview's text nodes, builds `Range`s for each match, and paints them with the CSS Custom Highlight API — zero DOM mutation, so it coexists with morphdom live-reload, `data-sourcepos` scroll anchoring, and the mermaid/image preservation hooks. A native **Find…** menu item (⌘F) reuses the existing `edit-action` event channel to open the bar.

**Tech Stack:** Vanilla JS (no framework, no build step), CSS Custom Highlight API, Node `node:test` (zero deps), Tauri 2 menu API (Rust).

---

## File structure

- **Create** `ui/search.js` — pure `findMatches(text, query, opts)` + `isWordChar`. No DOM/Tauri deps.
- **Create** `ui/search.test.js` — `node:test` unit tests for `findMatches`.
- **Create** `ui/package.json` — `{ "type": "module" }` so Node treats `ui/*.js` as ES modules. No dependencies.
- **Modify** `ui/index.html` — add the find-bar markup inside `.preview-pane`, outside `#preview`.
- **Modify** `ui/styles.css` — find-bar chrome, `::highlight()` rules, `.preview-pane { position: relative }`.
- **Modify** `ui/app.js` — import `findMatches`; add the find module (text walk, Range building, Highlight registration, UI wiring, keys); add the `"find"` case to `runEditAction`; re-run search after `renderActive` when the bar is open.
- **Modify** `src-tauri/src/menu.rs` — add the **Find…** (⌘F) item to the *Actions* submenu, emitting `edit-action` `"find"`.
- **Modify** `.github/workflows/ci.yml` — add a `node --test 'ui/*.test.js'` step.

Why a separate module: `app.js` reads `window.__TAURI__` at load, so it can't be imported under Node. Splitting the pure matching logic into `ui/search.js` makes the bug-prone part (case folding, whole-word boundaries, non-overlapping matches) unit-testable while the DOM-touching parts are verified by running the app.

---

## Task 1: Pure text-matching module + unit tests

**Files:**
- Create: `ui/package.json`
- Create: `ui/search.test.js`
- Create: `ui/search.js`

- [ ] **Step 1: Create the package marker so Node treats `ui/*.js` as ES modules**

Create `ui/package.json`:

```json
{
  "name": "mdviewer-ui",
  "version": "0.0.0",
  "private": true,
  "type": "module"
}
```

- [ ] **Step 2: Write the failing tests**

Create `ui/search.test.js`:

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { findMatches, isWordChar } from "./search.js";

test("returns no matches for an empty query", () => {
  assert.deepEqual(findMatches("hello", ""), []);
});

test("finds multiple occurrences in document order", () => {
  assert.deepEqual(findMatches("hello world hello", "hello"), [
    [0, 5],
    [12, 17],
  ]);
});

test("is case-insensitive by default", () => {
  assert.deepEqual(findMatches("Hello HELLO", "hello"), [
    [0, 5],
    [6, 11],
  ]);
});

test("respects the case-sensitive option", () => {
  assert.deepEqual(
    findMatches("Hello hello", "hello", { caseSensitive: true }),
    [[6, 11]],
  );
});

test("whole-word skips substrings inside larger words", () => {
  assert.deepEqual(findMatches("cat category cat", "cat", { wholeWord: true }), [
    [0, 3],
    [13, 16],
  ]);
});

test("whole-word treats punctuation as a boundary", () => {
  assert.deepEqual(findMatches("(cat)", "cat", { wholeWord: true }), [[1, 4]]);
});

test("whole-word treats underscore as part of the word", () => {
  assert.deepEqual(findMatches("cat_x cat", "cat", { wholeWord: true }), [
    [6, 9],
  ]);
});

test("returns non-overlapping matches", () => {
  assert.deepEqual(findMatches("aaaa", "aa"), [
    [0, 2],
    [2, 4],
  ]);
});

test("isWordChar recognizes letters, digits, and underscore", () => {
  assert.equal(isWordChar("a"), true);
  assert.equal(isWordChar("7"), true);
  assert.equal(isWordChar("_"), true);
  assert.equal(isWordChar(" "), false);
  assert.equal(isWordChar(null), false);
});
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `node --test 'ui/*.test.js'`
Expected: FAIL — `Cannot find module` / `Cannot find package` for `./search.js` (the module does not exist yet).

- [ ] **Step 4: Implement the matching module**

Create `ui/search.js`:

```js
// Pure text-matching for the in-document find bar. No DOM or Tauri imports, so
// it runs under `node --test` as well as in the WebView.

/** True if `ch` is a word character: Unicode letter, number, or underscore. */
export function isWordChar(ch) {
  return ch != null && /[\p{L}\p{N}_]/u.test(ch);
}

function isWholeWord(text, start, end) {
  const before = start > 0 ? text[start - 1] : null;
  const after = end < text.length ? text[end] : null;
  return !isWordChar(before) && !isWordChar(after);
}

/**
 * Find every occurrence of `query` in `text`.
 *
 * @param {string} text
 * @param {string} query
 * @param {{caseSensitive?: boolean, wholeWord?: boolean}} [opts]
 * @returns {Array<[number, number]>} [start, end) offset pairs, in order,
 *   non-overlapping.
 */
export function findMatches(text, query, opts = {}) {
  const { caseSensitive = false, wholeWord = false } = opts;
  if (!query) return [];
  const hay = caseSensitive ? text : text.toLowerCase();
  const needle = caseSensitive ? query : query.toLowerCase();
  const out = [];
  let from = 0;
  let lastEnd = -1;
  for (;;) {
    const i = hay.indexOf(needle, from);
    if (i === -1) break;
    const end = i + needle.length;
    if (i >= lastEnd && (!wholeWord || isWholeWord(text, i, end))) {
      out.push([i, end]);
      lastEnd = end;
    }
    from = i + 1;
  }
  return out;
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `node --test 'ui/*.test.js'`
Expected: PASS — `# pass 9`, `# fail 0`.

- [ ] **Step 6: Commit**

```bash
git add ui/package.json ui/search.js ui/search.test.js
git commit -m "Add pure text-matching module for in-document search"
```

---

## Task 2: Find-bar markup and styles

**Files:**
- Modify: `ui/index.html` (inside `<main class="preview-pane">`)
- Modify: `ui/styles.css`

This task adds inert UI (no behavior yet). Verification is that the app still builds and renders unchanged with the bar hidden.

- [ ] **Step 1: Add the find-bar markup**

In `ui/index.html`, the preview pane currently ends like this:

```html
      <div class="preview-scroll" id="preview-scroll">
        <div class="preview-empty" id="preview-empty">
          Select a file from the tree to preview.
        </div>
        <article class="markdown-body" id="preview" hidden></article>
      </div>
    </main>
```

Replace it with (find bar added after `.preview-scroll`, still inside `<main>`, so morphdom — which only patches `#preview` — never touches it):

```html
      <div class="preview-scroll" id="preview-scroll">
        <div class="preview-empty" id="preview-empty">
          Select a file from the tree to preview.
        </div>
        <article class="markdown-body" id="preview" hidden></article>
      </div>
      <div class="find-bar" id="find-bar" hidden role="search">
        <input
          id="find-input"
          class="find-input"
          type="text"
          placeholder="Find"
          aria-label="Find in document"
          autocomplete="off"
          spellcheck="false"
        />
        <span class="find-count" id="find-count" aria-live="polite"></span>
        <button
          id="find-case"
          class="find-toggle"
          type="button"
          aria-pressed="false"
          title="Match case"
        >
          Aa
        </button>
        <button
          id="find-word"
          class="find-toggle"
          type="button"
          aria-pressed="false"
          title="Whole word"
        >
          \b
        </button>
        <button
          id="find-prev"
          class="find-btn"
          type="button"
          title="Previous match (⇧⏎)"
          aria-label="Previous match"
        >
          ↑
        </button>
        <button
          id="find-next"
          class="find-btn"
          type="button"
          title="Next match (⏎)"
          aria-label="Next match"
        >
          ↓
        </button>
        <button
          id="find-close"
          class="find-btn"
          type="button"
          title="Close (Esc)"
          aria-label="Close find"
        >
          ×
        </button>
      </div>
    </main>
```

- [ ] **Step 2: Anchor the pane and style the bar**

In `ui/styles.css`, find this rule:

```css
.preview-pane {
  background: var(--bg);
  display: flex;
  flex-direction: column;
  min-width: 0;
  overflow: hidden;
}
```

Replace it with (adds `position: relative` so the absolutely-positioned find bar anchors to the pane):

```css
.preview-pane {
  background: var(--bg);
  display: flex;
  flex-direction: column;
  min-width: 0;
  overflow: hidden;
  position: relative;
}
```

- [ ] **Step 3: Append the find-bar and highlight styles**

Append to the end of `ui/styles.css`:

```css
/* ---- In-document find bar ---- */

.find-bar {
  position: absolute;
  top: 42px;
  right: 16px;
  z-index: 50;
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 6px;
  background: var(--sidebar-bg);
  border: 1px solid var(--sidebar-border);
  border-radius: 6px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.18);
}

.find-input {
  font: inherit;
  font-size: 13px;
  width: 180px;
  padding: 3px 6px;
  border: 1px solid var(--sidebar-border);
  border-radius: 4px;
  background: var(--bg);
  color: var(--fg);
  outline: none;
}

.find-input:focus {
  border-color: #5599ff;
}

.find-bar.no-match .find-input {
  border-color: #cf222e;
}

.find-count {
  font-size: 12px;
  color: var(--sidebar-muted);
  min-width: 56px;
  text-align: center;
  white-space: nowrap;
}

.find-toggle,
.find-btn {
  font: inherit;
  font-size: 12px;
  min-width: 24px;
  height: 24px;
  padding: 0 6px;
  background: transparent;
  color: var(--sidebar-muted);
  border: 1px solid transparent;
  border-radius: 4px;
  cursor: pointer;
}

.find-toggle:hover,
.find-btn:hover {
  background: var(--sidebar-hover);
  color: var(--sidebar-fg);
}

.find-toggle[aria-pressed="true"] {
  background: #5599ff;
  color: #fff;
  border-color: #5599ff;
}

/* Match highlighting via the CSS Custom Highlight API (no DOM mutation). */
::highlight(search-match) {
  background-color: rgba(255, 214, 0, 0.4);
}

::highlight(search-current) {
  background-color: #ff9632;
  color: #1f2328;
}

@media (prefers-color-scheme: dark) {
  ::highlight(search-match) {
    background-color: rgba(255, 214, 0, 0.3);
  }
  ::highlight(search-current) {
    background-color: #d2691e;
    color: #ffffff;
  }
}
```

- [ ] **Step 4: Build and verify no regression**

Run: `cd src-tauri && cargo run -- ../README.md`
Expected: the app launches and renders `README.md` exactly as before. The find bar is **not** visible (it has the `hidden` attribute and no code shows it yet). Close the app.

- [ ] **Step 5: Commit**

```bash
git add ui/index.html ui/styles.css
git commit -m "Add find-bar markup and styles (inert)"
```

---

## Task 3: Find… menu item

**Files:**
- Modify: `src-tauri/src/menu.rs`

- [ ] **Step 1: Build the menu item**

In `src-tauri/src/menu.rs`, the *Actions* items are built like this:

```rust
    let edit_copy = MenuItemBuilder::with_id("edit-copy", "Copy")
        .accelerator("CmdOrCtrl+C")
        .build(app)?;
    let edit_copy_source =
        MenuItemBuilder::with_id("edit-copy-source", "Copy Source").build(app)?;
    let edit_toggle_raw = MenuItemBuilder::with_id("edit-toggle-raw", "Toggle Raw").build(app)?;
```

Replace that block with (adds `edit_find`):

```rust
    let edit_copy = MenuItemBuilder::with_id("edit-copy", "Copy")
        .accelerator("CmdOrCtrl+C")
        .build(app)?;
    let edit_find = MenuItemBuilder::with_id("edit-find", "Find…")
        .accelerator("CmdOrCtrl+F")
        .build(app)?;
    let edit_copy_source =
        MenuItemBuilder::with_id("edit-copy-source", "Copy Source").build(app)?;
    let edit_toggle_raw = MenuItemBuilder::with_id("edit-toggle-raw", "Toggle Raw").build(app)?;
```

- [ ] **Step 2: Add the item to the Actions submenu**

In the same function, this builds the submenu:

```rust
    let edit_menu = SubmenuBuilder::new(app, "Actions")
        .item(&edit_copy)
        .separator()
        .item(&edit_copy_source)
        .item(&edit_toggle_raw)
        .build()?;
```

Replace it with (inserts Find… after Copy, with a separator):

```rust
    let edit_menu = SubmenuBuilder::new(app, "Actions")
        .item(&edit_copy)
        .item(&edit_find)
        .separator()
        .item(&edit_copy_source)
        .item(&edit_toggle_raw)
        .build()?;
```

- [ ] **Step 3: Route the menu event to the frontend**

In the `on_menu_event` match in `install`, this handles the copy event:

```rust
            "edit-copy" => {
                let _ = app.emit("edit-action", "copy");
            }
```

Add a `edit-find` arm immediately after it:

```rust
            "edit-copy" => {
                let _ = app.emit("edit-action", "copy");
            }
            "edit-find" => {
                let _ = app.emit("edit-action", "find");
            }
```

- [ ] **Step 4: Verify lint and build are clean**

Run: `cd src-tauri && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo build`
Expected: no formatting diff, no clippy warnings, build succeeds. (The frontend does not handle `"find"` yet — that is Task 4 — so the menu item is inert for now.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/menu.rs
git commit -m "Add Find… menu item (Cmd+F)"
```

---

## Task 4: Find behavior in the frontend

**Files:**
- Modify: `ui/app.js`

- [ ] **Step 1: Import the matching function**

At the very top of `ui/app.js`, the file begins:

```js
// mdviewer frontend
// Uses Tauri v2 IPC; window.__TAURI__ is injected because tauri.conf.json sets withGlobalTauri.

const { invoke, convertFileSrc } = window.__TAURI__.core;
```

Insert the import as the first line, before the comment block:

```js
import { findMatches } from "./search.js";

// mdviewer frontend
// Uses Tauri v2 IPC; window.__TAURI__ is injected because tauri.conf.json sets withGlobalTauri.

const { invoke, convertFileSrc } = window.__TAURI__.core;
```

- [ ] **Step 2: Add the `"find"` case to `runEditAction`**

In `ui/app.js`, `runEditAction` currently is:

```js
async function runEditAction(name) {
  switch (name) {
    case "copy":
      await actionCopySelection();
      break;
    case "copy-source":
      await actionCopySource();
      break;
    case "toggle-raw":
      onToggleRaw();
      break;
  }
}
```

Replace it with (adds the `find` case):

```js
async function runEditAction(name) {
  switch (name) {
    case "copy":
      await actionCopySelection();
      break;
    case "copy-source":
      await actionCopySource();
      break;
    case "toggle-raw":
      onToggleRaw();
      break;
    case "find":
      openFind();
      break;
  }
}
```

- [ ] **Step 3: Add the find module**

In `ui/app.js`, locate the update-check section, which starts with:

```js
/* ---- Update check ---- */

const DISMISS_KEY = "mdviewer.update.dismissed_version";
```

Insert this entire block immediately **before** that `/* ---- Update check ---- */` comment:

```js
/* ---- In-document find ---- */

const findBar = document.getElementById("find-bar");
const findInput = document.getElementById("find-input");
const findCount = document.getElementById("find-count");
const findCaseBtn = document.getElementById("find-case");
const findWordBtn = document.getElementById("find-word");
const findPrevBtn = document.getElementById("find-prev");
const findNextBtn = document.getElementById("find-next");
const findCloseBtn = document.getElementById("find-close");

const HIGHLIGHT_SUPPORTED =
  typeof CSS !== "undefined" &&
  CSS.highlights &&
  typeof Highlight !== "undefined";

const findState = {
  caseSensitive: false,
  wholeWord: false,
  matches: [], // Range[]
  current: -1,
};

function findOpen() {
  return !findBar.hidden;
}

function openFind() {
  if (!activeTab()) return;
  const sel = selectedText();
  if (sel && sel.length <= 200 && !sel.includes("\n")) {
    findInput.value = sel;
  }
  findBar.hidden = false;
  findInput.focus();
  findInput.select();
  runFind({ keepCurrent: false });
}

function closeFind() {
  findBar.hidden = true;
  clearFindHighlights();
  findState.matches = [];
  findState.current = -1;
}

function clearFindHighlights() {
  if (!HIGHLIGHT_SUPPORTED) return;
  CSS.highlights.delete("search-match");
  CSS.highlights.delete("search-current");
}

/** Flatten the preview's text into one string plus an offset→node map,
 *  skipping rendered mermaid diagrams. */
function collectFindSegments() {
  const walker = document.createTreeWalker(preview, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      if (!node.nodeValue) return NodeFilter.FILTER_REJECT;
      if (node.parentElement && node.parentElement.closest("pre.mermaid")) {
        return NodeFilter.FILTER_REJECT;
      }
      return NodeFilter.FILTER_ACCEPT;
    },
  });
  let text = "";
  const segs = []; // { node, start } — start is the offset of node within text
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    segs.push({ node: n, start: text.length });
    text += n.nodeValue;
  }
  return { text, segs };
}

/** Map a global text offset to its containing node and local offset. */
function locateFindOffset(segs, offset) {
  let lo = 0;
  let hi = segs.length - 1;
  let found = 0;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (segs[mid].start <= offset) {
      found = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  const seg = segs[found];
  return { node: seg.node, offset: offset - seg.start };
}

function findRangeFor(segs, start, end) {
  const a = locateFindOffset(segs, start);
  const b = locateFindOffset(segs, end);
  const range = document.createRange();
  range.setStart(a.node, a.offset);
  range.setEnd(b.node, b.offset);
  return range;
}

function runFind({ keepCurrent = true, scroll = true } = {}) {
  const query = findInput.value;
  const { text, segs } = collectFindSegments();
  const spans = segs.length
    ? findMatches(text, query, {
        caseSensitive: findState.caseSensitive,
        wholeWord: findState.wholeWord,
      })
    : [];
  const prev = keepCurrent ? findState.current : -1;
  findState.matches = spans.map(([s, e]) => findRangeFor(segs, s, e));
  if (findState.matches.length === 0) {
    findState.current = -1;
  } else {
    findState.current = Math.min(
      Math.max(prev, 0),
      findState.matches.length - 1,
    );
  }
  paintFindHighlights();
  updateFindCount(query);
  if (scroll && findState.current >= 0) scrollToFindCurrent();
}

function paintFindHighlights() {
  if (!HIGHLIGHT_SUPPORTED) return;
  CSS.highlights.delete("search-match");
  CSS.highlights.delete("search-current");
  if (findState.matches.length === 0) return;
  CSS.highlights.set("search-match", new Highlight(...findState.matches));
  if (findState.current >= 0) {
    const cur = new Highlight(findState.matches[findState.current]);
    cur.priority = 1;
    CSS.highlights.set("search-current", cur);
  }
}

function updateFindCount(query) {
  const n = findState.matches.length;
  if (!query) {
    findCount.textContent = "";
    findBar.classList.remove("no-match");
    return;
  }
  if (n === 0) {
    findCount.textContent = "No results";
    findBar.classList.add("no-match");
    return;
  }
  findBar.classList.remove("no-match");
  findCount.textContent = `${findState.current + 1} / ${n}`;
}

function scrollToFindCurrent() {
  const range = findState.matches[findState.current];
  if (!range) return;
  const rect = range.getBoundingClientRect();
  const paneRect = previewScroll.getBoundingClientRect();
  if (rect.top < paneRect.top || rect.bottom > paneRect.bottom) {
    const target =
      previewScroll.scrollTop +
      (rect.top - paneRect.top) -
      paneRect.height / 3;
    previewScroll.scrollTop = Math.max(0, target);
  }
}

function findStep(delta) {
  const n = findState.matches.length;
  if (n === 0) return;
  findState.current = (findState.current + delta + n) % n;
  paintFindHighlights();
  updateFindCount(findInput.value);
  scrollToFindCurrent();
}

function toggleFindOption(key, btn) {
  findState[key] = !findState[key];
  btn.setAttribute("aria-pressed", findState[key] ? "true" : "false");
  runFind({ keepCurrent: false });
  findInput.focus();
}

findInput.addEventListener("input", () => runFind({ keepCurrent: false }));
findCaseBtn.addEventListener("click", () =>
  toggleFindOption("caseSensitive", findCaseBtn),
);
findWordBtn.addEventListener("click", () =>
  toggleFindOption("wholeWord", findWordBtn),
);
findPrevBtn.addEventListener("click", () => findStep(-1));
findNextBtn.addEventListener("click", () => findStep(1));
findCloseBtn.addEventListener("click", () => closeFind());

findInput.addEventListener("keydown", (ev) => {
  if (ev.key === "Enter") {
    ev.preventDefault();
    findStep(ev.shiftKey ? -1 : 1);
  }
});

// ⌘G / ⇧⌘G navigate and Esc closes while the bar is open. (⌘F is delivered by
// the native Find… menu accelerator, not here.)
document.addEventListener("keydown", (ev) => {
  if (!findOpen()) return;
  const meta = ev.metaKey || ev.ctrlKey;
  if (meta && (ev.key === "g" || ev.key === "G")) {
    ev.preventDefault();
    findStep(ev.shiftKey ? -1 : 1);
  } else if (ev.key === "Escape") {
    ev.preventDefault();
    closeFind();
  }
});

```

- [ ] **Step 4: Re-run the search after a re-render when the bar is open**

In `ui/app.js`, `renderActive` ends:

```js
  if (anchor) restoreAnchor(anchor);
  else previewScroll.scrollTop = 0;
}
```

Replace it with (re-runs find without scrolling, so live reload preserves the reading position while refreshing highlights — old `Range`s are stale after morphdom patches):

```js
  if (anchor) restoreAnchor(anchor);
  else previewScroll.scrollTop = 0;

  if (findOpen()) runFind({ keepCurrent: true, scroll: false });
}
```

- [ ] **Step 5: Build and verify the full feature**

Run: `cd src-tauri && cargo run -- ../README.md`

Verify each behavior:
1. Press **⌘F** → the find bar appears (top-right) and the input is focused.
2. Type a word that occurs several times → all occurrences highlight (yellow), the current one is orange, and the count shows e.g. `1 / 7`.
3. Press **Enter** / **Shift+Enter** (and **⌘G** / **⇧⌘G**) → the current match advances/retreats, scrolls into view, and the count updates; navigation wraps around.
4. Click **Aa** → matching becomes case-sensitive (count changes for a mixed-case query); click again to turn off.
5. Click **\b** (whole word) → substring-only matches drop out.
6. Type a string with no matches → input border turns red and the count reads `No results`.
7. Press **Esc** (or click ×) → the bar closes and all highlighting clears.
8. With the bar open and a query active, edit and save `README.md` in another editor → highlights refresh and the scroll position is preserved (no jump to the first match).
9. Toggle **Raw** (menu *Actions ▸ Toggle Raw*) with the bar open → search still highlights within the raw text.
10. Switch the OS to dark mode → highlight colors adapt without reopening the bar.

Close the app.

- [ ] **Step 6: Commit**

```bash
git add ui/app.js
git commit -m "Wire up in-document find bar"
```

---

## Task 5: Run the JS unit tests in CI

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Add the test step**

In `.github/workflows/ci.yml`, the final step is:

```yaml
      - name: build (debug)
        working-directory: src-tauri
        run: cargo build
```

Append a new step after it (runs from the repo root; the `macos-14` runner has Node preinstalled):

```yaml
      - name: build (debug)
        working-directory: src-tauri
        run: cargo build

      - name: js unit tests
        run: node --test ui/*.test.js
```

- [ ] **Step 2: Verify locally**

Run: `node --test 'ui/*.test.js'`
Expected: PASS — `# pass 9`, `# fail 0`.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "Run search unit tests in CI"
```

---

## Self-review notes

- **Spec coverage:** find bar UI (Task 2), case + whole-word toggles (Tasks 1 & 4), match count + no-results state (Task 4), CSS Custom Highlight API painting (Tasks 2 & 4), ⌘F trigger via menu reusing `edit-action` (Tasks 3 & 4), Enter/Shift+Enter/⌘G/⇧⌘G/Esc keys (Task 4), prefill from selection (Task 4), flat-string cross-node matching + mermaid skip (Task 4), live-reload re-run without scroll jump (Task 4), raw-view and theme coexistence (Task 4 verification), active-document scope (no cross-tab code), `CSS.highlights` guard (Task 4). All covered.
- **No regex** anywhere, matching the spec — `findMatches` is `indexOf`-based, so no pattern escaping or injection surface.
- **Naming consistency:** `findMatches`, `findState`, `runFind`, `findOpen`, `openFind`, `closeFind`, `paintFindHighlights`, `clearFindHighlights`, `collectFindSegments`, `locateFindOffset`, `findRangeFor`, `scrollToFindCurrent`, `findStep`, `toggleFindOption` are used consistently across tasks. Highlight registry keys `search-match` / `search-current` match the `::highlight()` rules in `ui/styles.css`.
- **No CSP / dependency / Rust-logic changes** beyond the single menu item, per the spec.
```
