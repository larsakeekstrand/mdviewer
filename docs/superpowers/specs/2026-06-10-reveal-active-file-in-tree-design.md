# Reveal the active tab's file in the tree

**Date:** 2026-06-10
**Status:** Approved (brainstorming) — pending implementation plan

## Goal

When the active tab changes, reveal its file in the sidebar tree: expand any
collapsed ancestor folders, scroll the file's row into view, and highlight it.
Today's `highlightSelectedByPath` only tints the row *if it already happens to be
rendered*, so for a file inside a collapsed folder it silently does nothing —
which is why the highlight feels like it isn't working.

This is a frontend-only enhancement; no Rust, no IPC changes.

## Decisions (locked during brainstorming)

| Question | Decision |
|---|---|
| Scope | **Full auto-reveal** (VS Code style): expand collapsed ancestor folders, scroll into view, highlight. Not just "highlight if already visible." |
| Trigger | **Active-tab change** (`setActiveTab` and the existing `:581` highlight call site). Not on live-reload or tree refresh. |
| Out-of-tree files | **No-op** — a file opened from outside the current tree root has no row; clear the selection and return. |
| Highlight | Keep the existing `.selected` background; **add a left accent bar** so the revealed file is unmistakable and distinct from hover. |
| Expansion sharing | Extract the "expand" half of `onDirClick` into `expandDir(li)` so click-expand and reveal-expand share one code path (DRY). |

**Out of scope (YAGNI):** a setting to disable auto-reveal; revealing on
live-reload / tree refresh; auto-collapsing folders the user didn't open.

---

## Why these decisions

**Reveal, not just highlight.** The tree is lazy — only expanded folders have
rows in the DOM. A highlight on a row that isn't rendered is invisible, so "see
which file it is" genuinely requires expanding ancestors and scrolling, not a
styling tweak. The user explicitly chose full auto-reveal over a styling-only
change.

**Pure ancestor computation.** The only real logic — turning `(root, filePath)`
into the ordered list of folders to expand — is pure and belongs in a unit-tested
helper (`treeops.js`), keeping the DOM walk in `app.js` thin.

**Share the expand path.** Reveal must expand folders exactly as a click does
(same lazy load, same cache-busting, same git decoration, same watch update).
Extracting `expandDir` from `onDirClick` guarantees that rather than duplicating
a subtly-different expand.

---

## Part 1 — Behavior

On every active-tab change, `revealInTree(path)` runs:

1. Clear `.selected` from any currently-selected row.
2. `treeAncestors(treeRoot, path)`:
   - `null` (file not under the tree root, or path is the root itself) → return
     (out-of-tree; nothing to reveal).
   - an array of ancestor directory paths (top-down) otherwise.
3. For each ancestor dir in order: find its `li[data-path]`; if its folder isn't
   open, `expandDir(li)` and await (its children — including the next ancestor's
   row — are now in the DOM).
4. Find the file's `li[data-path] > .row`; add `.selected`; `scrollIntoView({
   block: "nearest" })`.

Fired without blocking the preview render. Because each call clears `.selected`
before setting its own, the most recent active tab always wins under rapid
switching; in-flight expansions only add rows, so they're harmless.

## Part 2 — Architecture

### Pure helper — `ui/treeops.js`

```
treeAncestors(root, filePath) -> string[] | null
```

Returns the ancestor directory paths strictly between `root` and `filePath`,
top-down (`/r`, `/r/a/b/c.md` → `["/r/a", "/r/a/b"]`; a file directly in root →
`[]`; `filePath` not under `root`, or equal to `root` → `null`). DOM-free,
handles `/` and `\` separators and a trailing separator on `root`. Mirrors the
existing `relativeToRoot` containment logic.

### `app.js`

- **`expandDir(li)`** — extracted from `onDirClick`'s "open" branch: if the row
  is already open (`li :scope > ul` exists) or isn't a directory, no-op; else
  add `.open` to the row, `listDir(li.dataset.path)`, build the `<ul>` of
  `makeNode(child, depth + 1)` (depth from `li.dataset.depth`), append,
  `applyGitDecorations(ul)`, `updateTreeWatch()`. `onDirClick` is refactored so
  its open branch calls `expandDir(li)`.
- **`revealInTree(path)`** (async) — the Part 1 algorithm, using `treeAncestors`
  and `expandDir`. Replaces `highlightSelectedByPath`; called at the two sites
  that call it today (`setActiveTab` ~`:985`, and ~`:581`). `cssEscape` is
  reused for the `data-path` selector, as in the current code.

The current `highlightSelectedByPath` is removed (its body becomes step 1 + 4 of
`revealInTree`).

## Part 3 — Styling

`ui/styles.css`, the `.tree .row.selected` rule keeps its
`background: var(--sidebar-selected)` and gains a left accent bar:

```css
.tree .row.selected {
  background: var(--sidebar-selected);
  box-shadow: inset 2px 0 0 0 var(--accent, #0969da);
}
```

(`box-shadow` inset avoids any layout shift a border would cause.) A dark-mode
accent value is set if `--accent` isn't already theme-aware; verify in the smoke
test.

## Part 4 — Testing

`node --test` (`ui/treeops.test.js`, alongside the existing `validateName` test):

- `treeAncestors("/r", "/r/a/b/c.md")` → `["/r/a", "/r/a/b"]`
- `treeAncestors("/r", "/r/x.md")` → `[]`
- `treeAncestors("/r", "/other/x.md")` → `null`
- `treeAncestors("/r", "/r")` → `null`
- `treeAncestors("/r/", "/r/a/x.md")` → `["/r/a"]` (trailing-slash root)
- `treeAncestors("C:\\r", "C:\\r\\a\\x.md")` → `["C:\\r\\a"]` (Windows separators)

Manual GUI smoke test (the DOM expand/scroll/highlight can't be unit-tested):

- Collapse the tree. Open a file nested several folders deep, switch to a
  different tab, then back → the ancestor folders expand, the row scrolls into
  view, and shows the accent bar + selected background.
- Switch to a sibling tab → the selection moves to the correct row.
- Open a file from *Open File…* outside the current tree root → no crash; the
  selection clears (nothing highlighted).
- Single-click a deep tree file directly → still opens and ends up selected
  (reveal is idempotent on an already-visible row).
- Toggle dark mode → the accent bar and selected background are both legible.

## Build reminder

Frontend-only; still requires `cargo build` to rebundle (`frontendDist` is
compiled in). Smoke-test against this repo's own nested tree.
