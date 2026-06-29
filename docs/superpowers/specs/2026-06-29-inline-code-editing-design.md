# In-place editing for code files + tab edit-state indicators

**Date:** 2026-06-29
**Status:** Approved

## Problem

Code/text files now render with a syntax-highlighted, line-numbered read view
(see `2026-06-29-code-file-rendering-design.md`). Editing them, however, still
opens the markdown-style **side-by-side split** (CodeMirror left, live preview
right), which feels heavy for code — there's no value in a separate preview pane
when the editor itself can show highlighted code. Users want to edit code *in
place*: the read view becomes an editor in the same pane.

Separately, a tab that is in edit mode and/or has unsaved changes should be
recognizable from the tab bar even when another tab is active. Today only the
unsaved state shows (a `●` dot); being in edit mode while clean has no marker.

## Goal

1. For code/text files, **Edit** enters an in-place, single-pane CodeMirror
   editor (no split, no preview), syntax-highlighted while typing, returning to
   the syntect read view on exit. Markdown keeps its split + live preview.
2. Make a tab's **editing** and **dirty** states visible on inactive tabs.

## Dependency

This builds directly on the **code-file-rendering** branch (the syntect read
view, `isCodeView`, code-view toolbar gating). It must be implemented on top of
that branch — either after it merges to `main`, or branched from it.

## Approach (chosen)

Reuse the single existing CodeMirror instance and the `.editor-pane` element;
add a layout branch (code → full-pane editor with `#preview` hidden; markdown →
existing split). Rejected: `contenteditable` on the syntect HTML with
re-highlighting (cursor/selection/re-highlight fragility); a second CodeMirror
instance (duplicate state/wiring).

## Design

### 1. Interaction & layout

Edit on a **code tab** (not markdown, not image) enters in-place editing:
`#preview` and the editor splitter are hidden; `editorPane` fills the pane. The
editor *is* the view, so there is **no live-preview render** while editing code
(no `render_preview` calls on the code path). **Done** (toolbar) or the menu
toggle exits to the syntect read view via `renderActive`. **⌘S** saves and stays
in the editor. **Markdown** tabs keep today's split + live preview unchanged.

A new layout flag drives this: `showEditorChrome(on, { inPlace })` — or an
equivalent branch — hides `#preview`/splitter and makes `editorPane` full-width
when `inPlace` is true (code), and uses the existing split layout otherwise
(markdown).

### 2. Editor mode + vendored modes

On `enterEditMode`, the shared `cm` instance gets per-file options:

- Markdown → `mode: "markdown"`, `lineWrapping: true` (unchanged).
- Code → CodeMirror mode chosen by extension, `lineWrapping: false` (horizontal
  scroll, matching the read view's `overflow-x`).
- Unknown extension → no mode (plain), still fully editable.

A pure helper `ui/editor-modes.js` maps an extension to a CodeMirror mode name
(`modeForPath(path) -> string | null`), unit-tested. A curated set of CodeMirror
5 modes is vendored under `ui/codemirror/` and loaded as classic `<script>`s in
`index.html`, respecting dependencies (`htmlmixed` needs `xml` + `javascript` +
`css`; load order matters):

- **javascript** (js, jsx, mjs, cjs, ts, tsx, json)
- **python** (py)
- **rust** (rs)
- **clike** (c, h, cpp, hpp, cc, java, cs)
- **css** (css, scss, less)
- **htmlmixed** (html, htm)
- **shell** (sh, bash, zsh)
- **yaml** (yml, yaml)
- **go** (go)
- **sql** (sql)
- plus the already-vendored **markdown** and **xml** (svg).

Adding a language later is a vendored file + one map entry. Unmapped extensions
edit as plain text (no error).

### 3. Theming

The in-place editor reuses the `.editor-pane` element, so the existing light/dark
CodeMirror CSS applies. The current dark overrides cover
`cm-string/keyword/comment/number/tag/link/url/header/quote`; extend them to the
extra token classes code modes emit so dark-mode code editing is fully colored:
`cm-def`, `cm-variable`, `cm-variable-2`, `cm-variable-3`, `cm-property`,
`cm-operator`, `cm-atom`, `cm-builtin`, `cm-meta`. The editor's CodeMirror
palette won't be pixel-identical to the syntect read-view palette (both are
highlighted) — an accepted cosmetic difference.

### 4. Save / dirty / conflict

Reused unchanged — all file-type agnostic: `read_source` → `save_file`
(read-verify-write), the `●` dirty dot, the discard-on-close prompt, and the
`classifyFileChange` conflict banner. Switching tabs mid-edit preserves each
tab's `editBuffer` (existing). Toggling theme mid-edit recolors live (CSS-only).

### 5. Tab edit-state indicators

In `makeTabEl`, add an `editing` class to a tab whose model has `editing: true`.
CSS tints `.tab.editing .tab-name` with the existing accent color (the same one
the active-tab underline uses). The unsaved state keeps the existing `●`
(`tab-dirty`) dot; since `dirty` implies `editing`, a dirty tab shows both the
accent name and the dot. Because `renderTabBar` rebuilds every tab from the
model, both states already render correctly on inactive tabs — no extra wiring
beyond the class + CSS. The accent must keep enough contrast against the active
tab's background in both themes.

### 6. Edge cases

- **Binary files:** `read_source` already fails on non-UTF-8 input and surfaces
  a transient error, so a binary tab cannot actually enter the editor today.
  This stays the guard; optionally the Edit button can be hidden once a tab is
  known to be binary, but that requires the frontend to learn binary-ness from
  the backend and is out of scope for this change.
- **Markdown unchanged:** only code/text tabs go in-place; markdown editing is
  byte-for-byte the current split behavior.
- **Review/Export gating** from the code-view branch is unaffected.

## Testing

- **JS unit (`ui/editor-modes.test.js`):** `modeForPath` — `.rs`→`rust`,
  `.tsx`/`.json`→`javascript`, `.py`→`python`, `.css`→`css`, `.md`→`markdown`,
  unknown (`.xyz`, extensionless)→`null`.
- **Manual:** edit `.rs`/`.py`/`.json`/`.css` in place (colored, single pane,
  no preview); an unknown `.xyz` (plain but editable); save then trigger an
  external change (conflict banner); toggle theme mid-edit; confirm markdown
  still opens the split + live preview; verify an inactive tab shows the accent
  name while editing and the `●` when dirty.

## Non-goals

- Click-/double-click-into-code to edit (Edit button only for now).
- In-place editing for markdown (keeps the split).
- Matching the editor's colors exactly to the syntect read view.
- Language modes beyond the curated set (extensible later).

## Files touched

- Create: `ui/editor-modes.js`, `ui/editor-modes.test.js`,
  `ui/codemirror/*.min.js` (vendored modes).
- Modify: `ui/index.html` (load mode scripts), `ui/app.js`
  (`enterEditMode`/`exitEditMode`/`showEditorChrome` layout + mode selection;
  `makeTabEl` editing class; skip live preview for code), `ui/styles.css`
  (in-place layout, dark token classes, `.tab.editing .tab-name`).
