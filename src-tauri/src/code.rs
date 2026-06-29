use std::path::Path;
use std::sync::LazyLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::html::{styled_line_to_highlighted_html, IncludeBackground};
use syntect::parsing::{SyntaxReference, SyntaxSet};

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_nonewlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_LINES: usize = 50_000;

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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

fn find_syntax(source: &str, path: &Path) -> &'static SyntaxReference {
    let ss: &'static SyntaxSet = &SYNTAX_SET;
    path.extension()
        .and_then(|e| e.to_str())
        .and_then(|ext| ss.find_syntax_by_extension(ext))
        .or_else(|| {
            source
                .lines()
                .next()
                .and_then(|first| ss.find_syntax_by_first_line(first))
        })
        .unwrap_or_else(|| ss.find_syntax_plain_text())
}

/// Name of the syntect syntax chosen for this file (by extension, then first
/// line, then plain text). Only used by unit tests.
#[cfg(test)]
pub fn detect_language(source: &str, path: &Path) -> String {
    find_syntax(source, path).name.clone()
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

    let ss: &'static SyntaxSet = &SYNTAX_SET;
    let syntax = find_syntax(source, path);

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
        assert!(
            html.contains("style="),
            "expected colored output, got: {html}"
        );
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
        assert!(is_binary(&[0xff, 0xfe]));
    }

    #[test]
    fn unsupported_notice_has_marker() {
        assert!(unsupported_html().contains("code-unsupported"));
    }
}
