# Configurable Table Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user control table appearance in exported documents — a Table style (Editorial/Grid/Minimal), a wide-table fit mode (Wrap text/Scale to fit), page Orientation (Portrait/Landscape), plus always-on repeating-header pagination for long tables.

**Architecture:** Table *style* is a paint concern that rides the shared `settingsToCss` (reaching both PDF and HTML export). Fit, orientation, and long-table pagination are page-geometry concerns that live only on the PDF export path, leaving HTML export untouched. New settings persist through the existing `PdfSettings`/`get_pdf_settings`/`save_pdf_settings` plumbing; the PDF export window gains three `<select>` controls.

**Tech Stack:** Tauri 2 (Rust, macOS objc2 print pipeline), vanilla JS frontend (no build step for tests — `node --test`), serde for settings persistence.

**Test commands:**
- Rust: `cd src-tauri && cargo test --all-features`
- JS: `node --test ui/*.test.js` (run from repo root)
- Lint before any commit: `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings`

**Background the implementer must know:**
- Frontend changes do not show in the running app until `cargo build` (Tauri bundles `ui/` at compile time). Tests run against `ui/*.js` directly and need no build.
- `settingsToCss` (in `ui/pdf-presets.js`) is injected during export only and is shared by PDF and HTML. Anything PDF-only must NOT go there.
- `@media print` rules in `ui/styles.css` apply during the native PDF capture only (never on screen), so they are a safe place for PDF-only table behavior.
- Pure JS helpers live in `ui/pdf-presets.js` and are unit-tested in `ui/pdf-presets.test.js`. DOM wiring lives in `ui/app.js` / `ui/pdf-export.js` and is verified by build + manual smoke test.

---

## File structure

- `src-tauri/src/recent.rs` — `PdfSettings` struct gains `table_style`, `table_fit`, `orientation` (serde defaults + `Default` impl + tests).
- `src-tauri/src/pdf_postprocess.rs` — new pure `oriented(paper, landscape)` helper + test.
- `src-tauri/src/export.rs` — `export_pdf` gains a `landscape: bool` arg; swaps paper dimensions via `oriented`.
- `ui/pdf-presets.js` — settings keys on each preset + in the persisted subset; new pure `tableStyleCss` (folded into `settingsToCss`) and `tableFitCss` helpers.
- `ui/pdf-presets.test.js` — tests for the above.
- `ui/pdf-export.html` — three new `<select>` controls.
- `ui/pdf-export.js` — reflect + change-listener wiring for the three selects.
- `ui/styles.css` — `@media print` table rules: `display: table`, `thead` header group, per-row `break-inside: avoid`, stop avoiding break-inside on the table itself.
- `ui/app.js` — gate `fitWideTablesForPrint` on fit mode; inject `tableFitCss` on the PDF path; pass `landscape` to `export_pdf`; make the print content-width orientation-aware; reflect orientation/wrap in the live preview sheet.

---

## Task 1: Rust — add the three settings fields to `PdfSettings`

**Files:**
- Modify: `src-tauri/src/recent.rs:19-39` (struct + `Default`), `:367-384` (existing round-trip test)
- Test: `src-tauri/src/recent.rs` (tests module, near line 357)

- [ ] **Step 1: Write the failing test**

Add these two tests inside the `#[cfg(test)] mod tests` block (after `store_defaults_pdf_settings_when_absent`, before its closing `}` at line 391):

```rust
    #[test]
    fn pdf_settings_default_has_new_table_fields() {
        let s = PdfSettings::default();
        assert_eq!(s.table_style, "editorial");
        assert_eq!(s.table_fit, "wrap");
        assert_eq!(s.orientation, "portrait");
    }

    #[test]
    fn pdf_settings_old_json_gets_new_field_defaults() {
        // Settings persisted by a version without the new fields must still load.
        let json = r#"{"preset":"clean","baseSize":11.0,"paper":"a4","margins":"normal","pageNumbers":"bottom-center"}"#;
        let s: PdfSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.table_style, "editorial");
        assert_eq!(s.table_fit, "wrap");
        assert_eq!(s.orientation, "portrait");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test --all-features pdf_settings 2>&1 | tail -20`
Expected: compile error (`table_style` / `table_fit` / `orientation` fields don't exist on `PdfSettings`).

- [ ] **Step 3: Add the fields, serde defaults, and `Default` impl**

Replace the struct + `Default` impl at `src-tauri/src/recent.rs:19-39` with:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfSettings {
    pub preset: String,
    pub base_size: f64,
    pub paper: String,
    pub margins: String,
    pub page_numbers: String,
    #[serde(default = "default_table_style")]
    pub table_style: String,
    #[serde(default = "default_table_fit")]
    pub table_fit: String,
    #[serde(default = "default_orientation")]
    pub orientation: String,
}

fn default_table_style() -> String {
    "editorial".into()
}
fn default_table_fit() -> String {
    "wrap".into()
}
fn default_orientation() -> String {
    "portrait".into()
}

impl Default for PdfSettings {
    fn default() -> Self {
        Self {
            preset: "clean".into(),
            base_size: 11.0,
            paper: "a4".into(),
            margins: "normal".into(),
            page_numbers: "bottom-center".into(),
            table_style: default_table_style(),
            table_fit: default_table_fit(),
            orientation: default_orientation(),
        }
    }
}
```

- [ ] **Step 4: Fix the existing round-trip test so it compiles**

The struct literal in `pdf_settings_round_trip_camel_case` (line ~369) now misses three fields. Replace that test's struct construction:

```rust
        let s = PdfSettings {
            preset: "report".into(),
            base_size: 12.5,
            paper: "letter".into(),
            margins: "wide".into(),
            page_numbers: "bottom-right".into(),
            table_style: "minimal".into(),
            table_fit: "fit".into(),
            orientation: "landscape".into(),
        };
```

And add two assertions after the existing `assert_eq!(back.preset, "report");`:

```rust
        assert!(json.contains("\"tableStyle\":\"minimal\""), "got: {json}");
        assert_eq!(back.orientation, "landscape");
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --all-features pdf_settings 2>&1 | tail -20`
Expected: all `pdf_settings*` tests PASS.

- [ ] **Step 6: Lint + commit**

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
cd /Users/laek/source/mdviewer
git add src-tauri/src/recent.rs
git commit -m "Add table_style/table_fit/orientation to PdfSettings"
```

---

## Task 2: Rust — orientation-aware paper dimensions

**Files:**
- Modify: `src-tauri/src/pdf_postprocess.rs:85-90` (add `oriented` next to `paper_points`), tests module (near line 291)
- Modify: `src-tauri/src/export.rs:20-37` (`export_pdf` signature), `:57-70` (macos `export` paper resolution), `:32-35` (non-macos stub)
- Modify: `ui/app.js` (the `export_pdf` invoke — done in Task 6; the Rust arg is added here)

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src-tauri/src/pdf_postprocess.rs` (next to `paper_points_known_and_fallback`):

```rust
    #[test]
    fn oriented_swaps_dimensions_for_landscape() {
        assert_eq!(oriented("a4", false), (595.28, 841.89));
        assert_eq!(oriented("a4", true), (841.89, 595.28));
        assert_eq!(oriented("letter", false), (612.0, 792.0));
        assert_eq!(oriented("letter", true), (792.0, 612.0));
        // Unknown paper still falls back to A4, swapped when landscape.
        assert_eq!(oriented("garbage", true), (841.89, 595.28));
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test --all-features oriented 2>&1 | tail -20`
Expected: compile error (`oriented` not found).

- [ ] **Step 3: Implement `oriented`**

Add immediately after the `paper_points` function (after its closing `}` near line 90) in `src-tauri/src/pdf_postprocess.rs`:

```rust
/// Portrait paper dimensions from `paper_points`, swapped to landscape when
/// requested. Width/height are in PostScript points.
pub fn oriented(paper: &str, landscape: bool) -> (f64, f64) {
    let (w, h) = paper_points(paper);
    if landscape {
        (h, w)
    } else {
        (w, h)
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --all-features oriented 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Thread `landscape` through `export_pdf`**

In `src-tauri/src/export.rs`, change the command signature and both branches. Replace lines 20-37:

```rust
#[tauri::command]
pub async fn export_pdf(
    window: tauri::WebviewWindow,
    path: String,
    paper: String,
    margins: Margins,
    page_numbers: String,
    landscape: bool,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::export(window, path, paper, margins, page_numbers, landscape).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, path, paper, margins, page_numbers, landscape);
        Err("PDF export is not yet supported on Windows".to_string())
    }
}
```

In the macos `export` function, add the parameter (line 57-63) and use `oriented`. Change the signature to add `landscape: bool` after `page_numbers: String`:

```rust
    pub async fn export(
        window: tauri::WebviewWindow,
        path: String,
        paper: String,
        margins: super::Margins,
        page_numbers: String,
        landscape: bool,
    ) -> Result<(), String> {
```

And replace line 70:

```rust
        let (paper_w, paper_h) = pdf_postprocess::oriented(&paper, landscape);
```

- [ ] **Step 6: Build to verify Rust compiles (both cfgs as available)**

Run: `cd src-tauri && cargo build 2>&1 | tail -15`
Expected: builds (warnings about the unused frontend arg are fine; `landscape` is consumed in both branches).

- [ ] **Step 7: Lint + commit**

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
cd /Users/laek/source/mdviewer
git add src-tauri/src/pdf_postprocess.rs src-tauri/src/export.rs
git commit -m "Add landscape orientation to PDF export geometry"
```

---

## Task 3: JS — settings keys + table-style/fit CSS helpers

**Files:**
- Modify: `ui/pdf-presets.js` (PRESETS records, `settingsFromPreset`, `settingsToCss`; add `tableStyleCss` + `tableFitCss`)
- Test: `ui/pdf-presets.test.js`

- [ ] **Step 1: Write the failing tests**

Append to `ui/pdf-presets.test.js` (and add `tableStyleCss, tableFitCss` to the import block at the top):

```js
test("presetDefaults includes the new table + orientation keys", () => {
  const s = presetDefaults("clean");
  for (const k of ["tableStyle", "tableFit", "orientation"]) {
    assert.ok(k in s, `missing ${k}`);
  }
});

test("defaults are editorial / wrap / portrait", () => {
  const s = defaultSettings();
  assert.equal(s.tableStyle, "editorial");
  assert.equal(s.tableFit, "wrap");
  assert.equal(s.orientation, "portrait");
});

test("editorial style: bounding rules, no zebra, no full grid", () => {
  const css = settingsToCss(presetDefaults("clean")); // clean => editorial
  assert.match(css, /\.markdown-body table\s*\{[^}]*border-top:\s*2px solid/);
  assert.match(css, /tr:nth-child\(2n\)\s*\{\s*background-color:\s*transparent/);
});

test("grid style keeps the accent header tint", () => {
  const css = settingsToCss(mergeSettings(defaultSettings(), { tableStyle: "grid" }));
  assert.match(css, /table th\s*\{\s*background:\s*color-mix\(in srgb, var\(--pdf-accent\)/);
});

test("minimal style underlines the header but draws no table top rule", () => {
  const css = settingsToCss(mergeSettings(defaultSettings(), { tableStyle: "minimal" }));
  assert.match(css, /table th\s*\{[^}]*border-bottom:\s*2px solid/);
  assert.doesNotMatch(css, /\.markdown-body table\s*\{[^}]*border-top:\s*2px solid/);
});

test("tableFitCss emits wrap layout only in wrap mode", () => {
  const wrap = tableFitCss(mergeSettings(defaultSettings(), { tableFit: "wrap" }));
  assert.match(wrap, /display:\s*table/);
  assert.match(wrap, /table-layout:\s*fixed/);
  assert.match(wrap, /overflow-wrap:\s*anywhere/);
  assert.equal(tableFitCss(mergeSettings(defaultSettings(), { tableFit: "fit" })), "");
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `node --test ui/pdf-presets.test.js 2>&1 | tail -20`
Expected: failures — `tableStyleCss`/`tableFitCss` import undefined and the new keys are absent.

- [ ] **Step 3: Add the keys to every preset record**

In `ui/pdf-presets.js`, add three properties to each of the three preset objects in `PRESETS` (clean, report, compact). For all three, insert after their `pageNumbers` line:

```js
    tableStyle: "editorial",
    tableFit: "wrap",
    orientation: "portrait",
```

- [ ] **Step 4: Add the keys to the persisted subset**

Replace the returned object in `settingsFromPreset` (the `return { ... }` inside it) with:

```js
  return {
    preset: PRESETS[id] ? id : "clean",
    baseSize: p.baseSize,
    paper: p.paper,
    margins: p.margins,
    pageNumbers: p.pageNumbers,
    tableStyle: p.tableStyle,
    tableFit: p.tableFit,
    orientation: p.orientation,
  };
```

- [ ] **Step 5: Add `tableStyleCss` and `tableFitCss`, and call `tableStyleCss` from `settingsToCss`**

In `settingsToCss`, replace the single line:

```js
.markdown-body table th { background: color-mix(in srgb, var(--pdf-accent) 12%, transparent); }
```

with:

```js
${tableStyleCss(settings)}
```

Then add these two exported functions at the end of `ui/pdf-presets.js`:

```js
/** Paint-only table CSS by style. Appended after github-markdown.css (equal
 *  specificity, later wins), so it overrides the base grid. Shared by PDF and
 *  HTML export via settingsToCss. */
export function tableStyleCss(settings) {
  switch (settings.tableStyle) {
    case "grid":
      // Re-assert today's look: github keeps full borders + zebra; add the
      // accent header tint that used to be emitted unconditionally.
      return `.markdown-body table th { background: color-mix(in srgb, var(--pdf-accent) 12%, transparent); }`;
    case "minimal":
      return `.markdown-body table th, .markdown-body table td { border: 0; }
.markdown-body table th { border-bottom: 2px solid var(--borderColor-default, #d0d7de); font-weight: 700; }
.markdown-body table tr { background-color: transparent; border-top: 0; }`;
    case "editorial":
    default:
      return `.markdown-body table { border-top: 2px solid var(--fgColor-default, #1f2328); border-bottom: 2px solid var(--fgColor-default, #1f2328); }
.markdown-body table th, .markdown-body table td { border: 0; }
.markdown-body table th { border-bottom: 1px solid var(--fgColor-default, #1f2328); font-weight: 700; }
.markdown-body table td { border-bottom: 1px solid var(--borderColor-muted, #d8dee4); }
.markdown-body table tr { background-color: transparent; border-top: 0; }
.markdown-body table tr:nth-child(2n) { background-color: transparent; }`;
  }
}

/** Page-geometry table CSS for Wrap mode: hold the table to the page width and
 *  wrap cell text (table grows taller) instead of scaling it down. Empty in Fit
 *  mode (the JS scaler handles that). PDF-only — injected by app.js, never by
 *  settingsToCss, so HTML export is unaffected. */
export function tableFitCss(settings) {
  if (settings.tableFit !== "wrap") return "";
  return `.markdown-body table { display: table; width: 100%; table-layout: fixed; }
.markdown-body table th, .markdown-body table td { overflow-wrap: anywhere; white-space: normal; }`;
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `node --test ui/pdf-presets.test.js 2>&1 | tail -10`
Expected: all PASS (including the pre-existing tests).

- [ ] **Step 7: Run the whole JS suite (no regressions)**

Run: `node --test ui/*.test.js 2>&1 | tail -6`
Expected: `# fail 0`.

- [ ] **Step 8: Commit**

```bash
git add ui/pdf-presets.js ui/pdf-presets.test.js
git commit -m "Add table style + fit CSS helpers and settings keys"
```

---

## Task 4: PDF export window — three new controls

**Files:**
- Modify: `ui/pdf-export.html:44` (after the page-numbers `<label>`)
- Modify: `ui/pdf-export.js` (`reflect`, change listeners)

No unit tests (DOM wiring); verified by build + smoke test in Task 7.

- [ ] **Step 1: Add the three selects to the HTML**

In `ui/pdf-export.html`, insert directly after the closing `</label>` of the Page-numbers control (line 44, before the `<div class="pdf-actions">`):

```html
        <label>Table style
          <select id="table-style">
            <option value="editorial">Editorial</option>
            <option value="grid">Grid</option>
            <option value="minimal">Minimal</option>
          </select>
        </label>

        <label>Wide tables
          <select id="table-fit">
            <option value="wrap">Wrap text</option>
            <option value="fit">Scale to fit</option>
          </select>
        </label>

        <label>Orientation
          <select id="orientation">
            <option value="portrait">Portrait</option>
            <option value="landscape">Landscape</option>
          </select>
        </label>
```

- [ ] **Step 2: Reflect the new settings into the selects**

In `ui/pdf-export.js`, add three lines to the end of the `reflect()` function body (after `el("page-numbers").value = settings.pageNumbers;`):

```js
  el("table-style").value = settings.tableStyle;
  el("table-fit").value = settings.tableFit;
  el("orientation").value = settings.orientation;
```

- [ ] **Step 3: Wire change listeners**

In `ui/pdf-export.js`, add after the existing `el("page-numbers").addEventListener(...)` line:

```js
el("table-style").addEventListener("change", (e) => update({ tableStyle: e.target.value }));
el("table-fit").addEventListener("change", (e) => update({ tableFit: e.target.value }));
el("orientation").addEventListener("change", (e) => update({ orientation: e.target.value }));
```

- [ ] **Step 4: Commit**

```bash
git add ui/pdf-export.html ui/pdf-export.js
git commit -m "Add table style / wide-table / orientation controls to PDF export window"
```

---

## Task 5: `@media print` — long-table pagination

**Files:**
- Modify: `ui/styles.css:952-967` (the `@media print` table/row rules)

No unit tests (CSS); verified by smoke test in Task 7.

- [ ] **Step 1: Remove the table from the break-inside-avoid group and add pagination rules**

In `ui/styles.css`, the `@media print` block currently has (around line 952):

```css
  /* Never split an atomic block across a page boundary. */
  .markdown-body pre,
  .markdown-body table,
  .markdown-body img,
  .markdown-body blockquote,
  .markdown-body pre.mermaid,
  .markdown-body .katex-display {
    break-inside: avoid;
  }
```

Replace that whole rule with (drop `table` from the avoid list, add the pagination rules):

```css
  /* Never split an atomic block across a page boundary. Tables are excluded —
     they are allowed to paginate (see below) so long tables don't clip. */
  .markdown-body pre,
  .markdown-body img,
  .markdown-body blockquote,
  .markdown-body pre.mermaid,
  .markdown-body .katex-display {
    break-inside: avoid;
  }

  /* Long tables: lay out as a real table (github-markdown sets display:block,
     which disables header-group repetition), repeat the header on every page,
     and keep individual rows intact. display:table here only affects the print
     capture, so the on-screen horizontal-scroll behavior is preserved. */
  .markdown-body table {
    display: table;
  }
  .markdown-body thead {
    display: table-header-group;
  }
  .markdown-body tr {
    break-inside: avoid;
  }
```

- [ ] **Step 2: Commit**

```bash
git add ui/styles.css
git commit -m "Paginate long tables in PDF with a repeating header row"
```

---

## Task 6: `app.js` — wire fit/wrap, orientation, and the live preview

**Files:**
- Modify: `ui/app.js:41-47` (import), `:2069-2080` (PDF export branch), `:2112-2115` (preview branch), `:2126-2145` (`wrapInPageSheet`), `:2308-2317` (content-width constant → function), `:2327-2360` (`fitWideTablesForPrint` signature)

No unit tests (DOM/print wiring); verified by build + smoke test in Task 7.

- [ ] **Step 1: Import `tableFitCss`**

In `ui/app.js`, add `tableFitCss,` to the `from "./pdf-presets.js"` import block (after `mergeSettings,`):

```js
import {
  settingsToCss,
  marginMm,
  paperMm,
  defaultSettings,
  mergeSettings,
  tableFitCss,
} from "./pdf-presets.js";
```

- [ ] **Step 2: Replace the content-width constant with an orientation-aware function**

Replace `ui/app.js:2317`:

```js
const PRINT_CONTENT_WIDTH_PX = Math.round(((210 - 2 * 25.4) * 96) / 25.4) - 2 * 48 - 8;
```

with (keep the comment block above it; it still explains the 25.4mm WKWebView default and the padding/safety subtractions):

```js
/** Printable width (px) available to the markdown body when shrinking wide
 *  tables to fit. Generalizes the old A4-portrait constant: uses the chosen
 *  paper's width in the active orientation (landscape uses the long edge),
 *  minus WKWebView's ~1in default margins, the .markdown-body padding, and an
 *  8px safety buffer. Only consulted in Scale-to-fit mode. */
function printContentWidthPx(settings) {
  const paper = paperMm(settings.paper);
  const landscape = settings.orientation === "landscape";
  const pageWmm = landscape ? paper.h : paper.w;
  return Math.round(((pageWmm - 2 * 25.4) * 96) / 25.4) - 2 * 48 - 8;
}
```

- [ ] **Step 3: Make `fitWideTablesForPrint` take the width as a parameter**

In `ui/app.js`, change the function signature (line ~2327) from `function fitWideTablesForPrint() {` to:

```js
function fitWideTablesForPrint(contentWidthPx) {
```

Then inside its body replace the three uses of `PRINT_CONTENT_WIDTH_PX` with `contentWidthPx`:
- the comparison `if (rect.width <= PRINT_CONTENT_WIDTH_PX + 1) {` → `if (rect.width <= contentWidthPx + 1) {`
- the scale `const scale = PRINT_CONTENT_WIDTH_PX / rect.width;` → `const scale = contentWidthPx / rect.width;`
- the wrapper width `wrap.style.width = \`${PRINT_CONTENT_WIDTH_PX}px\`;` → `wrap.style.width = \`${contentWidthPx}px\`;`

- [ ] **Step 4: Gate fit vs wrap in the PDF export branch and pass `landscape`**

Replace the PDF branch body in `exportDocument` (`ui/app.js:2069-2080`):

```js
      await neutralizeOutsideWorkspaceImages(preview, boundary);
      fittedTables = fitWideTablesForPrint();
      headingWraps = keepHeadingsWithNext();
      await invoke("export_pdf", {
        path,
        paper: settings.paper,
        margins: marginMm(settings.margins),
        pageNumbers: settings.pageNumbers,
      });
```

with:

```js
      await neutralizeOutsideWorkspaceImages(preview, boundary);
      // Fit mode scales wide tables down as a unit; Wrap mode skips the scaler
      // and injects wrap CSS (PDF-only — appended to the export style element,
      // never into the shared settingsToCss / HTML output).
      if (settings.tableFit === "fit") {
        fittedTables = fitWideTablesForPrint(printContentWidthPx(settings));
      } else {
        styleEl.textContent += "\n" + tableFitCss(settings);
      }
      headingWraps = keepHeadingsWithNext();
      await invoke("export_pdf", {
        path,
        paper: settings.paper,
        margins: marginMm(settings.margins),
        pageNumbers: settings.pageNumbers,
        landscape: settings.orientation === "landscape",
      });
```

- [ ] **Step 5: Gate fit in the live-preview render**

Replace in `renderExportPreviewHtml` (`ui/app.js:2112-2114`):

```js
    await neutralizeOutsideWorkspaceImages(preview, boundary);
    fitted = fitWideTablesForPrint();
    const body = await buildExportHtml(t, boundary, settings);
```

with:

```js
    await neutralizeOutsideWorkspaceImages(preview, boundary);
    if (settings.tableFit === "fit") {
      fitted = fitWideTablesForPrint(printContentWidthPx(settings));
    }
    const body = await buildExportHtml(t, boundary, settings);
```

- [ ] **Step 6: Reflect orientation + wrap in the preview page sheet**

Replace `wrapInPageSheet` (`ui/app.js:2126-2145`) with:

```js
function wrapInPageSheet(docHtml, settings) {
  const paper = paperMm(settings.paper);
  const m = marginMm(settings.margins);
  const landscape = settings.orientation === "landscape";
  const sheetW = landscape ? paper.h : paper.w;
  const sheetH = landscape ? paper.w : paper.h;
  // Wrap mode needs its layout CSS in the preview too (settingsToCss carries
  // only the paint style, not the PDF-only fit behavior).
  const wrapCss = settings.tableFit === "wrap" ? "\n" + tableFitCss(settings) : "";
  // The sheet fits the preview width (reflowing when the pane is narrower than
  // the paper) so the whole page is always visible — the Exact PDF tab is the
  // pixel-faithful, paginated view. min-height keeps the empty-page proportions.
  const sheetCss = `
  body { background: #777; margin: 0; padding: 16px; }
  article.markdown-body {
    background: #fff;
    width: 100%;
    max-width: ${sheetW}mm;
    min-height: ${sheetH}mm;
    margin: 0 auto;
    padding-top: ${m.top}mm;
    padding-bottom: ${m.bottom}mm;
    box-shadow: 0 2px 16px rgba(0,0,0,.4);
  }${wrapCss}`;
  return docHtml.replace("</head>", `<style>${sheetCss}</style></head>`);
}
```

- [ ] **Step 7: Syntax-check app.js, then build**

`app.js` is not imported by any test, so guard its syntax directly (parses without executing, safe despite browser globals):

Run: `node --check ui/app.js && echo OK`
Expected: `OK` (no syntax errors).

Run: `node --test ui/*.test.js 2>&1 | tail -6`
Expected: `# fail 0` (no regressions in the tested pure modules).

Run: `cd src-tauri && cargo build 2>&1 | tail -15`
Expected: builds successfully (bundles the updated `ui/`).

- [ ] **Step 8: Commit**

```bash
git add ui/app.js
git commit -m "Wire wide-table fit/wrap, landscape, and live preview"
```

---

## Task 7: Build, GUI smoke test, and final verification

**Why a manual GUI pass:** prior UI features shipped visual/theme regressions that automated tests didn't catch. Run the app and look at real output before declaring done.

- [ ] **Step 1: Full test + lint sweep**

```bash
node --test ui/*.test.js 2>&1 | tail -6
cd src-tauri && cargo test --all-features 2>&1 | tail -10
cargo fmt --check && cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
```
Expected: JS `# fail 0`; Rust tests pass; clippy clean.

- [ ] **Step 2: Run the app on a document with tables**

```bash
cd src-tauri && cargo run -- ../README.md
```

(If a richer table is needed, open any markdown file containing a wide multi-column table and a long many-row table.)

- [ ] **Step 3: Exercise the PDF export window**

Open **File ▸ Export as PDF…** and verify, watching the live preview update on each change:
- Table style **Editorial** (default): no vertical cell borders, top/bottom rules, ruled bold header, no zebra.
- Switch to **Grid**: full cell borders + zebra + tinted header return.
- Switch to **Minimal**: header underline only, zebra kept, no other borders.
- **Wide tables = Wrap text** (default): a wide table's cells wrap, table stays full-size; switch to **Scale to fit**: the wide table shrinks as a unit.
- **Orientation = Landscape**: the preview sheet turns landscape; export and confirm the saved PDF is landscape.
- Export a PDF of a **long** table and confirm the header row repeats on each page and rows aren't split across the page break.

- [ ] **Step 4: Confirm HTML export honors style but not geometry**

Open **File ▸ Export as HTML…**, open the resulting `.html` in a browser, and confirm the table uses the selected **style** (e.g. Editorial), and that it is NOT scaled/wrapped to a fake page width (HTML has no page geometry).

- [ ] **Step 5: Confirm persistence**

Change the three controls, export, close and reopen the PDF export window, and confirm the selects show the last-used values (round-tripped through `save_pdf_settings`/`get_pdf_settings`).

- [ ] **Step 6: Update docs**

- `README.md`: mention the new Table style / Wide-tables / Orientation export options under the export feature description.
- `CLAUDE.md`: extend the **Export (HTML)** / PDF architecture note to mention the table style (paint, shared `settingsToCss`) vs fit/orientation/pagination (page-geometry, PDF-only) split.

- [ ] **Step 7: Commit docs**

```bash
git add README.md CLAUDE.md
git commit -m "Document configurable table rendering in exports"
```

- [ ] **Step 8: Finish the branch**

Use the superpowers:finishing-a-development-branch skill to decide how to integrate `feature/configurable-table-rendering` (merge / PR).

---

## Self-review notes (for the implementer)

- **Spec coverage:** Task 1 = settings persistence; Task 2 = orientation native; Task 3 = table style + fit CSS (style §2, fit §3); Task 4 = controls; Task 5 = long-table pagination §4; Task 6 = fit/wrap/orientation wiring + preview; Task 7 = verification + docs. Every spec section maps to a task.
- **Type/name consistency:** the JS settings keys are `tableStyle`, `tableFit`, `orientation` everywhere; the Rust fields are the snake_case `table_style`, `table_fit`, `orientation` with serde `camelCase` rename, so the JSON keys match the JS. The `export_pdf` invoke arg `landscape` (JS) maps to the `landscape: bool` Rust param.
- **Default behavior change:** existing users get Editorial + Wrap by default (intended). Grid + Scale-to-fit reproduce today's output.
