# Retained preview DOM: make tab switching instant

**Date:** 2026-08-21
**Status:** Approved (brainstorming) — pending implementation plan

## Goal

Switching tabs should feel like switching tabs, not like opening a file.

Today every switch rebuilds the document from nothing. `setActiveTab` awaits
`render_file`, morphs the result into `#preview`, and then re-runs the whole
`postRender()` pipeline — link annotation, image resolution, copy buttons, task
hooks, KaTeX, Mermaid (one diagram at a time, awaited in a loop), review
markers. Returning to a document you read ten seconds ago costs exactly as much
as opening it cold, and the cost is paid again on the way back.

The preceding change (commit `c42929f`) removed the *render* from that path: the
backend now answers `html: None` when the caller's file stamp still matches, so
a revisit skips comrak and syntect and the HTML round-trip. What remains is the
expensive half — painting the DOM and re-running the post-render passes.

This design keeps the rendered DOM alive per tab, so a revisit reattaches nodes
that are already painted, already highlighted, already diagrammed. It also gives
each tab its own scroll position, which the current reset-to-top behavior does
not.

Pure frontend: no Rust changes, no new IPC commands. It builds on the
`file_stamp` protocol `render_file` already speaks.

## Decisions (locked during brainstorming)

| Question | Decision |
|---|---|
| Where retained DOM lives | **One live `#preview`, children stashed per tab** in a `DocumentFragment`. Not one `<article>` per tab. |
| Freshness on revisit | **Optimistic paint, validate after.** Reattach synchronously, then check the stamp in the background and re-render only if the file actually changed. |
| Scroll | **Per-tab scroll restore**, recorded on the tab object so it works on a cache miss too. |
| Capacity | **Count-bounded LRU, 8 entries.** |
| Excluded tabs | Image tabs and tabs in edit mode. |
| Invalidation | `file-changed` for that path, theme toggle, raw toggle, tab close/rename, and after any export or PDF run. |
| Backend | **None.** The `stamp` parameter on `render_file` already exists. |

**Out of scope (YAGNI):** retaining DOM for image or editing tabs, persisting
anything across launches, a byte-accurate memory budget for detached nodes,
lazy/off-screen rendering of Mermaid (a separate change), and any rework of the
tree-refresh amplification (option #4 from the performance review).

---

## Why these decisions

**Stash children, don't multiply articles.** `#preview` is captured once as a
module-level `const` and used in roughly fifty places — export, PDF capture,
review markers, find, Mermaid, math, link and image rewriting, click delegation.
Giving each tab its own `<article>` would duplicate an `id`, force all those
call sites through an accessor, and invalidate the CSS and `@media print` rules
keyed on `#preview`. Moving child nodes in and out of the single live element
gets the same win with none of that: the live element always holds the active
tab's nodes, so every existing call site stays correct without being touched.

**Optimistic paint is the whole point.** Awaiting a stamp check before painting
would still make a switch wait on an IPC round-trip; the retained DOM would save
the paint but not the latency. Painting first and validating after makes the
switch synchronous — zero awaits between click and pixels. The exposure is
narrow and bounded: it only shows anything wrong when the file changed while its
tab sat in the background, and only until the validation returns (~one round
trip), after which the view corrects itself.

**Validation cannot be event-driven.** `WatcherSlot` holds a single watcher and
rewires it to the active file on every switch, so a background tab produces no
`file-changed` event at all. The stamp check is what makes background edits
visible; without it, retained DOM would be stale-by-construction for exactly the
case the user is most likely to hit (editing a file in another app while its tab
is open here).

**Theme invalidates everything.** Syntect writes its colors into inline `style=`
attributes at render time, so retained nodes are theme-specific. The existing
HTML cache handles this by keying on theme; the DOM cache has no such luxury
(there is one live element), so a theme toggle drops all entries.

**Export must not poison the cache.** `exportDocument` mutates the live preview
during capture — forcing a light render, swapping Mermaid configs, wrapping
headings, scaling tables, neutralizing out-of-workspace images — and restores by
re-rendering in a `finally` block. Whatever is in the element mid-export must
never become a cache entry.

## Architecture

### New module: `ui/domcache.js`

A count-bounded LRU whose values are opaque, following the split the codebase
already uses (`rendercache.js`, `gitstatus.js`, `treeops.js`): the policy is
pure and unit-tested, the DOM handling stays in `app.js`.

```
class DomCache {
  constructor(maxEntries)
  get(path)            // → value | null, refreshes recency
  set(path, value)     // evicts LRU when over capacity
  delete(path)
  clear()
  has(path)
}
```

Because values are opaque, the tests exercise eviction order, recency refresh,
and invalidation with plain objects — no DOM required under `node --test`.

The value stored per tab is:

```
{ fragment, className, stamp, raw, theme }
```

`fragment` is a `DocumentFragment` holding the detached children; `className`
carries the `markdown-body` / `raw-body` / `code-body` variant; `stamp` is what
gets handed back to `render_file` for validation; `raw` and `theme` are recorded
so a mismatch is treated as a miss rather than a wrong repaint.

### Tab model

One new per-tab field on `tabs[]`: `scrollTop`. The retained DOM itself is keyed
by path in the `DomCache` rather than hung off the tab object, so closing a tab
is one `delete` rather than a field to forget. `scrollTop` is **ephemeral**:
excluded from session restore, like Review Mode state.

### Flow

`setActiveTab(idx)` gains a fast path:

1. **Stash the outgoing tab** — record `previewScroll.scrollTop` on the tab,
   clear find highlights, and if the tab is eligible (not image, not editing, no
   export in flight) move `#preview`'s children into a fragment and store it.
2. **Restore the incoming tab** — on a valid entry (path, `raw` and `theme` all
   match), `replaceChildren(fragment)`, restore `className` and `scrollTop`, and
   return. No `await` anywhere on this path.
3. **Validate in the background** — call `render_file` with the stored stamp. A
   `null` html means the retained DOM was correct and nothing happens. Otherwise
   paint the fresh HTML through the existing `paintHtml` path, which re-runs
   `postRender` as usual.

A miss falls through to today's `renderActive` unchanged, with the tab's stored
`scrollTop` restored instead of resetting to top.

### Interaction with the HTML render cache

Three tiers, cheapest first: retained DOM (8 hottest tabs, no paint) → cached
HTML (32 MB, no render) → backend render. They invalidate independently and
neither depends on the other.

## Error handling

**Stale validation.** The background check must not paint into a tab the user
has since left. It carries a sequence token, checked after the await against the
active tab's path — the same guard `revealInTree` already uses (`revealSeq`).

**Validation failure.** A cold render error blanks the preview via `showError`.
A background validation failure must not: the retained content is still the last
known-good render, so the failure surfaces through `showTransientError` and the
document stays on screen.

**Find highlights.** `CSS.highlights` is document-global and its ranges point
into nodes that stashing detaches. Highlights are cleared when a tab is stashed
and `runFind` re-runs on restore, matching how `paintHtml` already re-runs find
after a repaint.

**Listener retention.** Detached fragments keep their listeners alive — copy
buttons, task checkboxes, review cards. That is the point of retaining them, and
it is also why eviction on close, rename and delete matters: without it, a long
session accumulates live DOM for files that no longer have tabs.

## Testing

- **Unit (`node --test`):** `ui/domcache.test.js` covers eviction order, recency
  refresh on read, capacity, delete, and clear.
- **End-to-end:** extend the throwaway MCP-socket harness used to verify the
  render cache — A→B→A followed by `generate_pdf` proves the retained DOM paints
  real content, and editing a file while its tab is backgrounded then switching
  back proves the optimistic validation corrects itself.
- **Manual:** Mermaid and KaTeX survive a round trip, review markers survive a
  round trip, find works after switching, HTML and PDF export are unaffected,
  theme toggle repaints every tab correctly.
- **Gate:** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`, `node --test ui/*.test.js`.

## Constraints carried in from CLAUDE.md

- **`cargo build` after frontend edits** — Tauri bundles `frontendDist` at
  compile time, so `ui/*` changes are invisible until rebuilt.
- **`postRender()` stays the seam** — the fast path deliberately *skips*
  `postRender` (its work is already in the retained nodes); every path that
  paints new HTML still runs it in the documented order.
- **`[hidden]` vs flex** — `#preview` toggling stays as-is; nothing in this
  change adds a flex element toggled via `.hidden`.
- **No CSP change, no new IPC** — no new attack surface. The untrusted-content
  posture is unchanged: the retained nodes are the same nodes comrak already
  escaped and morphdom already inserted.
- **Cross-platform** — pure frontend, no `cfg`-gated behavior, identical on
  macOS and Windows.
