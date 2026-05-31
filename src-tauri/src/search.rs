//! Folder content search engine.
//!
//! The walk and per-file scan live in [`search_in_folder`]; the pure helpers
//! [`match_line`], [`is_binary`], and [`is_word_char`] are split out so they
//! can be unit-tested without any filesystem. Semantics mirror
//! `ui/search.js::findMatches` so the in-document find bar and the folder
//! search behave identically on the same inputs.

use std::path::Path;

use serde::Serialize;

/// `Default` is "least filtering" (all bools false). The IPC layer always
/// supplies an explicit value for every field, and tests use `Default` to
/// get predictable behaviour regardless of any host's global gitignore. The
/// user-facing default of "respect .gitignore" is set in the frontend
/// (`ui/folder_search.js`), not here.
#[derive(Clone, Copy, Debug, Default)]
pub struct SearchOpts {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub respect_gitignore: bool,
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

const PER_FILE_MATCH_CAP: usize = 200;
const TOTAL_MATCH_CAP: usize = 5000;
const PER_FILE_SIZE_CAP: u64 = 10 * 1024 * 1024;
const BINARY_SNIFF_BYTES: usize = 8192;
const LINE_TEXT_MAX_CHARS: usize = 300;

/// Walk `root` recursively, scanning every text file for `query`. See module
/// docs and the design spec for limits / skip rules.
pub fn search_in_folder(
    root: &Path,
    query: &str,
    opts: SearchOpts,
) -> Result<SearchResults, String> {
    if !root.is_dir() {
        return Err(format!("not a directory: {}", root.display()));
    }

    let mut results = SearchResults::default();

    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .standard_filters(false)
        .hidden(false)
        .follow_links(false);
    if opts.respect_gitignore {
        builder
            .git_ignore(true)
            .git_exclude(true)
            .git_global(true)
            .ignore(true)
            .require_git(false)
            .parents(true);
    }

    'outer: for entry in builder.build() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                results.files_unreadable += 1;
                continue;
            }
        };
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => {
                results.files_unreadable += 1;
                continue;
            }
        };
        if metadata.len() > PER_FILE_SIZE_CAP {
            results.files_skipped_too_large += 1;
            continue;
        }
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => {
                results.files_unreadable += 1;
                continue;
            }
        };
        if is_binary(&bytes[..bytes.len().min(BINARY_SNIFF_BYTES)]) {
            results.files_skipped_binary += 1;
            continue;
        }
        let contents = match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                // Invalid UTF-8: treat as binary for this feature.
                results.files_skipped_binary += 1;
                continue;
            }
        };
        results.files_scanned += 1;

        let path_str = path.to_string_lossy().into_owned();
        let mut per_file = 0usize;
        for (line_idx, line) in contents.split('\n').enumerate() {
            if per_file >= PER_FILE_MATCH_CAP {
                break;
            }
            let spans = match_line(line, query, opts);
            if spans.is_empty() {
                continue;
            }
            for (start, end) in spans {
                if results.matches.len() >= TOTAL_MATCH_CAP {
                    results.truncated = true;
                    break 'outer;
                }
                let column = line[..start].chars().count() as u32 + 1;
                let (display, display_start, display_end) =
                    truncate_around(line, start, end, LINE_TEXT_MAX_CHARS);
                results.matches.push(Match {
                    path: path_str.clone(),
                    line: (line_idx as u32) + 1,
                    column,
                    line_text: display,
                    match_start: display_start as u32,
                    match_end: display_end as u32,
                });
                per_file += 1;
                if per_file >= PER_FILE_MATCH_CAP {
                    break;
                }
            }
        }
    }

    Ok(results)
}

/// Centre an excerpt of `line` on `[start, end)` so the displayed substring is
/// no longer than `max` chars. Returns the excerpt and the adjusted match
/// offsets relative to that excerpt.
fn truncate_around(line: &str, start: usize, end: usize, max: usize) -> (String, usize, usize) {
    if line.chars().count() <= max {
        return (line.to_string(), start, end);
    }
    let char_indices: Vec<(usize, char)> = line.char_indices().collect();
    let start_char = char_indices
        .iter()
        .position(|(b, _)| *b >= start)
        .unwrap_or(char_indices.len());
    let end_char = char_indices
        .iter()
        .position(|(b, _)| *b >= end)
        .unwrap_or(char_indices.len());
    let match_len = end_char.saturating_sub(start_char);
    let context = max.saturating_sub(match_len) / 2;
    let win_start_char = start_char.saturating_sub(context);
    let win_end_char = (end_char + context).min(char_indices.len());
    let win_start_byte = char_indices
        .get(win_start_char)
        .map(|(b, _)| *b)
        .unwrap_or(line.len());
    let win_end_byte = char_indices
        .get(win_end_char)
        .map(|(b, _)| *b)
        .unwrap_or(line.len());
    let prefix = if win_start_char > 0 { "…" } else { "" };
    let suffix = if win_end_char < char_indices.len() {
        "…"
    } else {
        ""
    };
    let mut out = String::new();
    out.push_str(prefix);
    out.push_str(&line[win_start_byte..win_end_byte]);
    out.push_str(suffix);
    let new_start = prefix.len() + (start - win_start_byte);
    let new_end = prefix.len() + (end - win_start_byte);
    (out, new_start, new_end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(cs: bool, ww: bool) -> SearchOpts {
        SearchOpts {
            case_sensitive: cs,
            whole_word: ww,
            respect_gitignore: false,
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

    use std::collections::HashSet;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn unique_temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "mdviewer_search_test_{}_{nanos}_{n}",
            std::process::id()
        ));
        fs::create_dir(&dir).unwrap();
        dir
    }

    #[test]
    fn search_finds_matches_across_nested_dirs() {
        let dir = unique_temp_dir();
        fs::write(dir.join("a.md"), "hello world\nfoo bar\nhello again\n").unwrap();
        fs::create_dir(dir.join("sub")).unwrap();
        fs::write(dir.join("sub").join("b.txt"), "no match here\n").unwrap();
        fs::write(dir.join("sub").join("c.md"), "second hello\n").unwrap();

        let res = search_in_folder(&dir, "hello", SearchOpts::default()).unwrap();

        let paths: HashSet<String> = res.matches.iter().map(|m| m.path.clone()).collect();
        assert!(paths.iter().any(|p| p.ends_with("a.md")));
        assert!(paths.iter().any(|p| p.ends_with("c.md")));
        assert!(!paths.iter().any(|p| p.ends_with("b.txt")));
        assert_eq!(res.matches.len(), 3);
        assert!(!res.truncated);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn search_returns_one_based_line_and_column() {
        let dir = unique_temp_dir();
        fs::write(dir.join("x.md"), "alpha\nbeta\ngamma needle here\n").unwrap();

        let res = search_in_folder(&dir, "needle", SearchOpts::default()).unwrap();

        assert_eq!(res.matches.len(), 1);
        let m = &res.matches[0];
        assert_eq!(m.line, 3);
        assert_eq!(m.column, 7);
        assert_eq!(m.line_text, "gamma needle here");
        assert_eq!(m.match_start, 6);
        assert_eq!(m.match_end, 12);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn search_skips_binary_files() {
        let dir = unique_temp_dir();
        fs::write(dir.join("text.md"), "needle in text\n").unwrap();
        let mut blob = b"needle in binary\n".to_vec();
        blob.push(0);
        fs::write(dir.join("blob.bin"), &blob).unwrap();

        let res = search_in_folder(&dir, "needle", SearchOpts::default()).unwrap();

        assert_eq!(res.matches.len(), 1);
        assert!(res.matches[0].path.ends_with("text.md"));
        assert_eq!(res.files_skipped_binary, 1);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn search_skips_files_larger_than_10mb() {
        let dir = unique_temp_dir();
        let mut big = String::with_capacity(11 * 1024 * 1024);
        big.push_str("needle\n");
        while big.len() < 10 * 1024 * 1024 + 1 {
            big.push_str("padding line\n");
        }
        fs::write(dir.join("big.md"), &big).unwrap();
        fs::write(dir.join("small.md"), "needle\n").unwrap();

        let res = search_in_folder(&dir, "needle", SearchOpts::default()).unwrap();

        assert_eq!(res.matches.len(), 1);
        assert!(res.matches[0].path.ends_with("small.md"));
        assert_eq!(res.files_skipped_too_large, 1);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn search_enforces_per_file_cap() {
        let dir = unique_temp_dir();
        let mut s = String::new();
        for _ in 0..250 {
            s.push_str("needle\n");
        }
        fs::write(dir.join("many.md"), &s).unwrap();

        let res = search_in_folder(&dir, "needle", SearchOpts::default()).unwrap();

        assert_eq!(res.matches.len(), 200);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn search_enforces_total_cap_and_sets_truncated() {
        let dir = unique_temp_dir();
        for i in 0..30 {
            let mut s = String::new();
            for _ in 0..200 {
                s.push_str("needle\n");
            }
            fs::write(dir.join(format!("f{i:02}.md")), &s).unwrap();
        }

        let res = search_in_folder(&dir, "needle", SearchOpts::default()).unwrap();

        assert_eq!(res.matches.len(), 5000);
        assert!(res.truncated);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn search_returns_err_on_non_directory_root() {
        let dir = unique_temp_dir();
        let file = dir.join("a.md");
        fs::write(&file, "x").unwrap();
        assert!(search_in_folder(&file, "x", SearchOpts::default()).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    fn gitignore_opts() -> SearchOpts {
        SearchOpts {
            respect_gitignore: true,
            ..SearchOpts::default()
        }
    }

    #[test]
    fn search_respects_gitignore_when_enabled() {
        let dir = unique_temp_dir();
        fs::write(dir.join(".gitignore"), "ignored.md\nbuild/\n").unwrap();
        fs::write(dir.join("kept.md"), "needle here\n").unwrap();
        fs::write(dir.join("ignored.md"), "needle in ignored\n").unwrap();
        fs::create_dir(dir.join("build")).unwrap();
        fs::write(dir.join("build").join("artifact.md"), "needle in build\n").unwrap();

        let res = search_in_folder(&dir, "needle", gitignore_opts()).unwrap();

        let paths: HashSet<String> = res.matches.iter().map(|m| m.path.clone()).collect();
        assert!(paths.iter().any(|p| p.ends_with("kept.md")));
        assert!(
            !paths.iter().any(|p| p.ends_with("ignored.md")),
            "ignored.md should be skipped: {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p.contains("build/")),
            "build/ should be skipped: {paths:?}"
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn search_includes_gitignored_files_when_option_off() {
        let dir = unique_temp_dir();
        fs::write(dir.join(".gitignore"), "ignored.md\n").unwrap();
        fs::write(dir.join("kept.md"), "needle here\n").unwrap();
        fs::write(dir.join("ignored.md"), "needle in ignored\n").unwrap();

        let res = search_in_folder(&dir, "needle", SearchOpts::default()).unwrap();

        let paths: HashSet<String> = res.matches.iter().map(|m| m.path.clone()).collect();
        assert!(paths.iter().any(|p| p.ends_with("kept.md")));
        assert!(
            paths.iter().any(|p| p.ends_with("ignored.md")),
            "ignored.md should be searched when respect_gitignore=false: {paths:?}"
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn search_with_gitignore_finds_hidden_dotfiles() {
        // The ignore crate's `hidden` filter is OFF in our config; dotfiles
        // that aren't in .gitignore should still be searched.
        let dir = unique_temp_dir();
        fs::write(dir.join(".gitignore"), "build/\n").unwrap();
        fs::write(dir.join(".env"), "API_KEY=needle\n").unwrap();
        fs::write(dir.join("kept.md"), "needle here\n").unwrap();

        let res = search_in_folder(&dir, "needle", gitignore_opts()).unwrap();

        let paths: HashSet<String> = res.matches.iter().map(|m| m.path.clone()).collect();
        assert!(paths.iter().any(|p| p.ends_with(".env")));
        assert!(paths.iter().any(|p| p.ends_with("kept.md")));

        fs::remove_dir_all(&dir).unwrap();
    }
}
