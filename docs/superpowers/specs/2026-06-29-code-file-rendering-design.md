# Syntax-highlighted, line-numbered view for code & text files

**Date:** 2026-06-29
**Status:** Approved

## Problem

Markdown files render richly and images get an image view, but every other
text file (`.rs`, `.py`, `.js`, `.json`, `.txt`, extensionless files, …) falls
through to `markdown::render_plain`, which HTML-escapes the text into a flat,
uncolored `<pre>`. Code looks ugly and is hard to read.

## Goal

Give non-markdown text files a beautiful, theme-aware **syntax-highlighted view
with a line-number gutter**, reusing the syntect engine the app already bundles
for markdown code fences. Unknown text types are still line-numbered (just
uncolored); binary files show a friendly "can't preview" notice instead of an
error or garbage.

## Approach (chosen)

Server-side syntect, line-numbered HTML. Rejected alternatives: a frontend JS
highlighter (duplicates syntect, widens CSP, re-solves theming) and wrapping
files in a synthetic ```` ```lang ```` fence through comrak (fragile, no native
line numbers, awkward escaping).

## Design

### 1. Render pipeline & detection order

`commands::render_file` currently renders markdown-and-not-raw via
`markdown::render_markdown`, else `markdown::render_plain`. New branching:

- **Markdown path** → `render_markdown` (rendered) or `render_plain` (raw
  toggle). Unchanged.
- **Non-markdown path** → new `code::render_code(contents, path, theme)`.

Images never reach `render_file` — the frontend short-circuits to `renderImage`
before the IPC. `commands::render_preview` (the split-view editor's live
preview) mirrors the same branch, so editing a code file shows a highlighted
preview.

### 2. New Rust module `src-tauri/src/code.rs`

`pub fn render_code(source: &str, path: &Path, theme: &str) -> String`:

- **Language detection:** syntect `SyntaxSet::find_syntax_by_extension(ext)` →
  fall back to `find_syntax_by_first_line(first_line)` (catches
  `#!/usr/bin/env python` on extensionless files) → fall back to the plain-text
  syntax.
- **Theme:** reuse the exact themes the markdown code-fence adapter uses —
  `InspiredGitHub` (light) and `base16-ocean.dark` (dark), loaded from
  `ThemeSet::load_defaults()` — so a standalone `.rs` file and a ```` ```rust ````
  fence look identical.
- **Output:** highlight line by line (`HighlightLines` +
  `styled_line_to_highlighted_html` with `IncludeBackground::No`), wrap each
  source line in `<span class="cl">…</span>`, join with `\n`, wrap the whole in
  `<pre class="code-view"><code>…</code></pre>`. Line numbers are drawn by CSS
  counters on `.cl` (see §4), so they are never selected or copied.
- **Binary / unreadable:** because invalid UTF-8 cannot reach a `&str`, binary
  detection operates on the raw bytes in `render_file`, via two pure helpers in
  `code.rs`: `is_binary(bytes: &[u8]) -> bool` (true when the bytes aren't valid
  UTF-8, or contain a NUL in the first chunk — mirrors `search.rs`) and
  `unsupported_html() -> String` (the centered `<div class="code-unsupported">
  Can't preview this file type</div>`). `render_file` reads the bytes once; for
  a non-markdown path, if `is_binary` it returns `unsupported_html`, otherwise it
  converts to a string and calls `render_code`. A genuine read failure such as
  permission-denied keeps the existing error path that surfaces through
  `showError`.
- **Large-file guard:** when the source exceeds ~2 MB or ~50,000 lines, skip
  syntect and emit line-numbered *escaped plain text*, so huge or minified
  files stay responsive.

### 3. Frontend

- `ui/filetype.js`: add `isMarkdownPath(path)` mirroring Rust
  `markdown::is_markdown_path` (extensions `md`, `markdown`, `mdown`, `mkd`,
  `mkdn`), unit-tested. A "code tab" is simply *not markdown and not image*.
- `ui/app.js` `paintHtml`: set the `#preview` article class from the tab's
  path — `markdown-body` for markdown, `code-view` for code/text (images
  already use their own `renderImage`/`image-view` path). This single change
  covers both the disk view (`renderActive`) and the editor live preview
  (`render_preview`), since both call `paintHtml`. The `raw-body` class is only
  applied on the markdown path.

### 4. CSS (`ui/styles.css`)

- `pre.code-view { counter-reset: line; tab-size: 4; overflow-x: auto; }`
- `.code-view .cl { display: block; }`
- `.code-view .cl::before { counter-increment: line; content: counter(line);
  …right-aligned, dimmed, fixed-width gutter with a separating border… }`
- `.code-unsupported { …centered notice… }`
- Dark mode: syntect emits the dark theme's token colors directly; the gutter
  and notice use the existing theme CSS variables, so no extra dark rules
  beyond what attribute-driven theming already provides.

### 5. Toolbar gating for code tabs

A code tab = not markdown, not image.

- **Raw** button → hidden (the highlighted view *is* the source).
- **Review** button → hidden (as for images).
- **Export HTML / PDF** → guarded/hidden (as for images). Avoids inlining
  code-view CSS into the export pipeline; possible later follow-up.
- **Edit** → stays available; text files remain editable and the preview is now
  highlighted. The CodeMirror editor keeps its current mode — per-language
  editor modes are out of scope.
- **Copy Source** → stays (copies the file text).

## Testing

- **Rust `code.rs` unit tests:** extension detection (`.rs` → Rust syntax),
  shebang/first-line fallback, plain-text fallback for an unknown extension,
  exactly one `.cl` span per source line, large input → highlighting skipped
  (still line-wrapped); `is_binary` true for NUL-containing / invalid-UTF-8
  bytes and false for normal source; `unsupported_html` contains the
  `code-unsupported` marker. Tests pass `&str`/`&[u8]` + `Path`, no file IO.
- **JS:** `ui/filetype.test.js` cases for `isMarkdownPath` (true for md
  variants, false for code/image/extensionless).
- **Manual:** open `.rs` / `.py` / `.json` (colored + numbered), a `.txt` and an
  extensionless file (numbered, possibly uncolored), and a binary (notice), each
  in light and dark themes; confirm theme toggle re-colors; confirm Raw/Review/
  Export are hidden and Edit still works on a code tab.

## Non-goals

- Per-language modes in the CodeMirror editor (editor stays markdown-mode).
- Exporting code listings to HTML/PDF.
- CSV/tabular or other rich non-text viewers.
- Changing image or markdown rendering.

## Files touched

- Create: `src-tauri/src/code.rs` (+ `mod code;` in `lib.rs`).
- Modify: `src-tauri/src/commands.rs` (`render_file`, `render_preview`
  branching; bytes read + binary detection).
- Modify: `ui/filetype.js`, `ui/filetype.test.js`, `ui/app.js` (`paintHtml`
  class + toolbar gating), `ui/styles.css`.
