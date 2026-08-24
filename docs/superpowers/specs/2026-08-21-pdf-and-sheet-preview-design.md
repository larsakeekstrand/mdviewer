# PDF and spreadsheet preview

**Date:** 2026-08-21
**Status:** Approved (brainstorming) — pending implementation plan

## Goal

MDViewer opens whatever the file tree shows it. Today that means markdown gets a
real render, images get an `<img>`, and everything else falls through
`isCodeView` to a syntax-highlighted source view — which is right for `.ts` and
`.toml`, and useless for a `.pdf` or an `.xlsx`, where it degrades to
`code::unsupported_html()`'s "Can't preview this file type".

This design adds two genuine renderers:

- **PDF** — the file itself, rendered by the webview's native PDF viewer in an
  iframe. No parsing on our side.
- **Excel** — sheet data as HTML tables, parsed server-side with `calamine`.

and refactors the render dispatch so that adding either one doesn't mean
scattering `|| isPdfPath(...)` through fourteen call sites.

## Decisions (locked during brainstorming)

| Question | Decision |
|---|---|
| Scope | **One spec, phased plan.** Seam → PDF → XLSX; shippable after any phase. |
| HTML preview | **Out of scope** (see below). `.html` keeps today's source view. |
| Excel fidelity | **Data grid, all sheets stacked.** Values and cached formula results; no fonts, colors, or column widths. |
| Excel sheet UI | **No tab strip.** One `<table>` per sheet under an `<h2>`, document order. |
| Where the seam lives | `viewKind(path)` in `ui/filetype.js`, plus **named capability predicates** derived from it. |
| PDF backend work | **None.** Frontend short-circuits before the `render_file` IPC, like images. |
| Excel backend work | New `src-tauri/src/xlsx.rs`, hooked into `render_file` before the `is_binary` check. |
| PDF in `#preview` | `<iframe>` via `convertFileSrc`. Not retained in the DOM cache. |
| Excel in `#preview` | Ordinary HTML through the existing `paintHtml` path, so it inherits the render cache, DOM retention, find, and print. |

**Out of scope (YAGNI):** HTML rendering; cell formatting, column widths, charts,
and pivot tables in Excel; a sheet tab strip; text extraction or search inside
PDFs; editing either format; thumbnails in the file tree.

---

## Why HTML was cut, and what it would have taken

Worth recording, because the seam is being built partly so this stays cheap
later.

Rendered HTML **must not** be injected into `#preview`. That element lives on
`tauri://localhost` with `window.__TAURI__` exposed — file read/write,
`open_path`, the lot — and `script-src 'self'` does not save us there, because a
relative `<script src="./x.js">` in an untrusted document resolves *into the app
bundle origin*. The correct shape is `<iframe sandbox="" src="asset://…">`: no
`allow-scripts`, no `allow-same-origin`, opaque origin, no reach into the parent.
The existing CSP already permits the frame.

Two things would need empirical verification before committing to it: whether
Tauri applies the app CSP to `asset://` responses (which would break relative
CSS and images inside the framed document), and confirmation that `sandbox`
severs `parent.__TAURI__` under an `asset:` origin. There is also a policy
question with no free answer — a sandboxed frame still makes network requests,
so an untrusted page's CDN references and tracking pixels would fire on open.

None of that is hard; all of it is *deciding*, and it was holding up two
renderers that need no decisions at all.

---

## 1. The seam: `viewKind` and capability predicates

### The problem with a straight substitution

`isImagePath` and `isCodeView` are currently consulted at fourteen sites in
`app.js`, but they are not asking one question each. `isCodeView` is doing
double duty:

| Meaning | Sites |
|---|---|
| "edits in place; there is no split live-preview pane" | 1395, 1618, 1662, 1744, 1804, 2282 |
| "raw view is meaningless for this file" | 1484, 3459 |

and `isImagePath` is standing in for "not a text document at all":

| Meaning | Sites |
|---|---|
| "live reload needs a URL cache-bust, not a re-render" | 286 |
| "not eligible for retained-DOM restore" | 1379 |
| "not editable" | 1483, 1596, 3453 |
| "has its own render path, before the IPC" | 1843 |

(1483 and 1484 sit adjacent in `renderTabBar`, which consults both predicates.)

Replacing both with `switch (viewKind(path))` at fourteen sites would preserve
the conflation and make every future format a fourteen-site edit. So the seam is
two layers, not one.

### The design

`ui/filetype.js` gains:

```js
export function viewKind(path)   // "markdown" | "image" | "pdf" | "sheet" | "code"
```

resolved by extension, in that precedence order, with `code` as the fallback —
preserving today's property that an unrecognized extension degrades to source
view rather than breaking. Then the questions the call sites actually ask, each
a pure function of the kind:

```js
isEditable(kind)      // markdown, code          — Edit button, toggle-edit
hasSplitPreview(kind) // markdown                — live-preview pane vs in-place
hasRawToggle(kind)    // markdown                — Raw button, toggle-raw
isAnnotatable(kind)   // markdown                — Review Mode
isRetainable(kind)    // markdown, code, sheet   — retained-DOM eligibility
isExportable(kind)    // markdown, sheet         — HTML/PDF export
rendersFromDisk(kind) // pdf, image              — own path, before render_file
bustsCacheOnChange(kind) // pdf, image           — live reload cache-bust
```

Every one is pure and table-driven, unit-tested in `ui/filetype.test.js` under
`node --test` — the repo's standing pattern. The call sites read as the question
they mean (`if (!isEditable(kind)) return;`), and a future format is one row in
the tables, not a fourteen-site audit.

`isMarkdownPath` stays: `markdown.rs` and `onExport` both use it as a positive
allowlist and that reading is still correct. `isImagePath` stays as a thin
wrapper over `viewKind`, since `renderImage` and the tree both want it by name.
`isCodeView` is deleted — it is exactly the conflation being removed.

**Gate audit.** The first implementation task is a table mapping each of the
fourteen sites to its predicate and the intended answer for `pdf` and `sheet`,
reviewed before any of it is edited. Two sites are known to need *new* gating
rather than substitution, because they have no type check today:

- `openFind` / `collectFindSegments` (3846, 3920) — operates on whatever
  `#preview` holds. For a PDF tab that is an iframe whose contents we cannot
  reach, so find would open and report no matches. Gate it on the kind.
- `onExport` (2229) — already correctly excludes PDF via its `isMarkdownPath`
  allowlist. Enabling sheet export is a deliberate relaxation to
  `isExportable(viewKind(path))`, with a test, not a freebie.

---

## 2. PDF preview

**Zero Rust.** `renderActive` short-circuits to `renderPdf(t)` *before* the
`render_file` IPC — the same position and for the same reason as `renderImage`:
the backend does `std::fs::read`, and a PDF would hit `code::is_binary` and come
back as `unsupported_html()`.

```js
function renderPdf(t, { scrollLock = true } = {}) {
  // <iframe src={convertFileSrc(path)}> into #preview
  // preview.className = "pdf-view"
  // liveRender = null
}
```

Mirrors `renderImage` closely enough to be read side by side with it, including
the same-file/different-file scroll decision and the `?v=N` cache-bust on live
reload (a `pdfVersions` counter alongside `imageVersions`, bumped at site 286).

**No CSP change.** `frame-src 'self' asset: http://asset.localhost` is already
present, and `assetProtocol.scope` is `["**"]`. This exact mechanism is already
shipping: `ui/pdf-export.html`'s `exact-preview` iframe is handed
`convertFileSrc(dest)` at `app.js:484` and the webview renders it natively.

**Not retained.** `liveRender = null` keeps PDF tabs out of the DOM cache. An
iframe's internal viewer state (scroll, zoom, page) is not ours to stash in a
`DocumentFragment`, and reattaching one would reload the document anyway.

**Gates off:** Edit, Save, Raw, Review, find, both exports, copy-source.

**Deliberate limitation:** a PDF tab is inert to the app's own find and export.
It is the webview's viewer in a box; we do not own its DOM. The alternative —
vendoring pdf.js — is a multi-megabyte dependency plus a `worker-src` CSP
widening, to re-implement what both target webviews already do natively.

**Cross-platform.** macOS WKWebView renders PDF in an iframe (proven by
`exact-preview`). Windows WebView2 ships Edge's built-in PDF viewer and is
expected to behave the same. This is the one platform assumption in the design,
so the plan carries an **explicit Windows verification task** rather than
inheriting it silently. If WebView2 declines to frame an `asset://` PDF, the
fallback is a "Open in default application" panel via the existing `open_path`
command — note that `pdf` is not in `UNSAFE_OPEN_EXTS`, and must not be added.

---

## 3. Excel preview

**New module** `src-tauri/src/xlsx.rs`, using `calamine` (MIT, pure Rust; reads
xlsx/xlsm/xlsb/xls, and ods for free).

**Hook point.** In `commands::render_file`, *before* the `is_binary` check —
xlsx is a zip, so it is binary by that test and would otherwise short-circuit to
`unsupported_html()`:

```rust
let html = if markdown::is_markdown_path(&p) {
    …
} else if xlsx::is_spreadsheet_path(&p) {
    xlsx::render_workbook(&bytes, &p)?
} else if crate::code::is_binary(&bytes) {
    …
```

`render_file` has already read the bytes, so `calamine`'s
`open_workbook_auto_from_rs(Cursor::new(bytes))` reuses that read. This matters
beyond tidiness: it leaves the existing `current_stamp` / `stable_stamp` flow
completely untouched, so the render cache, the mid-read-change guard, and the
`html: None` unchanged-answer protocol all keep working with no xlsx-specific
handling.

**Output shape.** One `<h2>` per sheet name, followed by one `<table>`, in
workbook order. First row rendered as `<thead>` — a heuristic, but the near
universal shape of a spreadsheet, and it makes the existing
`thead { display: table-header-group }` print rule repeat headers across PDF
pages for free. All cell text escaped through `code.rs`'s `escape_html`, which is private today and
becomes `pub(crate)` — the one incidental change outside the new module.
`preview.className = "sheet-body"`, with table styling in `styles.css` keyed on
it, theme-aware via the existing CSS variables.

**Why server-side HTML and not a JS parser.** Because it lands in `#preview` as
ordinary markup, an Excel tab inherits find-in-page, HTML and PDF export, print,
the byte-bounded render cache, retained-DOM restore, and per-tab scroll — all
unmodified. A vendored SheetJS rendering client-side would have to re-earn every
one of those.

**Caps.** The input is untrusted and zip-backed, so `code.rs`'s
`MAX_BYTES`/`MAX_LINES` precedent extends here with xlsx-appropriate numbers: a
compressed-file ceiling, a decompressed-size ceiling (zip-bomb defense —
`code.rs`'s flat byte cap does not cover an archive), plus per-sheet row and
total-cell caps. Exceeding any cap renders what fits and appends a visible
truncation notice, matching how `code.rs` handles an over-long file. Exact
numbers are a plan-time decision; `code.rs`'s 2 MB is too low for real
spreadsheets.

**Fidelity is data, not formatting.** `calamine` exposes cell values and cached
formula results, not styling. Merged ranges are available in recent versions and
will be honored via `colspan`/`rowspan` if the pinned version's API allows;
otherwise merged cells render as their top-left value with blanks beside, and
that is acceptable. Dates need explicit handling — they arrive as serial numbers
and must be converted via `calamine`'s date helpers, or a spreadsheet of dates
previews as a column of five-digit integers.

**Gates off:** Edit, Save, Raw, Review. Review Mode's `ANNOTATABLE_TAGS` is
`P/H1-6/LI/PRE/BLOCKQUOTE` — it would skip the tables entirely and annotate only
the sheet headings, which is nonsense. **Gates on:** both exports, find, print.

**Pure-function split.** The cell-grid → HTML transformation is a pure function
over an in-memory representation, unit-tested with `#[cfg(test)]` against
hand-built grids; only workbook opening touches `calamine`. Cap enforcement,
`is_spreadsheet_path`, and date conversion are pure and tested independently.

---

## Cross-cutting

**Tab model.** No new persisted per-tab fields. `pdfVersions` is a module-level
`Map`, matching `imageVersions` — ephemeral, never in session restore. Existing
tabs keep working unchanged; `viewKind` is derived from `tab.path` on demand,
never stored, so the rename-retarget path needs no new invalidation.

**Retained-DOM invariant.** CLAUDE.md's warning that "any site that repoints
`tab.path` must evict that tab's DOM-cache entry first" is unaffected: PDF tabs
are never cached, and sheet tabs go through the ordinary `paintHtml` path whose
eviction rules already cover them. No new eviction sites.

**`postRender`.** Sheet renders pass through it. Its hooks (`annotateLinks`,
`resolveImages`, `addCopyButtons`, `renderMath`, `renderMermaid`,
`renderReviewMarkers`) are no-ops on a table of escaped text, so no reordering
is needed. PDF renders bypass it entirely, as image renders do.

**New IPC commands: none.** `render_file` gains a branch; its signature and
`Result<T, String>` contract are unchanged. No new attack surface beyond the
`calamine` parser itself, which is why the caps above are part of the design
rather than a hardening pass.

**Security review.** The xlsx path parses untrusted, attacker-controllable
archives in Rust — new territory for this codebase. A `security-reviewer` pass
is a plan task, focused on the cap enforcement and the escaping path.

**Cross-platform.** Excel is pure Rust and identical on both targets. PDF is the
platform question, covered above with its own verification task. Nothing here is
`cfg`-gated and no menu item needs hiding on Windows.

**Build.** Phases 1 and 2 touch only `ui/*`, which Tauri bundles at compile time
via `tauri-codegen` — `cargo build` is required to see any of it, and testing a
change by reloading the webview will show stale UI.

**Verification gate** for every task: `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test`, and `node --test` for
the JS helpers.

---

## Phasing

1. **Seam** — `viewKind` + capability predicates + gate audit. Behavior
   unchanged; the test suite is the proof. Shippable alone as a refactor.
2. **PDF** — `renderPdf`, cache-bust, gates, Windows verification.
3. **Excel** — `xlsx.rs`, `render_file` branch, caps, styling, export
   relaxation, security review.

Each phase leaves the app in a shippable state.
