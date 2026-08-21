# Retained Preview DOM Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make switching between open tabs instant by keeping each tab's rendered DOM alive and reattaching it, instead of repainting and re-running the whole post-render pipeline on every switch.

**Architecture:** The single live `#preview` element stays; its child nodes are moved into a per-tab `DocumentFragment` on the way out and moved back on the way in, held in a count-bounded LRU (`ui/domcache.js`). A reattach is synchronous — no `await` between click and pixels — and freshness is confirmed afterwards by calling `render_file` with the stored file stamp, repainting only if the file actually changed. Each tab also remembers its scroll offset.

**Tech Stack:** Vanilla ES modules (no build step), `node --test` for pure helpers, Tauri 2 IPC (`render_file` already accepts a `stamp` and answers `html: null` when unchanged), Rust integration test driving the GUI over the MCP socket.

**Spec:** `docs/superpowers/specs/2026-08-21-retained-preview-dom-design.md`

## Global Constraints

- Frontend edits are invisible until `cargo build` — Tauri bundles `frontendDist` at compile time.
- Pure logic goes in `ui/*.js` helper modules with a sibling `*.test.js`; DOM/IPC wiring stays thin in `app.js`.
- No new IPC commands, no CSP change, no Rust source changes (the `stamp` protocol already exists).
- `postRender()` remains the seam for anything that paints new HTML. The reattach path deliberately skips it — that work is already baked into the retained nodes.
- New per-tab state is ephemeral: `scrollTop` is never written to session restore (`persistSession` sends paths only, so this is automatic — do not add it there).
- Retained DOM is never kept for image tabs, editing tabs, or while an export is in flight.
- Lint/test gate before every commit, from `src-tauri/`: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`; and from the repo root: `node --test ui/*.test.js`.
- Commit style: imperative subject, no `Co-Authored-By` trailer.
- Work happens on the existing `perf/render-caching` branch.

---

### Task 1: `ui/domcache.js` — LRU and eligibility helpers

**Files:**
- Create: `ui/domcache.js`
- Test: `ui/domcache.test.js`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `class DomCache { constructor(maxEntries); get(path); set(path, value); delete(path); clear(); }` — `get` returns the value or `null` and refreshes recency; `set` evicts the least recently used entry when over capacity.
  - `canRetain({ tab, live, theme, exporting })` → boolean. `tab` is a tab object, `live` is the `{ path, raw, theme, fromDisk }` descriptor of what is currently painted (or `null`).
  - `entryUsable(entry, { raw, theme })` → boolean.

- [ ] **Step 1: Write the failing tests**

Create `ui/domcache.test.js`:

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { DomCache, canRetain, entryUsable } from "./domcache.js";

test("a stored entry comes back", () => {
  const c = new DomCache(2);
  c.set("/a.md", { tag: "a" });
  assert.deepEqual(c.get("/a.md"), { tag: "a" });
});

test("an unknown path misses", () => {
  const c = new DomCache(2);
  assert.equal(c.get("/nope.md"), null);
});

test("the least recently used entry is evicted at capacity", () => {
  const c = new DomCache(2);
  c.set("/a.md", { tag: "a" });
  c.set("/b.md", { tag: "b" });
  c.set("/c.md", { tag: "c" });
  assert.equal(c.get("/a.md"), null);
  assert.deepEqual(c.get("/b.md"), { tag: "b" });
  assert.deepEqual(c.get("/c.md"), { tag: "c" });
});

test("reading an entry makes it the most recently used", () => {
  const c = new DomCache(2);
  c.set("/a.md", { tag: "a" });
  c.set("/b.md", { tag: "b" });
  c.get("/a.md");
  c.set("/c.md", { tag: "c" });
  assert.equal(c.get("/b.md"), null);
  assert.deepEqual(c.get("/a.md"), { tag: "a" });
});

test("re-storing a path replaces its entry without consuming capacity", () => {
  const c = new DomCache(2);
  c.set("/a.md", { tag: "old" });
  c.set("/a.md", { tag: "new" });
  c.set("/b.md", { tag: "b" });
  assert.deepEqual(c.get("/a.md"), { tag: "new" });
  assert.deepEqual(c.get("/b.md"), { tag: "b" });
});

test("delete and clear drop entries", () => {
  const c = new DomCache(4);
  c.set("/a.md", { tag: "a" });
  c.set("/b.md", { tag: "b" });
  c.delete("/a.md");
  assert.equal(c.get("/a.md"), null);
  c.clear();
  assert.equal(c.get("/b.md"), null);
});

const TAB = { path: "/a.md", raw: false, editing: false };
const LIVE = { path: "/a.md", raw: false, theme: "light", fromDisk: true };

test("a plain rendered tab can be retained", () => {
  assert.equal(canRetain({ tab: TAB, live: LIVE, theme: "light", exporting: false }), true);
});

test("a tab is not retained when nothing was painted from disk", () => {
  assert.equal(canRetain({ tab: TAB, live: null, theme: "light", exporting: false }), false);
  assert.equal(
    canRetain({ tab: TAB, live: { ...LIVE, fromDisk: false }, theme: "light", exporting: false }),
    false,
  );
});

test("a tab is not retained when the painted view does not match it", () => {
  assert.equal(
    canRetain({ tab: TAB, live: { ...LIVE, path: "/other.md" }, theme: "light", exporting: false }),
    false,
  );
  assert.equal(
    canRetain({ tab: TAB, live: { ...LIVE, raw: true }, theme: "light", exporting: false }),
    false,
  );
  assert.equal(
    canRetain({ tab: TAB, live: LIVE, theme: "dark", exporting: false }),
    false,
  );
});

test("editing tabs and in-flight exports are never retained", () => {
  assert.equal(
    canRetain({ tab: { ...TAB, editing: true }, live: LIVE, theme: "light", exporting: false }),
    false,
  );
  assert.equal(canRetain({ tab: TAB, live: LIVE, theme: "light", exporting: true }), false);
});

test("an entry is usable only for the same raw mode and theme", () => {
  const entry = { fragment: {}, raw: false, theme: "light" };
  assert.equal(entryUsable(entry, { raw: false, theme: "light" }), true);
  assert.equal(entryUsable(entry, { raw: true, theme: "light" }), false);
  assert.equal(entryUsable(entry, { raw: false, theme: "dark" }), false);
  assert.equal(entryUsable(null, { raw: false, theme: "light" }), false);
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `node --test ui/domcache.test.js`
Expected: FAIL with `ERR_MODULE_NOT_FOUND` for `./domcache.js`.

- [ ] **Step 3: Write the implementation**

Create `ui/domcache.js`:

```js
// Per-tab retained render: the detached child nodes of #preview, so returning
// to a tab reattaches an already-painted document instead of repainting it.
// DOM-free — values are opaque, which is what keeps the policy unit-testable.
//
// Bounded by entry count rather than bytes: detached nodes have no cheap size
// measure, and the cost that matters (live listeners, SVG, KaTeX spans) tracks
// documents, not characters.

export class DomCache {
  constructor(maxEntries) {
    this.maxEntries = maxEntries;
    this.entries = new Map();
  }

  get(path) {
    const hit = this.entries.get(path);
    if (!hit) return null;
    this.entries.delete(path);
    this.entries.set(path, hit);
    return hit;
  }

  set(path, value) {
    this.entries.delete(path);
    this.entries.set(path, value);
    for (const key of this.entries.keys()) {
      if (this.entries.size <= this.maxEntries) break;
      this.entries.delete(key);
    }
  }

  delete(path) {
    this.entries.delete(path);
  }

  clear() {
    this.entries.clear();
  }
}

/** Whether what is currently painted may be kept for `tab`.
 *
 *  `live` describes what the preview actually shows — set by the paint path,
 *  cleared by the image/error/empty paths. Requiring it to match the tab is
 *  what stops a half-finished raw toggle, an editor-buffer preview, or an
 *  error panel from being retained as if it were the document. */
export function canRetain({ tab, live, theme, exporting }) {
  if (!tab || !live || exporting) return false;
  if (tab.editing) return false;
  if (!live.fromDisk) return false;
  return live.path === tab.path && live.raw === tab.raw && live.theme === theme;
}

/** Whether a stored entry can be reattached for the current view. */
export function entryUsable(entry, { raw, theme }) {
  if (!entry || !entry.fragment) return false;
  return entry.raw === raw && entry.theme === theme;
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `node --test ui/domcache.test.js`
Expected: PASS, 11 tests.

- [ ] **Step 5: Commit**

```bash
git add ui/domcache.js ui/domcache.test.js
git commit -m "Add DomCache LRU and retention-eligibility helpers"
```

---

### Task 2: Per-tab scroll position

**Files:**
- Modify: `ui/app.js` (tab creation sites, `setActiveTab`, `renderActive`, `paintHtml`)

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces:
  - A `scrollTop` field on every tab object, updated when a tab is switched away from.
  - `renderActive({ scrollLock, forceMermaid, scrollTo })` and `paintHtml(t, html, raw, { scrollLock, forceMermaid, scrollTo })` — `scrollTo` (default `0`) is the offset applied when there is no anchor to restore.

This task stands alone: after it, switching away from a tab and back returns you to where you were, with no caching involved.

- [ ] **Step 1: Add the field to every tab creation site**

In `ui/app.js` there are four literals that build a tab object. Add `scrollTop: 0` to each:

- in `openPreview`, the `tabs.push({ path, sticky: false, ... })` call;
- in `openSticky`, the `tabs.push({ path, sticky: true, ... })` call;
- in `restoreSession`, the `tabs.push({ path: p, sticky: true, ... })` call;
- in `openPreview`'s reuse branch, where the preview tab is reset field-by-field (`tabs[previewIdx].raw = false;` and friends), add `tabs[previewIdx].scrollTop = 0;` next to `tabs[previewIdx].pendingJumpLine = null;`.

- [ ] **Step 2: Record the offset when leaving a tab**

In `setActiveTab`, immediately after `const same = idx === activeIdx;`, insert:

```js
  if (!same) {
    const outgoing = activeTab();
    if (outgoing) outgoing.scrollTop = previewScroll.scrollTop;
  }
```

- [ ] **Step 3: Plumb `scrollTo` through the paint path**

In `paintHtml`, change the signature and the final scroll decision:

```js
async function paintHtml(t, html, raw, { scrollLock = true, forceMermaid = false, scrollTo = 0 } = {}) {
```

and replace

```js
  if (!hadPendingJump) {
    if (anchor) restoreAnchor(anchor);
    else previewScroll.scrollTop = 0;
  }
```

with

```js
  if (!hadPendingJump) {
    if (anchor) restoreAnchor(anchor);
    else previewScroll.scrollTop = scrollTo;
  }
```

In `renderActive`, accept and forward it:

```js
async function renderActive({ scrollLock = true, forceMermaid = false, scrollTo = 0 } = {}) {
```

and its final line becomes:

```js
  await paintHtml(t, html, result.raw, { scrollLock, forceMermaid, scrollTo });
```

Leave every other `renderActive` call site alone — the default of `0` preserves today's behavior for theme toggles, raw toggles and live reload.

- [ ] **Step 4: Use it when switching tabs**

In `setActiveTab`, in the non-editing branch, replace

```js
    await renderActive({ scrollLock: same && !forceRender });
```

with

```js
    await renderActive({
      scrollLock: same && !forceRender,
      scrollTo: same ? 0 : t.scrollTop || 0,
    });
```

- [ ] **Step 5: Verify by hand**

```bash
cd src-tauri && cargo build && ./target/debug/mdviewer ..
```

Open two markdown files long enough to scroll. Scroll down in the first, switch to the second, switch back: the first must return to where you left it. Scroll the second, switch away and back: same. Open a third file for the first time: it must start at the top.

- [ ] **Step 6: Run the gate and commit**

```bash
node --test ui/*.test.js
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --lib
cd .. && git add ui/app.js
git commit -m "Remember scroll position per tab"
```

---

### Task 3: Stash and reattach the rendered DOM

**Files:**
- Modify: `ui/app.js` (imports, module state, `paintHtml`, `renderImage`, `showError`, `showEmptyState`, `setActiveTab`)

**Interfaces:**
- Consumes: `DomCache`, `canRetain`, `entryUsable` from Task 1; `scrollTop` and `scrollTo` from Task 2.
- Produces:
  - `stashActiveTab(t)` — detaches the live nodes into the cache when eligible.
  - `restoreTabDom(t)` → `{ stamp }` when the preview now shows `t`'s retained render, else `null`.
  - `validateRestored(t, stamp)` — background freshness check, repaints on change.
  - `liveRender` — module-level `{ path, raw, theme, fromDisk }` describing what is painted, or `null`.

- [ ] **Step 1: Import and instantiate**

Next to the other helper imports in `ui/app.js` (after the `rendercache.js` import) add:

```js
import { DomCache, canRetain, entryUsable } from "./domcache.js";
```

Next to `const renderCache = new RenderCache(RENDER_CACHE_BYTES);` add:

```js
// Retained rendered DOM for the hottest tabs, so a revisit reattaches nodes
// that are already painted, highlighted and diagrammed instead of repainting.
const DOM_CACHE_ENTRIES = 8;
const domCache = new DomCache(DOM_CACHE_ENTRIES);
// What #preview currently shows. Gates retention: only a disk render of the
// active tab in its current raw mode and theme may be kept.
let liveRender = null;
let validateSeq = 0;
```

- [ ] **Step 2: Track what is painted**

In `paintHtml`, add a `fromDisk` option and record the descriptor. The signature becomes:

```js
async function paintHtml(t, html, raw, { scrollLock = true, forceMermaid = false, scrollTo = 0, fromDisk = false } = {}) {
```

Immediately after the `window.morphdom(preview, incoming, { ... })` call, add:

```js
  liveRender = fromDisk
    ? { path: t.path, raw, theme: currentTheme, fromDisk: true }
    : null;
```

In `renderActive`, pass it on the single `paintHtml` call:

```js
  await paintHtml(t, html, result.raw, { scrollLock, forceMermaid, scrollTo, fromDisk: true });
```

Leave `renderFromEditor`'s `paintHtml` call as it is — the default `fromDisk: false` is what keeps editor-buffer previews out of the cache.

Then clear the descriptor on the three paths that put something other than a document in the preview. Add `liveRender = null;` as the last statement of `showEmptyState()`, of `showError(msg)`, and of `renderImage(t, ...)`.

- [ ] **Step 3: Write the stash and restore helpers**

Add these directly above `async function setActiveTab(`:

```js
/** Detach the active tab's rendered nodes into the DOM cache, so returning to
 *  it can reattach them instead of repainting. The stamp comes from the HTML
 *  cache entry that produced this render; without one (an uncacheable render,
 *  or an evicted entry) the retained DOM is still reattached, it just always
 *  revalidates with a full render. */
function stashActiveTab(t) {
  if (!canRetain({ tab: t, live: liveRender, theme: currentTheme, exporting: exportInProgress })) {
    domCache.delete(t.path);
    return;
  }
  const cached = renderCache.get(t.path, currentTheme, t.raw);
  const fragment = document.createDocumentFragment();
  fragment.append(...preview.childNodes);
  domCache.set(t.path, {
    fragment,
    className: preview.className,
    stamp: cached ? cached.stamp : null,
    raw: t.raw,
    theme: currentTheme,
  });
  clearFindHighlights();
  liveRender = null;
}

/** Reattach `t`'s retained render. Returns the stamp to revalidate against, or
 *  null when there was nothing usable to reattach. */
function restoreTabDom(t) {
  const entry = domCache.get(t.path);
  if (!entryUsable(entry, { raw: t.raw, theme: currentTheme })) return null;
  previewEmpty.hidden = true;
  preview.hidden = false;
  preview.className = entry.className;
  preview.replaceChildren(entry.fragment);
  previewScroll.scrollTop = t.scrollTop || 0;
  // replaceChildren MOVES the nodes out of the fragment, leaving it empty — the
  // entry is spent, and the nodes are live again until the next stash.
  domCache.delete(t.path);
  liveRender = { path: t.path, raw: t.raw, theme: currentTheme, fromDisk: true };
  if (findOpen()) runFind({ keepCurrent: true, scroll: false });
  return { stamp: entry.stamp };
}

/** Confirm a reattached render still matches disk, repainting if it doesn't.
 *  Runs after the paint, so the switch itself never waits on IPC. */
async function validateRestored(t, stamp) {
  const token = ++validateSeq;
  let result;
  try {
    result = await invoke("render_file", {
      path: t.path,
      theme: currentTheme,
      raw: t.raw,
      stamp,
    });
  } catch (e) {
    // The retained nodes are still the last known-good render; keep them on
    // screen rather than blanking the preview the way a cold failure does.
    console.error("render_file (revalidation) failed", e);
    showTransientError(String(e));
    return;
  }
  if (token !== validateSeq || activeTab() !== t) return;
  if (result.html == null) return; // the retained render was current
  if (result.stamp) {
    renderCache.set(t.path, currentTheme, t.raw, result.html, result.stamp);
  }
  await paintHtml(t, result.html, result.raw, { scrollLock: true, fromDisk: true });
}
```

- [ ] **Step 4: Wire the fast path into `setActiveTab`**

Two edits inside `setActiveTab`.

First, extend the block added in Task 2 so it stashes as well as records scroll:

```js
  if (!same) {
    const outgoing = activeTab();
    if (outgoing) {
      outgoing.scrollTop = previewScroll.scrollTop;
      stashActiveTab(outgoing);
    }
  }
```

Second, replace the non-editing branch. It currently reads:

```js
  } else {
    showEditorChrome(false);
    await renderActive({
      scrollLock: same && !forceRender,
      scrollTo: same ? 0 : t.scrollTop || 0,
    });
  }
```

Make it:

```js
  } else {
    showEditorChrome(false);
    if (restored) {
      validateRestored(t, restored.stamp);
    } else {
      await renderActive({
        scrollLock: same && !forceRender,
        scrollTo: same ? 0 : t.scrollTop || 0,
      });
    }
  }
```

and compute `restored` *before* the `await invoke("open_file", ...)` that sits above it, so the paint happens with no await in front of it. Move the existing `const t = tabs[idx];` (currently just below that await) up to immediately after `revealInTree(tabs[idx].path);`, delete it from its old position, and follow it with:

```js
  let restored = null;
  if (!forceRender && !same && !t.editing && !isImagePath(t.path)) {
    // Settle the chrome before painting: the outgoing tab may have been in the
    // split editor, and the restored document must not flash beside it.
    showEditorChrome(false);
    restored = restoreTabDom(t);
  }
```

The later `showEditorChrome(false)` in the non-editing branch stays where it is — it is idempotent and still needed on the miss path.

Note that `restoreTabDom` must not run for a `forceRender` switch: those callers (`openTabAtLine`, the MCP open path, the review path) require a fresh render because they set `pendingJumpLine` or reset review state.

- [ ] **Step 5: Rebuild and verify by hand**

```bash
cd src-tauri && cargo build && ./target/debug/mdviewer ..
```

Check each of these:
- Open a file with a Mermaid diagram and one with math. Switch back and forth: the diagram and the formulas must appear instantly and must not flicker or re-render.
- Copy buttons on code blocks still work after switching away and back (proves the retained listeners came with the nodes).
- Enter Review Mode, add a comment, switch tabs, switch back: the comment card is still there.
- Toggle Raw on a tab, switch away, come back: you get the raw view, not the rendered one.
- Toggle the theme with several tabs open, then visit each: every tab is in the new theme.
- Edit a file in another editor while its tab is in the background, then switch to it: it shows the old content for an instant and then corrects itself.

- [ ] **Step 6: Run the gate and commit**

```bash
node --test ui/*.test.js
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --lib
cd .. && git add ui/app.js
git commit -m "Reattach retained preview DOM when switching tabs"
```

---

### Task 4: Invalidation

**Files:**
- Modify: `ui/app.js` (`file-changed` listener, `applyTheme`, `closeTab`, `retargetTabsForRename`, `closeTabsUnder`, `exportDocument`)

**Interfaces:**
- Consumes: `domCache` from Task 3.
- Produces: no new symbols. This task makes retention safe against the events that make a retained render wrong or leak it.

- [ ] **Step 1: Drop the entry when the file changes**

In the `file-changed` listener, next to the existing `renderCache.deleteByPath(ev.payload);` add:

```js
    domCache.delete(ev.payload);
```

- [ ] **Step 2: Clear everything on a theme change**

Syntect bakes its colors into inline `style=` attributes, so every retained render belongs to the theme it was painted in. In `applyTheme`, immediately after `currentTheme = theme;` add:

```js
  domCache.clear();
```

- [ ] **Step 3: Evict on close**

In `closeTab`, immediately before `tabs.splice(idx, 1);` add:

```js
  domCache.delete(t.path);
```

- [ ] **Step 4: Clear on rename and on delete**

Both operations can move or remove a whole subtree of paths, and both are rare enough that clearing beats tracking prefixes. In `retargetTabsForRename`, as the first statement of the function body, add:

```js
  domCache.clear();
```

In `closeTabsUnder`, as the first statement of the function body, add:

```js
  domCache.clear();
```

Raw mode needs no step of its own: an entry records the `raw` it was painted in
and `entryUsable` rejects a mismatch, so toggling Raw is a miss rather than a
wrong repaint.

- [ ] **Step 5: Never cache a mid-export preview**

`exportDocument` mutates the live preview during capture — light re-render, Mermaid config swap, heading wraps, table scaling, image neutralization — and restores by re-rendering in its `finally`. `canRetain`'s `exporting` check covers a switch during the export; this covers an export that throws before its restore completes. In `exportDocument`'s `finally` block, after the call to `restoreViewState(t, snap)`, add:

```js
    domCache.delete(t.path);
```

- [ ] **Step 6: Rebuild and verify by hand**

```bash
cd src-tauri && cargo build && ./target/debug/mdviewer ..
```

- Export a document to HTML, then switch away and back: the tab shows the document, not export leftovers (no wrapped headings, no scaled tables).
- Rename a file that has an open tab, then switch away and back: the tab renders the renamed file.
- Delete a file with an open tab: the tab closes and no stale render survives for a later file of the same name.

- [ ] **Step 7: Run the gate and commit**

```bash
node --test ui/*.test.js
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --lib
cd .. && git add ui/app.js
git commit -m "Invalidate retained DOM on theme, close, rename, delete and export"
```

---

### Task 5: End-to-end smoke test and documentation

**Files:**
- Create: `src-tauri/tests/tab_switch_smoke.rs`
- Modify: `CLAUDE.md`

**Interfaces:**
- Consumes: the shipped behavior from Tasks 2–4.
- Produces: `cargo test --test tab_switch_smoke -- --ignored` — an opt-in macOS test that drives a real GUI over the MCP socket.

The test mirrors `src-tauri/tests/launch_smoke.rs` (read it first), with two differences: it drives the **debug** binary so it needs no bundle build, and it works in a temp workspace it creates, because it has to modify a file mid-test.

- [ ] **Step 1: Write the failing test**

Create `src-tauri/tests/tab_switch_smoke.rs`:

```rust
//! Tab-switching smoke test (macOS, `--ignored`). Drives a real GUI over the
//! MCP socket to prove that a revisited tab still paints real content, and that
//! a file edited while its tab was in the background is corrected by the
//! background revalidation. Run with:
//! `cargo test --test tab_switch_smoke -- --ignored --nocapture`
#![cfg(target_os = "macos")]

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::Stream;
use serde_json::{json, Value};

const READY_TIMEOUT: Duration = Duration::from_secs(40);
const CALL_TIMEOUT: Duration = Duration::from_secs(90);

struct Gui(Child);

impl Drop for Gui {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "launches the GUI; run manually via cargo test --test tab_switch_smoke -- --ignored"]
fn revisited_tab_paints_and_revalidates() {
    let bin = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/mdviewer");
    if !bin.exists() {
        println!("SKIP: {} not built (run `cargo build`)", bin.display());
        return;
    }

    let ws = std::env::temp_dir().join(format!("mdv-tabswitch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).unwrap();
    let a = ws.join("a.md");
    let b = ws.join("b.md");
    std::fs::write(&a, "# Alpha\n\nAlpha prose.\n\n```rust\nfn main() {}\n```\n").unwrap();
    std::fs::write(&b, "# Beta\n\nBeta prose.\n").unwrap();

    // AF_UNIX paths are capped near 104 bytes, so keep this short.
    let sock = format!("/tmp/mdv-tabswitch-{}.sock", std::process::id());
    let _ = std::fs::remove_file(&sock);
    std::env::set_var("MDVIEWER_MCP_SOCKET", &sock);

    let _gui = Gui(Command::new(&bin)
        .arg(&ws)
        .env("MDVIEWER_MCP_SOCKET", &sock)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("launch the debug binary"));

    wait_until_ready();

    call("open_document", json!({ "path": a.to_string_lossy() }));
    call("open_document", json!({ "path": b.to_string_lossy() }));
    call("open_document", json!({ "path": a.to_string_lossy() }));

    let first_pdf = ws.join("revisit.pdf");
    call(
        "generate_pdf",
        json!({ "path": a.to_string_lossy(), "output": first_pdf.to_string_lossy() }),
    );
    let first = std::fs::metadata(&first_pdf).expect("first pdf written").len();
    assert!(
        first > 3000,
        "a revisited tab produced a {first}-byte PDF — the reattached DOM is empty"
    );

    // Grow the file while its tab is active-but-retained, then revisit it. The
    // background revalidation must replace the retained render.
    let mut grown = std::fs::read_to_string(&a).unwrap();
    for _ in 0..30 {
        grown.push_str("\n\n## Appended\n\nMore prose to make the document longer.\n");
    }
    std::fs::write(&a, grown).unwrap();
    call("open_document", json!({ "path": b.to_string_lossy() }));
    call("open_document", json!({ "path": a.to_string_lossy() }));

    let second_pdf = ws.join("after-edit.pdf");
    call(
        "generate_pdf",
        json!({ "path": a.to_string_lossy(), "output": second_pdf.to_string_lossy() }),
    );
    let second = std::fs::metadata(&second_pdf).expect("second pdf written").len();
    assert!(
        second > first,
        "edited file still rendered at the old size ({first} -> {second}) — stale retained DOM"
    );

    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_file(&sock);
}

fn wait_until_ready() {
    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        if request("get_viewer_state", json!({})).is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(250));
    }
    panic!("the GUI never answered get_viewer_state within {READY_TIMEOUT:?}");
}

fn call(tool: &str, args: Value) -> String {
    let deadline = Instant::now() + CALL_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(result) = request(tool, args.clone()) {
            return result;
        }
        thread::sleep(Duration::from_millis(250));
    }
    panic!("{tool} never succeeded within {CALL_TIMEOUT:?}");
}

fn request(tool: &str, args: Value) -> Option<String> {
    let name = mdviewer_lib::mcp::socket_name().ok()?;
    let stream = Stream::connect(name.borrow()).ok()?;
    let req = mdviewer_lib::mcp::GuiRequest {
        id: 1,
        tool: tool.into(),
        args,
    };
    let mut line = serde_json::to_string(&req).ok()?;
    line.push('\n');
    (&stream).write_all(line.as_bytes()).ok()?;
    let mut reader = BufReader::new(&stream);
    let mut resp = String::new();
    reader.read_line(&mut resp).ok()?;
    let reply: mdviewer_lib::mcp::GuiReply = serde_json::from_str(resp.trim()).ok()?;
    reply.result
}
```

- [ ] **Step 2: Run it against the pre-change build to confirm it is a real test**

```bash
# c42929f is the commit before this feature started (the render-cache change).
git checkout c42929f -- ui/app.js
cd src-tauri && cargo build && cargo test --test tab_switch_smoke -- --ignored --nocapture
cd .. && git checkout HEAD -- ui/app.js
cd src-tauri && cargo build
```

Expected: PASS (the behavior it asserts — a revisited tab shows correct, fresh content — is also true before the change). This step is a control: it proves the test is not vacuously passing on a broken build. If it fails here, the test itself is wrong; fix it before continuing.

- [ ] **Step 3: Run it against the new build**

```bash
cd src-tauri && cargo build && cargo test --test tab_switch_smoke -- --ignored --nocapture
```

Expected: PASS, both assertions.

- [ ] **Step 4: Document the design in CLAUDE.md**

In the "Architecture quick-tour" list, immediately after the `**Render cache**` bullet added by the previous change, insert:

```markdown
- **Retained preview DOM**: switching tabs stashes `#preview`'s child nodes into
  a per-tab `DocumentFragment` (`ui/domcache.js`, 8-entry LRU) and reattaches
  them on return — synchronously, so there is no `await` between the click and
  the pixels, and `postRender` is skipped entirely because its work is already
  in those nodes. Freshness is confirmed *after* the paint by `validateRestored`
  calling `render_file` with the stored stamp; `html: null` means the retained
  render was current, anything else repaints. `liveRender` (set by `paintHtml`,
  cleared by `showError`/`showEmptyState`/`renderImage`) is what makes retention
  safe: only a disk render of the active tab in its current raw mode and theme
  is eligible, so editor-buffer previews and error panels are never retained.
  Entries are dropped on `file-changed`, theme toggle (syntect colors are baked
  into the HTML), tab close, rename, delete and after an export.
  `replaceChildren` MOVES nodes out of the fragment, so a restored entry is
  deleted, not reused. Each tab also keeps its own `scrollTop`.
```

In the file-layout block, next to the `rendercache.js` entry, add:

```
  domcache.js     — DomCache (LRU of retained preview DOM) + canRetain /
                    entryUsable eligibility helpers; unit-tested
```

In the "Build / develop / release" section, under the smoke-test command, add:

```markdown
# tab-switching smoke test (macOS, needs `cargo build` first): proves a
# revisited tab still paints and that a background edit is picked up
cargo test --test tab_switch_smoke -- --ignored --nocapture
```

- [ ] **Step 5: Run the full gate and commit**

```bash
node --test ui/*.test.js
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --lib
cd .. && git add src-tauri/tests/tab_switch_smoke.rs CLAUDE.md
git commit -m "Add tab-switching smoke test and document retained preview DOM"
```

---

## Verification summary

| Task | Checked by |
|---|---|
| 1 | `node --test ui/domcache.test.js` (11 unit tests) |
| 2 | Manual scroll round-trip + full gate |
| 3 | Manual matrix (Mermaid, math, copy buttons, review cards, raw, theme, background edit) + full gate |
| 4 | Manual export/rename/delete round-trip + full gate |
| 5 | `cargo test --test tab_switch_smoke -- --ignored`, run against both the old and new build |

Full gate, run before every commit: `node --test ui/*.test.js` from the repo root; `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --lib` from `src-tauri/`. Remember `cargo build` before any manual check — the frontend is bundled at compile time.
