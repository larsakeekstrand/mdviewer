# Review Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Review Mode that lets the user annotate blocks of a rendered markdown doc and copy a structured review (quoted passages + comments, in document order, with the file path) to the clipboard for pasting into Claude Code.

**Architecture:** A new pure-helper module `ui/review.js` (DOM-free, unit-tested under `node --test`, mirroring `ui/editor.js` / `ui/treeops.js` / `ui/export.js`) holds the clipboard formatter, the re-anchor-by-text algorithm, and the quote truncator. The impure wiring lives in `ui/app.js`: a per-tab `reviewMode` toggle, a `renderReviewMarkers()` hook added to the existing `postRender()` chain (so markers survive live-reload exactly like the copy buttons), and a Copy Review action. State (`reviews`, `generalNote`, `orphanedReviews`) lives on the tab object and is ephemeral. No Rust, no new IPC, no session-restore persistence.

**Tech Stack:** Vanilla ES modules (no build step for JS), `node --test` for JS unit tests, Tauri 2 (only the existing `cargo build` to rebundle the frontend). Anchoring via comrak's `data-sourcepos`. Clipboard via `navigator.clipboard` (already wrapped by `copyText`).

---

## File structure

- **Create `ui/review.js`** — pure helpers: `quoteBlock`, `formatReview`, `reanchorReviews`. No DOM, no Tauri.
- **Create `ui/review.test.js`** — `node --test` unit tests for the three helpers.
- **Modify `ui/index.html`** — add the `⊕ Review` toolbar button.
- **Modify `ui/app.js`** — import the helpers; add tab-model fields; toolbar wiring + gating; `renderReviewMarkers()` and its DOM helpers; hook into `postRender()`; the Copy Review action and review bar.
- **Modify `ui/styles.css`** — gutter `+`, reviewed-block highlight, comment card, comment input, review bar; light + dark.

## Conventions to follow

- JS tests run with `node --test ui/*.test.js` (see `.github/workflows/ci.yml:50`). Run one file with `node --test ui/review.test.js`.
- `ui/review.js` must have **no** `import` of DOM or Tauri — it has to load under bare Node, like `ui/editor.js`.
- Frontend edits require `cd src-tauri && cargo build` to show up in the app (Tauri bundles `frontendDist` at compile time). The JS unit tests do **not** need a build.
- Commit after every task. No `Co-Authored-By` trailer (project convention).

---

## Task 1: `quoteBlock` — truncate a block's text for the clipboard quote

**Files:**
- Create: `ui/review.js`
- Create: `ui/review.test.js`

- [ ] **Step 1: Write the failing test**

Create `ui/review.test.js`:

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { quoteBlock } from "./review.js";

test("quoteBlock returns short text unchanged, trimmed", () => {
  assert.equal(quoteBlock("  store bookmarks in localStorage  "), "store bookmarks in localStorage");
});

test("quoteBlock collapses internal whitespace and newlines to single spaces", () => {
  assert.equal(quoteBlock("line one\n   line two"), "line one line two");
});

test("quoteBlock truncates long text with a trailing ellipsis", () => {
  const long = "a".repeat(100);
  const out = quoteBlock(long, 80);
  assert.equal(out.length, 82); // 80 chars + " …"
  assert.ok(out.endsWith(" …"));
});

test("quoteBlock handles empty/undefined input", () => {
  assert.equal(quoteBlock(""), "");
  assert.equal(quoteBlock(undefined), "");
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test ui/review.test.js`
Expected: FAIL — `Cannot find module './review.js'` (or `quoteBlock is not a function`).

- [ ] **Step 3: Write minimal implementation**

Create `ui/review.js`:

```js
// Pure helpers for Review Mode. DOM-free + Tauri-free so they unit-test under
// `node --test`; the annotation UI and clipboard wiring live in app.js.

/** Collapse a block's text to one trimmed line, truncating long text with " …".
 *  Used both for the clipboard blockquote and (via the same normalization) as
 *  the stable key for re-anchoring across re-renders. */
export function quoteBlock(sourceText, max = 80) {
  const s = (sourceText || "").trim().replace(/\s+/g, " ");
  if (s.length <= max) return s;
  return s.slice(0, max).trimEnd() + " …";
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `node --test ui/review.test.js`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add ui/review.js ui/review.test.js
git commit -m "Add review.js quoteBlock helper (clipboard quote truncation)"
```

---

## Task 2: `formatReview` — assemble the clipboard review block

**Files:**
- Modify: `ui/review.js`
- Modify: `ui/review.test.js`

The format (from the spec):

```
Review of <relativePath>

General note: <generalNote>

---

> <orphaned quoted block>  ⚠ this block changed
↳ <orphaned comment>

> <quoted block>
↳ <comment>
```

Rules: orphaned comments first (no reliable position), then anchored comments sorted by source start-line ascending. The `General note:` line and the `---` divider appear only when a non-empty general note exists.

- [ ] **Step 1: Write the failing test**

Append to `ui/review.test.js`:

```js
import { formatReview } from "./review.js";

test("formatReview emits header, general note, divider, and ordered comments", () => {
  const reviews = [
    { sourcepos: "58:1-58:40", quotedText: "Wire the toolbar button before the command.", comment: "do this after the command wiring" },
    { sourcepos: "42:1-42:45", quotedText: "store bookmarks in localStorage keyed by path", comment: "use recent.json, not localStorage" },
  ];
  const out = formatReview(reviews, "This plan never says where bookmarks persist.", "docs/x.md", []);
  assert.equal(
    out,
    "Review of docs/x.md\n" +
    "\n" +
    "General note: This plan never says where bookmarks persist.\n" +
    "\n" +
    "---\n" +
    "\n" +
    "> store bookmarks in localStorage keyed by path\n" +
    "↳ use recent.json, not localStorage\n" +
    "\n" +
    "> Wire the toolbar button before the command.\n" +
    "↳ do this after the command wiring\n",
  );
});

test("formatReview omits the general-note line and divider when note is blank", () => {
  const out = formatReview(
    [{ sourcepos: "3:1-3:5", quotedText: "hello", comment: "fix" }],
    "   ",
    "a.md",
    [],
  );
  assert.equal(out, "Review of a.md\n\n> hello\n↳ fix\n");
});

test("formatReview lists orphaned comments first with a changed tag", () => {
  const out = formatReview(
    [{ sourcepos: "10:1-10:5", quotedText: "still here", comment: "keep" }],
    "",
    "a.md",
    [{ quotedText: "was here", comment: "this is gone now" }],
  );
  assert.equal(
    out,
    "Review of a.md\n\n" +
    "> was here  ⚠ this block changed\n↳ this is gone now\n\n" +
    "> still here\n↳ keep\n",
  );
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test ui/review.test.js`
Expected: FAIL — `formatReview is not a function`.

- [ ] **Step 3: Write minimal implementation**

Append to `ui/review.js`:

```js
/** Build the clipboard review string: header + optional general note + divider,
 *  then orphaned comments (tagged), then anchored comments in document order. */
export function formatReview(reviews, generalNote, relativePath, orphaned = []) {
  const note = (generalNote || "").trim();
  const out = [`Review of ${relativePath}`, ""];
  if (note) out.push(`General note: ${note}`, "", "---", "");

  const ordered = [...reviews].sort(
    (a, b) => startLine(a.sourcepos) - startLine(b.sourcepos),
  );
  const items = [
    ...orphaned.map((o) => ({ ...o, changed: true })),
    ...ordered,
  ];
  for (const it of items) {
    const tag = it.changed ? "  ⚠ this block changed" : "";
    out.push(`> ${quoteBlock(it.quotedText)}${tag}`, `↳ ${it.comment}`, "");
  }
  return out.join("\n").trimEnd() + "\n";
}

function startLine(sourcepos) {
  const m = /^(\d+):/.exec(sourcepos || "");
  return m ? parseInt(m[1], 10) : 0;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `node --test ui/review.test.js`
Expected: PASS (7 tests total).

- [ ] **Step 5: Commit**

```bash
git add ui/review.js ui/review.test.js
git commit -m "Add review.js formatReview helper (clipboard review block)"
```

---

## Task 3: `reanchorReviews` — re-locate comments after a re-render

**Files:**
- Modify: `ui/review.js`
- Modify: `ui/review.test.js`

`newBlocks` is `[{ sourcepos, text }]` where `text` is the block's normalized text (the app will produce it with the same normalization `quoteBlock` uses, so the compare is exact). Each review is matched by `quotedText === block.text`; the first match wins; updated `sourcepos` is carried over. Unmatched reviews become orphaned (only `quotedText` + `comment` survive).

- [ ] **Step 1: Write the failing test**

Append to `ui/review.test.js`:

```js
import { reanchorReviews } from "./review.js";

test("reanchorReviews refreshes sourcepos for matched blocks", () => {
  const reviews = [{ sourcepos: "42:1-42:9", quotedText: "hello world", comment: "c" }];
  const newBlocks = [
    { sourcepos: "1:1-1:5", text: "intro" },
    { sourcepos: "52:1-52:9", text: "hello world" },
  ];
  const { anchored, orphaned } = reanchorReviews(reviews, newBlocks);
  assert.equal(orphaned.length, 0);
  assert.equal(anchored.length, 1);
  assert.equal(anchored[0].sourcepos, "52:1-52:9");
  assert.equal(anchored[0].comment, "c");
});

test("reanchorReviews orphans a comment whose block text is gone", () => {
  const reviews = [{ sourcepos: "42:1-42:9", quotedText: "was here", comment: "c" }];
  const { anchored, orphaned } = reanchorReviews(reviews, [{ sourcepos: "1:1-1:3", text: "new" }]);
  assert.equal(anchored.length, 0);
  assert.deepEqual(orphaned, [{ quotedText: "was here", comment: "c" }]);
});

test("reanchorReviews matches the first block when text repeats", () => {
  const reviews = [{ sourcepos: "9:1-9:3", quotedText: "dup", comment: "c" }];
  const newBlocks = [
    { sourcepos: "2:1-2:3", text: "dup" },
    { sourcepos: "8:1-8:3", text: "dup" },
  ];
  const { anchored } = reanchorReviews(reviews, newBlocks);
  assert.equal(anchored[0].sourcepos, "2:1-2:3");
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test ui/review.test.js`
Expected: FAIL — `reanchorReviews is not a function`.

- [ ] **Step 3: Write minimal implementation**

Append to `ui/review.js`:

```js
/** Re-locate each review against freshly-rendered blocks by matching its
 *  quotedText. Matched reviews get the new sourcepos; unmatched become orphaned.
 *  newBlocks: [{ sourcepos, text }] with text normalized like quoteBlock input. */
export function reanchorReviews(reviews, newBlocks) {
  const anchored = [];
  const orphaned = [];
  for (const r of reviews) {
    const match = newBlocks.find((b) => b.text === r.quotedText);
    if (match) {
      anchored.push({ ...r, sourcepos: match.sourcepos });
    } else {
      orphaned.push({ quotedText: r.quotedText, comment: r.comment });
    }
  }
  return { anchored, orphaned };
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `node --test ui/review.test.js`
Expected: PASS (10 tests total).

- [ ] **Step 5: Commit**

```bash
git add ui/review.js ui/review.test.js
git commit -m "Add review.js reanchorReviews helper (re-locate comments by text)"
```

---

## Task 4: Add the `⊕ Review` toolbar button

**Files:**
- Modify: `ui/index.html:145-153` (insert after the `toggle-edit` button, inside `.toolbar`)

- [ ] **Step 1: Add the button**

In `ui/index.html`, the `.toolbar` div currently ends with the `toggle-edit` button (lines 145-153). Insert a new button immediately after the `toggle-edit` button's closing `</button>` and before the `</div>` that closes `.toolbar`:

```html
          <button
            id="toggle-review"
            class="toolbar-btn"
            type="button"
            aria-pressed="false"
            title="Review this document (annotate + copy for Claude Code)"
          >
            ⊕ Review
          </button>
```

- [ ] **Step 2: Verify the markup**

Run: `grep -n 'id="toggle-review"' ui/index.html`
Expected: one match, between `toggle-edit` and the toolbar's closing `</div>`.

- [ ] **Step 3: Commit**

```bash
git add ui/index.html
git commit -m "Add Review toolbar button"
```

---

## Task 5: Tab-model fields + toolbar wiring for Review Mode

**Files:**
- Modify: `ui/app.js` (import; button ref; tab push sites at `:912`, `:923`, `:953`; toolbar-state block at `:1045-1059`; init listener near `:288-289`)

- [ ] **Step 1: Import the helpers**

`ui/app.js:31-32` already imports `./editor.js` and `./treeops.js`. Add a line immediately after `:32` (`import { validateName } from "./treeops.js";`):

```js
import { formatReview, reanchorReviews, quoteBlock } from "./review.js";
```

- [ ] **Step 2: Add the button reference**

Near `ui/app.js:67` (`const editBtn = document.getElementById("toggle-edit");`), add:

```js
const reviewBtn = document.getElementById("toggle-review");
```

- [ ] **Step 3: Add tab-model fields at all three push sites**

`ui/app.js` pushes new tabs in three places (currently lines 912, 923, 953). Each looks like:

```js
tabs.push({ path, sticky: false, raw: false, editing: false, dirty: false, savedContent: null });
```

For **each** of the three, add the review fields before the closing brace:

```js
tabs.push({ path, sticky: false, raw: false, editing: false, dirty: false, savedContent: null, reviewMode: false, reviews: [], generalNote: "", orphanedReviews: [] });
```

(Match each line's existing `sticky`/`path` values — only append the four new fields. The line at `:923` uses `sticky: true`; the line at `:953` uses `path: p, sticky: true`. Do not change those.)

Do **not** add these fields to the session-restore serialization (around `:1305-1310`) — reviews are intentionally ephemeral.

- [ ] **Step 4: Gate + label the Review button in the toolbar-state block**

In the block at `ui/app.js:1045-1059` (where `editBtn`/`rawBtn` visibility is set), after the `rawBtn` lines (after line 1058), add:

```js
    reviewBtn.hidden = image || t.editing || t.raw;
    if (!reviewBtn.hidden) {
      reviewBtn.setAttribute("aria-pressed", t.reviewMode ? "true" : "false");
    }
```

- [ ] **Step 5: Wire the click handler**

Near `ui/app.js:288-289` (where `rawBtn`/`editBtn` listeners are added in init), add:

```js
  reviewBtn.addEventListener("click", onToggleReview);
```

Then add the handler function near `onToggleRaw` / `onToggleEdit` (search for `function onToggleRaw`):

```js
function onToggleReview() {
  const t = activeTab();
  if (!t) return;
  t.reviewMode = !t.reviewMode;
  reviewBtn.setAttribute("aria-pressed", t.reviewMode ? "true" : "false");
  renderReviewMarkers(t);
}
```

(`renderReviewMarkers` is defined in Task 6; this references it forward, which is fine for a function declaration.)

- [ ] **Step 6: Verify it still builds and tests pass**

Run: `node --test ui/*.test.js`
Expected: PASS (no regressions).
Run: `cd src-tauri && cargo build 2>&1 | tail -3`
Expected: `Finished` (compiles; `renderReviewMarkers` is defined next task — it's only *called*, and JS function declarations hoist, so this still bundles).

- [ ] **Step 7: Commit**

```bash
git add ui/app.js
git commit -m "Wire Review Mode tab state and toolbar toggle"
```

---

## Task 6: `renderReviewMarkers` — gutter +, highlight, comment cards, inline input

**Files:**
- Modify: `ui/app.js` (add the helpers; hook into `postRender` at `:1494-1509`)

The model on a tab: `reviews: [{ sourcepos, quotedText, comment }]`, `orphanedReviews: [{ quotedText, comment }]`, `generalNote: string`, `reviewMode: bool`.

- [ ] **Step 1: Add the rendering + interaction helpers**

Add this block near the other `postRender` hooks in `ui/app.js` (e.g. just after `addCopyButtons` / `onCopyButtonClick`, around `:2055`):

```js
/* ---- Review Mode ---- */

const ANNOTATABLE_TAGS = new Set([
  "P", "H1", "H2", "H3", "H4", "H5", "H6", "LI", "PRE", "BLOCKQUOTE",
]);

function annotatableBlocks() {
  return [...preview.querySelectorAll("[data-sourcepos]")].filter((el) =>
    ANNOTATABLE_TAGS.has(el.tagName),
  );
}

/** A block's source text with our injected UI stripped, normalized the same way
 *  quoteBlock normalizes — so it matches reanchorReviews keys exactly. */
function blockText(block) {
  const clone = block.cloneNode(true);
  for (const el of clone.querySelectorAll(
    ".review-gutter, .review-card, .review-input, .copy-btn",
  )) {
    el.remove();
  }
  return quoteBlock(clone.textContent, Infinity);
}

/** postRender hook: re-anchor against the fresh DOM, then inject gutter + and
 *  cards. Removes its own prior nodes first so it is idempotent across patches. */
function renderReviewMarkers(t) {
  for (const el of preview.querySelectorAll(
    ".review-gutter, .review-card, .review-input, .review-bar",
  )) {
    el.remove();
  }
  preview.classList.toggle("reviewing", !!t.reviewMode);
  if (!t.reviewMode) return;

  if (t.reviews && t.reviews.length) {
    const newBlocks = annotatableBlocks().map((b) => ({
      sourcepos: b.dataset.sourcepos,
      text: blockText(b),
    }));
    const { anchored, orphaned } = reanchorReviews(t.reviews, newBlocks);
    t.reviews = anchored;
    if (orphaned.length) {
      t.orphanedReviews = (t.orphanedReviews || []).concat(orphaned);
    }
  }

  renderReviewBar(t);

  for (const block of annotatableBlocks()) {
    const sp = block.dataset.sourcepos;
    const existing = (t.reviews || []).find((r) => r.sourcepos === sp);
    block.appendChild(makeGutterButton(t, block));
    if (existing) {
      block.classList.add("reviewed");
      block.appendChild(makeReviewCard(t, existing));
    } else {
      block.classList.remove("reviewed");
    }
  }
}

function makeGutterButton(t, block) {
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "review-gutter";
  btn.textContent = "+";
  btn.setAttribute("aria-label", "Add review comment");
  btn.addEventListener("click", (ev) => {
    ev.preventDefault();
    ev.stopPropagation();
    openCommentBox(t, block, null);
  });
  return btn;
}

function makeReviewCard(t, review) {
  const card = document.createElement("div");
  card.className = "review-card";

  const text = document.createElement("span");
  text.className = "review-comment-text";
  text.textContent = review.comment;
  text.title = "Click to edit";
  text.addEventListener("click", (ev) => {
    ev.stopPropagation();
    openCommentBox(t, null, review);
  });

  const del = document.createElement("button");
  del.type = "button";
  del.className = "review-delete";
  del.textContent = "×";
  del.setAttribute("aria-label", "Delete comment");
  del.addEventListener("click", (ev) => {
    ev.preventDefault();
    ev.stopPropagation();
    t.reviews = (t.reviews || []).filter((r) => r !== review);
    renderReviewMarkers(t);
  });

  card.append(text, del);
  return card;
}

/** Inline editor under a block. `block` for a new comment, `existing` to edit. */
function openCommentBox(t, block, existing) {
  const prior = preview.querySelector(".review-input");
  if (prior) prior.remove();

  const host = block || findBlockForSourcepos(existing.sourcepos);
  if (!host) return;

  const box = document.createElement("div");
  box.className = "review-input";
  const input = document.createElement("textarea");
  input.rows = 2;
  input.value = existing ? existing.comment : "";
  input.placeholder = "Comment — Enter to save, Esc to cancel";
  box.appendChild(input);
  host.appendChild(box);
  input.focus();

  input.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter" && !ev.shiftKey) {
      ev.preventDefault();
      const val = input.value.trim();
      if (val) {
        if (existing) {
          existing.comment = val;
        } else {
          if (!t.reviews) t.reviews = [];
          t.reviews.push({
            sourcepos: host.dataset.sourcepos,
            quotedText: blockText(host),
            comment: val,
          });
        }
      }
      renderReviewMarkers(t);
    } else if (ev.key === "Escape") {
      ev.preventDefault();
      renderReviewMarkers(t);
    }
  });
}

function findBlockForSourcepos(sp) {
  return [...preview.querySelectorAll("[data-sourcepos]")].find(
    (el) => el.dataset.sourcepos === sp,
  );
}
```

Note: `blockText` calls `quoteBlock(text, Infinity)` to get full normalized text (no truncation) as the match key; `formatReview` later truncates with the default `max` for display. This keeps one normalization path.

- [ ] **Step 2: Add `renderReviewBar` (general note + Copy Review button)**

Add immediately after `renderReviewMarkers` (the Copy action body is finished in Task 7; define the button + general-note field here):

```js
function renderReviewBar(t) {
  const bar = document.createElement("div");
  bar.className = "review-bar";

  const note = document.createElement("textarea");
  note.className = "review-general-note";
  note.rows = 2;
  note.placeholder = "General note about this document (optional)";
  note.value = t.generalNote || "";
  note.addEventListener("input", () => {
    t.generalNote = note.value;
  });

  const copy = document.createElement("button");
  copy.type = "button";
  copy.className = "review-copy-btn";
  copy.textContent = "Copy Review";
  copy.addEventListener("click", (ev) => {
    ev.preventDefault();
    copyReview(t, copy);
  });

  bar.append(note, copy);
  preview.prepend(bar);
}
```

- [ ] **Step 3: Hook into `postRender`**

In `postRender` (`ui/app.js:1494-1509`), inside the `if (!raw)` block, after `addMermaidExportButtons();`, add:

```js
    renderReviewMarkers(t);
```

So the block reads:

```js
  if (!raw) {
    renderMath();
    await renderMermaid({ force: forceMermaid });
    addMermaidExportButtons();
    renderReviewMarkers(t);
  }
```

- [ ] **Step 4: Verify build + tests**

Run: `node --test ui/*.test.js`
Expected: PASS.
Run: `cd src-tauri && cargo build 2>&1 | tail -3`
Expected: `Finished` (`copyReview` is referenced but defined in Task 7; function declarations hoist, so the bundle compiles. If you want to verify before Task 7, add a temporary `function copyReview() {}` stub and remove it in Task 7 — optional.)

- [ ] **Step 5: Commit**

```bash
git add ui/app.js
git commit -m "Render Review Mode markers (gutter, cards, inline input) in postRender"
```

---

## Task 7: Copy Review action

**Files:**
- Modify: `ui/app.js` (add `copyReview`; uses `formatReview`, `relativeToRoot`, `copyText`, `basename`)

- [ ] **Step 1: Add the Copy Review action**

Add near `renderReviewBar` in `ui/app.js`:

```js
async function copyReview(t, btn) {
  const rel = relativeToRoot(t.path, treeRoot) || basename(t.path);
  const text = formatReview(
    t.reviews || [],
    t.generalNote || "",
    rel,
    t.orphanedReviews || [],
  );
  await copyText(text);
  t.reviews = [];
  t.orphanedReviews = [];
  t.generalNote = "";
  if (btn) {
    btn.textContent = "Copied";
    setTimeout(() => {
      btn.textContent = "Copy Review";
    }, 1200);
  }
  renderReviewMarkers(t);
}
```

(If you added a temporary `copyReview` stub in Task 6, remove it now.)

- [ ] **Step 2: Verify build + tests**

Run: `node --test ui/*.test.js`
Expected: PASS.
Run: `cd src-tauri && cargo build 2>&1 | tail -3`
Expected: `Finished`.

- [ ] **Step 3: Commit**

```bash
git add ui/app.js
git commit -m "Add Copy Review action (assemble + clipboard + clear)"
```

---

## Task 8: Styles (light + dark)

**Files:**
- Modify: `ui/styles.css` (add a Review Mode section; follow the `.copy-btn` / `.toolbar-btn` patterns at `:399-415` and `:527-564`)

- [ ] **Step 1: Add the Review Mode styles**

Append to `ui/styles.css` (after the copy-button rules is a natural spot):

```css
/* ---- Review Mode ---- */

.markdown-body.reviewing [data-sourcepos] {
  position: relative;
}

.review-gutter {
  position: absolute;
  left: -1.6em;
  top: 0;
  width: 1.3em;
  height: 1.3em;
  line-height: 1.1em;
  padding: 0;
  border: 1px solid var(--border, #d0d7de);
  border-radius: 4px;
  background: var(--bg, #fff);
  color: var(--fg, #1f2328);
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.1s;
}

.markdown-body.reviewing [data-sourcepos]:hover > .review-gutter,
.review-gutter:focus-visible {
  opacity: 1;
}

.markdown-body .reviewed {
  border-left: 3px solid #0969da;
  padding-left: 0.6em;
  margin-left: -0.6em;
}

.review-card {
  display: flex;
  align-items: flex-start;
  gap: 0.5em;
  margin: 0.4em 0 0.6em;
  padding: 0.4em 0.6em;
  border-radius: 6px;
  background: #ddf4ff;
  border: 1px solid #b6e3ff;
  font-size: 0.9em;
}

.review-comment-text {
  flex: 1;
  cursor: text;
  white-space: pre-wrap;
}

.review-delete {
  border: none;
  background: transparent;
  cursor: pointer;
  color: #57606a;
  font-size: 1.1em;
  line-height: 1;
  padding: 0 0.2em;
}

.review-input {
  margin: 0.4em 0 0.6em;
}

.review-input textarea,
.review-general-note {
  width: 100%;
  box-sizing: border-box;
  font: inherit;
  padding: 0.4em 0.6em;
  border-radius: 6px;
  border: 1px solid var(--border, #d0d7de);
  background: var(--bg, #fff);
  color: var(--fg, #1f2328);
  resize: vertical;
}

.review-bar {
  display: flex;
  gap: 0.5em;
  align-items: flex-start;
  margin-bottom: 1em;
  padding-bottom: 0.8em;
  border-bottom: 1px solid var(--border, #d0d7de);
}

.review-general-note {
  flex: 1;
}

.review-copy-btn {
  flex: 0 0 auto;
  padding: 0.4em 0.8em;
  border-radius: 6px;
  border: 1px solid var(--border, #d0d7de);
  background: #0969da;
  color: #fff;
  cursor: pointer;
}

[data-theme="dark"] .review-card {
  background: #121d2f;
  border-color: #1f3b5c;
}

[data-theme="dark"] .review-delete {
  color: #8b949e;
}

[data-theme="dark"] .review-gutter,
[data-theme="dark"] .review-input textarea,
[data-theme="dark"] .review-general-note {
  background: #0d1117;
  color: #e6edf3;
  border-color: #30363d;
}

[data-theme="dark"] .markdown-body .reviewed {
  border-left-color: #2f81f7;
}
```

- [ ] **Step 2: Commit**

```bash
git add ui/styles.css
git commit -m "Style Review Mode (gutter, cards, input, bar) for light and dark"
```

---

## Task 9: Build and manual GUI smoke test

**Files:** none (verification only)

The project memory note "GUI smoke test before merge" applies: automated work misses theme/visual bugs. This is required before declaring done.

- [ ] **Step 1: Rebuild the bundled frontend**

Run: `cd src-tauri && cargo build 2>&1 | tail -3`
Expected: `Finished`.

- [ ] **Step 2: Run the app on a real plan/spec**

Run: `cd src-tauri && cargo run -- ../docs/superpowers/specs/2026-06-09-review-mode-design.md`

- [ ] **Step 3: Smoke-test the workflow (light mode)**

Verify each:
- The `⊕ Review` button appears in the toolbar and toggles `aria-pressed`.
- Toggling Review on shows the general-note bar at the top and a `+` in the left gutter when hovering a paragraph/heading/list item/code block.
- Clicking `+` opens an inline box; typing + Enter saves a card under the block; the block gets a blue left-border.
- Clicking a card's text re-opens the editor; editing + Enter updates it; `×` deletes it.
- Typing a general note, adding 2-3 block comments, then **Copy Review** copies a block matching the Task 2 format (paste into a scratch file or the terminal to confirm). Annotations clear after copy.
- The Review button is hidden in Raw view, in Edit mode, and on an image tab.

- [ ] **Step 4: Smoke-test dark mode**

Toggle the theme (`☾/☀`) while reviewing. Confirm the gutter `+`, the reviewed left-border, the comment cards, the inline input, and the review bar are all legible in dark mode (no white-on-white / black-on-black).

- [ ] **Step 5: Smoke-test live-reload re-anchoring**

With the app open and a comment on a block, edit the source file in another editor (or use in-app edit) to insert lines **above** the commented block and save. Confirm the comment stays attached to the same block. Then change the commented block's text and save; confirm the comment moves into the review bar area tagged "⚠ this block changed" and still appears in Copy Review output.

- [ ] **Step 6: Final full test run**

Run: `node --test ui/*.test.js`
Expected: PASS (10 review tests + all existing).
Run: `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: clean (no Rust changed, but confirm nothing regressed).

- [ ] **Step 7: Commit any smoke-test fixes**

```bash
git add -A
git commit -m "Polish Review Mode after GUI smoke test"
```

(Skip this commit if no fixes were needed.)

---

## Self-review notes (for the implementer)

- **Spec coverage:** Part 1 → Tasks 4-6 + 8 (toolbar, gutter, cards, general note). Part 2 → Tasks 5-6 (tab state, postRender hook, `data-sourcepos`). Part 3 → Task 2 (`formatReview`) + Task 7 (path + copy). Part 4 → Task 3 (`reanchorReviews`) + Task 6 (per-render re-anchor, orphan accumulation). Part 5 → all (pure helpers in `review.js`, wiring in `app.js`, dark CSS, Task 9 smoke test). Scope exclusions honored: no Rust, no persistence (fields excluded from session-restore), no text-range selection.
- **Type consistency:** review object shape `{ sourcepos, quotedText, comment }` is identical across Tasks 2, 3, 6, 7. Orphaned shape `{ quotedText, comment }` identical across Tasks 2, 3, 6. `newBlocks` shape `{ sourcepos, text }` identical across Tasks 3 and 6. `formatReview(reviews, generalNote, relativePath, orphaned)` arg order identical in Tasks 2 and 7.
- **Known v1 limitations (acceptable):** a live-reload while the inline comment box or general-note field is focused can drop the in-progress text (the doc you're reviewing changing under you is the rare case); list-level (UL/OL) and table-cell granularity are intentionally excluded (comment on the LI or a paragraph instead).
```
