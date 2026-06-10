# Review Mode UX: clearer purpose, finish-to-copy

**Date:** 2026-06-10
**Status:** Approved (brainstorming) — pending implementation plan

## Goal

Make the shipped **Review Mode** easier for a first-time user to understand on
two points raised in use:

1. The `⊕ Review` toolbar button doesn't convey *what* the mode is for
   (commenting on a document to send feedback to Claude Code).
2. The **Copy Review** button is hard to find — it floats inside a bar at the
   top of the document, not where a primary "finish" action is expected.

Both are addressed by collapsing the lifecycle into the toggle itself
(enter → comment → **Finish & Copy**) and explaining the loop in-context.

This is a UX refinement of existing code; no Rust, no IPC, and the pure
`ui/review.js` helpers (`formatReview`, `reanchorReviews`, `quoteBlock`) are
untouched.

## Decisions (locked during brainstorming)

| Question | Decision |
|---|---|
| Copy trigger | **Toggle-off finishes & copies.** The Review button is the whole lifecycle: click to start; click again (now **✓ Finish & Copy**) to copy + clear + exit. The floating Copy Review button is removed. |
| Purpose clarity | **Both** a clearer button (label/icon/tooltip) **and** an instructional header shown on entry. |
| "Has content" | Finishing copies only when there is ≥1 block comment **or** a non-empty general note; otherwise it just exits quietly. |
| Confirmation | A transient, auto-dismissing **toast** on copy, via the existing transient-banner mechanism (neutral, not the red error path). |

**Out of scope (YAGNI):** a separate "cancel review without copying" affordance;
"peek at clean doc mid-review" (toggling off now ends the review); changing the
clipboard format or any annotation behavior; configurable button text.

---

## Why these decisions

**Finish-to-copy removes a control instead of relocating it.** The confusing
Copy Review button isn't moved to a better spot — it's eliminated. The natural
"I'm done" gesture (toggling the mode off) becomes the copy, so the lifecycle —
enter, comment, finish — is legible from the button label alone. The annotations
were already cleared after copy (the spent-lifecycle model), so coupling that to
exit is consistent.

**Accepted behavior change.** Toggling review off previously just hid the
markers while keeping annotations; now it ends the review (copies + clears).
Reviews are short-lived, so losing the "toggle off to peek at a clean document"
affordance is an acceptable trade for a clearer mental model.

**The header carries the explanation a button can't.** Two words of button label
can't describe the Claude-feedback loop; a one-line header shown exactly when the
user enters review mode can, and it naturally houses the general-note field.

---

## Part 1 — Button states

The toolbar toggle (`#toggle-review`, gated as today — hidden in raw/edit/image
tabs) reflects the mode via the existing toolbar-update path that already flips
`aria-pressed`/text:

| State (`t.reviewMode`) | Label | Tooltip |
|---|---|---|
| off | **💬 Review** | "Comment on this document and copy your review for Claude Code" |
| on | **✓ Finish & Copy** | "Copy your review to the clipboard and exit review mode" |

The off-state label/icon and tooltip live in `index.html`; the on-state label is
set in the toolbar-update block alongside `editBtn`/`rawBtn`.

## Part 2 — Behavior

`onToggleReview()` branches on the current mode:

- **Entering** (`reviewMode` was false): set `reviewMode = true`, update the
  toolbar, render markers (as today).
- **Finishing** (`reviewMode` was true): call `finishReview(t)`.

`finishReview(t)`:
1. Determine `hasContent = (t.reviews?.length > 0) || (t.orphanedReviews?.length > 0) || (t.generalNote || "").trim() !== ""`.
2. If `hasContent`: build the clipboard text via the existing `formatReview`
   path (the current `copyReview` body), `copyText` it, then show the toast
   *"Review copied — paste into Claude Code."*
3. Clear `reviews` / `orphanedReviews` / `generalNote` (as `copyReview` does
   today).
4. Set `reviewMode = false`, update the toolbar, re-render (markers removed).

The existing `copyReview(t)` is refactored: its assemble-and-copy core is reused
by `finishReview`, and the old in-bar Copy button that called it is removed.

## Part 3 — The review bar (header + general note)

`renderReviewBar(t)` changes:

- **Remove** the `.review-copy-btn`.
- **Add** a `.review-hint` line at the top of the bar:
  > Comment on any block (hover for the **+**), then **Finish & Copy** to paste
  > your review into Claude Code.
- **Keep** the `.review-general-note` textarea below the hint (placeholder
  unchanged: "General note about this document (optional)").

## Part 4 — Toast

A neutral, auto-dismissing transient message. If a neutral transient helper does
not already exist (the current `showTransientError` is red/error-styled), add
`showTransientMessage(text)` that reuses the same transient banner element with a
neutral class and the same auto-dismiss timing. The toast must not clear the
preview (unlike `showError`).

## Part 5 — Testing

- The pure `ui/review.js` unit tests are unchanged and still pass (clipboard
  format is untouched).
- **Manual GUI smoke test** (the part tests can't cover):
  - Off button reads **💬 Review**; entering shows the hint line and the
    general-note field (no Copy button in the bar).
  - Active button reads **✓ Finish & Copy**.
  - Add a comment + general note → Finish & Copy → clipboard holds the review,
    annotations clear, mode exits, toast appears.
  - Enter review mode, add nothing, Finish & Copy → exits quietly, no clipboard
    write, no toast.
  - Dark-mode the `.review-hint`.

## Build reminder

Frontend-only; still requires `cargo build` to rebundle (`frontendDist` is
compiled in). Smoke-test against a real Claude-written doc.
