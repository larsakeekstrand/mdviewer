# Export rendered document (HTML & PDF) — design

**Date:** 2026-05-23
**Status:** Approved. Split into two implementation plans — **Plan 1: HTML**
(first), **Plan 2: PDF** (second). See "Implementation phasing" below.

## Goal

Let the user export the active document as a self-contained **HTML** file and as
a paginated **PDF**. Both are always rendered in **light** theme, regardless of
the app's current theme, so the output looks right when shared or printed. Math
(KaTeX), Mermaid diagrams, code highlighting, tables, and local images all
survive the export.

## Non-goals

- **docx** — explicitly deferred to a later iteration. A high-fidelity `.docx`
  needs either pandoc (an external runtime dependency, against this project's
  vendor-everything philosophy) or a from-scratch AST→docx writer in Rust. We
  ship HTML + PDF first and revisit docx separately.
- Exporting more than one tab at a time (active document only).
- Theme choice per export — always light (see decision below).
- A print *preview* UI. PDF goes straight to a file via the save dialog.

## Key decisions (settled during brainstorming)

1. **Scope:** HTML and PDF now; docx later.
2. **PDF mechanism:** direct silent save (no system print panel) via native
   `WKWebView` print interop — not `window.print()`.
3. **Export theme:** always light, consistent with the existing Mermaid SVG/PNG
   export (which already forces `theme:"default"`).

## Implementation phasing

HTML and PDF ship as two separate implementation plans. HTML carries no external
or native risk; PDF is gated on the objc print interop, so it's isolated.

**Plan 1 — HTML export (first):**

- The shared `exportDocument` orchestrator foundation: in-progress guard, state
  snapshot, **light re-render**, restore. (PDF reuses this untouched.)
- HTML serialize + inline (skeleton, CSS embed, KaTeX-font inlining, image
  data-URLs).
- The pure helpers (`exportFilename`, `inlineFontUrls`, `documentNeedsKatex` /
  `documentNeedsMermaid`) and their `node --test` coverage.
- The `export-html` File-menu item and the `export` event listener (dispatching
  only `"html"` for now).
- `save_export` is reused as-is.

**Plan 2 — PDF export (second):**

- `src-tauri/src/export.rs` with the native `export_pdf` command and objc interop.
- `objc2` / `objc2-app-kit` / `objc2-foundation` deps; `lib.rs` registration.
- The `@media print` + `body.exporting` stylesheet.
- The `export-pdf` File-menu item; the orchestrator's PDF dispatch branch.

Everything below describes the full feature; each plan implements its slice.

## Why a re-render is required

The fully rendered document only exists in the **frontend DOM** after
`postRender` (KaTeX → HTML spans, Mermaid → inline SVG). When the app is in dark
theme, that DOM has **baked-in dark artifacts**: syntect emits inline `style=`
colors on code blocks, and Mermaid SVGs are generated against the dark theme.
These can't be reliably recolored to light by overriding CSS.

Therefore export does not scrape the current DOM — it **re-renders the active
document in light theme** through the existing pipeline, exports, then restores
the previous state. This guarantees fidelity (same renderer, no second engine)
at the cost of a brief visible flash to light during export. The flash is
unavoidable for PDF regardless, because the native print captures the on-screen
webview.

## Approach

### Shared orchestrator — `exportDocument(format, path)` (app.js)

The `export` event listener opens the native save dialog first (filename derived
from the active tab) and only calls `exportDocument` once it has a path — a
cancelled dialog does no work. The orchestrator then drives both formats:

1. Guard against concurrent/overlapping exports and live-reload races with an
   in-progress flag.
2. Snapshot current state: active tab's `theme`, `raw` flag, and
   `previewScroll.scrollTop`.
3. Re-render the active tab with `theme:"light"`, `raw:false` through the
   existing `renderActive` / `postRender` path so math and Mermaid render light.
4. Dispatch to the format step:
   - **HTML** (Plan 1): serialize + inline (below), then `save_export`.
   - **PDF** (Plan 2): add `body.exporting`, call the native `export_pdf`
     command, remove `body.exporting`.
5. Restore: re-render the original `theme`/`raw`, restore scroll position.

The save dialog is the native one from `tauri-plugin-dialog`, already used by the
Mermaid export.

### HTML export — serialize + inline

Produce one portable `.html` from the light-rendered `#preview` subtree:

- **Skeleton:** `<!doctype html>` … `<body class="markdown-body">…</body>` with a
  white background.
- **Styles:** fetch the vendored `github-markdown.css` and embed it in a
  `<style>`. Fetch and embed `katex/katex.css` **only when the document contains
  math** (a `data-math-*` element is present).
- **KaTeX fonts:** rewrite `url(fonts/*.woff2)` references in the embedded KaTeX
  CSS to `data:` URLs (fetch each referenced woff2 → base64). Only done when math
  is present. This is what makes the file standalone — without it the `.html`
  would reference font files that don't travel with it.
- **Images:** rewrite local / `asset:` `<img src>` to `data:` URLs (fetch → blob
  → dataURL). Remote (`http(s):`) and existing `data:` srcs are left alone.
- **Mermaid:** SVGs are already inline in the DOM after the light re-render;
  serialized verbatim.
- **Write:** reuse the existing `save_export` command (`base64_encoded: false`).

Pure, testable helpers (extracted so `node --test` can cover them without a DOM):

- `exportFilename(srcPath, ext)` — `…/README.md` → `README.html` / `README.pdf`.
- `inlineFontUrls(cssText, fontMap)` — replace `url(fonts/x.woff2)` with the
  matching `data:` URL.
- `documentNeedsKatex(html)` / `documentNeedsMermaid(html)` — feature detection
  driving conditional inlining.

### PDF export — native silent print

New macOS-gated Rust command in **`src-tauri/src/export.rs`**:

```text
export_pdf(window, path) -> Result<(), String>
```

- Reach the `WKWebView` via Tauri's `webview.with_webview(|w| …)`. The closure
  runs on the **main thread**, which `NSPrintOperation` requires.
- Build `NSPrintInfo`: `jobDisposition = NSPrintSaveJob`, dictionary
  `NSPrintJobSavingURL = file://<path>`.
- `op = webView.printOperation(with: printInfo)` — this is WebKit's own
  **paginated** print path (proper Letter/A4 pages via `@page` CSS), unlike
  `createPDF`, which tends to emit a single oversized page.
- `op.showsPrintPanel = false`, `op.showsProgressPanel = false`,
  `op.runOperation()`.
- objc interop via `objc2` + `objc2-app-kit` + `objc2-foundation`.

Before the call, the orchestrator sets `body.exporting`; the print stylesheet
(below) hides app chrome and reflows the preview so the capture is the document
only.

### Print stylesheet (styles.css)

`@media print`, plus a `body.exporting` selector so the same rules apply during
the native capture:

- Hide sidebar, tab bar, banner, splitter, and any export/copy buttons.
- `.preview` / `.preview-scroll`: full width, `overflow: visible`, height
  driven by content (so it paginates instead of clipping to the viewport).
- `break-inside: avoid` on `pre`, `pre.mermaid`, `table`, `img` to reduce ugly
  splits across page boundaries.
- Sensible `@page` margins.

### Menu + event wiring (menu.rs + app.js)

- `menu.rs`: add two items to the **File** submenu after Open Recent (before the
  `close_window` separator): `export-html` "Export as HTML…" and `export-pdf`
  "Export as PDF…". Each emits an `export` event carrying the format
  (`"html"` / `"pdf"`), mirroring the existing `edit-action` pattern.
- `app.js`: a `listen("export", …)` handler opens the save dialog (filename
  derived from the active tab) and calls `exportDocument(format)`.
- When no tab is open, the handler is a no-op (a short banner message).

## Components and changes

| File | Change |
|------|--------|
| `src-tauri/src/export.rs` | **new** — `export_pdf` command; native `WKWebView` print interop (macOS-gated) |
| `src-tauri/src/lib.rs` | register `export_pdf` in the command handler |
| `src-tauri/src/menu.rs` | two File-menu items; emit `export` event `{format}` |
| `src-tauri/Cargo.toml` | add `objc2`, `objc2-app-kit`, `objc2-foundation` (macOS target) |
| `ui/app.js` | `export` listener; `exportDocument` orchestrator; HTML serialize + inline helpers; reuse `save_export` |
| `ui/styles.css` | `@media print` + `body.exporting` rules |
| `CLAUDE.md` | document the export pipeline and the print-interop gotchas |

## Testing

- **JS unit tests** (`node --test`, as `search.test.js` already does): the pure
  helpers — `exportFilename`, `inlineFontUrls`, `documentNeedsKatex` /
  `documentNeedsMermaid`. DOM serialization and the native print are verified
  manually (no DOM/objc in the test runner).
- **Rust:** thin — a unit test for any path/filename helper on the Rust side if
  one exists; the native print path can't be unit-tested.
- **Manual checklist:** export a document containing code, a table, inline +
  display math, a Mermaid diagram, and a local image. Verify:
  - the `.html` opens standalone in a browser **offline** (fonts, images,
    diagram all present), and is light-themed;
  - the `.pdf` is light, paginated across multiple pages, with no panel shown,
    and faithful to the on-screen render.

## Risks & mitigations

- **objc interop fragility (primary risk):** messaging the raw `WKWebView`
  handle is sensitive to macOS/Tauri versions and must stay on the main thread
  (satisfied by `with_webview`). All native code is isolated in `export.rs`. If
  it proves unworkable, the fallback is `window.print()` — same print stylesheet,
  but routed through the system print panel. This is a small, contained swap.
- **Export flash:** the brief switch to light during export is accepted
  (unavoidable for PDF; minor for HTML).
- **Self-contained HTML size:** inlining KaTeX woff2 fonts adds ~0.8 MB, but
  only when the document actually contains math.

## Edge cases

- No tab open → export menu items are a no-op with a short banner.
- Non-markdown / plain-text file → exports the plain-rendered `<pre>` block; the
  same pipeline still applies.
- Export triggered during a live reload → the in-progress flag serializes them.
- Cancelled save dialog → no re-render, no work.
