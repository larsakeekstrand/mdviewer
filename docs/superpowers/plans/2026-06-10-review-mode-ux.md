# Review Mode UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Review Mode self-explanatory by collapsing its lifecycle into the toggle — enter → comment → **Finish & Copy** — and explaining the loop with an in-context header, removing the confusing floating Copy button.

**Architecture:** Frontend-only refinement of the shipped Review Mode. `onToggleReview` branches enter-vs-finish; a new `finishReview(t)` copies (when there's content), clears, exits, and toasts. `renderReviewBar` drops the Copy button and gains an instructional hint. The toolbar button label/tooltip flip with `reviewMode`. A neutral `showTransientMessage` reuses the existing transient banner. No Rust, no IPC; the pure `ui/review.js` helpers are untouched.

**Tech Stack:** Vanilla JS, `node --test` (regression only — no new unit tests), Tauri rebuild to bundle the frontend.

---

## File structure

- **Modify `ui/index.html`** — off-state button label/tooltip.
- **Modify `ui/app.js`** — `onToggleReview` (enter vs finish), `finishReview`, `renderReviewBar` (hint, no Copy button), toolbar-update label, `showTransientMessage` (+ `showTransientError` info-class cleanup); remove the now-dead `copyReview`.
- **Modify `ui/styles.css`** — `.review-hint`, `.task-error-banner.info` (light + dark); remove `.review-copy-btn` rules.

## Conventions

- JS regression: `node --test ui/*.test.js` (must stay green; `review.js`'s `formatReview` is unchanged). Frontend changes require `cargo build` to rebundle.
- This is DOM/UI wiring, so verification is build + the Task 7 manual GUI smoke test, not new unit tests.
- No `Co-Authored-By` trailer. Commit after each task.

---

## Task 1: Replace `onToggleReview` and `copyReview` with enter/finish flow

**Files:**
- Modify: `ui/app.js`

- [ ] **Step 1: Rewrite `onToggleReview`**

Replace the current function:

```js
function onToggleReview() {
  const t = activeTab();
  if (!t) return;
  t.reviewMode = !t.reviewMode;
  renderTabBar();
  renderReviewMarkers(t);
}
```

with the enter-vs-finish branch:

```js
function onToggleReview() {
  const t = activeTab();
  if (!t) return;
  if (t.reviewMode) {
    finishReview(t);
  } else {
    t.reviewMode = true;
    renderTabBar();
    renderReviewMarkers(t);
  }
}
```

- [ ] **Step 2: Replace `copyReview` with `finishReview`**

Replace the entire current `copyReview` function:

```js
async function copyReview(t) {
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
  renderReviewMarkers(t);
  const freshBtn = preview.querySelector(".review-copy-btn");
  if (freshBtn) {
    freshBtn.textContent = "Copied";
    setTimeout(() => {
      freshBtn.textContent = "Copy Review";
    }, 1200);
  }
}
```

with:

```js
/** Finish a review: copy it (when there's anything to send), clear the
 *  annotations, exit review mode, and confirm with a toast. */
async function finishReview(t) {
  const hasContent =
    (t.reviews && t.reviews.length > 0) ||
    (t.orphanedReviews && t.orphanedReviews.length > 0) ||
    (t.generalNote || "").trim() !== "";
  if (hasContent) {
    const rel = relativeToRoot(t.path, treeRoot) || basename(t.path);
    const text = formatReview(
      t.reviews || [],
      t.generalNote || "",
      rel,
      t.orphanedReviews || [],
    );
    await copyText(text);
    showTransientMessage("Review copied — paste into Claude Code");
  }
  t.reviews = [];
  t.orphanedReviews = [];
  t.generalNote = "";
  t.reviewMode = false;
  renderTabBar();
  renderReviewMarkers(t);
}
```

- [ ] **Step 3: Verify build + JS regression**

Run: `node --test ui/*.test.js 2>&1 | grep -E "# (pass|fail)"`
Expected: PASS (unchanged).
Run: `cd src-tauri && cargo build 2>&1 | tail -3`
Expected: `Finished` (note: `renderReviewBar` still references the old Copy button and `showTransientMessage` doesn't exist yet — both are JS runtime symbols, not compile errors, so the bundle builds; they're fixed in Tasks 2-3).

- [ ] **Step 4: Commit**

```bash
git add ui/app.js
git commit -m "Review Mode: toggle-off finishes & copies (replace copyReview with finishReview)"
```

---

## Task 2: Review bar — drop Copy button, add the hint line

**Files:**
- Modify: `ui/app.js` (the `renderReviewBar` function)

- [ ] **Step 1: Rewrite `renderReviewBar`**

Replace the current function:

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
    copyReview(t);
  });

  bar.append(note, copy);
  preview.prepend(bar);
}
```

with (hint line on top, general-note below, no Copy button):

```js
function renderReviewBar(t) {
  const bar = document.createElement("div");
  bar.className = "review-bar";

  const hint = document.createElement("div");
  hint.className = "review-hint";
  hint.textContent =
    "Comment on any block (hover for the +), then Finish & Copy to paste your review into Claude Code.";

  const note = document.createElement("textarea");
  note.className = "review-general-note";
  note.rows = 2;
  note.placeholder = "General note about this document (optional)";
  note.value = t.generalNote || "";
  note.addEventListener("input", () => {
    t.generalNote = note.value;
  });

  bar.append(hint, note);
  preview.prepend(bar);
}
```

- [ ] **Step 2: Verify build**

Run: `cd src-tauri && cargo build 2>&1 | tail -3`
Expected: `Finished`.

- [ ] **Step 3: Commit**

```bash
git add ui/app.js
git commit -m "Review bar: instructional hint, drop the in-bar Copy button"
```

---

## Task 3: Neutral transient toast

**Files:**
- Modify: `ui/app.js`

- [ ] **Step 1: Add `showTransientMessage` and clean the info class in `showTransientError`**

The current `showTransientError` is:

```js
function showTransientError(msg) {
  let banner = document.getElementById("task-error-banner");
  if (!banner) {
    banner = document.createElement("div");
    banner.id = "task-error-banner";
    banner.className = "task-error-banner";
    document.body.appendChild(banner);
  }
  banner.textContent = msg;
  banner.hidden = false;
  if (transientErrorTimer) clearTimeout(transientErrorTimer);
  transientErrorTimer = setTimeout(() => {
    banner.hidden = true;
  }, 3000);
}
```

Add one line so a reused banner is never left in the neutral `info` style when showing an error — change the body to insert `banner.classList.remove("info");` right after the `banner.textContent = msg;` line:

```js
  banner.textContent = msg;
  banner.classList.remove("info");
  banner.hidden = false;
```

Then add a neutral variant immediately after `showTransientError`:

```js
/** Neutral, auto-dismissing toast (reuses the transient banner element with an
 *  `info` style). Unlike showError, it never clears the preview. */
function showTransientMessage(msg) {
  let banner = document.getElementById("task-error-banner");
  if (!banner) {
    banner = document.createElement("div");
    banner.id = "task-error-banner";
    banner.className = "task-error-banner";
    document.body.appendChild(banner);
  }
  banner.textContent = msg;
  banner.classList.add("info");
  banner.hidden = false;
  if (transientErrorTimer) clearTimeout(transientErrorTimer);
  transientErrorTimer = setTimeout(() => {
    banner.hidden = true;
    banner.classList.remove("info");
  }, 3000);
}
```

- [ ] **Step 2: Verify build + JS regression**

Run: `node --test ui/*.test.js 2>&1 | grep -E "# (pass|fail)"` (pass).
Run: `cd src-tauri && cargo build 2>&1 | tail -3` (`Finished`).

- [ ] **Step 3: Commit**

```bash
git add ui/app.js
git commit -m "Add neutral showTransientMessage toast (shared transient banner)"
```

---

## Task 4: Toolbar button — label/tooltip flip with review mode

**Files:**
- Modify: `ui/app.js` (the toolbar-update block in `renderTabBar`, around `:1070`)

- [ ] **Step 1: Set the label and tooltip when the button is shown**

The current block is:

```js
    reviewBtn.hidden = image || t.editing || t.raw;
    if (!reviewBtn.hidden) {
      reviewBtn.setAttribute("aria-pressed", t.reviewMode ? "true" : "false");
    }
```

Replace it with:

```js
    reviewBtn.hidden = image || t.editing || t.raw;
    if (!reviewBtn.hidden) {
      reviewBtn.setAttribute("aria-pressed", t.reviewMode ? "true" : "false");
      reviewBtn.textContent = t.reviewMode ? "✓ Finish & Copy" : "💬 Review";
      reviewBtn.title = t.reviewMode
        ? "Copy your review to the clipboard and exit review mode"
        : "Comment on this document and copy your review for Claude Code";
    }
```

- [ ] **Step 2: Verify build**

Run: `cd src-tauri && cargo build 2>&1 | tail -3`
Expected: `Finished`.

- [ ] **Step 3: Commit**

```bash
git add ui/app.js
git commit -m "Review toggle: flip label/tooltip between Review and Finish & Copy"
```

---

## Task 5: Off-state button markup

**Files:**
- Modify: `ui/index.html` (the `#toggle-review` button)

- [ ] **Step 1: Update the static label and tooltip**

The current button is:

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

Replace the `title` and the visible text:

```html
          <button
            id="toggle-review"
            class="toolbar-btn"
            type="button"
            aria-pressed="false"
            title="Comment on this document and copy your review for Claude Code"
          >
            💬 Review
          </button>
```

- [ ] **Step 2: Verify**

Run: `grep -n '💬 Review' ui/index.html`
Expected: one match.
Run: `cd src-tauri && cargo build 2>&1 | tail -3` (`Finished`).

- [ ] **Step 3: Commit**

```bash
git add ui/index.html
git commit -m "Review button: clearer label and tooltip (off state)"
```

---

## Task 6: Styles — hint, toast, remove dead Copy-button rules

**Files:**
- Modify: `ui/styles.css`

- [ ] **Step 1: Add `.review-hint` and the neutral toast variant**

Find the Review Mode section (search for `/* ---- Review Mode ---- */`). Add the hint style near the `.review-bar` rules:

```css
.review-hint {
  font-size: 0.85em;
  color: #57606a;
  margin-bottom: 0.5em;
  line-height: 1.4;
}

[data-theme="dark"] .review-hint {
  color: #8b949e;
}
```

Then add a neutral variant for the transient banner. Find the existing
`.task-error-banner` rule and add, right after it:

```css
.task-error-banner.info {
  background: #ddf4ff;
  color: #0a3069;
  border-color: #b6e3ff;
}

[data-theme="dark"] .task-error-banner.info {
  background: #121d2f;
  color: #e6edf3;
  border-color: #1f3b5c;
}
```

(If `.task-error-banner` has no `border-color` to override, the `border-color`
lines are harmless; keep them for parity with the review-card palette.)

- [ ] **Step 2: Remove the now-dead `.review-copy-btn` rules**

Search `ui/styles.css` for `.review-copy-btn` and delete the rule block(s) that
target it (the floating Copy button no longer exists). Do not remove
`.review-bar`, `.review-general-note`, or other review styles.

- [ ] **Step 3: Verify**

Run: `grep -n 'review-copy-btn' ui/styles.css`
Expected: no matches.
Run: `cd src-tauri && cargo build 2>&1 | tail -3` (`Finished`).

- [ ] **Step 4: Commit**

```bash
git add ui/styles.css
git commit -m "Style review hint + neutral toast; drop dead .review-copy-btn rules"
```

---

## Task 7: Build + manual GUI smoke test

**Files:** none (verification only)

- [ ] **Step 1: Gates**

Run: `node --test ui/*.test.js 2>&1 | grep -E "# (pass|fail)"` (pass) ·
`cd src-tauri && cargo build 2>&1 | tail -2` (`Finished`).

- [ ] **Step 2: Run the app**

Run: `cd src-tauri && cargo run -- ../docs/superpowers/specs/2026-06-10-review-mode-ux-design.md`

- [ ] **Step 3: Verify the flow (light mode)**

- The toolbar button reads **💬 Review**; its tooltip mentions Claude Code.
- Click it → enters review mode; the top bar shows the **hint line** then the
  general-note field, and there is **no Copy button** in the bar.
- The button now reads **✓ Finish & Copy**.
- Add a block comment and type a general note → click **✓ Finish & Copy** →
  a toast *"Review copied — paste into Claude Code"* appears, annotations clear,
  the mode exits, and the button reads **💬 Review** again. Paste somewhere to
  confirm the clipboard holds the review.
- Enter review mode again, add nothing, click **✓ Finish & Copy** → exits
  quietly with **no toast** and nothing written to the clipboard.
- Confirm the button is hidden in Raw view, Edit mode, and on image tabs (as before).

- [ ] **Step 4: Dark mode**

Toggle the theme (☾) while the hint and (after a copy) the toast are visible —
confirm both are legible.

- [ ] **Step 5: Commit any fixes**

```bash
git add -A && git commit -m "Polish Review Mode UX after smoke test"   # skip if none
```

---

## Self-review notes (for the implementer)

- **Spec coverage:** Part 1 button states → Tasks 4 (active label) + 5 (off label). Part 2 behavior (`onToggleReview` branch, `finishReview`, `hasContent`) → Task 1. Part 3 review bar (hint, no Copy button) → Tasks 2 + 6. Part 4 toast → Tasks 3 + 6. Part 5 testing → Task 7 + the unchanged `review.js` tests. Out-of-scope items (no cancel-without-copy, no mid-review peek, format unchanged) are honored — nothing adds them.
- **Type/symbol consistency:** `finishReview(t)` defined (Task 1) and called by `onToggleReview` (Task 1). `showTransientMessage` defined (Task 3) and called by `finishReview` (Task 1) — forward reference is fine (function declarations hoist; both land before any runtime call). `copyReview` is fully removed (Task 1) and its only caller (the in-bar button) is removed (Task 2) — no dangling reference. `.review-copy-btn` removed from JS (Task 2) and CSS (Task 6). The toolbar block sets `reviewBtn.textContent`/`title` (Task 4), and `index.html` provides the matching off-state default (Task 5).
- **Build-order note:** Tasks 1-3 leave transient intermediate states (a removed symbol referenced until the next task) but all are JS *runtime* symbols — the Tauri bundle compiles regardless; the app is only exercised at Task 7 after every task has landed.
