# In-document search — design

**Date:** 2026-05-22
**Status:** Approved, pending implementation plan

## Goal

When a document is open, let the user search for text within it: a find bar with
match highlighting, next/previous navigation, a match count, and case-sensitive
and whole-word toggles. Scope is the **active document only**.

## Non-goals

- Searching across all open tabs (active document only).
- Regular-expression search (case-sensitive and whole-word toggles only).
- Searching the file tree or file contents on disk (this is in-view find, over
  the rendered/raw preview text).
- A global "search all files in folder" feature.

## Approach

### Highlighting: CSS Custom Highlight API

Matches are painted with the CSS Custom Highlight API: build `Range` objects over
the preview's text nodes and register them via `CSS.highlights.set(...)`, styled
with `::highlight(search-match)` / `::highlight(search-current)`.

This paints highlights **without mutating the DOM tree**, which is the deciding
factor in this codebase:

- `renderActive` diffs `#preview` with morphdom on every live reload; injected
  `<mark>` wrappers would fight the diff.
- Scroll anchoring reads element positions via `data-sourcepos`.
- mermaid/image preservation hooks compare nodes in `onBeforeElUpdated`.

Wrapping matches in `<mark>` (the traditional approach) would disturb all three
and require careful teardown. The Highlight API has zero DOM mutation, nothing to
tear down, and no CSP impact (pure JS + a stylesheet rule).

Alternatives considered and rejected:

- **`window.find()`** — hijacks the selection, no clean match count, styling is
  the OS selection color, fights our scroll anchoring. Too little control.
- **`<mark>` wrapping** — mutates the rendered DOM; conflicts with morphdom,
  sourcepos anchoring, and the mermaid/image hooks.

The API is available in the WKWebView on modern macOS (Safari 17.2+); the app is
macOS-only. The implementation still guards for `CSS.highlights` presence.

## Components and changes

Almost entirely frontend. The only Rust change is a menu item that reuses the
existing `edit-action` event channel.

### 1. Trigger — `menu.rs` + `app.js`

- `menu.rs`: add a **Find…** item with accelerator `CmdOrCtrl+F` to the *Actions*
  submenu, emitting `edit-action` `"find"` (same pattern as Copy / Toggle Raw).
  `⌘F` is currently unbound — no conflict.
- `app.js`: `runEditAction` gains a `"find"` case calling `openFind()`.

### 2. Find bar UI — `index.html` + `styles.css`

- A find-bar element, `hidden` by default, placed **inside `.preview-pane` but
  outside `#preview`** so morphdom never touches it. Anchored floating at the
  top-right of the preview area (below the tab bar).
- Contents: text input, match counter (`"3 / 12"`), previous/next buttons,
  case-sensitive toggle (**Aa**), whole-word toggle, and a close (×) button.
- Styled to match existing toolbar/context-menu chrome; light/dark aware.
- `.preview-pane` gets `position: relative` to anchor the absolutely-positioned
  bar; the bar is clipped by the pane's existing `overflow: hidden`.

### 3. Search engine — `app.js` (new self-contained section)

State: `{ query, caseSensitive, wholeWord, matches: Range[], currentIdx }`.

- **Collect text:** walk `#preview` text nodes with a `TreeWalker`, skipping
  `pre.mermaid` subtrees (don't search inside rendered SVG diagrams). Build one
  flat string plus an offset → `(node, offset)` map.
- **Find matches:** `indexOf` scan over the flat string (case-folded with
  `toLowerCase` when case-insensitive). Whole-word filters matches by Unicode
  word-boundary checks on the neighboring characters. **No regex**, so no pattern
  escaping or injection surface.
- The flat-string approach matches across inline-formatting boundaries (e.g. a
  phrase split by `**bold**`), since a `Range` can span multiple nodes.
- **Highlight:** each match → a `Range`. Register all matches as the
  `search-match` highlight and the active one as `search-current` (distinct
  color) via `CSS.highlights`. Scroll the current match into view using its
  `Range` rect, adjusting `previewScroll.scrollTop`.

### 4. Interactions and keys

- Enter = next, Shift+Enter = previous.
- `⌘G` / `⇧⌘G` = next / previous while the bar is open.
- `Esc` = close the bar and clear highlights.
- `⌘F` with the bar already open re-focuses and selects the input.
- Opening with a non-empty preview selection prefills the query (browser
  behavior).
- Find-as-you-type updates results live.
- No matches → `"0 results"` and a subtle red input state.

### 5. Coexistence with existing machinery

- **Live reload:** after `renderActive`, if the bar is open, re-run the search
  (old `Range`s are stale post-morph) and clamp `currentIdx`. Highlights repaint,
  the count updates.
- **Raw view:** the `<pre class="plain-text">` is a text node — search works
  there too; re-run on raw/rendered toggle.
- **Theme:** highlight colors are CSS, so dark/light switches need no JS.
- **Tabs:** switching tabs re-renders; reset the bar's match state for the new
  document (keep the query text so the same search re-runs).

## Error handling and edge cases

- Guard for `CSS.highlights` presence; if unavailable, the bar still opens but
  highlighting/navigation is inert (not expected on supported macOS).
- Empty query → clear highlights, blank/zeroed count, no scroll.
- `openFind()` is a no-op when no document is open (no active tab).

## Known limitation

Case-insensitive matching uses `toLowerCase`, which can mis-map offsets for a
handful of exotic Unicode characters that change length when lowercased — the
same edge case present in most simple find implementations. Accepted.

## Out of scope for backend

No new dependencies, no CSP changes, no Rust logic beyond the single menu item.
Frontend edits require the usual `cargo build` to re-bundle `frontendDist`.
