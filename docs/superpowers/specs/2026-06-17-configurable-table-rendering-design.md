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

Give the user control over how tables render in exports:

1. **Table style** — choose the visual treatment (Editorial / Grid / Minimal).
2. **Wide tables** — choose how a too-wide table reconciles with page width
   (Scale to fit / Wrap text).
3. **Long tables** — repeat the header row and paginate cleanly across pages,
   instead of running off the page edge with no header (always on).
4. **Orientation** — export in Portrait or Landscape, so very wide tables can
   use the long edge of the page.

## Scope decisions (resolved during brainstorming)

- **Table style applies to PDF *and* HTML export**, not the on-screen viewer.
  The on-screen viewer and split editor keep today's github grid.
- **All page-geometry behavior is PDF-only** (wide-table fit, long-table
  pagination, orientation). Reconciling content with a fixed paginated page has
  no meaning in an HTML document viewed at arbitrary width, and the relevant
  code already runs only on the PDF path.
- **Defaults change intentionally:** Editorial style + Wrap text become the new
  defaults. Existing users' wide tables will start wrapping instead of
  shrinking; "Grid" + "Scale to fit" are the escape hatches back to today's
  behavior.

## Why style is separate from the PDF-page concerns

There are two kinds of concern here, and they live in different code paths:

- **Paint** — table *style* (borders, zebra, header treatment). It rides in the
  shared `settingsToCss`, which already feeds both the PDF print and the HTML
  export builder, so it reaches both formats with no extra wiring.
- **Page geometry** — fit (wide-table reconciliation), long-table pagination,
  and orientation. These are all about reconciling content with a fixed,
  paginated page, which only exists for PDF. They live on the PDF export path,
  *outside* `settingsToCss`, so HTML export is unaffected.

Keeping paint separate from page geometry is what lets "style spans both
formats" and "everything page-related is PDF-only" both be true without
conditional branching inside the shared CSS.

## The controls

The user-facing selects are added to the PDF export window (`pdf-export.html`),
below the existing Margins / Page-numbers selects, each updating the live
preview on change. Long-table pagination has no control — it is always-on
behavior.

| Control      | Values                        | Default     | Applies to          |
| ------------ | ----------------------------- | ----------- | ------------------- |
| Table style  | Editorial · Grid · Minimal    | Editorial   | PDF and HTML export |
| Wide tables  | Scale to fit · Wrap text      | Wrap text   | PDF only            |
| Orientation  | Portrait · Landscape          | Portrait    | PDF only            |
| (Long tables)| always on (no control)        | —           | PDF only            |

## Design

### 1. Settings & persistence

Add three fields to the exported/persisted settings object:

- `tableStyle`: `"editorial" | "grid" | "minimal"`
- `tableFit`: `"fit" | "wrap"`
- `orientation`: `"portrait" | "landscape"`

**Frontend (`pdf-presets.js`):**

- Add `tableStyle`, `tableFit`, and `orientation` to each preset record. All
  three presets (clean, report, compact) default to `editorial` + `wrap` +
  `portrait`. (Presets may differ later; uniform defaults are the YAGNI choice
  now.)
- Add all three keys to the persisted settings subset returned by
  `settingsFromPreset` / `presetDefaults`, so they round-trip through
  `get_pdf_settings` / `save_pdf_settings` and are restored by "Reset to
  preset".

**Backend (`recent.rs::PdfSettings`):**

- Add `table_style: String`, `table_fit: String`, and `orientation: String`.
- Each gets `#[serde(default = "…")]` returning `"editorial"` / `"wrap"` /
  `"portrait"` so settings JSON persisted by older versions (which lack the
  fields) deserializes cleanly.
- Update the `Default` impl to set all three.

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

### 4. Long-table pagination (always on, PDF only)

Today two rules sabotage tables that are taller than one page:

- github-markdown.css sets `.markdown-body table { display: block }`, which
  disables the browser's automatic header-row repetition (that needs
  `display: table` + `thead { display: table-header-group }`).
- our `@media print` block sets `.markdown-body table { break-inside: avoid }`,
  telling a too-tall table never to split — which it cannot honor, so it
  overruns or clips at the page edge with no repeated header.

Fix, applied on the PDF path only (so the on-screen viewer's horizontal-scroll
table behavior is preserved):

- Lay tables out as `display: table` (not `block`) during print so pagination
  and header groups work. In **Wrap** mode this already happens; the change is
  to also apply it in the default flow for tables that aren't being scaled.
- `thead { display: table-header-group }` so the header repeats on every page a
  table spans.
- Drop `break-inside: avoid` on the table itself for tables taller than a page,
  letting them paginate. (Tables shorter than a page still sit on one page
  naturally.)
- `tr { break-inside: avoid }` so an individual row never tears across a page
  boundary (idea C).

Interaction with the fit knob: **Scale to fit** renders the table as a single
scaled unit (`transform: scale`), which is atomic and cannot paginate — header
repetition does not apply there, and that is fine (a scaled table fits on one
page by construction). Header repetition and clean breaks are therefore a
**Wrap-mode** benefit, which is the new default. No new control; it is part of
how Wrap renders.

### 5. Orientation (Portrait / Landscape, PDF only)

A `<select>` for page orientation. Portrait is the default and today's only
behavior.

- **Native (`export.rs` / `pdf_postprocess.rs`):** `export_pdf` gains an
  `orientation` (or `landscape: bool`) argument. `paper_points` returns the
  portrait `(w, h)`; when landscape, swap to `(h, w)` before
  `info.setPaperSize(...)` and before the post-process page geometry. This is
  the load-bearing change — paper size is applied natively, not via CSS.
- **Live preview (`wrapInPageSheet`):** swap the `paperMm` width/height when
  landscape so the simulated page sheet matches.
- **Scale-to-fit threshold (`PRINT_CONTENT_WIDTH_PX`, `app.js`):** this constant
  is currently hardcoded to A4 portrait content width (and already ignores the
  configurable margins — a pre-existing limitation). Landscape makes it more
  wrong. The plan should derive the threshold from the actual paper width minus
  margins and account for orientation, rather than leave the constant. This
  matters only for **Scale to fit**; Wrap mode does not read it.
- Margins are unchanged (the same top/right/bottom/left values apply to the
  rotated page).

HTML export ignores orientation (an HTML document has no fixed page geometry),
consistent with the fit knob.

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
- **`recent.rs`**: round-trip test proving JSON without `tableStyle` /
  `tableFit` / `orientation` loads with defaults `editorial` / `wrap` /
  `portrait`, and a full round-trip preserving explicit non-default values
  (camelCase serialization).
- **`pdf_postprocess.rs`**: assert the portrait→landscape dimension swap
  (e.g. A4 landscape is `(841.89, 595.28)`), and that the default/unknown paper
  still falls back correctly in both orientations.

## Out of scope

- Per-table overrides (a global per-export choice only).
- Changing the on-screen viewer's table style.
- Wide-table behavior and orientation for HTML export (style only spans both).
- Additional table knobs (border color, padding, zebra toggle, etc.) — a single
  named style + a single fit mode only.
- A control for long-table pagination — it is always-on behavior, not a toggle.

## Files touched

- `ui/pdf-presets.js` — settings keys, preset records, `settingsToCss` table
  branches, (optional) wrap-CSS helper.
- `ui/pdf-export.html` — three new `<select>` controls (table style, wide
  tables, orientation).
- `ui/pdf-export.js` — reflect + change-listener wiring for the three selects.
- `ui/app.js` — gate `fitWideTablesForPrint()` on `tableFit`; inject wrap +
  long-table pagination CSS (`display:table`, `thead` header group, row
  `break-inside:avoid`, drop table `break-inside:avoid`) on the PDF path and PDF
  live preview; pass `orientation` to `export_pdf`; make
  `PRINT_CONTENT_WIDTH_PX` paper/margin/orientation-aware; swap `wrapInPageSheet`
  dimensions for landscape.
- `ui/styles.css` — adjust the `@media print` table rules (header group, row
  break-inside) so they cooperate with the JS-applied layout.
- `src-tauri/src/export.rs` — `export_pdf` `orientation` argument; landscape
  dimension swap before `setPaperSize` and post-process.
- `src-tauri/src/pdf_postprocess.rs` — orientation-aware paper dimensions +
  test.
- `src-tauri/src/recent.rs` — `PdfSettings` fields, serde defaults, `Default`
  impl, round-trip test.
- Test files for `pdf-presets.js`.
