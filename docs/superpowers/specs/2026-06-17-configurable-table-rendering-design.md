# Configurable table rendering for exports

**Date:** 2026-06-17
**Status:** Design approved, pending implementation plan

## Problem

Tables in exported PDFs don't look good. They inherit the github-markdown grid
style (a 1px border on every cell, even-row zebra striping) plus a single
accent-tinted header background. On paper the full grid is the most visually
dominant element, which reads as heavy and busy. Separately, any table wider
than the page is unconditionally shrunk via `transform: scale()`
(`fitWideTablesForPrint`), so dense tables come out with tiny, hard-to-read
text and the user has no say in it.

The customizable PDF export shipped in v1.20.0 (`pdf-export.html`,
`pdf-presets.js`) added knobs for typography, paper, margins, and page numbers,
but tables were left untouched.

## Goal

Give the user two controls over how tables render in exports:

1. **Table style** — choose the visual treatment (Editorial / Grid / Minimal).
2. **Wide tables** — choose how a too-wide table reconciles with page width
   (Scale to fit / Wrap text).

## Scope decisions (resolved during brainstorming)

- **Table style applies to PDF *and* HTML export**, not the on-screen viewer.
  The on-screen viewer and split editor keep today's github grid.
- **Wide-table behavior is PDF-only.** Auto-fitting a table to a fixed page
  width has no meaning in an HTML document viewed at arbitrary width, and the
  scaling code already runs only on the PDF path.
- **Defaults change intentionally:** Editorial style + Wrap text become the new
  defaults. Existing users' wide tables will start wrapping instead of
  shrinking; "Grid" + "Scale to fit" are the escape hatches back to today's
  behavior.

## Why these two are separate layers

Style and fit are orthogonal:

- **Style** is a paint concern (borders, zebra, header treatment). It rides in
  the shared `settingsToCss`, which already feeds both the PDF print and the
  HTML export builder, so it reaches both formats with no extra wiring.
- **Fit** is a layout concern (how a too-wide table reconciles with a fixed
  page width). It is inherently PDF-only and lives on the PDF export path,
  *outside* `settingsToCss`, so HTML export is unaffected.

Keeping them in separate code paths is what lets "style spans both formats" and
"fit is PDF-only" both be true without conditional branching inside the shared
CSS.

## The two controls

Both are added to the PDF export window (`pdf-export.html`), below the existing
Margins / Page-numbers selects, each updating the live preview on change.

| Control      | Values                        | Default     | Applies to          |
| ------------ | ----------------------------- | ----------- | ------------------- |
| Table style  | Editorial · Grid · Minimal    | Editorial   | PDF and HTML export |
| Wide tables  | Scale to fit · Wrap text      | Wrap text   | PDF only            |

## Design

### 1. Settings & persistence

Add two fields to the exported/persisted settings object:

- `tableStyle`: `"editorial" | "grid" | "minimal"`
- `tableFit`: `"fit" | "wrap"`

**Frontend (`pdf-presets.js`):**

- Add `tableStyle` and `tableFit` to each preset record. All three presets
  (clean, report, compact) default to `editorial` + `wrap`. (Presets may differ
  later; uniform defaults are the YAGNI choice now.)
- Add both keys to the persisted settings subset returned by
  `settingsFromPreset` / `presetDefaults`, so they round-trip through
  `get_pdf_settings` / `save_pdf_settings` and are restored by "Reset to
  preset".

**Backend (`recent.rs::PdfSettings`):**

- Add `table_style: String` and `table_fit: String`.
- Each gets `#[serde(default = "…")]` returning `"editorial"` / `"wrap"` so
  settings JSON persisted by older versions (which lack the fields)
  deserializes cleanly.
- Update the `Default` impl to set both.

### 2. Table style → `settingsToCss`

`settingsToCss(settings)` emits a block of table CSS chosen by
`settings.tableStyle`, appended after the github-markdown rules. Equal
specificity means the later rules win, overriding the base grid. The header
accent tint currently emitted unconditionally
(`.markdown-body table th { background: … }`) becomes part of the "grid" branch
only.

- **Editorial** — `border-collapse: collapse`; 2px dark top/bottom rules
  bounding the table; header cells get a 1px dark bottom rule and bold weight;
  body cells get a thin light (`--borderColor-muted`) bottom separator; no
  vertical borders; no zebra.
- **Grid** — re-assert today's look: full 1px cell borders, even-row zebra,
  faint accent header tint (`color-mix(... var(--pdf-accent) 12% ...)`).
- **Minimal** — header cells get a bottom underline and bold weight; subtle
  even-row zebra; no other borders.

Because `settingsToCss` is injected only during export (as `#pdf-export-style`,
removed in a `finally`) and baked into `buildExportHtml`'s output, this reaches
both export formats and never affects the live on-screen viewer.

### 3. Wide tables → PDF-path behavior layer

Kept out of `settingsToCss` so HTML export is unaffected. The choice is read on
the PDF export path (`exportDocument`'s `format === "pdf"` branch) and the PDF
window's live preview (`renderExportPreviewHtml`):

- **Scale to fit** (`"fit"`) — run the existing `fitWideTablesForPrint()`
  scaling exactly as today.
- **Wrap text** (`"wrap"`) — skip the scaling and inject a wrap stylesheet that
  overrides github-markdown's `.markdown-body table { display: block; width:
  max-content; max-width: 100%; overflow: auto }` with `display: table; width:
  100%; table-layout: fixed`, plus `overflow-wrap: anywhere; white-space:
  normal` on cells. The table is held to the content width; cells wrap and the
  table grows taller instead of shrinking.

The wrap stylesheet is applied as its own injected style (or appended to the
existing `#pdf-export-style` element on the PDF path only), never inside
`buildExportHtml`, so the HTML export output never carries it.

### Data flow (unchanged plumbing)

- The PDF export window (`pdf-export.js`) reads settings via `get_pdf_settings`,
  reflects them in the new selects, and emits `pdf-export-request-preview` /
  `pdf-export-run` with the settings object — same as the existing knobs.
- The main window's `savedPdfSettings()` (used by the menu-driven
  File ▸ Export as HTML… / PDF… actions) already merges `get_pdf_settings`
  over `defaultSettings()`, so the new keys flow through automatically.

## Testing

- **`pdf-presets.js`** (`node --test`): assert the new keys are present in
  `defaultSettings()` / `presetDefaults(id)` with the right defaults; assert
  `settingsToCss` emits the correct distinguishing rules for each `tableStyle`
  (e.g. editorial has no vertical cell borders / no zebra, grid keeps the accent
  header tint and zebra, minimal has only the header underline); assert the wrap
  CSS helper (if extracted as a pure function) emits the override only for
  `tableFit === "wrap"`.
- **`recent.rs`**: round-trip test proving JSON without `tableStyle` / `tableFit`
  loads with defaults `editorial` / `wrap`, and a full round-trip preserving
  explicit non-default values (camelCase serialization).

## Out of scope

- Per-table overrides (a global per-export choice only).
- Changing the on-screen viewer's table style.
- Wide-table behavior for HTML export.
- Additional table knobs (border color, padding, zebra toggle, etc.) — a single
  named style + a single fit mode only.

## Files touched

- `ui/pdf-presets.js` — settings keys, preset records, `settingsToCss` table
  branches, (optional) wrap-CSS helper.
- `ui/pdf-export.html` — two new `<select>` controls.
- `ui/pdf-export.js` — reflect + change-listener wiring for both selects.
- `ui/app.js` — gate `fitWideTablesForPrint()` on `tableFit`; inject wrap CSS on
  the PDF path and PDF live preview when `tableFit === "wrap"`.
- `src-tauri/src/recent.rs` — `PdfSettings` fields, serde defaults, `Default`
  impl, round-trip test.
- Test files for `pdf-presets.js`.
