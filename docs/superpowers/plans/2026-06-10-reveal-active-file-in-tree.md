# Reveal Active File in Tree — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When the active tab changes, reveal its file in the sidebar tree — expand collapsed ancestor folders, scroll the row into view, and highlight it with an accent bar.

**Architecture:** A pure, unit-tested `treeops.js::treeAncestors(root, filePath)` computes the folders to expand. In `app.js`, the "open" half of `onDirClick` is extracted into a reusable `expandDir(li)`, and a new async `revealInTree(path)` (replacing `highlightSelectedByPath`) expands each ancestor via `expandDir`, then scrolls + highlights the file's row. A CSS accent bar makes the selection unmistakable.

**Tech Stack:** Vanilla JS, `node --test` (pure helper), Tauri rebuild to bundle. No Rust, no IPC.

---

## File structure

- **Modify `ui/treeops.js`** — add pure `treeAncestors(root, filePath)`.
- **Modify `ui/treeops.test.js`** — add its `node --test` cases.
- **Modify `ui/app.js`** — extract `expandDir(li)` from `onDirClick`; add `revealInTree(path)` replacing `highlightSelectedByPath` at both call sites; import `treeAncestors`.
- **Modify `ui/styles.css`** — accent bar on `.tree .row.selected` (light + dark).

## Conventions

- JS tests: `node --test ui/*.test.js`. Frontend changes require `cargo build` to rebundle.
- The DOM walk (`expandDir`, `revealInTree`) is verified by build + the Task 5 manual smoke test, not new unit tests; only `treeAncestors` is unit-tested.
- No `Co-Authored-By` trailer. Commit after each task.

---

## Task 1: `treeops.js::treeAncestors`

**Files:**
- Modify: `ui/treeops.js`, `ui/treeops.test.js`

- [ ] **Step 1: Write the failing test**

Append to `ui/treeops.test.js`:

```js
import { treeAncestors } from "./treeops.js";

test("treeAncestors lists ancestor dirs top-down between root and file", () => {
  assert.deepEqual(treeAncestors("/r", "/r/a/b/c.md"), ["/r/a", "/r/a/b"]);
});

test("treeAncestors returns [] for a file directly in root", () => {
  assert.deepEqual(treeAncestors("/r", "/r/x.md"), []);
});

test("treeAncestors returns null for a file outside root", () => {
  assert.equal(treeAncestors("/r", "/other/x.md"), null);
  assert.equal(treeAncestors("/r", "/r2/x.md"), null); // not fooled by a prefix
});

test("treeAncestors returns null when path equals root", () => {
  assert.equal(treeAncestors("/r", "/r"), null);
});

test("treeAncestors tolerates a trailing separator on root", () => {
  assert.deepEqual(treeAncestors("/r/", "/r/a/x.md"), ["/r/a"]);
});

test("treeAncestors handles Windows separators", () => {
  assert.deepEqual(treeAncestors("C:\\r", "C:\\r\\a\\x.md"), ["C:\\r\\a"]);
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `node --test ui/treeops.test.js`
Expected: FAIL — `treeAncestors is not a function`.

- [ ] **Step 3: Implement**

Append to `ui/treeops.js`:

```js
/** Ancestor directory paths to expand to reveal `filePath`, top-down and
 *  strictly between `root` and the file. Returns `[]` when the file sits
 *  directly in `root`, or `null` when `filePath` is not under `root` (or equals
 *  it). DOM-free; handles `/` and `\` separators and a trailing root separator. */
export function treeAncestors(root, filePath) {
  if (!root || !filePath) return null;
  const sep = filePath.includes("\\") ? "\\" : "/";
  const r = root.endsWith(sep) ? root.slice(0, -sep.length) : root;
  if (filePath === r) return null;
  const prefix = r + sep;
  if (!filePath.startsWith(prefix)) return null;
  const segs = filePath.slice(prefix.length).split(sep).filter((s) => s.length > 0);
  const ancestors = [];
  let cur = r;
  for (let i = 0; i < segs.length - 1; i++) {
    cur = cur + sep + segs[i];
    ancestors.push(cur);
  }
  return ancestors;
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `node --test ui/treeops.test.js`
Expected: PASS (the new tests + the existing `validateName` tests).

- [ ] **Step 5: Commit**

```bash
git add ui/treeops.js ui/treeops.test.js
git commit -m "Add treeops.treeAncestors (folders to expand to reveal a file)"
```

---

## Task 2: Extract `expandDir(li)` from `onDirClick`

**Files:**
- Modify: `ui/app.js`

- [ ] **Step 1: Add `expandDir`**

Add this function just before `onDirClick` (search for `async function onDirClick`):

```js
/** Expand a directory `<li>` (lazy-load + render its children). No-op if it
 *  isn't a directory row or is already open. Shared by onDirClick and
 *  revealInTree so click-expand and reveal-expand behave identically. */
async function expandDir(li) {
  const row = li.querySelector(":scope > .row");
  if (!row || li.dataset.isDir !== "1") return;
  if (li.querySelector(":scope > ul")) return; // already open
  const path = li.dataset.path;
  const depth = parseInt(li.dataset.depth, 10);
  row.classList.add("open");
  let children;
  try {
    children = await listDir(path);
  } catch (e) {
    console.error("list_dir failed", e);
    row.classList.remove("open");
    return;
  }
  const ul = document.createElement("ul");
  for (const child of children) {
    ul.appendChild(makeNode(child, depth + 1));
  }
  li.appendChild(ul);
  applyGitDecorations(ul);
  updateTreeWatch();
}
```

- [ ] **Step 2: Refactor `onDirClick` to use it**

The current `onDirClick` is:

```js
async function onDirClick(entry, li, row, depth) {
  const open = li.querySelector(":scope > ul");
  if (open) {
    open.remove();
    row.classList.remove("open");
    // Bust the cache so a folder that changes while collapsed (and thus
    // unwatched) reads fresh on the next expand.
    childCache.delete(entry.path);
    updateTreeWatch();
    return;
  }
  row.classList.add("open");
  let children;
  try {
    children = await listDir(entry.path);
  } catch (e) {
    console.error("list_dir failed", e);
    row.classList.remove("open");
    return;
  }
  const ul = document.createElement("ul");
  for (const child of children) {
    ul.appendChild(makeNode(child, depth + 1));
  }
  li.appendChild(ul);
  applyGitDecorations(ul);
  updateTreeWatch();
}
```

Replace it with (collapse branch unchanged; open branch delegates to `expandDir`):

```js
async function onDirClick(entry, li, row, depth) {
  const open = li.querySelector(":scope > ul");
  if (open) {
    open.remove();
    row.classList.remove("open");
    // Bust the cache so a folder that changes while collapsed (and thus
    // unwatched) reads fresh on the next expand.
    childCache.delete(entry.path);
    updateTreeWatch();
    return;
  }
  await expandDir(li);
}
```

- [ ] **Step 3: Verify build + behavior unchanged**

Run: `node --test ui/*.test.js 2>&1 | grep -E "# (pass|fail)"` (pass).
Run: `cd src-tauri && cargo build 2>&1 | tail -3` (`Finished`).

- [ ] **Step 4: Commit**

```bash
git add ui/app.js
git commit -m "Extract expandDir from onDirClick (shared expand path)"
```

---

## Task 3: `revealInTree` replaces `highlightSelectedByPath`

**Files:**
- Modify: `ui/app.js`

- [ ] **Step 1: Import `treeAncestors`**

Find the existing `import { validateName } from "./treeops.js";` and change it to:

```js
import { validateName, treeAncestors } from "./treeops.js";
```

- [ ] **Step 2: Replace `highlightSelectedByPath` with `revealInTree`**

The current function is:

```js
function highlightSelectedByPath(path) {
  for (const el of document.querySelectorAll(".tree .row.selected")) {
    el.classList.remove("selected");
  }
  if (!path) return;
  const li = tree.querySelector(`li[data-path="${cssEscape(path)}"]`);
  if (li) {
    const row = li.querySelector(":scope > .row");
    if (row) row.classList.add("selected");
  }
}
```

Replace it entirely with:

```js
/** Reveal a file in the tree for the active tab: clear any prior selection,
 *  expand the collapsed ancestor folders, then highlight + scroll the file's
 *  row into view. A file not under the current tree root (treeAncestors → null)
 *  is a no-op beyond clearing the selection. */
async function revealInTree(path) {
  for (const el of document.querySelectorAll(".tree .row.selected")) {
    el.classList.remove("selected");
  }
  const ancestors = treeAncestors(treeRoot, path);
  if (ancestors === null) return;
  for (const dir of ancestors) {
    const li = tree.querySelector(`li[data-path="${cssEscape(dir)}"]`);
    if (!li) return; // an ancestor row is missing (e.g. stale tab) — give up quietly
    await expandDir(li);
  }
  const li = tree.querySelector(`li[data-path="${cssEscape(path)}"]`);
  if (!li) return;
  const row = li.querySelector(":scope > .row");
  if (row) {
    row.classList.add("selected");
    row.scrollIntoView({ block: "nearest" });
  }
}
```

- [ ] **Step 3: Update the two call sites**

There are two calls to `highlightSelectedByPath`. Change both to `revealInTree`
(fire-and-forget; the reveal runs async without blocking the render):

- The site near `:581`: `if (tab) highlightSelectedByPath(tab.path);` →
  `if (tab) revealInTree(tab.path);`
- The site in `setActiveTab` near `:985`: `highlightSelectedByPath(tabs[idx].path);` →
  `revealInTree(tabs[idx].path);`

Verify no other references remain: `grep -n "highlightSelectedByPath" ui/app.js`
should return nothing.

- [ ] **Step 4: Verify build + tests**

Run: `grep -n "highlightSelectedByPath" ui/app.js` → no matches.
Run: `node --test ui/*.test.js 2>&1 | grep -E "# (pass|fail)"` (pass).
Run: `cd src-tauri && cargo build 2>&1 | tail -3` (`Finished`).

- [ ] **Step 5: Commit**

```bash
git add ui/app.js
git commit -m "Reveal active tab's file in the tree (expand ancestors + scroll)"
```

---

## Task 4: Accent bar on the selected row

**Files:**
- Modify: `ui/styles.css`

- [ ] **Step 1: Add the accent**

The current rule (around `:180`) is:

```css
.tree .row.selected {
  background: var(--sidebar-selected);
}
```

Replace it with (inset box-shadow avoids any layout shift a border would cause)
and add a brighter dark-mode accent right after:

```css
.tree .row.selected {
  background: var(--sidebar-selected);
  box-shadow: inset 2px 0 0 0 var(--accent, #0969da);
}

[data-theme="dark"] .tree .row.selected {
  box-shadow: inset 2px 0 0 0 #2f81f7;
}
```

- [ ] **Step 2: Verify build**

Run: `cd src-tauri && cargo build 2>&1 | tail -3` (`Finished`).

- [ ] **Step 3: Commit**

```bash
git add ui/styles.css
git commit -m "Add accent bar to the selected tree row"
```

---

## Task 5: Build + manual GUI smoke test

**Files:** none (verification only)

- [ ] **Step 1: Gates**

Run: `node --test ui/*.test.js 2>&1 | grep -E "# (pass|fail)"` (pass) ·
`cd src-tauri && cargo build 2>&1 | tail -2` (`Finished`).

- [ ] **Step 2: Run the app on this repo (a deep tree)**

Run: `cd src-tauri && cargo run -- ..`

- [ ] **Step 3: Verify reveal (light mode)**

- In the tree, collapse everything. Double-click to open a file a few folders
  deep — e.g. expand to `src-tauri/src/`, open `commands.rs` as a sticky tab,
  then collapse `src-tauri` again.
- Open a second file in a different folder (e.g. `ui/app.js`), then **click back
  to the `commands.rs` tab** → the `src-tauri` → `src` folders **expand**, the
  `commands.rs` row **scrolls into view** and shows the **accent bar +
  selected background**.
- Click between several tabs → the selection + reveal follows the active tab each
  time, to the correct row.
- Use **File ▸ Open File…** to open a markdown file from **outside** this repo →
  no crash; the tree selection clears (the file isn't in this tree).
- Single-click a deep file directly in the tree → it opens and ends up selected
  (reveal is idempotent on an already-visible row).

- [ ] **Step 4: Dark mode**

Toggle the theme (☾) and repeat one reveal → the accent bar (brighter blue) and
the selected background are both legible.

- [ ] **Step 5: Commit any fixes**

```bash
git add -A && git commit -m "Polish reveal-in-tree after smoke test"   # skip if none
```

---

## Self-review notes (for the implementer)

- **Spec coverage:** Part 1 behavior → Tasks 1 (`treeAncestors`) + 3 (`revealInTree`). Part 2 architecture (`treeAncestors`, `expandDir` extraction, `revealInTree` replacing `highlightSelectedByPath` at both sites) → Tasks 1-3. Part 3 styling (accent bar) → Task 4. Part 4 testing → Tasks 1 + 5. Out-of-scope items (no on/off setting, no reveal on live-reload, no auto-collapse) are honored — nothing adds them.
- **Type/symbol consistency:** `treeAncestors(root, filePath) -> string[] | null` defined (Task 1) and consumed by `revealInTree` (Task 3, branching on `=== null`). `expandDir(li)` defined (Task 2) and called by both `onDirClick` (Task 2) and `revealInTree` (Task 3). `revealInTree` replaces `highlightSelectedByPath` at exactly its two call sites; `grep` confirms the old name is gone. `treeRoot`, `tree`, `cssEscape`, `listDir`, `makeNode`, `applyGitDecorations`, `updateTreeWatch` are all existing module-level symbols.
- **Async note:** `revealInTree` is async and fired without `await` at both call sites (fire-and-forget) so it never delays the preview render; the clear-then-set of `.selected` makes the latest active tab win under rapid switching.
- **Known v1 limitations (accepted, per spec):** out-of-tree files can't be revealed (no row exists); auto-reveal always runs on tab change (no opt-out setting).
