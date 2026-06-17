# Customizable PDF Export — Design

**Date:** 2026-06-17
**Status:** Approved, pending implementation plan

## Problem

The current PDF export is "print the preview to a file": it reuses the on-screen
stylesheet through WKWebView's native print pipeline with fixed 16mm-ish margins,
a 980px content column, and font sizes baked into `styles.css` /
`github-markdown.css`. There is no customization and no preview before writing
the file.

Three pain points, in priority order:

- **A — Looks generic.** The PDF reads like a screenshot of the app, not a
  document that stands on its own.
- **B — Not tunable.** No control over font size, paper size, margins.
- **D — No feedback loop.** You export blind, open the file, dislike it, repeat.

Explicit goal from the user: **the PDF does not need to be identical to the
markdown render.** It should be a polished, first-class document in its own
right — while rendering *all* original content (text, tables, diagrams, math,
code) cleanly, readably, and professionally.

## Goals

1. A polished, professional PDF look driven by curated **presets** plus a small
   set of per-export knobs.
2. The five knobs: **preset, base font size, paper size, margins, page numbers.**
3. A separate **tuning window** (like the preferences window) with a **live
   preview** so the user can adjust and see the result before committing.
4. Persist the last-used configuration as the global default.
5. macOS-only, matching today's PDF support. Windows keeps HTML export only.

## Non-goals (YAGNI)

- Per-document settings (global last-used only).
- Custom/embedded fonts (system font stacks only; KaTeX fonts stay embedded).
- Cover pages, free-text headers/footers.
- Full manual font-family/heading control (lives inside presets).
- Windows PDF export.

## Key technical constraint

WKWebView's print pipeline **ignores both CSS `@page` margins and programmatic
`NSPrintInfo` margins**, forcing its own ~1-inch margins (documented at
`ui/app.js:2138`, and it bit the project before). It also exposes **no API for
print headers/footers** (page numbers).

Consequences for the design:

- **Paper size** is controllable via `NSPrintInfo` (`setPaperSize`/`setPaperName`).
- **Left/right margins** are controllable via export **CSS** (body horizontal
  padding / max-width flows on every page).
- **Top/bottom margins + page numbers** cannot come from the print engine. They
  are handled by a **PDFKit post-processing pass** after WebKit writes the
  content PDF.

The preset stylesheet, font size, paper size, and left/right margins — i.e. the
bulk of the visual win — do **not** depend on the PDFKit pass.

## Architecture

Three new pieces, each following an existing pattern in the codebase:

1. **Preset system + PDF stylesheet** — `ui/pdf-presets.js` (pure helpers,
   `node --test`) + a PDF-specific stylesheet. Source of the "stands out as its
   own document" look. Pure CSS/data, fully in our control.

2. **Export tuning window** — `ui/pdf-export.html` / `ui/pdf-export.js`, a
   separate webview window registered in `capabilities/default.json` and opened
   like `preferences` / `claude-integration`. Left rail = controls; right pane =
   preview. Owns no document state.

3. **Reworked native export path** — `src-tauri/src/export.rs` gains paper-size
   setting via `NSPrintInfo` and a **PDFKit post-processing step** for top/bottom
   margins + page numbers. Keeps the existing objc2-0.3 pinning, the async-print
   invocation, and the `%%EOF` completion poll.

### Data flow

```
pdf-export window (controls)
  → settings object { preset, baseSize, paper, margins, pageNumbers }
  → request rendered HTML from main window (reuse light-render pipeline)
  → settingsToCss(settings) applied to the export render
  → WebKit prints content PDF to temp file (paper size + L/R margins applied)
  → PDFKit pass: top/bottom margins + page-number stamping
  → final PDF at chosen destination
```

The live HTML preview uses the **same** `settingsToCss` output, so tuning is
instant and faithful for everything except exact page-break positions.

## Presets & the PDF look (point A)

A preset is a plain data object:

```
{ id, label, bodyFont, headingFont, baseSize, lineHeight, headingScale,
  accent, margins, paper, justify, pageNumbers }
```

Selecting a preset loads its defaults into the controls; the user can then tweak;
the resulting config persists globally. Starter presets:

- **Clean** (default) — humanist sans, refined GitHub-ish: generous spacing,
  subtle accent on headings + table headers. Closest to today but properly
  typeset.
- **Report** — serif body (system Georgia/serif stack), sans headings, justified
  text, classic document feel.
- **Compact** — smaller base size + narrow margins + tighter line height, for
  dense reference docs.

Every preset gets the quality baseline: real typographic heading scale, balanced
paragraph spacing, refined code-block + table styling, proper link treatment, and
the existing page-break safety (keep-headings-with-next, no-split atomic blocks,
wide-table fitting) carried over from today's export.

Fonts: **system font stacks only** — no embedding, preserving the no-build/no-bloat
constraint. KaTeX fonts remain embedded as data URLs as they are today.

## Export window & controls (Section 3)

`pdf-export` webview window, layout mirroring `preferences.html`:

**Left rail — controls:**

- **Preset** — Clean / Report / Compact. Selecting loads that preset's defaults.
- **Base font size** — slider + number (≈9–16pt), pre-filled from preset.
- **Paper size** — A4 / Letter / Legal.
- **Margins** — Narrow / Normal / Wide (named → mm values; no raw fields).
- **Page numbers** — None / Bottom-center / Bottom-right.
- Footer actions: **Reset to preset**, **Export…** (native save dialog → write).

**Right pane — preview** (see next section).

The window requests the active document's rendered HTML from the main window
(reusing the light-render pipeline) and sends settings back. It owns no document
state; everything but the persisted last-used config is ephemeral.

Pure helpers in `ui/pdf-presets.js`, all unit-tested without DOM (like
`export.js` / `review.js`):

- `presetDefaults(id)` — the preset's default settings object.
- `mergeSettings(preset, overrides)` — override precedence.
- `marginMm(name)` — Narrow/Normal/Wide → mm.
- `settingsToCss(settings)` — settings → the export stylesheet text.

## Preview mechanism (point D, Section 4)

Hybrid (live HTML + exact PDF), as a separate window.

**Live HTML preview (instant tuning):** right pane renders the document HTML with
`settingsToCss(settings)` applied, inside a simulated "page" — a white sheet at
the chosen paper's aspect ratio with the chosen margins as padding. Updates on
every control change (debounced ~100ms). Exact for typography, fonts, sizing,
colors, accent, and margins; only precise page-break positions are approximate.
Page-break guide lines are overlaid best-effort at multiples of the page content
height, clearly labeled "approximate".

**Exact preview + Export (the real PDF):** an **"Exact preview"** button (and an
auto-run when settings settle) renders the actual PDF to a **temp file** via the
native path and shows it in a second iframe (the WebView renders PDFs natively via
the asset protocol) — that iframe *is* the file. **Export…** runs the same render
straight to the chosen destination.

## Native page geometry (Section 4, native)

- **Paper size** → `NSPrintInfo.setPaperSize` / `setPaperName`. Reliable.
- **Left/right margins** → export CSS body padding / max-width. Flows per page.
- **Top/bottom margins + page numbers** → **PDFKit post-processing pass**
  (`PDFDocument` + Core Text) after WebKit writes the content PDF and the
  `%%EOF` poll confirms completeness.

**First implementation task is a spike** to settle the exact PDFKit mechanism:
re-laying content pages onto the target geometry vs. stamping an overlay, and
whether `NSPrintInfo` top/bottom margins can be coaxed to work via the
set-frame-to-imageable-area trick — picking whichever proves reliable. The
CSS-driven knobs, paper size, and the preset stylesheet are unblocked regardless
of the spike outcome.

## Persistence (Section 5)

Last-used settings stored in `recent.json` (new `pdf_export` field, alongside
`channel` / `last_folder`), read on window open, written on export. First-run
default = the **Clean** preset's defaults.

## MCP `generate_pdf`

Keeps working headlessly (no window) and **uses the persisted settings** (Clean
defaults if none), so Claude-generated PDFs get the same polished look. No new MCP
surface; the tool routes through the same settings-driven export. Optional future
work (out of scope): per-call preset/size overrides.

## Platform

macOS-only, as today. The window, presets, and live HTML preview are inherently
cross-platform code, but the menu item + window stay cfg-gated to macOS; Windows
keeps HTML export only. The native `export.rs` additions live in the existing
macOS-only module.

## Testing

- **JS pure helpers** (`ui/pdf-presets.js`) under `node --test`: preset defaults,
  merge/override precedence, margin mapping, `settingsToCss` output for known
  inputs.
- **Rust** (`export.rs`): factor page-number string formatting and margin
  geometry math into pure-ish units → `#[cfg(test)]`.
- **Manual smoke-test matrix**: each preset × {A4, Letter} × {page numbers on/off}
  on a torture-test document (wide tables, Mermaid, KaTeX, long code). Content
  fidelity is the non-negotiable bar.
- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` clean.

## Implementation task ordering (high level)

1. **Spike:** PDFKit post-processing for top/bottom margins + page numbers; settle
   the mechanism. (Unblocks the riskiest part first.)
2. Preset data + `pdf-presets.js` pure helpers + tests + the PDF stylesheet.
3. Wire `settingsToCss` into the existing export light-render path; paper size +
   L/R margins via `NSPrintInfo`/CSS.
4. PDFKit pass integrated into `export.rs` per the spike outcome.
5. `pdf-export` window (HTML/JS/CSS), controls, capability registration, menu
   item rewire.
6. Live HTML preview + exact PDF preview wiring.
7. Persistence in `recent.json`; `generate_pdf` routes through settings.
8. Smoke-test matrix; README + CHANGELOG.
