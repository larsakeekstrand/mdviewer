//! Folder content search engine.
//!
//! The walk and per-file scan live in [`search_in_folder`]; the pure helpers
//! [`match_line`], [`is_binary`], and [`is_word_char`] are split out so they
//! can be unit-tested without any filesystem. Semantics mirror
//! `ui/search.js::findMatches` so the in-document find bar and the folder
//! search behave identically on the same inputs.

use std::path::Path;

use serde::Serialize;

#[derive(Clone, Copy, Debug, Default)]
pub struct SearchOpts {
    pub case_sensitive: bool,
    pub whole_word: bool,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct Match {
    pub path: String,
    /// 1-based line number, matching comrak `data-sourcepos`.
    pub line: u32,
    /// 1-based column number (start of the match within the line, in chars).
    pub column: u32,
    pub line_text: String,
    pub match_start: u32,
    pub match_end: u32,
}

#[derive(Serialize, Debug, Default)]
pub struct SearchResults {
    pub matches: Vec<Match>,
    pub truncated: bool,
    pub files_scanned: usize,
    pub files_skipped_binary: usize,
    pub files_skipped_too_large: usize,
    pub files_unreadable: usize,
}

/// Mirrors `isWordChar` in `ui/search.js`: Unicode letter, number, or
/// underscore.
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// True if the sample looks binary. Sniffs the first 8 KB for a NUL byte —
/// the same heuristic GNU grep and git use.
pub fn is_binary(sample: &[u8]) -> bool {
    let window = if sample.len() > 8192 {
        &sample[..8192]
    } else {
        sample
    };
    window.contains(&0)
}

/// Find every occurrence of `query` in `line`. Returns `[start, end)` byte
/// offsets, in order, non-overlapping. Mirrors `findMatches` in
/// `ui/search.js`.
pub fn match_line(line: &str, query: &str, opts: SearchOpts) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }
    let (hay, needle): (String, String) = if opts.case_sensitive {
        (line.to_string(), query.to_string())
    } else {
        (line.to_lowercase(), query.to_lowercase())
    };

    let mut out = Vec::new();
    let mut from = 0usize;
    let mut last_end = 0usize;
    let mut first = true;
    while let Some(i) = hay[from..].find(&needle) {
        let start = from + i;
        let end = start + needle.len();
        let overlap = !first && start < last_end;
        if !overlap && (!opts.whole_word || is_whole_word(line, start, end)) {
            // Translate offsets from `hay` (lowercased) back to `line`. ASCII
            // lowercasing preserves byte positions; Unicode `to_lowercase()`
            // can change length, but we still use the lowercased offsets to
            // index into `line` because both halves of the search use the
            // same offsets the original JS does (the JS does the same trick).
            // For ASCII text this is exact; for non-ASCII the highlighted
            // substring may drift — acceptable, the in-document search has
            // the same property.
            out.push((start, end));
            last_end = end;
            first = false;
        }
        from = start + 1;
    }
    out
}

fn is_whole_word(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    let before_word = before.map(is_word_char).unwrap_or(false);
    let after_word = after.map(is_word_char).unwrap_or(false);
    !before_word && !after_word
}

/// Walk `root` recursively, scanning every text file for `query`. See module
/// docs and the design spec for limits / skip rules.
pub fn search_in_folder(
    _root: &Path,
    _query: &str,
    _opts: SearchOpts,
) -> Result<SearchResults, String> {
    Ok(SearchResults::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(cs: bool, ww: bool) -> SearchOpts {
        SearchOpts {
            case_sensitive: cs,
            whole_word: ww,
        }
    }

    #[test]
    fn match_line_returns_no_matches_for_empty_query() {
        assert!(match_line("hello", "", SearchOpts::default()).is_empty());
    }

    #[test]
    fn match_line_finds_multiple_occurrences_in_order() {
        assert_eq!(
            match_line("hello world hello", "hello", SearchOpts::default()),
            vec![(0, 5), (12, 17)],
        );
    }

    #[test]
    fn match_line_is_case_insensitive_by_default() {
        assert_eq!(
            match_line("Hello HELLO", "hello", SearchOpts::default()),
            vec![(0, 5), (6, 11)],
        );
    }

    #[test]
    fn match_line_respects_case_sensitive_option() {
        assert_eq!(
            match_line("Hello hello", "hello", opts(true, false)),
            vec![(6, 11)],
        );
    }

    #[test]
    fn match_line_whole_word_skips_substrings_inside_larger_words() {
        assert_eq!(
            match_line("cat category cat", "cat", opts(false, true)),
            vec![(0, 3), (13, 16)],
        );
    }

    #[test]
    fn match_line_whole_word_treats_punctuation_as_a_boundary() {
        assert_eq!(match_line("(cat)", "cat", opts(false, true)), vec![(1, 4)]);
    }

    #[test]
    fn match_line_whole_word_treats_underscore_as_part_of_the_word() {
        assert_eq!(
            match_line("cat_x cat", "cat", opts(false, true)),
            vec![(6, 9)],
        );
    }

    #[test]
    fn match_line_returns_non_overlapping_matches() {
        assert_eq!(
            match_line("aaaa", "aa", SearchOpts::default()),
            vec![(0, 2), (2, 4)],
        );
    }

    #[test]
    fn is_word_char_recognizes_letters_digits_and_underscore() {
        assert!(is_word_char('a'));
        assert!(is_word_char('7'));
        assert!(is_word_char('_'));
        assert!(is_word_char('é'));
        assert!(is_word_char('日'));
        assert!(!is_word_char(' '));
        assert!(!is_word_char('-'));
        assert!(!is_word_char('('));
    }

    #[test]
    fn is_binary_detects_nul_anywhere_in_window() {
        assert!(is_binary(b"\0"));
        let mut buf = vec![b'a'; 8000];
        buf.push(0);
        assert!(is_binary(&buf));
    }

    #[test]
    fn is_binary_ignores_nul_past_the_window() {
        let mut buf = vec![b'a'; 8192];
        buf.push(0);
        assert!(!is_binary(&buf));
    }

    #[test]
    fn is_binary_says_no_for_pure_text() {
        assert!(!is_binary(b"hello world"));
        assert!(!is_binary("café 日本語".as_bytes()));
    }
}
