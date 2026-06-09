# Review Mode: annotate a doc and send the review to Claude Code

**Date:** 2026-06-09
**Status:** Approved (brainstorming) — pending implementation plan

## Goal

Make mdviewer a better *review surface* for the Claude Code loop. Today the loop
is: Claude writes a plan/spec/report → you read it rendered in mdviewer → you
alt-tab to the terminal and re-type your reactions, re-describing *where* in the
doc you meant. The expensive part is that transcription.

Review Mode lets you attach short comments to blocks of a rendered markdown
document, then **Copy Review** assembles your comments — quoted in document
order, with the file path — into one clipboard block you paste into Claude Code.
No re-locating, no re-describing.

This is a pure-frontend feature: no Rust, no new IPC commands. It builds on the
`data-sourcepos` anchoring and the `postRender()` injection seam the app already
uses for task-lists, search-jump, and copy buttons.

## Decisions (locked during brainstorming)

| Question | Decision |
|---|---|
| Workflow | **Annotate & send back** — comment on the doc in-app, hand the structured feedback to Claude. |
| Delivery to Claude | **Clipboard.** mdviewer assembles a review block and copies it; you paste into any Claude Code session. No file drop, no direct injection. |
| Annotation granularity | **Whole blocks** (paragraph, list item, heading, code block), anchored by `data-sourcepos`. Plus one doc-wide **general note**. No arbitrary text-range selections. |
| Lifecycle | **Ephemeral, short-lived.** Comments live from "start reviewing" to "copied & sent." In-memory per tab; not persisted to disk. |
| Live-reload survival | Markers re-render from the in-memory model every `postRender`. On a *content* change, re-anchor by matching quoted block text; non-matches become **orphaned** (surfaced, not silently dropped). |
| Backend | **None.** Pure frontend, like the image-view feature. Clipboard via the browser API. |

**Out of scope (YAGNI):** disk persistence, arbitrary text-range selection,
direct injection into a running Claude session (MCP/hook/socket), a feedback
file on disk, review mode on Edit / Raw / image tabs.

---

## Why these decisions

**Block-level anchoring, not character ranges.** Claude *rewrites the doc* as it
acts on feedback, so any anchor tied to character offsets or scroll position
evaporates on the next live-reload. `data-sourcepos` blocks are the only anchor
model already proven to survive morphdom patches in this codebase.

**Short lifecycle removes the hard problems.** Comments are born when you start
reviewing and dead the moment you paste them and Claude acts. That single fact
eliminates disk persistence, long-term anchor stability, and any sync concern —
it's a scratchpad, not a database.

**Re-anchor by content, not line number.** After Claude inserts lines, "line 42"
is meaningless, but the text *"store bookmarks in localStorage"* is stable until
Claude edits that exact line — and if it does, that's signal your comment is
probably addressed. So a non-matching anchor becomes "orphaned" and is surfaced,
never silently re-attached to the wrong block.

---

## Part 1 — User-facing behavior

A new toolbar toggle **⊕ Review** sits beside Edit / Raw. It is available only on
markdown preview tabs — disabled/hidden for Edit, Raw, and image tabs (the same
gating pattern the Raw button uses today).

When Review Mode is on:

- **Hover any block** → a small **+** appears in the left gutter of that block.
- **Click the +** → an inline comment box opens just beneath the block. **Enter**
  saves, **Esc** cancels.
- An **annotated block** gets a subtle left-border highlight; its comment renders
  as a small card beneath the block (an in-place thread).
- A **general note** field at the top of the preview captures doc-wide feedback.
- Comments are **editable** (click the card to edit) and **deletable** (×).
- **Copy Review** assembles the clipboard block (Part 3) and clears the
  annotations (they're spent once pasted). The general note clears too.

Dark mode: the gutter +, the left-border highlight, and the comment cards all
need a dark-theme pass (attribute-driven CSS under `[data-theme="dark"]`, as the
rest of the app does).

## Part 2 — Architecture

State lives on the tab object, alongside `editBuffer` / `savedContent`:

- `tab.reviews` — array of `{ sourcepos, quotedText, comment }`.
- `tab.generalNote` — string.

Both ephemeral (in-memory); they die with the tab and are never written to disk.

Rendering the markers is a **new hook in the `postRender()` chain**,
`renderReviewMarkers()`, added after the existing hooks. It reads `tab.reviews`
and injects, for each entry: the highlight class on the matching
`[data-sourcepos]` block and the comment card beneath it; plus the gutter **+**
affordances when Review Mode is active. Because it runs on *every* morphdom
patch, markers survive live-reload automatically — the same mechanism that
re-adds copy buttons.

Anchoring uses `data-sourcepos` exactly as `hookTaskListCheckboxes` and the
search-jump do. Clicking a block's + records that block's `data-sourcepos` and
its trimmed text content as `quotedText`.

No Rust. Clipboard write uses `navigator.clipboard.writeText` (the app already
runs copy-to-clipboard for code blocks and Copy Source).

## Part 3 — Clipboard format

```
Review of docs/superpowers/specs/2026-06-09-folder-bookmarks-design.md

General note: This plan never says where bookmarks persist across launches.

---

> store bookmarks in localStorage keyed by path
↳ use recent.json, not localStorage — consistent with last_folder

> Wire the toolbar button before the command.
↳ this step is out of order; do it after the command wiring
```

- **Relative path** (relative to the sidebar root) in the header, so Claude
  resolves it the way you'd type it.
- **`>` blockquote** of the source text — markdown-native, greppable by Claude.
- **`↳`** prefix marks your comment, visually unambiguous.
- Comments emitted in **document order** (top-to-bottom by sourcepos), not click
  order — matches how Claude reads the file.
- Multi-line blocks (code fences, lists) are quoted as **first line + `…`** to
  stay scannable; path + first line is enough for Claude to locate the block.
- The general note (and any orphaned comments, Part 4) lead the block before the
  per-block comments.

## Part 4 — Surviving live-reload

Two cases on a `file-changed`/re-render:

1. **Passive reload** (content unchanged, or scroll): markers re-render from the
   model via `postRender` — nothing to solve.
2. **Content changed** (Claude or you rewrote the file): re-anchor each comment
   by matching its `quotedText` against the new blocks (trimmed compare). Matches
   re-attach at the new position. **Non-matches become orphaned**: they move into
   the general-note area with a muted "⚠ this block changed" tag — still
   copyable, never silently dropped or mis-attached.

## Part 5 — Code layout and testing

Follows the project's pure-helper + `node --test` convention (`editor.js`,
`treeops.js`, `export.js`):

New file **`ui/review.js`** — pure, unit-tested helpers:

- `formatReview(reviews, generalNote, relativePath, orphaned)` → the clipboard
  string (Part 3), comments sorted in document order.
- `reanchorReviews(reviews, newBlocks)` → `{ anchored, orphaned }` (Part 4).
- `quoteBlock(sourceText)` → first-line-plus-`…` truncation for the blockquote.

`app.js` does the impure wiring: the **⊕ Review** toolbar toggle, the gutter-+
and comment-card DOM injection inside `postRender`, the clipboard write, and the
edit/delete handlers — mirroring how `editor.js`'s pure helpers pair with
`app.js` wiring.

`styles.css` gets the gutter, highlight, and comment-card styles, with a
`[data-theme="dark"]` pass.

**Manual GUI smoke test before merge** — per the project memory note that
automated/subagent work misses theme and visual bugs. Verify the comment cards,
gutter +, and left-border highlight in both light and dark mode, and confirm
Copy Review produces paste-ready text.

## Build reminder

Frontend-only changes still require `cargo build` (Tauri bundles `frontendDist`
at compile time via `tauri-codegen`); editing `ui/*` without rebuilding shows
stale UI.
