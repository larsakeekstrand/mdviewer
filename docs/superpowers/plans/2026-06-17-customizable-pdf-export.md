# Customizable PDF Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the blind "print the preview" PDF export with a settings-driven, professionally-styled export tuned in a dedicated window with a live preview.

**Architecture:** A pure JS preset/settings module (`ui/pdf-presets.js`) drives a `.markdown-body`-scoped export stylesheet. A separate `pdf-export` webview window (like `preferences`) collects settings and shows a live HTML preview plus an exact PDF preview; the **main window** owns all rendering and printing (the print pipeline can only print the main window's WKWebView). Page geometry the print engine refuses (top/bottom margins, page numbers) is applied by a native Core Graphics post-processing pass after WebKit writes a content PDF.

**Tech Stack:** Tauri 2.11, vanilla JS (no build step, ES modules + `node --test`), Rust 2021, objc2 0.3 framework crates (App Kit / Web Kit / Core Graphics), `serde`.

## Global Constraints

- **macOS-only** for PDF: the `pdf-export` window, its menu item, and all native code stay `#[cfg(target_os = "macos")]` / gated on `IS_MAC`. Windows keeps HTML export only.
- **No build step / no new vendored fonts.** System font stacks only. KaTeX fonts remain embedded as `data:` URLs.
- **objc2 framework crates pinned to `0.3`** (core `objc2` is `0.6`) — must match Tauri 2.11's generation or the `inner()`-pointer cast breaks.
- **CSP unchanged:** `script-src 'self'` (no inline scripts/handlers); window JS is an external module. `style-src` keeps `'unsafe-inline'`. PDF iframes load via the asset protocol (already allowed for images; the preview iframe uses `src` to a `convertFileSrc`-mapped temp file or `srcdoc`).
- **Tauri commands return `Result<T, String>`**; use `format!("…: {e}")`, not bare `?`.
- **Lint clean before every commit:** from `src-tauri/`, `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`. JS pure modules tested with `node --test`.
- **No comments unless the *why* is non-obvious.** Commit messages: imperative subject, NO `Co-Authored-By` trailer.
- **Frontend edits require `cargo build`** to take effect (frontend is bundled at compile time).
- **Settings shape (single source of truth), used identically in JS and Rust:**
  ```
  { preset: "clean"|"report"|"compact",
    baseSize: <number, pt, 9..16>,
    paper: "a4"|"letter"|"legal",
    margins: "narrow"|"normal"|"wide",
    pageNumbers: "none"|"bottom-center"|"bottom-right" }
  ```

---

## File structure

**Create:**
- `ui/pdf-presets.js` — pure preset/settings helpers (presets, merge, margin/paper mm, `settingsToCss`). Unit-tested.
- `ui/pdf-presets.test.js` — `node --test` suite for the above.
- `ui/pdf-export.html` — the tuning window (controls + preview panes).
- `ui/pdf-export.js` — window controller (DOM wiring + event protocol with main window).
- `src-tauri/src/pdf_postprocess.rs` — native Core Graphics re-layout + page-number stamping (macOS) + pure geometry/text helpers (unit-tested).

**Modify:**
- `src-tauri/Cargo.toml` — add `objc2-core-graphics` (macOS target).
- `src-tauri/src/export.rs` — new `export_pdf` signature (paper + margins + page numbers); print to a temp PDF, then call `pdf_postprocess::relayout`.
- `src-tauri/src/recent.rs` — persist `PdfSettings` in the store; load/save helpers + tests.
- `src-tauri/src/commands.rs` — `get_pdf_settings` / `save_pdf_settings` commands.
- `src-tauri/src/menu.rs` — `export-pdf` opens the `pdf-export` window (not the save dialog); add `open_pdf_export_window`.
- `src-tauri/src/lib.rs` — register `pdf_postprocess` module + new commands.
- `src-tauri/capabilities/default.json` — add `"pdf-export"` to the `windows` list.
- `ui/app.js` — refactor `exportDocument` to be settings-driven; extract `buildExportHtml`; add the main-window side of the preview/export event protocol; route `mcp-generate-pdf` through saved settings.
- `ui/styles.css` — minor: nothing structural; PDF look lives in injected CSS from `settingsToCss`. (Touched only if the preset CSS needs a print-rule tweak.)
- `README.md`, `CHANGELOG.md` — user-facing docs.

---

## Task 1: Native PDF re-layout + page numbers (spike + implementation)

This is the riskiest piece, done first. WebKit forces its own ~1-inch print margins and offers no header/footer API (see `ui/app.js:2138`). We therefore print content to a temp PDF, then re-lay each page onto the target paper at the requested margins and stamp page numbers — all via Core Graphics (`CGPDFDocument` to read, `CGContext`/`CGPDFContext` to write). The geometry math and the page-number text are pure and TDD'd; the FFI is implemented then manually verified.

**Files:**
- Create: `src-tauri/src/pdf_postprocess.rs`
- Modify: `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`

**Interfaces:**
- Produces (pure, used by tests + the FFI below):
  - `pub fn mm_to_points(mm: f64) -> f64`
  - `pub struct MarginsPts { pub top: f64, pub right: f64, pub bottom: f64, pub left: f64 }`
  - `pub struct RectPts { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }`
  - `pub fn content_rect(paper_w: f64, paper_h: f64, m: &MarginsPts) -> RectPts`
  - `pub fn fit_scale(src_w: f64, src_h: f64, dst: &RectPts) -> f64` (uniform scale to fit width, never upscale past 1.0)
  - `pub fn page_number_text(page_index: usize, total: usize, mode: &str) -> Option<String>`
  - `pub fn page_number_x(mode: &str, content: &RectPts, text_width: f64) -> Option<f64>`
- Produces (FFI):
  - `pub fn relayout(src: &std::path::Path, dst: &std::path::Path, paper_w: f64, paper_h: f64, margins: &MarginsPts, page_numbers: &str) -> Result<(), String>`
  - `pub fn paper_points(paper: &str) -> (f64, f64)` (portrait, points; A4 = 595.28×841.89, Letter = 612×792, Legal = 612×1008)

- [ ] **Step 1: Add the Core Graphics dependency**

In `src-tauri/Cargo.toml`, under the existing `[target.'cfg(target_os = "macos")'.dependencies]`, add:

```toml
objc2-core-graphics = { version = "0.3", features = [
    "CGPDFDocument",
    "CGPDFPage",
    "CGContext",
    "CGPDFContext",
    "CGGeometry",
    "CGColor",
    "CGColorSpace",
    "CGAffineTransform",
] }
```

Run: `cd src-tauri && cargo fetch`
Expected: resolves `objc2-core-graphics 0.3.x` (the registry cache already has `objc2-core-graphics-0.3.2`). If a listed feature name does not exist, run `cargo build` to see the available-features error and adjust to the actual names — record the final feature list in a comment above the dependency.

- [ ] **Step 2: Create the module skeleton with pure helpers + failing tests**

Create `src-tauri/src/pdf_postprocess.rs`:

```rust
//! Post-process a WebKit-printed content PDF: re-lay each page onto the target
//! paper at the requested margins (WebKit ignores both CSS @page and
//! NSPrintInfo margins) and stamp page numbers (WebKit has no footer API).
//! macOS-only; built on Core Graphics (CGPDFDocument in, CGPDFContext out).

#![cfg(target_os = "macos")]

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarginsPts {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectPts {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Millimetres → PostScript points (72 dpi).
pub fn mm_to_points(mm: f64) -> f64 {
    mm * 72.0 / 25.4
}

/// Portrait paper size in points. Unknown names fall back to A4.
pub fn paper_points(paper: &str) -> (f64, f64) {
    match paper {
        "letter" => (612.0, 792.0),
        "legal" => (612.0, 1008.0),
        _ => (595.28, 841.89), // a4
    }
}

/// The drawable box inside the page after subtracting margins. Origin is the
/// PDF coordinate system (bottom-left).
pub fn content_rect(paper_w: f64, paper_h: f64, m: &MarginsPts) -> RectPts {
    RectPts {
        x: m.left,
        y: m.bottom,
        w: (paper_w - m.left - m.right).max(1.0),
        h: (paper_h - m.top - m.bottom).max(1.0),
    }
}

/// Uniform scale so a `src_w`-wide source fits the content width, never
/// upscaling (so a normal page is reproduced 1:1; only over-wide content
/// shrinks). `src_h` is accepted for symmetry/future height-fit needs.
pub fn fit_scale(src_w: f64, _src_h: f64, dst: &RectPts) -> f64 {
    if src_w <= 0.0 {
        return 1.0;
    }
    (dst.w / src_w).min(1.0)
}

/// The footer text for a 0-based page index, or None when numbering is off.
pub fn page_number_text(page_index: usize, total: usize, mode: &str) -> Option<String> {
    match mode {
        "bottom-center" | "bottom-right" => Some(format!("{} / {}", page_index + 1, total)),
        _ => None,
    }
}

/// X origin (points, from page left) for footer text of width `text_width`
/// within `content`, or None when numbering is off.
pub fn page_number_x(mode: &str, content: &RectPts, text_width: f64) -> Option<f64> {
    match mode {
        "bottom-center" => Some(content.x + (content.w - text_width) / 2.0),
        "bottom-right" => Some(content.x + content.w - text_width),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mm_to_points_converts() {
        assert!((mm_to_points(25.4) - 72.0).abs() < 1e-9);
        assert!((mm_to_points(0.0)).abs() < 1e-9);
    }

    #[test]
    fn paper_points_known_and_fallback() {
        assert_eq!(paper_points("letter"), (612.0, 792.0));
        assert_eq!(paper_points("legal"), (612.0, 1008.0));
        assert_eq!(paper_points("a4"), (595.28, 841.89));
        assert_eq!(paper_points("garbage"), (595.28, 841.89));
    }

    #[test]
    fn content_rect_subtracts_margins() {
        let r = content_rect(
            600.0,
            800.0,
            &MarginsPts { top: 50.0, right: 40.0, bottom: 60.0, left: 30.0 },
        );
        assert_eq!(r, RectPts { x: 30.0, y: 60.0, w: 530.0, h: 690.0 });
    }

    #[test]
    fn fit_scale_never_upscales() {
        let dst = RectPts { x: 0.0, y: 0.0, w: 500.0, h: 700.0 };
        assert!((fit_scale(1000.0, 1400.0, &dst) - 0.5).abs() < 1e-9);
        assert!((fit_scale(400.0, 560.0, &dst) - 1.0).abs() < 1e-9); // would-be 1.25 clamped
    }

    #[test]
    fn page_number_text_modes() {
        assert_eq!(page_number_text(0, 3, "bottom-center"), Some("1 / 3".to_string()));
        assert_eq!(page_number_text(2, 3, "bottom-right"), Some("3 / 3".to_string()));
        assert_eq!(page_number_text(0, 3, "none"), None);
    }

    #[test]
    fn page_number_x_positions() {
        let c = RectPts { x: 30.0, y: 60.0, w: 500.0, h: 700.0 };
        assert_eq!(page_number_x("none", &c, 40.0), None);
        assert_eq!(page_number_x("bottom-right", &c, 40.0), Some(490.0));
        assert_eq!(page_number_x("bottom-center", &c, 40.0), Some(260.0));
    }
}
```

- [ ] **Step 3: Register the module and run the pure tests (expect pass)**

In `src-tauri/src/lib.rs`, add near the other `mod` declarations (it is self-gated by the inner `#![cfg]`, but gate the declaration too for clarity):

```rust
#[cfg(target_os = "macos")]
mod pdf_postprocess;
```

Run: `cd src-tauri && cargo test pdf_postprocess`
Expected: the 6 tests PASS. (They are pure; no FFI yet.)

- [ ] **Step 4: Implement the FFI `relayout`**

Append to `src-tauri/src/pdf_postprocess.rs` (inside the same file, above `#[cfg(test)]`). This reads the source PDF with `CGPDFDocument`, creates a `CGContext` PDF writer at the target paper size, and for each source page begins a new page, draws the (cropped-to-its-media-box) source page scaled+translated into the content rect (top-aligned), then draws the page-number string. Use `objc2_core_graphics` types; the exact import paths are confirmed during this step (record them).

```rust
use std::path::Path;

use objc2_core_foundation::{CFRetained, CFURL};
use objc2_core_graphics::{
    CGContext, CGPDFDocument, CGPDFPageBoxType, // confirm exact names during build
};

/// Re-lay `src` onto `dst` at `paper_w`×`paper_h` (points) with `margins`, and
/// stamp page numbers per `page_numbers`. Returns Err with context on any
/// Core Graphics failure.
pub fn relayout(
    src: &Path,
    dst: &Path,
    paper_w: f64,
    paper_h: f64,
    margins: &MarginsPts,
    page_numbers: &str,
) -> Result<(), String> {
    // SAFETY: all pointers below come from CG constructors checked for null.
    unsafe {
        let src_url = file_url(src).ok_or("bad source path")?;
        let doc = CGPDFDocument::with_url(Some(&src_url))
            .ok_or_else(|| format!("could not open content PDF: {}", src.display()))?;
        let total = CGPDFDocument::number_of_pages(Some(&doc)) as usize;
        if total == 0 {
            return Err("content PDF has no pages".to_string());
        }

        let dst_url = file_url(dst).ok_or("bad output path")?;
        let media = cg_rect(0.0, 0.0, paper_w, paper_h);
        let ctx = CGContext::pdf_with_url(Some(&dst_url), &media, None)
            .ok_or("could not create output PDF context")?;

        let content = content_rect(paper_w, paper_h, margins);

        for i in 1..=total {
            let page = CGPDFDocument::page(Some(&doc), i as i64)
                .ok_or_else(|| format!("missing source page {i}"))?;
            let src_box = CGPDFPage::box_rect(Some(&page), CGPDFPageBoxType::CropBox);
            let (sw, sh) = (src_box.size.width, src_box.size.height);

            CGContext::begin_pdf_page(Some(&ctx), None);
            CGContext::save_g_state(Some(&ctx));

            let scale = fit_scale(sw, sh, &content);
            let scaled_h = sh * scale;
            // Top-align inside the content box.
            let ty = content.y + content.h - scaled_h;
            CGContext::translate_ctm(Some(&ctx), content.x, ty);
            CGContext::scale_ctm(Some(&ctx), scale, scale);
            // Shift so the source crop-box origin maps to (0,0).
            CGContext::translate_ctm(Some(&ctx), -src_box.origin.x, -src_box.origin.y);
            CGContext::draw_pdf_page(Some(&ctx), Some(&page));

            CGContext::restore_g_state(Some(&ctx));

            if let Some(text) = page_number_text(i - 1, total, page_numbers) {
                draw_footer(&ctx, &content, margins, &text, page_numbers);
            }

            CGContext::end_pdf_page(Some(&ctx));
        }

        CGContext::close_pdf(Some(&ctx));
    }
    Ok(())
}
```

Implement the two small unsafe helpers in the same file:

```rust
unsafe fn file_url(p: &Path) -> Option<CFRetained<CFURL>> {
    let s = p.to_str()?;
    CFURL::from_file_system_path(/* confirm exact constructor */ s)
}

/// Draw `text` as a centered/right footer baseline `margins.bottom*0.5` up from
/// the page bottom, in 9pt Helvetica grey. Uses a CGContext text-drawing path
/// (CGContext::select_font / show_text_at_point) so no Core Text dependency is
/// added; width is estimated as 0.5em per char × 9pt for positioning.
unsafe fn draw_footer(
    ctx: &CGContext,
    content: &RectPts,
    margins: &MarginsPts,
    text: &str,
    mode: &str,
) {
    let font_size = 9.0_f64;
    let approx_w = text.chars().count() as f64 * font_size * 0.5;
    let Some(x) = page_number_x(mode, content, approx_w) else { return };
    let y = (margins.bottom * 0.5).max(8.0);
    // select_font + show_text_at_point with grey fill — confirm exact objc2 API.
    // Set grey fill, font, then draw the bytes at (x, y).
    // ... concrete calls confirmed during build ...
    let _ = (ctx, x, y, text, font_size);
}

unsafe fn cg_rect(x: f64, y: f64, w: f64, h: f64) -> objc2_core_graphics::CGRect {
    objc2_core_graphics::CGRect {
        origin: objc2_core_graphics::CGPoint { x, y },
        size: objc2_core_graphics::CGSize { width: w, height: h },
    }
}
```

> **Spike note (do this in Step 4, record findings inline as comments):** The exact `objc2-core-graphics` 0.3 method names (`with_url` vs `withURL`, `pdf_with_url`, `begin_pdf_page`, `draw_pdf_page`, `box_rect`, the text API) must be confirmed against the built crate. Use `cargo doc -p objc2-core-graphics --no-deps` (or jump-to-def) to read the generated signatures, then fix the calls. If the deprecated CG text API (`select_font`/`show_text`) is unavailable in 0.3, fall back to drawing the footer with Core Text (`CTLine`) via `objc2-core-text` — add it to Cargo.toml with the same `0.3` pin and adjust `draw_footer`. Keep `relayout`'s signature stable regardless.

- [ ] **Step 5: Build and fix the FFI against the real bindings**

Run: `cd src-tauri && cargo build`
Expected: compiles. Resolve every method-name/argument mismatch flagged by the compiler against the actual `objc2-core-graphics` API (this is the spike's core work). Re-run until clean. Then `cargo clippy --all-targets -- -D warnings` and `cargo fmt`.

- [ ] **Step 6: Manually verify on a real PDF**

Create a throwaway binary test by adding a temporary `#[test] #[ignore]` that calls `relayout` on a fixture PDF, OR verify via Task 4's wiring once available. For the spike, generate a 2-page content PDF (use macOS `cupsfilter` or a saved Safari print) at `/tmp/content.pdf`, then in a scratch `#[test]` run `relayout(Path::new("/tmp/content.pdf"), Path::new("/tmp/out.pdf"), 595.28, 841.89, &MarginsPts{top:48.0,right:48.0,bottom:48.0,left:48.0}, "bottom-center")` and open `/tmp/out.pdf`.
Expected: 2 pages, A4, ~17mm margins, "1 / 2" / "2 / 2" centered in the bottom margin, content unclipped. Record the confirmed API + any margin-mechanism decision as a comment block at the top of `pdf_postprocess.rs`. Delete the scratch test.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/pdf_postprocess.rs src-tauri/src/lib.rs
git commit -m "Add native PDF re-layout and page-number post-processing"
```

---

## Task 2: Preset & settings module (`ui/pdf-presets.js`)

Pure helpers, no DOM/Tauri, runnable under `node --test` (mirrors `ui/export.js`).

**Files:**
- Create: `ui/pdf-presets.js`, `ui/pdf-presets.test.js`

**Interfaces:**
- Produces:
  - `PRESETS` (object keyed by id)
  - `presetIds(): string[]`
  - `presetDefaults(id): Settings` — full settings object incl. `preset: id`
  - `defaultSettings(): Settings` — `presetDefaults("clean")`
  - `mergeSettings(base, overrides): Settings` — overrides win; `undefined`/`null` ignored
  - `clampBaseSize(pt): number` — clamp to `[9, 16]`
  - `marginMm(name): {top,right,bottom,left}`
  - `paperMm(name): {w, h}` (portrait)
  - `settingsToCss(settings): string` — CSS scoped to `.markdown-body`
- `Settings` shape is the Global Constraints shape.

- [ ] **Step 1: Write the failing test file**

Create `ui/pdf-presets.test.js`:

```js
import test from "node:test";
import assert from "node:assert/strict";
import {
  PRESETS,
  presetIds,
  presetDefaults,
  defaultSettings,
  mergeSettings,
  clampBaseSize,
  marginMm,
  paperMm,
  settingsToCss,
} from "./pdf-presets.js";

test("presetIds are the three documented presets", () => {
  assert.deepEqual(presetIds().sort(), ["clean", "compact", "report"]);
});

test("presetDefaults returns a complete settings object tagged with its id", () => {
  const s = presetDefaults("compact");
  assert.equal(s.preset, "compact");
  for (const k of ["preset", "baseSize", "paper", "margins", "pageNumbers"]) {
    assert.ok(k in s, `missing ${k}`);
  }
});

test("presetDefaults falls back to clean for unknown ids", () => {
  assert.deepEqual(presetDefaults("nope"), presetDefaults("clean"));
});

test("defaultSettings is the clean preset", () => {
  assert.deepEqual(defaultSettings(), presetDefaults("clean"));
});

test("compact uses a smaller base size than clean", () => {
  assert.ok(presetDefaults("compact").baseSize < presetDefaults("clean").baseSize);
});

test("mergeSettings overrides win, nullish ignored", () => {
  const base = presetDefaults("clean");
  const out = mergeSettings(base, { baseSize: 13, paper: undefined, margins: null });
  assert.equal(out.baseSize, 13);
  assert.equal(out.paper, base.paper);
  assert.equal(out.margins, base.margins);
});

test("clampBaseSize clamps to 9..16", () => {
  assert.equal(clampBaseSize(2), 9);
  assert.equal(clampBaseSize(99), 16);
  assert.equal(clampBaseSize(12), 12);
});

test("marginMm: wide > normal > narrow uniformly", () => {
  assert.ok(marginMm("wide").top > marginMm("normal").top);
  assert.ok(marginMm("normal").top > marginMm("narrow").top);
  const n = marginMm("normal");
  assert.equal(n.top, n.bottom);
  assert.equal(n.left, n.right);
});

test("marginMm unknown falls back to normal", () => {
  assert.deepEqual(marginMm("???"), marginMm("normal"));
});

test("paperMm portrait dimensions", () => {
  assert.deepEqual(paperMm("a4"), { w: 210, h: 297 });
  assert.deepEqual(paperMm("letter"), { w: 215.9, h: 279.4 });
});

test("settingsToCss is scoped to .markdown-body and reflects base size", () => {
  const css = settingsToCss(mergeSettings(defaultSettings(), { baseSize: 13 }));
  assert.match(css, /\.markdown-body\s*\{/);
  assert.match(css, /font-size:\s*13pt/);
});

test("settingsToCss justifies only the report preset", () => {
  assert.match(settingsToCss(presetDefaults("report")), /text-align:\s*justify/);
  assert.doesNotMatch(settingsToCss(presetDefaults("clean")), /text-align:\s*justify/);
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd ui && node --test pdf-presets.test.js`
Expected: FAIL — `Cannot find module './pdf-presets.js'`.

- [ ] **Step 3: Implement `ui/pdf-presets.js`**

```js
// Pure preset/settings helpers for PDF export. No DOM or Tauri imports, so this
// runs under `node --test` as well as in the WebView (mirrors export.js).

const FONT_SANS =
  '-apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif';
const FONT_SERIF = 'Georgia, "Iowan Old Style", "Times New Roman", Times, serif';
const FONT_MONO =
  'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace';

export const PRESETS = {
  clean: {
    label: "Clean",
    bodyFont: FONT_SANS,
    headingFont: FONT_SANS,
    baseSize: 11,
    lineHeight: 1.55,
    headingScale: 1.0,
    accent: "#0969da",
    margins: "normal",
    paper: "a4",
    justify: false,
    pageNumbers: "bottom-center",
  },
  report: {
    label: "Report",
    bodyFont: FONT_SERIF,
    headingFont: FONT_SANS,
    baseSize: 11,
    lineHeight: 1.6,
    headingScale: 1.05,
    accent: "#1f2328",
    margins: "wide",
    paper: "a4",
    justify: true,
    pageNumbers: "bottom-center",
  },
  compact: {
    label: "Compact",
    bodyFont: FONT_SANS,
    headingFont: FONT_SANS,
    baseSize: 9.5,
    lineHeight: 1.4,
    headingScale: 0.95,
    accent: "#0969da",
    margins: "narrow",
    paper: "a4",
    justify: false,
    pageNumbers: "bottom-right",
  },
};

const MONO = FONT_MONO;

const MARGINS = {
  narrow: { top: 12, right: 12, bottom: 14, left: 12 },
  normal: { top: 18, right: 18, bottom: 20, left: 18 },
  wide: { top: 25, right: 25, bottom: 27, left: 25 },
};

const PAPER = {
  a4: { w: 210, h: 297 },
  letter: { w: 215.9, h: 279.4 },
  legal: { w: 215.9, h: 355.6 },
};

// The persisted/exchanged settings keys (the subset the UI controls).
function settingsFromPreset(id) {
  const p = PRESETS[id] || PRESETS.clean;
  return {
    preset: PRESETS[id] ? id : "clean",
    baseSize: p.baseSize,
    paper: p.paper,
    margins: p.margins,
    pageNumbers: p.pageNumbers,
  };
}

export function presetIds() {
  return Object.keys(PRESETS);
}

export function presetDefaults(id) {
  return settingsFromPreset(id);
}

export function defaultSettings() {
  return presetDefaults("clean");
}

export function mergeSettings(base, overrides) {
  const out = { ...base };
  for (const [k, v] of Object.entries(overrides || {})) {
    if (v !== undefined && v !== null) out[k] = v;
  }
  return out;
}

export function clampBaseSize(pt) {
  const n = Number(pt);
  if (!Number.isFinite(n)) return defaultSettings().baseSize;
  return Math.min(16, Math.max(9, n));
}

export function marginMm(name) {
  return { ...(MARGINS[name] || MARGINS.normal) };
}

export function paperMm(name) {
  return { ...(PAPER[name] || PAPER.a4) };
}

// Look up the full preset record behind a settings object (for fonts/accent
// that aren't user-exposed knobs).
function presetRecord(settings) {
  return PRESETS[settings.preset] || PRESETS.clean;
}

/** CSS scoped to `.markdown-body`, applied both to the standalone HTML preview
 *  and (injected as a <style>) to the live #preview during the in-app PDF
 *  print. Typography + accent + left/right margins live here; paper size and
 *  top/bottom margins + page numbers are applied natively (not CSS). */
export function settingsToCss(settings) {
  const p = presetRecord(settings);
  const size = clampBaseSize(settings.baseSize);
  const m = marginMm(settings.margins);
  const justify = p.justify ? "\n  text-align: justify;" : "";
  return `.markdown-body {
  --pdf-accent: ${p.accent};
  font-family: ${p.bodyFont};
  font-size: ${size}pt;
  line-height: ${p.lineHeight};
  box-sizing: border-box;
  max-width: none;
  padding-left: ${m.left}mm;
  padding-right: ${m.right}mm;${justify}
}
.markdown-body h1, .markdown-body h2, .markdown-body h3,
.markdown-body h4, .markdown-body h5, .markdown-body h6 {
  font-family: ${p.headingFont};
  color: var(--pdf-accent);
  line-height: 1.25;
}
.markdown-body h1 { font-size: ${(2.0 * p.headingScale).toFixed(3)}em; }
.markdown-body h2 { font-size: ${(1.6 * p.headingScale).toFixed(3)}em; }
.markdown-body h3 { font-size: ${(1.3 * p.headingScale).toFixed(3)}em; }
.markdown-body a { color: var(--pdf-accent); text-decoration: underline; }
.markdown-body table th { background: color-mix(in srgb, var(--pdf-accent) 12%, transparent); }
.markdown-body pre, .markdown-body code { font-family: ${MONO}; }
.markdown-body pre { font-size: 0.85em; }
`;
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd ui && node --test pdf-presets.test.js`
Expected: all tests PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/pdf-presets.js ui/pdf-presets.test.js
git commit -m "Add PDF preset/settings module with pure helpers"
```

---

## Task 3: Persist settings in `recent.json`

**Files:**
- Modify: `src-tauri/src/recent.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`

**Interfaces:**
- Produces (Rust):
  - `recent::PdfSettings` (serde, camelCase, `Default` = clean defaults)
  - `recent::load_pdf_settings(app) -> PdfSettings`
  - `recent::save_pdf_settings(app, &PdfSettings)`
  - commands `get_pdf_settings(app) -> PdfSettings`, `save_pdf_settings(app, settings: PdfSettings)`
- The JSON serialization keys match the JS settings shape exactly (`preset`, `baseSize`, `paper`, `margins`, `pageNumbers`).

- [ ] **Step 1: Write failing tests in `recent.rs`**

Add to the `#[cfg(test)] mod tests` in `src-tauri/src/recent.rs`:

```rust
#[test]
fn pdf_settings_default_is_clean() {
    let s = PdfSettings::default();
    assert_eq!(s.preset, "clean");
    assert_eq!(s.paper, "a4");
    assert_eq!(s.margins, "normal");
    assert_eq!(s.page_numbers, "bottom-center");
    assert_eq!(s.base_size, 11.0);
}

#[test]
fn pdf_settings_round_trip_camel_case() {
    let s = PdfSettings {
        preset: "report".into(),
        base_size: 12.5,
        paper: "letter".into(),
        margins: "wide".into(),
        page_numbers: "bottom-right".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"baseSize\":12.5"), "got: {json}");
    assert!(json.contains("\"pageNumbers\":\"bottom-right\""), "got: {json}");
    let back: PdfSettings = serde_json::from_str(&json).unwrap();
    assert_eq!(back.preset, "report");
}

#[test]
fn store_defaults_pdf_settings_when_absent() {
    let back: Store = serde_json::from_str(r#"{"folders":["/a"]}"#).unwrap();
    assert_eq!(back.pdf_export.preset, "clean");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test -p mdviewer recent`
Expected: FAIL — `PdfSettings` not found, `Store` has no `pdf_export`.

- [ ] **Step 3: Implement the struct, store field, and helpers**

In `src-tauri/src/recent.rs`, add after the `UpdateChannel` enum:

```rust
/// User's PDF export preferences. Keys mirror the frontend settings object
/// (camelCase). Default = the "clean" preset's defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfSettings {
    pub preset: String,
    pub base_size: f64,
    pub paper: String,
    pub margins: String,
    pub page_numbers: String,
}

impl Default for PdfSettings {
    fn default() -> Self {
        Self {
            preset: "clean".into(),
            base_size: 11.0,
            paper: "a4".into(),
            margins: "normal".into(),
            page_numbers: "bottom-center".into(),
        }
    }
}
```

Add the field to `Store`:

```rust
    #[serde(default)]
    pdf_export: PdfSettings,
```

Add load/save helpers near `load_channel`/`save_channel`:

```rust
pub fn load_pdf_settings(app: &AppHandle) -> PdfSettings {
    load_store(app).pdf_export
}

/// Persists the PDF export settings, preserving every other field.
pub fn save_pdf_settings(app: &AppHandle, settings: &PdfSettings) {
    let mut store = load_store(app);
    store.pdf_export = settings.clone();
    write_store(app, &store);
}
```

- [ ] **Step 4: Run the tests (expect pass)**

Run: `cd src-tauri && cargo test -p mdviewer recent`
Expected: all `recent` tests PASS (new + existing).

- [ ] **Step 5: Add the commands and register them**

In `src-tauri/src/commands.rs`, add:

```rust
#[tauri::command]
pub fn get_pdf_settings(app: AppHandle) -> recent::PdfSettings {
    recent::load_pdf_settings(&app)
}

#[tauri::command]
pub fn save_pdf_settings(app: AppHandle, settings: recent::PdfSettings) {
    recent::save_pdf_settings(&app, &settings);
}
```

In `src-tauri/src/lib.rs`, add to the `generate_handler![…]` list:

```rust
            commands::get_pdf_settings,
            commands::save_pdf_settings,
```

- [ ] **Step 6: Build + lint**

Run: `cd src-tauri && cargo build && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/recent.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "Persist PDF export settings in recent.json"
```

---

## Task 4: Make the native + frontend export settings-driven

Rework `export_pdf` to take paper/margins/page-numbers and run the post-process pass, and make `exportDocument` accept a settings object (injecting `settingsToCss` and passing geometry to the command). Extract `buildExportHtml` for reuse by the live preview (Task 6).

**Files:**
- Modify: `src-tauri/src/export.rs`, `ui/app.js`

**Interfaces:**
- Consumes: `pdf_postprocess::{relayout, paper_points, mm_to_points, MarginsPts}` (Task 1); `settingsToCss`, `marginMm`, `paperMm`, `defaultSettings` from `pdf-presets.js` (Task 2).
- Produces:
  - Rust command `export_pdf(window, path, paper: String, margins: Margins, page_numbers: String)` where `Margins { top, right, bottom, left }` are millimetres (`#[serde(rename_all="camelCase")]`, but single-word fields need no rename).
  - JS `exportDocument(format, path, settings)` — `settings` defaults to `getActiveSettings()` (see Task 8); for now thread an explicit arg.
  - JS `buildExportHtml(t, boundary, settings): Promise<string>` — standalone HTML doc string.

- [ ] **Step 1: Change `export_pdf` to print to a temp file then post-process**

In `src-tauri/src/export.rs`, replace the command signature and body. The macOS `export` now: prints to `path + ".content.tmp.pdf"`, waits for `%%EOF`, then calls `pdf_postprocess::relayout` into `path`. Set the paper size on the `NSPrintInfo` before printing.

Add at top of the `macos` mod:

```rust
use crate::pdf_postprocess::{self, MarginsPts};
```

New command:

```rust
#[derive(serde::Deserialize)]
pub struct Margins {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

#[tauri::command]
pub async fn export_pdf(
    window: tauri::WebviewWindow,
    path: String,
    paper: String,
    margins: Margins,
    page_numbers: String,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::export(window, path, paper, margins, page_numbers).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, path, paper, margins, page_numbers);
        Err("PDF export is not yet supported on Windows".to_string())
    }
}
```

In `mod macos`, change `export` to:

```rust
pub async fn export(
    window: tauri::WebviewWindow,
    path: String,
    paper: String,
    margins: super::Margins,
    page_numbers: String,
) -> Result<(), String> {
    let content_path = format!("{path}.content.tmp.pdf");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&content_path);

    let (paper_w, paper_h) = pdf_postprocess::paper_points(&paper);

    let (started_tx, started_rx) = mpsc::channel::<Result<(), String>>();
    let p = content_path.clone();
    window
        .with_webview(move |pw| {
            let r = unsafe { start_print(pw.inner(), pw.ns_window(), &p, paper_w, paper_h) };
            let _ = started_tx.send(r);
        })
        .map_err(|e| format!("with_webview failed: {e}"))?;
    started_rx.recv().map_err(|e| format!("print task dropped: {e}"))??;

    wait_for_complete_pdf(Path::new(&content_path), PRINT_TIMEOUT)?;

    let m = MarginsPts {
        top: pdf_postprocess::mm_to_points(margins.top),
        right: pdf_postprocess::mm_to_points(margins.right),
        bottom: pdf_postprocess::mm_to_points(margins.bottom),
        left: pdf_postprocess::mm_to_points(margins.left),
    };
    let res = pdf_postprocess::relayout(
        Path::new(&content_path),
        Path::new(&path),
        paper_w,
        paper_h,
        &m,
        &page_numbers,
    );
    let _ = std::fs::remove_file(&content_path);
    res
}
```

Update `start_print` to set the paper size on the `NSPrintInfo` (so WebKit paginates to the real page height). Add params `paper_w`, `paper_h` and, after `let info = NSPrintInfo::new();`:

```rust
        info.setPaperSize(objc2_foundation::NSSize::new(paper_w, paper_h));
```

(Keep the existing disposition/URL/save logic, but the save URL now points at the temp content path passed in.)

- [ ] **Step 2: Build the Rust side**

Run: `cd src-tauri && cargo build`
Expected: compiles. Confirm `setPaperSize` exists in the `objc2-app-kit` `NSPrintInfo` features (it's part of `NSPrintInfo`); add the `NSSize`/`NSGeometry` feature to `objc2-foundation` if the compiler complains.

- [ ] **Step 3: Extract `buildExportHtml` in `app.js`**

Refactor `exportHtml` so the HTML-string construction is reusable. Add:

```js
import { settingsToCss, marginMm, paperMm, defaultSettings } from "./pdf-presets.js";
```

Add `buildExportHtml(t, boundary, settings)` that does what `exportHtml` did up to producing `html`, and append `settingsToCss(settings)` to the CSS:

```js
async function buildExportHtml(t, boundary, settings) {
  const clone = preview.cloneNode(true);
  clone.querySelectorAll(".export-btn-group, .copy-btn").forEach((el) => el.remove());
  clone.querySelectorAll('input[type="checkbox"]').forEach((cb) => cb.setAttribute("disabled", ""));
  await inlineImages(clone, boundary);
  const bodyHtml = clone.innerHTML;

  let css = forceLightCss(await fetchText("github-markdown.css"));
  if (documentNeedsKatex(bodyHtml)) {
    let katexCss = await fetchText("katex/katex.min.css");
    katexCss = inlineFontUrls(katexCss, await buildKatexFontMap(katexCss));
    css += "\n" + katexCss;
  }
  css += "\n" + EXPORT_PAGE_CSS;
  css += "\n" + settingsToCss(settings);

  return buildHtmlDocument({ title: baseName(t.path), css, bodyHtml });
}
```

Rewrite `exportHtml` to call it:

```js
async function exportHtml(t, path, boundary, settings) {
  const html = await buildExportHtml(t, boundary, settings);
  await invoke("save_export", { path, data: html, base64Encoded: false });
}
```

- [ ] **Step 4: Make `exportDocument` settings-driven**

Change the signature to `exportDocument(format, path, settings)`. After forcing light/raw-off and before serialization, inject a `<style id="pdf-export-style">settingsToCss(settings)</style>` into `document.head` (and remove it in `finally`). For HTML pass `settings` to `exportHtml`. For PDF, compute geometry and pass to the command:

```js
async function exportDocument(format, path, settings) {
  if (exportInProgress) return false;
  const t = activeTab();
  if (!t) return false;
  settings = settings || defaultSettings();
  exportInProgress = true;
  const prevTheme = currentTheme;
  const prevDataTheme = document.documentElement.dataset.theme;
  const prevRaw = t.raw;
  const prevReviewMode = t.reviewMode;
  const prevScroll = previewScroll.scrollTop;
  let fittedTables = [];
  let headingWraps = [];
  let styleEl = null;
  let succeeded = false;
  try {
    currentTheme = "light";
    document.documentElement.dataset.theme = "light";
    t.raw = false;
    t.reviewMode = false;
    initMermaid();
    await renderActive({ scrollLock: false, forceMermaid: true });
    await swapMermaidForPrint();

    styleEl = document.createElement("style");
    styleEl.id = "pdf-export-style";
    styleEl.textContent = settingsToCss(settings);
    document.head.appendChild(styleEl);

    const boundary = treeRoot || parentDir(t.path);
    if (format === "html") {
      await exportHtml(t, path, boundary, settings);
    } else if (format === "pdf") {
      await neutralizeOutsideWorkspaceImages(preview, boundary);
      fittedTables = fitWideTablesForPrint();
      headingWraps = keepHeadingsWithNext();
      await invoke("export_pdf", {
        path,
        paper: settings.paper,
        margins: marginMm(settings.margins),
        pageNumbers: settings.pageNumbers,
      });
    }
    succeeded = true;
  } catch (e) {
    console.error("export failed", e);
    showTransientError("Export failed: " + e);
  } finally {
    exportInProgress = false;
    if (styleEl) styleEl.remove();
    unwrapForPrint(headingWraps);
    unfitWideTables(fittedTables);
    currentTheme = prevTheme;
    document.documentElement.dataset.theme = prevDataTheme;
    t.raw = prevRaw;
    t.reviewMode = prevReviewMode;
    initMermaid();
    if (t.editing) {
      await renderFromEditor(t, { scrollLock: false, forceMermaid: true });
    } else {
      await renderActive({ scrollLock: false, forceMermaid: true });
    }
    previewScroll.scrollTop = prevScroll;
  }
  return succeeded;
}
```

> Note: `PRINT_CONTENT_WIDTH_PX` (used by `fitWideTablesForPrint`) is sized to the A4-minus-1in worst case and stays a safe conservative bound for all presets/papers — leave it unchanged.

- [ ] **Step 5: Keep existing callers compiling (temporary defaults)**

`onExport` and the `mcp-generate-pdf` listener still call `exportDocument(format, path)`. They now pass `defaultSettings()` implicitly via the `settings ||` fallback — no change needed yet. Task 5 rewires `onExport`; Task 8 rewires MCP.

- [ ] **Step 6: Build and manually smoke-test one export**

Run: `cd src-tauri && cargo build`
Then `cargo run -- ../README.md`, and trigger **File ▸ Export as PDF…** (still the old dialog path until Task 5 — if Task 5 not yet done, temporarily call `exportDocument("pdf", path, defaultSettings())` from `onExport`). Open the PDF.
Expected: A4, ~18mm margins, centered "1 / N" footer, clean typography, all content present.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/export.rs ui/app.js
git commit -m "Drive PDF export from a settings object and native geometry"
```

---

## Task 5: The `pdf-export` window (controls) + menu rewire

**Files:**
- Create: `ui/pdf-export.html`, `ui/pdf-export.js`
- Modify: `src-tauri/src/menu.rs`, `src-tauri/capabilities/default.json`, `ui/app.js`

**Interfaces:**
- Consumes: `presetDefaults`, `presetIds`, `PRESETS`, `mergeSettings`, `clampBaseSize`, `defaultSettings` (Task 2); `get_pdf_settings`/`save_pdf_settings` (Task 3).
- Produces:
  - Rust `menu::open_pdf_export_window(app)`; menu id `export-pdf` now opens the window.
  - Window emits (to main) `pdf-export-request-preview` `{ settings }` and `pdf-export-run` `{ settings, mode: "exact"|"save", path? }`; listens for `pdf-export-preview-html` `{ html }`, `pdf-export-exact-ready` `{ url }`, `pdf-export-done` `{ ok, error? }`. (Main-window side is Tasks 6–7.)

- [ ] **Step 1: Register the window in capabilities**

In `src-tauri/capabilities/default.json`, change the `windows` array to:

```json
  "windows": ["main", "preferences", "claude-integration", "pdf-export"],
```

- [ ] **Step 2: Add `open_pdf_export_window` and rewire the menu**

In `src-tauri/src/menu.rs`, change the `export-pdf` handler (currently `let _ = app.emit("export", "pdf");`) to:

```rust
            #[cfg(target_os = "macos")]
            "export-pdf" => open_pdf_export_window(app),
```

Add the function (mirrors `open_settings`):

```rust
#[cfg(target_os = "macos")]
pub fn open_pdf_export_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("pdf-export") {
        let _ = win.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(
        app,
        "pdf-export",
        WebviewUrl::App("pdf-export.html".into()),
    )
    .title("Export to PDF")
    .inner_size(900.0, 680.0)
    .min_inner_size(720.0, 480.0)
    .resizable(true)
    .build();
}
```

(The `export` event for `"html"` stays; only `"pdf"` is redirected.)

- [ ] **Step 3: Create `ui/pdf-export.html`**

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>Export to PDF</title>
    <link rel="stylesheet" href="styles.css" />
    <link rel="stylesheet" href="pdf-export.css" />
  </head>
  <body class="pdf-export-window">
    <div class="pdf-export-layout">
      <aside class="pdf-controls">
        <h2>Export to PDF</h2>

        <label>Preset
          <select id="preset"></select>
        </label>

        <label>Base font size <span id="size-val"></span>
          <input id="base-size" type="range" min="9" max="16" step="0.5" />
        </label>

        <label>Paper size
          <select id="paper">
            <option value="a4">A4</option>
            <option value="letter">Letter</option>
            <option value="legal">Legal</option>
          </select>
        </label>

        <label>Margins
          <select id="margins">
            <option value="narrow">Narrow</option>
            <option value="normal">Normal</option>
            <option value="wide">Wide</option>
          </select>
        </label>

        <label>Page numbers
          <select id="page-numbers">
            <option value="none">None</option>
            <option value="bottom-center">Bottom center</option>
            <option value="bottom-right">Bottom right</option>
          </select>
        </label>

        <div class="pdf-actions">
          <button id="reset" type="button">Reset to preset</button>
          <button id="export" type="button" class="primary">Export…</button>
        </div>
        <p id="status" class="pdf-status" role="status"></p>
      </aside>

      <main class="pdf-preview-pane">
        <div class="pdf-preview-tabs">
          <button id="tab-live" type="button" class="active">Live preview</button>
          <button id="tab-exact" type="button">Exact PDF</button>
        </div>
        <iframe id="live-preview" title="Live preview"></iframe>
        <iframe id="exact-preview" title="Exact PDF preview" hidden></iframe>
      </main>
    </div>
    <script type="module" src="pdf-export.js"></script>
  </body>
</html>
```

Also create `ui/pdf-export.css` with layout (left rail fixed width, preview fills the rest, iframes 100%/white). Keep it small; reuse `styles.css` variables for theme.

```css
.pdf-export-window { margin: 0; height: 100vh; overflow: hidden; }
.pdf-export-layout { display: grid; grid-template-columns: 280px 1fr; height: 100%; }
.pdf-controls { padding: 16px; border-right: 1px solid var(--sidebar-border); display: flex; flex-direction: column; gap: 14px; overflow: auto; }
.pdf-controls label { display: flex; flex-direction: column; gap: 4px; font-size: 13px; }
.pdf-actions { margin-top: auto; display: flex; gap: 8px; }
.pdf-actions .primary { font-weight: 600; }
.pdf-status { font-size: 12px; min-height: 1.2em; color: var(--fg); }
.pdf-preview-pane { display: flex; flex-direction: column; background: #777; }
.pdf-preview-tabs { display: flex; gap: 4px; padding: 6px; }
.pdf-preview-tabs button.active { font-weight: 600; }
.pdf-preview-pane iframe { flex: 1; width: 100%; border: 0; background: #fff; }
```

- [ ] **Step 4: Create `ui/pdf-export.js` (controls only; preview wiring added in Tasks 6–7)**

```js
const { invoke } = window.__TAURI__.core;
const { listen, emit } = window.__TAURI__.event;
import {
  PRESETS, presetIds, presetDefaults, defaultSettings,
  mergeSettings, clampBaseSize,
} from "./pdf-presets.js";

const el = (id) => document.getElementById(id);
let settings = defaultSettings();
let previewTimer = null;

function fillPresetOptions() {
  for (const id of presetIds()) {
    const opt = document.createElement("option");
    opt.value = id;
    opt.textContent = PRESETS[id].label;
    el("preset").appendChild(opt);
  }
}

function reflect() {
  el("preset").value = settings.preset;
  el("base-size").value = settings.baseSize;
  el("size-val").textContent = `${settings.baseSize}pt`;
  el("paper").value = settings.paper;
  el("margins").value = settings.margins;
  el("page-numbers").value = settings.pageNumbers;
}

function update(overrides) {
  settings = mergeSettings(settings, overrides);
  reflect();
  schedulePreview();
}

function schedulePreview() {
  clearTimeout(previewTimer);
  previewTimer = setTimeout(() => {
    emit("pdf-export-request-preview", { settings }).catch(() => {});
  }, 120);
}

el("preset").addEventListener("change", (e) => {
  // Switching preset loads that preset's defaults wholesale.
  settings = presetDefaults(e.target.value);
  reflect();
  schedulePreview();
});
el("base-size").addEventListener("input", (e) => {
  el("size-val").textContent = `${e.target.value}pt`;
  update({ baseSize: clampBaseSize(parseFloat(e.target.value)) });
});
el("paper").addEventListener("change", (e) => update({ paper: e.target.value }));
el("margins").addEventListener("change", (e) => update({ margins: e.target.value }));
el("page-numbers").addEventListener("change", (e) => update({ pageNumbers: e.target.value }));
el("reset").addEventListener("click", () => {
  settings = presetDefaults(settings.preset);
  reflect();
  schedulePreview();
});

async function init() {
  fillPresetOptions();
  try {
    settings = mergeSettings(defaultSettings(), await invoke("get_pdf_settings"));
  } catch (e) {
    console.error("get_pdf_settings failed", e);
  }
  reflect();
  schedulePreview();
}
init().catch((e) => console.error("pdf-export init failed", e));
```

- [ ] **Step 5: Build and verify the window opens with controls**

Run: `cd src-tauri && cargo build && cargo run -- ../README.md`
Open **File ▸ Export as PDF…**.
Expected: a 900×680 window titled "Export to PDF" with all controls populated from saved/clean settings. The live-preview iframe is blank for now (no main-window handler yet) — that's expected; no console errors from the controls themselves.

- [ ] **Step 6: Commit**

```bash
git add ui/pdf-export.html ui/pdf-export.css ui/pdf-export.js src-tauri/src/menu.rs src-tauri/capabilities/default.json
git commit -m "Add PDF export tuning window and rewire the menu item"
```

---

## Task 6: Live HTML preview (main window serves HTML to the window)

The main window owns the rendered content. On `pdf-export-request-preview`, it builds the standalone export HTML for the active tab and emits it back; the window shows it via `srcdoc`.

**Files:**
- Modify: `ui/app.js`, `ui/pdf-export.js`

**Interfaces:**
- Consumes: `buildExportHtml` (Task 4); window protocol events (Task 5).
- Produces: main-window listener for `pdf-export-request-preview` emitting `pdf-export-preview-html` `{ html, error? }`. The standalone HTML wraps `.markdown-body` in a simulated page sheet at the chosen paper aspect ratio + margins.

- [ ] **Step 1: Main window — handle preview requests**

In `ui/app.js` `init()` (near the other `listen(...)` calls), add:

```js
  await listen("pdf-export-request-preview", async (ev) => {
    const { settings } = ev.payload;
    try {
      const html = await renderExportPreviewHtml(settings);
      await emit("pdf-export-preview-html", { html });
    } catch (e) {
      console.error("preview build failed", e);
      await emit("pdf-export-preview-html", { html: "", error: String(e) });
    }
  });
```

Add `emit` to the existing `window.__TAURI__.event` destructure at the top of the module (alongside `listen`).

- [ ] **Step 2: Main window — build the preview HTML faithfully**

`renderExportPreviewHtml` reuses the real light-render pipeline (so Mermaid/KaTeX/images are correct) and then `buildExportHtml`, wrapping the body in a paper sheet. Implement it to snapshot/restore view state like `exportDocument` does, but produce a string instead of writing a file:

```js
async function renderExportPreviewHtml(settings) {
  const t = activeTab();
  if (!t) throw new Error("no active document");
  const prevTheme = currentTheme;
  const prevDataTheme = document.documentElement.dataset.theme;
  const prevRaw = t.raw;
  const prevReviewMode = t.reviewMode;
  const prevScroll = previewScroll.scrollTop;
  let fitted = [];
  try {
    currentTheme = "light";
    document.documentElement.dataset.theme = "light";
    t.raw = false;
    t.reviewMode = false;
    initMermaid();
    await renderActive({ scrollLock: false, forceMermaid: true });
    await swapMermaidForPrint();
    const boundary = treeRoot || parentDir(t.path);
    await neutralizeOutsideWorkspaceImages(preview, boundary);
    fitted = fitWideTablesForPrint();
    const body = await buildExportHtml(t, boundary, settings);
    return wrapInPageSheet(body, settings);
  } finally {
    unfitWideTables(fitted);
    currentTheme = prevTheme;
    document.documentElement.dataset.theme = prevDataTheme;
    t.raw = prevRaw;
    t.reviewMode = prevReviewMode;
    initMermaid();
    if (t.editing) await renderFromEditor(t, { scrollLock: false, forceMermaid: true });
    else await renderActive({ scrollLock: false, forceMermaid: true });
    previewScroll.scrollTop = prevScroll;
  }
}
```

Add `wrapInPageSheet`, which injects a wrapper stylesheet that paints a white page at the paper aspect ratio with the top/bottom margins as padding (left/right already come from `settingsToCss`). It modifies the `<head>` of the doc string returned by `buildExportHtml`:

```js
function wrapInPageSheet(docHtml, settings) {
  const paper = paperMm(settings.paper);
  const m = marginMm(settings.margins);
  const sheetCss = `
  body { background: #777; margin: 0; padding: 24px; }
  article.markdown-body {
    background: #fff;
    width: ${paper.w}mm;
    min-height: ${paper.h}mm;
    margin: 0 auto;
    padding-top: ${m.top}mm;
    padding-bottom: ${m.bottom}mm;
    box-shadow: 0 2px 16px rgba(0,0,0,.4);
  }`;
  return docHtml.replace("</head>", `<style>${sheetCss}</style></head>`);
}
```

> The live sheet approximates pages as one tall sheet (page-break positions are not drawn). That's the documented trade-off; exact page breaks come from the "Exact PDF" tab (Task 7).

- [ ] **Step 3: Window — display the served HTML**

In `ui/pdf-export.js`, add:

```js
await listen("pdf-export-preview-html", (ev) => {
  const { html, error } = ev.payload;
  if (error) { el("status").textContent = "Preview error: " + error; return; }
  el("live-preview").srcdoc = html;
  el("status").textContent = "";
});
```

(Place the `listen` import-side: `const { listen, emit } = window.__TAURI__.event;` already added in Task 5.)

- [ ] **Step 4: Build + verify live preview updates**

Run: `cd src-tauri && cargo build && cargo run -- ../README.md`
Open the window; drag the font slider, switch presets, change margins/paper.
Expected: the live preview re-renders within ~120ms of each change — font size, fonts, accent color, justification, margins all visibly change; Mermaid/KaTeX/code render correctly and light.

- [ ] **Step 5: Commit**

```bash
git add ui/app.js ui/pdf-export.js
git commit -m "Add live HTML preview for the PDF export window"
```

---

## Task 7: Exact PDF preview + Export action

The window asks the main window to render the real PDF — to a temp file (exact preview tab) or to a chosen destination (Export). The main window runs `exportDocument("pdf", path, settings)` and reports back.

**Files:**
- Modify: `ui/app.js`, `ui/pdf-export.js`

**Interfaces:**
- Consumes: `exportDocument` (Task 4); `save_pdf_settings` (Task 3); `convertFileSrc`.
- Produces: main-window listener `pdf-export-run` `{ settings, mode, path? }` → emits `pdf-export-done` `{ ok, url?, error? }`. Uses a Tauri temp path for `mode:"exact"`.

- [ ] **Step 1: Main window — handle run requests**

In `ui/app.js` `init()`:

```js
  await listen("pdf-export-run", async (ev) => {
    const { settings, mode, path } = ev.payload;
    if (exportInProgress) {
      await emit("pdf-export-done", { ok: false, error: "an export is already in progress" });
      return;
    }
    let dest = path;
    try {
      if (mode === "exact") {
        const dir = await pathApi.tempDir();
        dest = await joinPath(dir, `mdviewer-pdf-preview-${Date.now()}.pdf`);
      }
      const ok = await exportDocument("pdf", dest, settings);
      if (ok && mode === "save") {
        await invoke("save_pdf_settings", { settings }).catch(() => {});
      }
      const url = ok && mode === "exact" ? convertFileSrc(dest) : undefined;
      await emit("pdf-export-done", { ok, url, error: ok ? undefined : "PDF export failed" });
    } catch (e) {
      await emit("pdf-export-done", { ok: false, error: String(e) });
    }
  });
```

Use the path API already imported for the app (check the existing `window.__TAURI__.path` usage; if `tempDir`/`join` aren't imported, add `const pathApi = window.__TAURI__.path;` and a small `joinPath` wrapper, or build the path with the temp dir + `/`). `convertFileSrc` is already imported in `app.js` (used by `renderImage`).

- [ ] **Step 2: Window — Export and Exact preview**

In `ui/pdf-export.js`:

```js
const { save } = window.__TAURI__.dialog;

let pending = null; // "save" | "exact"

await listen("pdf-export-done", (ev) => {
  const { ok, url, error } = ev.payload;
  if (!ok) { el("status").textContent = "Export failed: " + (error || ""); pending = null; return; }
  if (pending === "exact" && url) {
    el("exact-preview").src = url;
    showExactTab();
  } else if (pending === "save") {
    el("status").textContent = "Saved.";
  }
  pending = null;
});

el("export").addEventListener("click", async () => {
  const path = await save({
    defaultPath: "export.pdf",
    filters: [{ name: "PDF document", extensions: ["pdf"] }],
  });
  if (!path) return;
  pending = "save";
  el("status").textContent = "Exporting…";
  await emit("pdf-export-run", { settings, mode: "save", path });
});

function showExactTab() {
  el("tab-exact").classList.add("active");
  el("tab-live").classList.remove("active");
  el("exact-preview").hidden = false;
  el("live-preview").hidden = true;
}
function showLiveTab() {
  el("tab-live").classList.add("active");
  el("tab-exact").classList.remove("active");
  el("live-preview").hidden = false;
  el("exact-preview").hidden = true;
}
el("tab-live").addEventListener("click", showLiveTab);
el("tab-exact").addEventListener("click", async () => {
  pending = "exact";
  el("status").textContent = "Rendering exact PDF…";
  await emit("pdf-export-run", { settings, mode: "exact" });
});
```

> The default save filename should be the document's name. Since the window doesn't know the active path, pass it through: in the main window's `pdf-export-request-preview` handler also `emit("pdf-export-active-name", { name: baseName(activeTab().path) })` once on first preview, and have the window store it for the `defaultPath`. (Simpler: have the window request it on init via a new `get_active_doc_name` flow. Implement the emit-on-preview approach to avoid a new command.)

- [ ] **Step 3: Build + verify exact preview and export**

Run: `cd src-tauri && cargo build && cargo run -- ../README.md`
Open window → click **Exact PDF** tab.
Expected: after ~1s, the embedded viewer shows the real paginated PDF with correct margins + footers + page breaks. Click **Export…**, choose a path, confirm the file matches the exact preview and the status shows "Saved."

- [ ] **Step 4: Commit**

```bash
git add ui/app.js ui/pdf-export.js
git commit -m "Add exact PDF preview and Export action to the export window"
```

---

## Task 8: Route menu HTML export, `onExport`, and MCP through settings

Make every export path consistent: the menu's PDF item already opens the window (Task 5). Ensure HTML export and `mcp-generate-pdf` use saved settings, and that `exportDocument`'s callers pass settings explicitly.

**Files:**
- Modify: `ui/app.js`

**Interfaces:**
- Consumes: `get_pdf_settings` (Task 3), `defaultSettings`, `mergeSettings`.

- [ ] **Step 1: Add a settings fetch helper**

In `ui/app.js`:

```js
async function savedPdfSettings() {
  try {
    return mergeSettings(defaultSettings(), await invoke("get_pdf_settings"));
  } catch (e) {
    console.error("get_pdf_settings failed", e);
    return defaultSettings();
  }
}
```

- [ ] **Step 2: `onExport` (HTML path only now) passes settings**

`onExport` is now reached only for `format === "html"` (PDF opens the window). Pass saved settings to the HTML export so the polished look applies there too:

```js
  const settings = await savedPdfSettings();
  await exportDocument(format, path, settings);
```

(Keep the dialog/guards as-is.)

- [ ] **Step 3: `mcp-generate-pdf` uses saved settings**

In the `mcp-generate-pdf` listener, change the export call:

```js
    const ok = await exportDocument("pdf", output, await savedPdfSettings());
```

- [ ] **Step 4: Build + verify**

Run: `cd src-tauri && cargo build && cargo run -- ../README.md`
- HTML export (**File ▸ Export as HTML…**): open the `.html`; it should reflect the saved preset's typography.
- MCP `generate_pdf` (if testing via Claude Code): the produced PDF uses saved settings.
Expected: both honor saved settings; no regressions.

- [ ] **Step 5: Commit**

```bash
git add ui/app.js
git commit -m "Route HTML export and MCP generate_pdf through saved PDF settings"
```

---

## Task 9: Documentation + smoke-test matrix

**Files:**
- Modify: `README.md`, `CHANGELOG.md`

- [ ] **Step 1: Run the full smoke-test matrix**

Build (`cargo build`) and run on a torture-test document containing: a wide table (8+ columns), a Mermaid flowchart, KaTeX inline + display math, a long fenced code block, nested lists, blockquotes, and a local image. For each combination below, open the window, set it, click **Exact PDF**, and verify content fidelity + geometry:

- Presets: Clean, Report, Compact
- Paper: A4, Letter
- Page numbers: off, bottom-center, bottom-right
- Base size: min (9), default, max (16)

Expected (the non-negotiable bar): no clipped tables, Mermaid labels render (not blank), math renders, code doesn't overflow the right margin, footers sit in the bottom margin, page breaks don't split atomic blocks. Note any failures and fix before documenting.

- [ ] **Step 2: Update README**

In `README.md`, update the PDF export description (Features + Usage/Menus) to describe the new **Export to PDF** window: presets (Clean/Report/Compact), font size, paper, margins, page numbers, live preview + exact preview. Note it remains macOS-only.

- [ ] **Step 3: Update CHANGELOG**

Add a new `## [X.Y.Z] - 2026-06-17` section (version per the release process; do NOT bump `Cargo.toml`/`tauri.conf.json` here unless cutting the release) with user-facing bullets:

```markdown
### Added
- Customizable PDF export: a dedicated Export to PDF window with Clean / Report /
  Compact presets, adjustable font size, paper size, margins, and page numbers.
- Live preview plus an exact PDF preview so you can tune the result before saving.
- Page numbers and custom margins, rendered via a native post-processing pass.

### Changed
- HTML export and the MCP `generate_pdf` tool now use your saved PDF look.
```

- [ ] **Step 4: Final lint + commit**

Run: `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` and `cd ../ui && node --test pdf-presets.test.js`
Expected: all clean/green.

```bash
git add README.md CHANGELOG.md
git commit -m "Document customizable PDF export"
```

---

## Self-review notes

- **Spec coverage:** Presets + look → Task 2; five knobs → Tasks 2/5; window like preferences → Task 5; live + exact preview (hybrid C) → Tasks 6/7; persistence (global last-used, preset defaults) → Tasks 3/7/8; native margins+page numbers via post-processing with the spike first → Task 1; paper via NSPrintInfo + L/R via CSS → Task 4; MCP routes through settings → Task 8; macOS-only gating → Tasks 1/5 + Global Constraints; testing → pure JS (Task 2), Rust pure (Task 1) + recent (Task 3), smoke matrix (Task 9). All spec sections map to a task.
- **Type consistency:** `Settings` keys (`preset/baseSize/paper/margins/pageNumbers`) are identical across `pdf-presets.js`, the Rust `PdfSettings` (camelCase serde), and the `export_pdf` command args (`paper`, `margins` object, `pageNumbers`). `MarginsPts`/`marginMm` distinguish points (Rust) vs mm (JS) consistently; `mm_to_points` is the only bridge. `relayout`, `paper_points`, `settingsToCss`, `buildExportHtml`, `exportDocument(format, path, settings)` signatures are stable across the tasks that reference them.
- **Known spike risk (Task 1):** the exact `objc2-core-graphics` 0.3 method/constructor names and the footer text-drawing API are confirmed at build time; the task includes a fallback to `objc2-core-text` for the footer if the deprecated CG text API is absent. `relayout`'s signature is fixed so downstream tasks are unaffected by the chosen internals.
