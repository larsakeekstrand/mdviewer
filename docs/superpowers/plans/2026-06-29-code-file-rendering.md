# Code & Text File Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give non-markdown text files a theme-aware, syntax-highlighted view with a line-number gutter (and a friendly notice for binaries), reusing the bundled syntect engine.

**Architecture:** A new server-side Rust module `code.rs` highlights a file line-by-line with syntect and returns `<pre class="code-view">` HTML where each source line is a `<span class="cl">`; CSS counters draw the gutter. `render_file`/`render_preview` route non-markdown files to it. The frontend tags `#preview` with a `code-body` class and hides Raw/Review/Export for code tabs.

**Tech Stack:** Rust + syntect 5.3 (already a direct dep), Tauri commands returning `Result<_, String>`, vanilla JS/CSS frontend (no build step; bundled at compile time, so changes need `cargo build`).

## Global Constraints

- syntect themes MUST match the markdown code-fence adapter: `InspiredGitHub` (light), `base16-ocean.dark` (dark). Loaded from `ThemeSet::load_defaults()`.
- Line numbers come from CSS counters on `.cl::before` — never literal text — so selection/copy excludes them.
- Binary detection runs on raw bytes in `render_file` (invalid UTF-8 cannot reach a `&str`), via pure `code::is_binary(&[u8])`.
- Large-file guard: skip syntect above ~2 MB or ~50,000 lines; still line-number as escaped plain text.
- A "code tab" = not markdown and not image (`isCodeView`).
- Toolbar for code tabs: Raw hidden, Review hidden, Export blocked (markdown-only), Edit kept, Copy Source kept.
- Markdown and image rendering paths are unchanged. Editor stays markdown-mode.
- Tauri commands return `Result<T, String>` with `format!("…: {e}")` errors.
- Lint clean before each commit: `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` from `src-tauri/`.

---

### Task 1: Rust `code.rs` highlighting module

**Files:**
- Create: `src-tauri/src/code.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod code;`)

**Interfaces:**
- Produces:
  - `pub fn render_code(source: &str, path: &std::path::Path, theme: &str) -> String`
  - `pub fn is_binary(bytes: &[u8]) -> bool`
  - `pub fn unsupported_html() -> String`
  - `pub fn detect_language(source: &str, path: &std::path::Path) -> String` (used internally; `pub` for tests)

- [ ] **Step 1: Register the module**

In `src-tauri/src/lib.rs`, add after `mod claude_hook;` (keep alphabetical-ish grouping; placing it before `mod commands;` is fine):

```rust
mod code;
```

- [ ] **Step 2: Write `src-tauri/src/code.rs` with implementation + tests**

```rust
use std::path::Path;
use std::sync::LazyLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::html::{styled_line_to_highlighted_html, IncludeBackground};
use syntect::parsing::SyntaxSet;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_nonewlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_LINES: usize = 50_000;

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// True when the bytes look binary: a NUL in the first 8 KB, or not valid UTF-8.
pub fn is_binary(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(8192)];
    if head.contains(&0) {
        return true;
    }
    std::str::from_utf8(bytes).is_err()
}

/// Centered notice shown in place of a preview for binary / unreadable files.
pub fn unsupported_html() -> String {
    "<div class=\"code-unsupported\">Can't preview this file type</div>".to_string()
}

/// Name of the syntect syntax chosen for this file (by extension, then first
/// line, then plain text). Public for unit tests.
pub fn detect_language(source: &str, path: &Path) -> String {
    let ss = &SYNTAX_SET;
    let syntax = path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(|ext| ss.find_syntax_by_extension(ext))
        .or_else(|| {
            source
                .lines()
                .next()
                .and_then(|first| ss.find_syntax_by_first_line(first))
        })
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    syntax.name.clone()
}

fn plain_numbered(source: &str) -> String {
    let mut out = String::from("<pre class=\"code-view\"><code>");
    for line in source.lines() {
        out.push_str("<span class=\"cl\">");
        out.push_str(&escape_html(line));
        out.push_str("</span>");
    }
    out.push_str("</code></pre>");
    out
}

/// Render `source` as a syntax-highlighted, line-wrapped HTML block. Each source
/// line becomes a `<span class="cl">`; the line-number gutter is drawn by CSS.
pub fn render_code(source: &str, path: &Path, theme: &str) -> String {
    if source.len() > MAX_BYTES || source.lines().count() > MAX_LINES {
        return plain_numbered(source);
    }

    let ss = &SYNTAX_SET;
    let syntax = path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(|ext| ss.find_syntax_by_extension(ext))
        .or_else(|| {
            source
                .lines()
                .next()
                .and_then(|first| ss.find_syntax_by_first_line(first))
        })
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let theme_obj = match theme {
        "dark" => &THEME_SET.themes["base16-ocean.dark"],
        _ => &THEME_SET.themes["InspiredGitHub"],
    };

    let mut h = HighlightLines::new(syntax, theme_obj);
    let mut out = String::from("<pre class=\"code-view\"><code>");
    for line in source.lines() {
        let html_line = h
            .highlight_line(line, ss)
            .ok()
            .and_then(|regions| {
                styled_line_to_highlighted_html(&regions, IncludeBackground::No).ok()
            })
            .unwrap_or_else(|| escape_html(line));
        out.push_str("<span class=\"cl\">");
        out.push_str(&html_line);
        out.push_str("</span>");
    }
    out.push_str("</code></pre>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_rust_by_extension() {
        assert_eq!(detect_language("fn main() {}", Path::new("x.rs")), "Rust");
    }

    #[test]
    fn falls_back_to_first_line_shebang() {
        assert_eq!(
            detect_language("#!/usr/bin/env python3\nprint(1)\n", Path::new("script")),
            "Python"
        );
    }

    #[test]
    fn unknown_extension_is_plain_text() {
        assert_eq!(
            detect_language("just some notes", Path::new("NOTES.zzz")),
            "Plain Text"
        );
    }

    #[test]
    fn one_cl_span_per_source_line() {
        let html = render_code("a\nb\nc", Path::new("x.rs"), "light");
        assert_eq!(html.matches("class=\"cl\"").count(), 3);
        assert!(html.starts_with("<pre class=\"code-view\">"));
    }

    #[test]
    fn rust_source_is_colored() {
        // syntect emits inline `style=` on highlighted tokens.
        let html = render_code("fn main() {}", Path::new("x.rs"), "light");
        assert!(html.contains("style="), "expected colored output, got: {html}");
    }

    #[test]
    fn large_file_skips_highlighting() {
        let big = "x\n".repeat(MAX_LINES + 1);
        let html = render_code(&big, Path::new("x.rs"), "light");
        assert!(
            !html.contains("style="),
            "large file should bypass syntect (no inline styles)"
        );
        assert!(html.contains("class=\"cl\""));
    }

    #[test]
    fn binary_detection() {
        assert!(!is_binary(b"hello world"));
        assert!(is_binary(b"a\0b"));
        assert!(is_binary(&[0xff, 0xfe, 0x00]));
    }

    #[test]
    fn unsupported_notice_has_marker() {
        assert!(unsupported_html().contains("code-unsupported"));
    }
}
```

- [ ] **Step 3: Run the tests (expect PASS)**

Run: `cd src-tauri && cargo test code:: 2>&1 | tail -15`
Expected: the 8 `code::tests::…` tests pass. (If `find_syntax_by_first_line` returns a different name than `"Python"` for the shebang in this syntect build, adjust the expected name to the reported one — run `cargo test code::tests::falls_back_to_first_line_shebang -- --nocapture` and read the assertion's actual value.)

- [ ] **Step 4: Lint**

Run: `cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings 2>&1 | tail -5`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/code.rs src-tauri/src/lib.rs
git commit -m "Add code.rs: syntect line-numbered highlighting for files"
```

---

### Task 2: Route non-markdown files through `code.rs`

**Files:**
- Modify: `src-tauri/src/commands.rs` (`render_file` ~lines 112-129, `render_preview` ~lines 133-146)

**Interfaces:**
- Consumes: `code::render_code`, `code::is_binary`, `code::unsupported_html` (Task 1).
- Produces: `render_file` returns highlighted HTML for code, the notice for binaries; `render_preview` returns highlighted HTML for code buffers.

- [ ] **Step 1: Rewrite `render_file` to read bytes and branch**

Replace the body of `render_file` (keep the signature and the `RenderedFile`/`crate::code` paths; `code` is a crate module, reference it as `crate::code`):

```rust
#[tauri::command]
pub fn render_file(
    path: String,
    theme: Option<String>,
    raw: Option<bool>,
) -> Result<RenderedFile, String> {
    let p = PathBuf::from(&path);
    let bytes =
        std::fs::read(&p).map_err(|e| format!("cannot read '{}': {}", p.display(), e))?;
    let theme = theme.as_deref().unwrap_or("light");
    let raw = raw.unwrap_or(false);
    let html = if markdown::is_markdown_path(&p) {
        let contents = String::from_utf8_lossy(&bytes);
        if raw {
            markdown::render_plain(&contents)
        } else {
            markdown::render_markdown(&contents, theme)
        }
    } else if crate::code::is_binary(&bytes) {
        crate::code::unsupported_html()
    } else {
        let contents = String::from_utf8_lossy(&bytes);
        crate::code::render_code(&contents, &p, theme)
    };
    Ok(RenderedFile { html, path, raw })
}
```

- [ ] **Step 2: Rewrite `render_preview`'s non-markdown branch**

```rust
#[tauri::command]
pub fn render_preview(
    source: String,
    path: String,
    theme: Option<String>,
) -> Result<String, String> {
    let p = PathBuf::from(&path);
    let theme = theme.as_deref().unwrap_or("light");
    if markdown::is_markdown_path(&p) {
        Ok(markdown::render_markdown(&source, theme))
    } else {
        Ok(crate::code::render_code(&source, &p, theme))
    }
}
```

- [ ] **Step 3: Build + lint**

Run: `cd src-tauri && cargo build 2>&1 | tail -3 && cargo clippy --all-targets -- -D warnings 2>&1 | tail -5`
Expected: compiles clean (no warnings). The existing 160+ Rust tests still pass: `cargo test 2>&1 | grep "test result" | head`.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "Render non-markdown files as highlighted code"
```

---

### Task 3: Frontend — class tagging, file-type helpers, and code-view CSS

**Files:**
- Modify: `ui/filetype.js`, `ui/filetype.test.js`
- Modify: `ui/app.js` (import at line 30; `paintHtml` at ~1662-1672)
- Modify: `ui/styles.css` (after the `.image-view` rules, ~line 507)

**Interfaces:**
- Consumes: nothing from Task 2 at the JS layer (it just paints whatever HTML the command returns).
- Produces: `isMarkdownPath(path)`, `isCodeView(path)` exports; `#preview` gains the `code-body` class for code tabs; `.code-view` gutter CSS.

- [ ] **Step 1: Write failing tests for the new filetype helpers**

First extend the existing import at the top of `ui/filetype.test.js` —
change `import { isImagePath } from "./filetype.js";` to:

```javascript
import { isImagePath, isMarkdownPath, isCodeView } from "./filetype.js";
```

Then append these tests (the file already imports `test` from `node:test` and
`assert` from `node:assert/strict`):

```javascript
test("isMarkdownPath true for markdown extensions", () => {
  for (const p of ["a.md", "A.MARKDOWN", "x.mdown", "y.mkd", "z.mkdn"]) {
    assert.equal(isMarkdownPath(p), true, p);
  }
});

test("isMarkdownPath false for non-markdown", () => {
  for (const p of ["main.rs", "pic.png", "Makefile", ""]) {
    assert.equal(isMarkdownPath(p), false, p);
  }
});

test("isCodeView is true only for non-markdown, non-image", () => {
  assert.equal(isCodeView("main.rs"), true);
  assert.equal(isCodeView("Makefile"), true);
  assert.equal(isCodeView("readme.md"), false);
  assert.equal(isCodeView("pic.png"), false);
});
```

(Check the top of `ui/filetype.test.js` for the existing `import { test } from "node:test"` / `import assert from "node:assert"` lines — reuse them; only add the `from "./filetype.js"` import shown above if not already importing these names.)

- [ ] **Step 2: Run the tests (expect FAIL)**

Run: `node --test ui/filetype.test.js 2>&1 | tail -8`
Expected: FAIL — `isMarkdownPath`/`isCodeView` are not exported yet.

- [ ] **Step 3: Add the helpers to `ui/filetype.js`**

The full file becomes:

```javascript
// Pure file-type detection (no DOM) so it's unit-testable under `node --test`.

export const IMAGE_EXT = /\.(png|jpe?g|gif|webp|avif|bmp|ico|svg)$/i;
export const MARKDOWN_EXT = /\.(md|markdown|mdown|mkd|mkdn)$/i;

export function isImagePath(path) {
  return IMAGE_EXT.test(path || "");
}

export function isMarkdownPath(path) {
  return MARKDOWN_EXT.test(path || "");
}

// A "code view" tab is any file that isn't markdown and isn't an image —
// it renders as syntax-highlighted, line-numbered text.
export function isCodeView(path) {
  return !isImagePath(path) && !isMarkdownPath(path);
}
```

- [ ] **Step 4: Run the tests (expect PASS)**

Run: `node --test ui/filetype.test.js 2>&1 | tail -6`
Expected: all pass.

- [ ] **Step 5: Update the `app.js` import**

At `ui/app.js:30`, replace:

```javascript
import { isImagePath } from "./filetype.js";
```

with:

```javascript
import { isImagePath, isMarkdownPath, isCodeView } from "./filetype.js";
```

- [ ] **Step 6: Set the `code-body` class in `paintHtml`**

In `ui/app.js`, `paintHtml` currently does (around lines 1665-1672):

```javascript
  preview.classList.toggle("raw-body", raw);

  const anchor = scrollLock ? captureAnchor() : null;

  const incoming = document.createElement("article");
  incoming.className = "markdown-body" + (raw ? " raw-body" : "");
  incoming.id = "preview";
  incoming.innerHTML = html;
```

Replace those lines with:

```javascript
  const code = isCodeView(t.path);
  preview.classList.toggle("raw-body", raw && !code);

  const anchor = scrollLock ? captureAnchor() : null;

  const incoming = document.createElement("article");
  incoming.className = code
    ? "code-body"
    : "markdown-body" + (raw ? " raw-body" : "");
  incoming.id = "preview";
  incoming.innerHTML = html;
```

- [ ] **Step 7: Add the code-view CSS**

In `ui/styles.css`, after the `.image-view .image-error { … }` rule (~line 507), add:

```css
/* Standalone code/text files: full-width monospace with a CSS-counter gutter.
   Background uses the theme canvas so the syntect token colors (tuned for
   InspiredGitHub / base16-ocean.dark) read correctly in both themes. */
.code-body {
  box-sizing: border-box;
  min-height: 100%;
  padding: 16px 0;
  background: var(--bg);
  color: var(--fg);
}

pre.code-view {
  margin: 0;
  padding: 0;
  counter-reset: line;
  tab-size: 4;
  overflow-x: auto;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 13px;
  line-height: 1.5;
}

.code-view .cl {
  display: block;
  white-space: pre;
}

.code-view .cl::before {
  counter-increment: line;
  content: counter(line);
  display: inline-block;
  width: 3em;
  margin-right: 1em;
  padding-right: 0.6em;
  text-align: right;
  color: var(--sidebar-muted);
  border-right: 1px solid var(--sidebar-border);
  user-select: none;
  -webkit-user-select: none;
}

.code-unsupported {
  padding: 40px;
  text-align: center;
  color: var(--sidebar-muted);
}
```

- [ ] **Step 8: Build and visually verify**

Run: `cd src-tauri && cargo build 2>&1 | tail -3`
Then `cargo run -- ../README.md` and open a `.rs` and a `.json` file from the tree.
Expected: colored code with a right-aligned line-number gutter; selecting text and copying excludes the numbers; toggling the theme (☾/☀) recolors it.

- [ ] **Step 9: Commit**

```bash
git add ui/filetype.js ui/filetype.test.js ui/app.js ui/styles.css
git commit -m "Render code files with highlighted line-numbered view"
```

---

### Task 4: Toolbar & action gating for code tabs

**Files:**
- Modify: `ui/app.js` (toolbar block ~1290-1311; `onExport` ~1966-1972; `runEditAction` guard ~3159-3168)

**Interfaces:**
- Consumes: `isCodeView`, `isMarkdownPath` (Task 3).
- Produces: code tabs hide Raw + Review, block Export, block toggle-raw; Edit and Copy Source remain.

- [ ] **Step 1: Hide Raw and Review for code tabs**

In `ui/app.js`, in the toolbar-update block, after `const image = isImagePath(t.path);` (line 1290) add a `code` flag and fold it into the Raw/Review `hidden` expressions. The block becomes:

```javascript
    const image = isImagePath(t.path);
    const code = isCodeView(t.path);
    editBtn.hidden = image;
    if (!image) {
      editBtn.textContent = t.editing ? "Done" : "Edit";
      editBtn.setAttribute("aria-pressed", t.editing ? "true" : "false");
    }
    saveBtn.hidden = !t.editing;
    rawBtn.hidden = image || code || t.editing;
    if (!image && !code) {
      rawBtn.textContent = t.raw ? "Rendered" : "Raw";
      rawBtn.setAttribute("aria-pressed", t.raw ? "true" : "false");
    }
    reviewBtn.hidden = image || code || t.editing || t.raw;
```

(Leave the `if (!reviewBtn.hidden) { … }` body below unchanged.)

- [ ] **Step 2: Block Export for non-markdown tabs**

In `onExport`, replace the image guard:

```javascript
  if (isImagePath(t.path)) {
    showTransientError("Export is only available for text documents.");
    return;
  }
```

with a markdown-only guard:

```javascript
  if (!isMarkdownPath(t.path)) {
    showTransientError("Export is only available for Markdown documents.");
    return;
  }
```

- [ ] **Step 3: Block the raw toggle for code tabs**

In `runEditAction`, after the existing image guard block:

```javascript
  if (
    t &&
    isImagePath(t.path) &&
    (name === "copy-source" || name === "toggle-raw" || name === "toggle-edit")
  ) {
    showTransientError("Not available for images.");
    return;
  }
```

add:

```javascript
  if (t && isCodeView(t.path) && name === "toggle-raw") {
    showTransientError("Raw view isn't available for code files.");
    return;
  }
```

- [ ] **Step 4: Build and verify gating**

Run: `cd src-tauri && cargo build 2>&1 | tail -3`
Then `cargo run -- ../README.md`, open a `.rs` file:
Expected: no Raw button, no Review button, Edit present; File ▸ Export as HTML… shows "Export is only available for Markdown documents." On a markdown tab, Raw/Review/Export all behave as before.

- [ ] **Step 5: Commit**

```bash
git add ui/app.js
git commit -m "Hide Raw/Review/Export for code-file tabs"
```

---

## Self-Review

**Spec coverage:**
- Render pipeline branch (markdown unchanged, non-markdown → code, render_preview mirrors) → Task 2. ✓
- `code.rs` (render_code, is_binary, unsupported_html, detect, theme match, line wrapping, large-file guard) → Task 1. ✓
- Frontend `isMarkdownPath` + code tab = not-md-not-image + `paintHtml` class → Task 3. ✓
- CSS gutter via counters, `.code-unsupported`, theme-driven colors → Task 3 Step 7. ✓
- Toolbar gating (Raw/Review hidden, Export blocked, Edit kept, Copy Source kept) → Task 4. ✓
- Binary detection on raw bytes in render_file → Task 2 Step 1 + Task 1 `is_binary`. ✓
- Testing (Rust unit tests, JS filetype tests, manual matrix) → Task 1 Step 2-3, Task 3 Step 1-4, manual steps. ✓

**Placeholder scan:** No TBD/TODO; all code shown in full. The shebang test name caveat (Step 3, Task 1) gives a concrete recovery command, not a placeholder. ✓

**Type consistency:** `code::render_code(&str, &Path, &str) -> String`, `code::is_binary(&[u8]) -> bool`, `code::unsupported_html() -> String` are defined in Task 1 and consumed with those exact signatures in Task 2. `isCodeView`/`isMarkdownPath` defined in Task 3 Step 3, consumed in Task 3 Step 6 and Task 4. `.cl` / `.code-view` / `.code-body` / `.code-unsupported` class names match between `code.rs` output, `paintHtml`, and the CSS. ✓
