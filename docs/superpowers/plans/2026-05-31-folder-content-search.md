# Folder Content Search Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Right-click a folder in the file tree to search every file inside (recursively) for a literal substring. Results appear in a sidebar takeover panel; clicking a result opens the file and jumps the preview to the match line.

**Architecture:** A new Rust module `src-tauri/src/search.rs` does the walk and matching with `walkdir` plus a hand-written substring matcher that mirrors `ui/search.js` semantically. The Tauri command wrapper lives in `commands.rs` matching the existing convention. The frontend gets a new module `ui/folder_search.js` with a pure reducer (`derivePanelState`) plus DOM glue; the sidebar takeover swaps `<nav id="tree">` for a new `<section id="search-panel">` via a class on `<aside class="sidebar">`. Jump-to-line uses comrak's existing `data-sourcepos` annotations.

**Tech Stack:** Rust 2021 (`walkdir` 2.x for recursion), Tauri 2.11 IPC, vanilla JS (no build step), Node `node:test` for pure-JS unit tests, CSS Custom Highlight API for the jump-to-line pulse.

---

## File structure

- **Create** `src-tauri/src/search.rs` — walker, matcher, binary sniff, `match_line`, `is_binary`, `is_word_char` + unit + integration tests.
- **Create** `ui/folder_search.js` — pure `derivePanelState`, `groupResults`, `truncateLineText` plus DOM-bound `enterSearchMode` / `exitSearchMode` / `renderResults`.
- **Create** `ui/folder_search.test.js` — `node:test` unit tests for the pure helpers.
- **Modify** `src-tauri/Cargo.toml` — add `walkdir = "2"`.
- **Modify** `src-tauri/src/lib.rs` — register `mod search;` and the `commands::search_in_folder` invocation.
- **Modify** `src-tauri/src/commands.rs` — `#[tauri::command] search_in_folder` wrapper delegating to `search::search_in_folder`.
- **Modify** `ui/index.html` — add `<section id="search-panel" hidden>` inside `<aside class="sidebar">`.
- **Modify** `ui/styles.css` — sidebar takeover, panel/result row styling, `::highlight(search-jump)` rule.
- **Modify** `ui/app.js` — context-menu item for directories; small `openTabAtLine(path, line)` seam consumed by a new step in `postRender`.
- **Modify** `CLAUDE.md` — append a "Folder content search" subsection to "Architecture quick-tour".

Why the split: the pure functions in both `search.rs` and `folder_search.js` are unit-testable without filesystem/DOM; the glue then composes them. This mirrors how `ui/search.js` and `ui/export.js` were already structured in this repo.

---

## Task 1: Add `walkdir` dependency

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Add the dependency**

In `src-tauri/Cargo.toml`, locate the block ending with `notify-debouncer-full = "0.7"` and add `walkdir` immediately after it under a `# Recursive directory walking for folder content search` comment.

```toml
# File-system watching
notify = "8.2"
notify-debouncer-full = "0.7"

# Recursive directory walking for folder content search
walkdir = "2"
```

- [ ] **Step 2: Verify it builds**

Run from `src-tauri/`:

```sh
cargo build
```

Expected: succeeds. `walkdir` 2.x pulls in only `same-file` as a transitive dep on macOS/Linux; no breakage.

- [ ] **Step 3: Commit**

```sh
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "Add walkdir dep for folder content search"
```

---

## Task 2: Create `search.rs` with the pure matcher + binary sniff + unit tests

**Files:**
- Create: `src-tauri/src/search.rs`
- Modify: `src-tauri/src/lib.rs` (register the module)

- [ ] **Step 1: Register the empty module**

In `src-tauri/src/lib.rs`, the existing `mod` declarations look like:

```rust
mod commands;
mod export;
mod git;
mod markdown;
mod menu;
#[cfg(target_os = "macos")]
mod open_files;
mod recent;
mod tasklist;
mod tree;
mod watcher;
```

Add `mod search;` alphabetically after `mod recent;` so the block reads:

```rust
mod commands;
mod export;
mod git;
mod markdown;
mod menu;
#[cfg(target_os = "macos")]
mod open_files;
mod recent;
mod search;
mod tasklist;
mod tree;
mod watcher;
```

- [ ] **Step 2: Create `search.rs` with type stubs and tests for `is_word_char`, `is_binary`, `match_line`**

Create `src-tauri/src/search.rs`:

```rust
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
        SearchOpts { case_sensitive: cs, whole_word: ww }
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
        assert_eq!(
            match_line("(cat)", "cat", opts(false, true)),
            vec![(1, 4)],
        );
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
```

- [ ] **Step 3: Run the unit tests and verify they pass**

Run from `src-tauri/`:

```sh
cargo test --lib search::
```

Expected: 12 tests pass.

- [ ] **Step 4: Commit**

```sh
git add src-tauri/src/search.rs src-tauri/src/lib.rs
git commit -m "Add pure matcher + binary sniff for folder search"
```

---

## Task 3: Implement `search_in_folder` with walker + caps + filesystem tests

**Files:**
- Modify: `src-tauri/src/search.rs`

- [ ] **Step 1: Write the failing filesystem tests**

Append to the `mod tests` block in `src-tauri/src/search.rs`:

```rust
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
        // "alpha\nbeta\ngamma needle here\n" — line 3, column 7
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
        // NUL byte in the first 8 KB → binary.
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
        // 30 files × 200 lines each = 6000 candidate matches → truncates at 5000.
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run from `src-tauri/`:

```sh
cargo test --lib search::
```

Expected: 7 new tests fail (matches empty / wrong shape), 12 existing pass.

- [ ] **Step 3: Replace the stub `search_in_folder` with the real implementation**

In `src-tauri/src/search.rs`, replace the existing stub:

```rust
pub fn search_in_folder(
    _root: &Path,
    _query: &str,
    _opts: SearchOpts,
) -> Result<SearchResults, String> {
    Ok(SearchResults::default())
}
```

with the working implementation:

```rust
const PER_FILE_MATCH_CAP: usize = 200;
const TOTAL_MATCH_CAP: usize = 5000;
const PER_FILE_SIZE_CAP: u64 = 10 * 1024 * 1024;
const BINARY_SNIFF_BYTES: usize = 8192;
const LINE_TEXT_MAX_CHARS: usize = 300;

pub fn search_in_folder(
    root: &Path,
    query: &str,
    opts: SearchOpts,
) -> Result<SearchResults, String> {
    if !root.is_dir() {
        return Err(format!("not a directory: {}", root.display()));
    }

    let mut results = SearchResults::default();

    'outer: for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                results.files_unreadable += 1;
                continue;
            }
        };
        if !entry.file_type().is_file() {
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
fn truncate_around(
    line: &str,
    start: usize,
    end: usize,
    max: usize,
) -> (String, usize, usize) {
    if line.chars().count() <= max {
        return (line.to_string(), start, end);
    }
    // We work in chars to avoid splitting multi-byte codepoints. Build a
    // (byte_offset, char) index so we can clamp to a window centred on the
    // match's char range.
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
    let context = max.saturating_sub(match_len).max(0) / 2;
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
    let suffix = if win_end_char < char_indices.len() { "…" } else { "" };
    let mut out = String::new();
    out.push_str(prefix);
    out.push_str(&line[win_start_byte..win_end_byte]);
    out.push_str(suffix);
    let new_start = prefix.len() + (start - win_start_byte);
    let new_end = prefix.len() + (end - win_start_byte);
    (out, new_start, new_end)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run from `src-tauri/`:

```sh
cargo test --lib search::
```

Expected: all 19 tests pass.

- [ ] **Step 5: Commit**

```sh
git add src-tauri/src/search.rs
git commit -m "Implement walkdir-based folder content search"
```

---

## Task 4: Expose `search_in_folder` as a Tauri command

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add the command wrapper**

Open `src-tauri/src/commands.rs`. The existing `use` statement is:

```rust
use crate::{git, markdown, recent, tasklist, tree, AppState};
```

Add `search` to it:

```rust
use crate::{git, markdown, recent, search, tasklist, tree, AppState};
```

Then, at the bottom of the file (after the existing last command — locate it by reading the file end, then append after the final closing brace of the last `pub fn`), append:

```rust
#[tauri::command]
pub fn search_in_folder(
    root: String,
    query: String,
    case_sensitive: bool,
    whole_word: bool,
) -> Result<search::SearchResults, String> {
    search::search_in_folder(
        Path::new(&root),
        &query,
        search::SearchOpts { case_sensitive, whole_word },
    )
}
```

- [ ] **Step 2: Register the command in the invoke handler**

In `src-tauri/src/lib.rs`, find the `invoke_handler` block:

```rust
        .invoke_handler(tauri::generate_handler![
            commands::get_initial_state,
            commands::list_dir,
            ...
            commands::platform,
        ])
```

Add `commands::search_in_folder,` after `commands::platform,`:

```rust
        .invoke_handler(tauri::generate_handler![
            commands::get_initial_state,
            commands::list_dir,
            ...
            commands::platform,
            commands::search_in_folder,
        ])
```

- [ ] **Step 3: Verify the build is clean**

Run from `src-tauri/`:

```sh
cargo build && cargo fmt --check && cargo clippy --all-targets -- -D warnings
```

Expected: builds, fmt clean, clippy clean.

- [ ] **Step 4: Commit**

```sh
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "Expose search_in_folder Tauri command"
```

---

## Task 5: Pure JS reducer + result helpers + unit tests

**Files:**
- Create: `ui/folder_search.test.js`
- Create: `ui/folder_search.js`

`ui/package.json` already declares `{ "type": "module" }` from the in-document search work, so the new test file picks up the same Node module mode automatically.

- [ ] **Step 1: Write the failing tests**

Create `ui/folder_search.test.js`:

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  derivePanelState,
  groupResults,
  truncateLineText,
} from "./folder_search.js";

test("derivePanelState reports the empty-query hint when query is blank", () => {
  const s = derivePanelState({ query: "", results: null, error: null, busy: false });
  assert.equal(s.kind, "hint");
  assert.equal(s.message, "Type to search");
});

test("derivePanelState requires at least 2 characters", () => {
  const s = derivePanelState({ query: "a", results: null, error: null, busy: false });
  assert.equal(s.kind, "hint");
  assert.equal(s.message, "Type at least 2 characters");
});

test("derivePanelState surfaces backend errors", () => {
  const s = derivePanelState({
    query: "foo",
    results: null,
    error: "Folder not found",
    busy: false,
  });
  assert.equal(s.kind, "error");
  assert.equal(s.message, "Folder not found");
});

test("derivePanelState shows 'searching' while busy", () => {
  const s = derivePanelState({ query: "foo", results: null, error: null, busy: true });
  assert.equal(s.kind, "busy");
});

test("derivePanelState reports no-results when complete with empty matches", () => {
  const s = derivePanelState({
    query: "foo",
    results: {
      matches: [],
      truncated: false,
      files_scanned: 12,
      files_skipped_binary: 0,
      files_skipped_too_large: 0,
      files_unreadable: 0,
    },
    error: null,
    busy: false,
  });
  assert.equal(s.kind, "empty");
  assert.equal(s.footer, "12 files searched · 0 matches");
});

test("derivePanelState groups matches and reports footer counts", () => {
  const matches = [
    { path: "/a.md", line: 1, column: 1, line_text: "x foo y", match_start: 2, match_end: 5 },
    { path: "/a.md", line: 4, column: 1, line_text: "foo again", match_start: 0, match_end: 3 },
    { path: "/sub/b.md", line: 2, column: 1, line_text: "foo here", match_start: 0, match_end: 3 },
  ];
  const s = derivePanelState({
    query: "foo",
    results: {
      matches,
      truncated: false,
      files_scanned: 5,
      files_skipped_binary: 1,
      files_skipped_too_large: 0,
      files_unreadable: 0,
    },
    error: null,
    busy: false,
  });
  assert.equal(s.kind, "results");
  assert.equal(s.groups.length, 2);
  assert.equal(s.groups[0].path, "/a.md");
  assert.equal(s.groups[0].matches.length, 2);
  assert.equal(s.groups[1].path, "/sub/b.md");
  assert.equal(s.footer, "5 files searched · 3 matches · 1 binary skipped");
});

test("derivePanelState reports truncation in the footer", () => {
  const matches = Array.from({ length: 5000 }, (_, i) => ({
    path: "/a.md",
    line: i + 1,
    column: 1,
    line_text: "foo",
    match_start: 0,
    match_end: 3,
  }));
  const s = derivePanelState({
    query: "foo",
    results: {
      matches,
      truncated: true,
      files_scanned: 200,
      files_skipped_binary: 0,
      files_skipped_too_large: 0,
      files_unreadable: 0,
    },
    error: null,
    busy: false,
  });
  assert.equal(s.kind, "results");
  assert.match(s.footer, /Showing first 5000/);
});

test("groupResults preserves walker order across files", () => {
  const matches = [
    { path: "/z.md", line: 1, column: 1, line_text: "x", match_start: 0, match_end: 1 },
    { path: "/a.md", line: 1, column: 1, line_text: "y", match_start: 0, match_end: 1 },
    { path: "/z.md", line: 2, column: 1, line_text: "x", match_start: 0, match_end: 1 },
  ];
  const groups = groupResults(matches);
  assert.deepEqual(
    groups.map((g) => g.path),
    ["/z.md", "/a.md"],
  );
  assert.equal(groups[0].matches.length, 2);
});

test("truncateLineText returns the line unchanged when short", () => {
  const out = truncateLineText("hello foo world", 5, 8, 300);
  assert.equal(out.text, "hello foo world");
  assert.equal(out.matchStart, 5);
  assert.equal(out.matchEnd, 8);
});

test("truncateLineText centres on the match and adds ellipses", () => {
  const line = "x".repeat(400) + " needle " + "y".repeat(400);
  const start = 401;
  const end = 407;
  const out = truncateLineText(line, start, end, 60);
  assert.ok(out.text.length <= 62, `length ${out.text.length}`);
  assert.ok(out.text.startsWith("…"));
  assert.ok(out.text.endsWith("…"));
  assert.equal(out.text.slice(out.matchStart, out.matchEnd), "needle");
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run from the repo root:

```sh
node --test 'ui/*.test.js'
```

Expected: FAIL — `Cannot find module ./folder_search.js`.

- [ ] **Step 3: Create `ui/folder_search.js` with the pure helpers**

Create `ui/folder_search.js`. (Only the pure exports for now; the DOM glue is added in Task 7.)

```js
// Folder content search panel. Pure helpers (no DOM/Tauri imports) live at
// the top so they run under `node --test`; the DOM-bound entry points live
// at the bottom.

const MIN_QUERY = 2;
const TOTAL_CAP = 5000;
const LINE_TEXT_MAX = 300;

/** Pure reducer: given the panel's inputs, return the renderable shape. */
export function derivePanelState({ query, results, error, busy }) {
  const trimmed = (query ?? "").trim();
  if (!trimmed) {
    return { kind: "hint", message: "Type to search" };
  }
  if (trimmed.length < MIN_QUERY) {
    return { kind: "hint", message: `Type at least ${MIN_QUERY} characters` };
  }
  if (error) {
    return { kind: "error", message: error };
  }
  if (busy && !results) {
    return { kind: "busy" };
  }
  if (!results) {
    return { kind: "busy" };
  }
  const footer = formatFooter(results);
  if (results.matches.length === 0) {
    return { kind: "empty", footer };
  }
  return { kind: "results", groups: groupResults(results.matches), footer };
}

/** Group flat matches by `path`, preserving the walker order. */
export function groupResults(matches) {
  const order = [];
  const map = new Map();
  for (const m of matches) {
    if (!map.has(m.path)) {
      map.set(m.path, { path: m.path, matches: [] });
      order.push(m.path);
    }
    map.get(m.path).matches.push(m);
  }
  return order.map((p) => map.get(p));
}

/** Centre an excerpt of `line` on `[start, end)` so the result is ≤ `max`
 *  chars. Match offsets are byte-indexed (matching the Rust side); we treat
 *  them as char-indexed here, which is identical for ASCII and acceptably
 *  close for the lengths we render. */
export function truncateLineText(line, start, end, max = LINE_TEXT_MAX) {
  if (line.length <= max) {
    return { text: line, matchStart: start, matchEnd: end };
  }
  const matchLen = Math.max(0, end - start);
  const context = Math.max(0, Math.floor((max - matchLen) / 2));
  const winStart = Math.max(0, start - context);
  const winEnd = Math.min(line.length, end + context);
  const prefix = winStart > 0 ? "…" : "";
  const suffix = winEnd < line.length ? "…" : "";
  const text = prefix + line.slice(winStart, winEnd) + suffix;
  const matchStart = prefix.length + (start - winStart);
  const matchEnd = prefix.length + (end - winStart);
  return { text, matchStart, matchEnd };
}

function formatFooter(r) {
  const parts = [`${r.files_scanned} files searched`];
  parts.push(`${r.matches.length} matches`);
  if (r.files_skipped_binary > 0) {
    parts.push(`${r.files_skipped_binary} binary skipped`);
  }
  if (r.files_skipped_too_large > 0) {
    parts.push(`${r.files_skipped_too_large} too large`);
  }
  if (r.files_unreadable > 0) {
    parts.push(`${r.files_unreadable} unreadable`);
  }
  let base = parts.join(" · ");
  if (r.truncated) {
    base += ` · Showing first ${TOTAL_CAP} — refine your query`;
  }
  return base;
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run from the repo root:

```sh
node --test 'ui/*.test.js'
```

Expected: PASS — all in-document-search tests plus 10 new folder-search tests.

- [ ] **Step 5: Commit**

```sh
git add ui/folder_search.js ui/folder_search.test.js
git commit -m "Add pure helpers + tests for folder search panel"
```

---

## Task 6: Search panel markup and styles (inert)

**Files:**
- Modify: `ui/index.html`
- Modify: `ui/styles.css`

This task adds the search panel DOM and its styles, but no behavior — the panel stays hidden, so the app should look unchanged afterwards.

- [ ] **Step 1: Add the search-panel markup**

In `ui/index.html`, the sidebar currently is:

```html
    <aside class="sidebar" id="sidebar">
      <div class="sidebar-header">
        <span class="sidebar-title" id="tree-title">Files</span>
      </div>
      <ul class="tree" id="tree" role="tree"></ul>
    </aside>
```

Replace it with (panel added as a sibling to the tree; both controlled by `sidebar.classList`):

```html
    <aside class="sidebar" id="sidebar">
      <div class="sidebar-header" id="sidebar-header-tree">
        <span class="sidebar-title" id="tree-title">Files</span>
      </div>
      <ul class="tree" id="tree" role="tree"></ul>
      <section class="search-panel" id="search-panel" hidden aria-label="Folder search">
        <div class="search-panel-header">
          <button
            id="search-back"
            class="search-back"
            type="button"
            title="Back to files"
            aria-label="Back to files"
          >
            ← Files
          </button>
          <span class="search-panel-title" id="search-panel-title"></span>
        </div>
        <div class="search-panel-input">
          <input
            id="search-input"
            class="search-input"
            type="text"
            placeholder="Search in folder"
            aria-label="Search in folder"
            autocomplete="off"
            spellcheck="false"
          />
          <button
            id="search-case"
            class="search-toggle"
            type="button"
            aria-pressed="false"
            title="Match case"
            aria-label="Match case"
          >
            Aa
          </button>
          <button
            id="search-word"
            class="search-toggle"
            type="button"
            aria-pressed="false"
            title="Whole word"
            aria-label="Whole word"
          >
            \b
          </button>
        </div>
        <div class="search-results" id="search-results"></div>
        <div class="search-footer" id="search-footer" aria-live="polite"></div>
      </section>
    </aside>
```

- [ ] **Step 2: Append the search panel styles**

Append to the end of `ui/styles.css`:

```css
/* ---- Folder search panel ---- */

.sidebar.searching #sidebar-header-tree,
.sidebar.searching #tree {
  display: none;
}

.search-panel {
  display: flex;
  flex-direction: column;
  min-height: 0;
  flex: 1 1 auto;
}

.search-panel[hidden] {
  display: none;
}

.search-panel-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border-bottom: 1px solid var(--sidebar-border);
  position: sticky;
  top: 0;
  background: var(--sidebar-bg);
  z-index: 1;
}

.search-back {
  background: transparent;
  border: 1px solid var(--sidebar-border);
  color: var(--sidebar-fg);
  border-radius: 4px;
  padding: 2px 8px;
  font-size: 12px;
  cursor: pointer;
}

.search-back:hover {
  background: var(--sidebar-hover);
}

.search-panel-title {
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--sidebar-muted);
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  direction: rtl;
  text-align: left;
  flex: 1 1 auto;
}

.search-panel-input {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 8px 10px;
  border-bottom: 1px solid var(--sidebar-border);
}

.search-input {
  flex: 1 1 auto;
  min-width: 0;
  height: 24px;
  padding: 0 6px;
  background: var(--bg);
  color: var(--sidebar-fg);
  border: 1px solid var(--sidebar-border);
  border-radius: 4px;
  font: inherit;
}

.search-input:focus {
  outline: 2px solid var(--accent, #0969da);
  outline-offset: -1px;
}

.search-toggle {
  height: 24px;
  min-width: 28px;
  padding: 0 6px;
  background: transparent;
  color: var(--sidebar-fg);
  border: 1px solid var(--sidebar-border);
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
}

.search-toggle[aria-pressed="true"] {
  background: var(--sidebar-selected);
}

.search-results {
  flex: 1 1 auto;
  overflow: auto;
  padding: 4px 0;
}

.search-results details {
  margin: 0;
}

.search-results summary {
  list-style: none;
  cursor: pointer;
  padding: 4px 10px;
  font-size: 12px;
  color: var(--sidebar-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.search-results summary::-webkit-details-marker {
  display: none;
}

.search-results summary:hover {
  background: var(--sidebar-hover);
}

.search-results .search-file-count {
  margin-left: 6px;
  font-variant-numeric: tabular-nums;
}

.search-results .search-match {
  display: block;
  width: 100%;
  text-align: left;
  background: transparent;
  border: 0;
  padding: 2px 10px 2px 28px;
  font: inherit;
  font-size: 12px;
  color: var(--sidebar-fg);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  cursor: pointer;
}

.search-results .search-match:hover {
  background: var(--sidebar-hover);
}

.search-results .search-match-line {
  color: var(--sidebar-muted);
  margin-right: 6px;
  font-variant-numeric: tabular-nums;
}

.search-results mark {
  background: #ffd33d55;
  color: inherit;
  padding: 0;
}

[data-theme="dark"] .search-results mark {
  background: #bb800988;
}

.search-results .search-hint,
.search-results .search-empty,
.search-results .search-error,
.search-results .search-busy {
  padding: 12px 10px;
  font-size: 12px;
  color: var(--sidebar-muted);
}

.search-results .search-error {
  color: #cf222e;
}

.search-footer {
  border-top: 1px solid var(--sidebar-border);
  padding: 6px 10px;
  font-size: 11px;
  color: var(--sidebar-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Jump-to-line highlight after opening a search result. */
::highlight(search-jump) {
  background: #fff8c4;
}

[data-theme="dark"] ::highlight(search-jump) {
  background: #ffd33d44;
}
```

- [ ] **Step 3: Build and visually verify the app is unchanged**

From `src-tauri/`:

```sh
cargo build
cargo run
```

Expected: the app launches normally; the sidebar still shows the file tree and the new search panel is invisible because `<section id="search-panel" hidden>` is hidden.

- [ ] **Step 4: Commit**

```sh
git add ui/index.html ui/styles.css
git commit -m "Add inert markup + styles for folder search panel"
```

---

## Task 7: Wire `enterSearchMode` / `exitSearchMode` and the debounced search loop

**Files:**
- Modify: `ui/folder_search.js`

This task adds the DOM-bound entry points and the debounced IPC call. The context-menu wiring + jump-to-line follow in Tasks 8 and 9.

- [ ] **Step 1: Append the DOM-bound section to `ui/folder_search.js`**

Append to the end of `ui/folder_search.js`:

```js
/* -------------------- DOM glue (browser only) -------------------- */

let _state = {
  query: "",
  results: null,
  error: null,
  busy: false,
  caseSensitive: false,
  wholeWord: false,
};
let _root = null;
let _rootRelativeTo = null; // tree root, for relative-path rendering
let _seq = 0;
let _debounceTimer = null;
let _invoke = null;
let _ui = null;
let _onOpenResult = null; // (path, line) => void

const DEBOUNCE_MS = 150;

/** Wire the panel once on app start. `opts.invoke` is `window.__TAURI__.core.invoke`;
 *  `opts.openResult` is the callback that opens a tab at a given line. */
export function initSearchPanel({ invoke, openResult }) {
  _invoke = invoke;
  _onOpenResult = openResult;
  _ui = {
    sidebar: document.getElementById("sidebar"),
    panel: document.getElementById("search-panel"),
    title: document.getElementById("search-panel-title"),
    back: document.getElementById("search-back"),
    input: document.getElementById("search-input"),
    caseBtn: document.getElementById("search-case"),
    wordBtn: document.getElementById("search-word"),
    results: document.getElementById("search-results"),
    footer: document.getElementById("search-footer"),
  };
  _ui.back.addEventListener("click", exitSearchMode);
  _ui.input.addEventListener("input", () => {
    _state.query = _ui.input.value;
    scheduleSearch();
    render();
  });
  _ui.input.addEventListener("keydown", (ev) => {
    if (ev.key === "Escape") {
      ev.preventDefault();
      exitSearchMode();
    }
  });
  _ui.caseBtn.addEventListener("click", () => {
    _state.caseSensitive = !_state.caseSensitive;
    _ui.caseBtn.setAttribute("aria-pressed", String(_state.caseSensitive));
    scheduleSearch();
  });
  _ui.wordBtn.addEventListener("click", () => {
    _state.wholeWord = !_state.wholeWord;
    _ui.wordBtn.setAttribute("aria-pressed", String(_state.wholeWord));
    scheduleSearch();
  });
}

export function enterSearchMode(folderPath, { treeRoot } = {}) {
  _root = folderPath;
  _rootRelativeTo = treeRoot || folderPath;
  _state = {
    query: "",
    results: null,
    error: null,
    busy: false,
    caseSensitive: false,
    wholeWord: false,
  };
  _ui.input.value = "";
  _ui.caseBtn.setAttribute("aria-pressed", "false");
  _ui.wordBtn.setAttribute("aria-pressed", "false");
  _ui.title.textContent = relPath(folderPath, _rootRelativeTo) || folderPath;
  _ui.panel.hidden = false;
  _ui.sidebar.classList.add("searching");
  _ui.input.focus();
  render();
}

export function exitSearchMode() {
  _ui.sidebar.classList.remove("searching");
  _ui.panel.hidden = true;
  _root = null;
  _state.results = null;
  _state.error = null;
  _state.busy = false;
  if (_debounceTimer) {
    clearTimeout(_debounceTimer);
    _debounceTimer = null;
  }
  _seq++; // invalidate in-flight responses
}

export function isSearchModeOpen() {
  return _ui && !_ui.panel.hidden;
}

function scheduleSearch() {
  if (_debounceTimer) clearTimeout(_debounceTimer);
  _debounceTimer = setTimeout(runSearch, DEBOUNCE_MS);
}

async function runSearch() {
  _debounceTimer = null;
  const query = _state.query.trim();
  if (query.length < MIN_QUERY) {
    _state.results = null;
    _state.error = null;
    _state.busy = false;
    render();
    return;
  }
  const seq = ++_seq;
  _state.busy = true;
  _state.error = null;
  render();
  try {
    const results = await _invoke("search_in_folder", {
      root: _root,
      query,
      caseSensitive: _state.caseSensitive,
      wholeWord: _state.wholeWord,
    });
    if (seq !== _seq) return; // stale
    _state.results = results;
    _state.busy = false;
    render();
  } catch (e) {
    if (seq !== _seq) return; // stale
    _state.error = String(e);
    _state.busy = false;
    _state.results = null;
    render();
  }
}

function render() {
  const view = derivePanelState({
    query: _state.query,
    results: _state.results,
    error: _state.error,
    busy: _state.busy,
  });
  _ui.results.replaceChildren();
  _ui.footer.textContent = view.footer || "";
  if (view.kind === "hint") {
    const el = document.createElement("div");
    el.className = "search-hint";
    el.textContent = view.message;
    _ui.results.appendChild(el);
    return;
  }
  if (view.kind === "error") {
    const el = document.createElement("div");
    el.className = "search-error";
    el.textContent = view.message;
    _ui.results.appendChild(el);
    return;
  }
  if (view.kind === "busy") {
    const el = document.createElement("div");
    el.className = "search-busy";
    el.textContent = "Searching…";
    _ui.results.appendChild(el);
    return;
  }
  if (view.kind === "empty") {
    const el = document.createElement("div");
    el.className = "search-empty";
    el.textContent = "No matches";
    _ui.results.appendChild(el);
    return;
  }
  for (const group of view.groups) {
    _ui.results.appendChild(renderGroup(group));
  }
}

function renderGroup(group) {
  const det = document.createElement("details");
  det.open = true;
  const sum = document.createElement("summary");
  sum.textContent = relPath(group.path, _rootRelativeTo);
  const cnt = document.createElement("span");
  cnt.className = "search-file-count";
  cnt.textContent = `(${group.matches.length})`;
  sum.appendChild(cnt);
  det.appendChild(sum);
  for (const m of group.matches) {
    det.appendChild(renderMatch(m));
  }
  return det;
}

function renderMatch(m) {
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "search-match";
  const { text, matchStart, matchEnd } = truncateLineText(
    m.line_text,
    m.match_start,
    m.match_end,
  );
  const lineLabel = document.createElement("span");
  lineLabel.className = "search-match-line";
  lineLabel.textContent = `${m.line}:`;
  btn.appendChild(lineLabel);
  btn.appendChild(document.createTextNode(text.slice(0, matchStart)));
  const mark = document.createElement("mark");
  mark.textContent = text.slice(matchStart, matchEnd);
  btn.appendChild(mark);
  btn.appendChild(document.createTextNode(text.slice(matchEnd)));
  btn.addEventListener("click", () => {
    if (_onOpenResult) _onOpenResult(m.path, m.line);
  });
  return btn;
}

function relPath(absolute, root) {
  if (!root) return absolute;
  if (absolute === root) return "";
  const sep = root.endsWith("/") || root.endsWith("\\") ? "" : "/";
  const prefix = root + sep;
  if (absolute.startsWith(prefix)) return absolute.slice(prefix.length);
  return absolute;
}
```

`MIN_QUERY` is already declared at the top of the file (Task 5) and is shared by both halves — `runSearch` reuses it here without redeclaring.

- [ ] **Step 2: Verify the tests still pass**

Run from the repo root:

```sh
node --test 'ui/*.test.js'
```

Expected: PASS — the existing tests still pass (the pure helpers were not changed; the new code is in a section the tests don't touch).

- [ ] **Step 3: Commit**

```sh
git add ui/folder_search.js
git commit -m "Wire search panel UI and debounced search loop"
```

---

## Task 8: Context-menu item on directories + sidebar wiring

**Files:**
- Modify: `ui/app.js`

This task adds the "Search in Folder…" item to the existing context menu and initialises the search panel on app start. Jump-to-line (`openTabAtLine`) follows in Task 9.

- [ ] **Step 1: Import the search panel and initialise it on startup**

In `ui/app.js`, near the top of the file, find the existing imports block (the first lines after the file header):

```js
import { findMatches } from "./search.js";
```

Add the folder-search import on the next line:

```js
import { findMatches } from "./search.js";
import {
  initSearchPanel,
  enterSearchMode,
  exitSearchMode,
  isSearchModeOpen,
} from "./folder_search.js";
```

Then find the `init()` function (search for `async function init`). Inside its body, BEFORE the call that loads `initial state` (`invoke("get_initial_state")`), add:

```js
  initSearchPanel({
    invoke,
    openResult: (path, line) => openTabAtLine(path, line),
  });
```

(`openTabAtLine` is added in Task 9. For this task it is fine to reference it: JS hoists function declarations and the panel only calls back on user click, which can't happen until after `init()` completes.)

- [ ] **Step 2: Add a temporary stub `openTabAtLine` so the panel works before Task 9 lands**

Below `openSticky`, add a temporary stub:

```js
function openTabAtLine(path, _line) {
  // Real jump-to-line behavior added in the next task; for now, just open
  // the file. Tabs use the existing single-click semantics.
  return openPreview(path);
}
```

- [ ] **Step 3: Extend the context menu**

In `ui/app.js`, the contextmenu handler currently includes (around line 1664):

```js
  if (treeRow && tree.contains(treeRow)) {
    const absolute = treeRow.dataset.path;
    const relative = relativeToRoot(absolute, treeRoot);
    items.push({
      label: "Copy Relative Path",
      action: () => copyText(relative),
      disabled: !relative,
    });
    items.push({
      label: "Copy Absolute Path",
      action: () => copyText(absolute),
    });
    buildContextMenu(items, ev.clientX, ev.clientY);
    return;
  }
```

Add a "Search in Folder…" item that only appears on directory rows (top of the menu — most prominent action). Replace the block above with:

```js
  if (treeRow && tree.contains(treeRow)) {
    const absolute = treeRow.dataset.path;
    const isDir = treeRow.dataset.isDir === "1";
    const relative = relativeToRoot(absolute, treeRoot);
    if (isDir) {
      items.push({
        label: "Search in Folder…",
        action: () => enterSearchMode(absolute, { treeRoot }),
      });
      items.push("---");
    }
    items.push({
      label: "Copy Relative Path",
      action: () => copyText(relative),
      disabled: !relative,
    });
    items.push({
      label: "Copy Absolute Path",
      action: () => copyText(absolute),
    });
    buildContextMenu(items, ev.clientX, ev.clientY);
    return;
  }
```

- [ ] **Step 4: Exit search mode when the user navigates away**

Search mode is per-folder; if the user switches the tree root, the panel should close. Locate `setTreeRoot` (search for `function setTreeRoot`). Add at the top of its body:

```js
  if (typeof isSearchModeOpen === "function" && isSearchModeOpen()) {
    exitSearchMode();
  }
```

- [ ] **Step 5: Build, run, and smoke-test**

From `src-tauri/`:

```sh
cargo build
cargo run
```

Verify:
- Right-click a folder in the tree → "Search in Folder…" appears at the top.
- Right-click a file → "Search in Folder…" is NOT present.
- Click "Search in Folder…" → sidebar swaps to the search panel; input is focused.
- Type a query → results appear, grouped by file.
- Click "← Files" → tree comes back, current preview tab is unchanged.
- Press Esc in the input → exits search mode.
- Click a result → file opens in the preview (line jump comes in Task 9).

- [ ] **Step 6: Commit**

```sh
git add ui/app.js
git commit -m "Add Search in Folder context-menu item and wire panel"
```

---

## Task 9: Jump-to-line in the preview after opening a result

**Files:**
- Modify: `ui/app.js`

This task replaces the temporary `openTabAtLine` stub with a real implementation that scrolls the preview to the matching line and pulses a CSS Highlight.

- [ ] **Step 1: Add a `pendingJumpLine` slot on the tab object**

The tab model `{ path, sticky, raw }` lives in `tabs[]`. We add a transient `pendingJumpLine` field set by `openTabAtLine` and consumed by `postRender`. No persisted state change.

Find the stub from Task 8:

```js
function openTabAtLine(path, _line) {
  // Real jump-to-line behavior added in the next task; for now, just open
  // the file. Tabs use the existing single-click semantics.
  return openPreview(path);
}
```

Replace it with:

```js
async function openTabAtLine(path, line) {
  const idx = findTab(path);
  if (idx !== -1) {
    tabs[idx].pendingJumpLine = line;
    await setActiveTab(idx, { forceRender: true });
    return;
  }
  // openPreview either reuses the non-sticky preview tab or pushes a new
  // one; in both cases the resulting tab is at the end of tabs[].
  const previewIdx = tabs.findIndex((t) => !t.sticky);
  await openPreview(path);
  const finalIdx = previewIdx !== -1 ? previewIdx : tabs.length - 1;
  if (finalIdx >= 0 && finalIdx < tabs.length) {
    tabs[finalIdx].pendingJumpLine = line;
    // open_file already ran; trigger another renderActive so postRender
    // sees pendingJumpLine. forceRender bypasses scrollLock.
    await renderActive({ scrollLock: false });
  }
}
```

- [ ] **Step 2: Make `postRender` consume the pending jump and pulse a highlight**

Find `async function postRender(t, { raw = false, forceMermaid = false } = {})`. At the end of the body, after `addMermaidExportButtons();`, add:

```js
  if (t.pendingJumpLine != null) {
    const line = t.pendingJumpLine;
    t.pendingJumpLine = null;
    jumpToLine(line);
  }
```

Then add a new function below `postRender`:

```js
function jumpToLine(line) {
  const target = findElementForLine(line);
  if (!target) return;
  target.scrollIntoView({ block: "center" });
  pulseJumpHighlight(target);
}

function findElementForLine(line) {
  // Comrak emits data-sourcepos="L1:C1-L2:C2"; pick the deepest element whose
  // [L1, L2] range contains `line`. Walk depth-first so nested blocks (e.g.
  // a list item inside a list) win over their parent.
  const all = preview.querySelectorAll("[data-sourcepos]");
  let best = null;
  for (const el of all) {
    const m = el.dataset.sourcepos.match(/^(\d+):\d+-(\d+):\d+$/);
    if (!m) continue;
    const a = parseInt(m[1], 10);
    const b = parseInt(m[2], 10);
    if (a <= line && line <= b) {
      best = el; // last one wins (depth-first DOM order)
    }
  }
  return best;
}

function pulseJumpHighlight(el) {
  if (
    typeof CSS === "undefined" ||
    !CSS.highlights ||
    typeof Highlight === "undefined"
  ) {
    return; // Highlight API unsupported — silently skip.
  }
  try {
    const range = document.createRange();
    range.selectNodeContents(el);
    const hl = new Highlight(range);
    CSS.highlights.set("search-jump", hl);
    setTimeout(() => CSS.highlights.delete("search-jump"), 1500);
  } catch {
    // selectNodeContents can throw on non-element targets; ignore.
  }
}
```

- [ ] **Step 3: Defer `restoreAnchor` when a jump is pending**

In `renderActive`, the tail currently reads:

```js
  await postRender(t, { raw: result.raw, forceMermaid });

  if (anchor) restoreAnchor(anchor);
  else previewScroll.scrollTop = 0;

  if (findOpen()) runFind({ keepCurrent: true, scroll: false });
```

The pending jump-line is set BEFORE `renderActive` runs (in `openTabAtLine`) and consumed inside `postRender`. To avoid `restoreAnchor` immediately undoing the jump's scroll position, check the flag on the way in. Replace the section with:

```js
  const hadPendingJump = t.pendingJumpLine != null;
  await postRender(t, { raw: result.raw, forceMermaid });

  if (!hadPendingJump) {
    if (anchor) restoreAnchor(anchor);
    else previewScroll.scrollTop = 0;
  }

  if (findOpen()) runFind({ keepCurrent: true, scroll: false });
```

- [ ] **Step 4: Build, run, and smoke-test**

From `src-tauri/`:

```sh
cargo build
cargo run
```

Verify:
- Right-click a folder containing several markdown files; search a term that appears on different lines.
- Click any result → the file opens and the preview scrolls to the matching line.
- The match's enclosing block briefly flashes yellow (light) / amber (dark) for ~1.5 s, then fades.
- Double-click a result is the same as single-click for now (tab promotion is a follow-up; not part of this issue).
- Switching theme while results are visible: result list restyles; clicking a result still jumps correctly.

- [ ] **Step 5: Run all the tests one last time**

From the repo root:

```sh
node --test 'ui/*.test.js'
```

From `src-tauri/`:

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Expected: all green.

- [ ] **Step 6: Commit**

```sh
git add ui/app.js
git commit -m "Jump preview to match line when opening a search result"
```

---

## Task 10: CLAUDE.md update

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Append a "Folder content search" subsection to the "Architecture quick-tour"**

In `CLAUDE.md`, locate the existing **Image files** bullet under "Architecture quick-tour" (it starts `- **Image files**: a frontend-only feature (no Rust).`). The "Architecture quick-tour" list ends a few items later, before "## Platform support". Append a new bullet at the end of that list (before the `## Platform support` heading) reading:

```markdown
- **Folder content search**: right-click a folder in the tree → "Search in
  Folder…" opens a sidebar takeover (`<section id="search-panel">` sibling
  to `<nav id="tree">`, toggled by `.searching` on the sidebar). Backend is
  `src-tauri/src/search.rs` (`walkdir` + a hand-written substring matcher
  whose semantics mirror `ui/search.js::findMatches` — non-overlapping
  matches, case-sensitive + whole-word options, Unicode-aware
  case-insensitive via `str::to_lowercase`). The walk is unfiltered (matches
  `tree.rs` behaviour); only detected binaries (NUL in first 8 KB) and
  files >10 MB are skipped, plus a per-file cap of 200 matches and a total
  cap of 5000 (truncation flag in the footer). The frontend (`ui/folder_search.js`)
  debounces input at 150 ms and uses a sequence number to drop stale IPC
  responses (Tauri's `invoke` has no abort). Clicking a result calls
  `openTabAtLine(path, line)` which stashes `pendingJumpLine` on the tab;
  the next `postRender` consumes it, scrolls the matching `data-sourcepos`
  element into view, and pulses `CSS.highlights["search-jump"]` for 1.5 s.
  `restoreAnchor` is skipped on that one render so the jump's scroll
  position survives.
```

- [ ] **Step 2: Commit**

```sh
git add CLAUDE.md
git commit -m "Document folder content search in CLAUDE.md"
```

---

## Task 11: Create the pull request

- [ ] **Step 1: Push the branch**

```sh
git push -u origin folder-content-search
```

- [ ] **Step 2: Open the PR**

```sh
gh pr create --title "Folder content search (closes #3)" --body "$(cat <<'EOF'
## Summary
- Right-click a folder in the file tree → **Search in Folder…** opens a sidebar takeover that recursively searches every file inside for a literal substring, with case-sensitive and whole-word toggles mirroring the existing in-document find bar.
- Backend (`src-tauri/src/search.rs`) uses `walkdir` + a hand-written matcher. Walk is unfiltered (matches the tree); only binaries (NUL in first 8 KB) and files >10 MB are skipped. Per-file cap 200 matches, total cap 5000 (truncation surfaced in the footer).
- Clicking a result opens the file in the preview tab and scrolls to the matching line using comrak's existing `data-sourcepos` annotations, with a 1.5 s `CSS.highlights["search-jump"]` pulse.

## Test plan
- [ ] `cargo test` from `src-tauri/` — pure + filesystem tests for `search.rs`
- [ ] `node --test 'ui/*.test.js'` from repo root — pure helpers in `folder_search.js`
- [ ] `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
- [ ] Right-click a folder → context menu shows "Search in Folder…"; right-click a file → it does not.
- [ ] Type a 2+ character query → results group by file with line numbers and highlighted matches.
- [ ] Click a result → preview jumps to the match line, brief yellow/amber highlight pulses.
- [ ] Esc in the search input or click "← Files" exits search mode.
- [ ] Switch theme while results are showing → list restyles, no glitches.

Closes #3.
EOF
)"
```

- [ ] **Step 3: Print the PR URL for the user**

`gh pr create` already prints the PR URL on stdout; surface it back to the user verbatim.

---

## Self-review

- **Spec coverage:** Every spec section is touched. Goal → Task 9 (click-to-open + jump). Non-goals → none introduce regex/replace/multi-folder logic. UI placement (sidebar takeover) → Tasks 6+7. Backend `walkdir` → Tasks 1+3. Matcher parity → Task 2. Limits table → Task 3. Frontend integration → Tasks 5+7. Context-menu wiring → Task 8. Jump-to-line → Task 9. Error handling table → Tasks 5 + 7 (the reducer + the runSearch try/catch). Testing → Tasks 2, 3, 5 + manual smoke in Tasks 6, 8, 9. Files-touched table → Tasks 1–10 cover every row. "Things that could go wrong" mitigations are encoded in: per-file/total caps (Task 3), 10 MB cap (Task 3), `pendingJumpLine` deferring `restoreAnchor` (Task 9), `data-sourcepos` block-level fallback (Task 9 — `findElementForLine` walks all sourcepos elements and the last covering one wins, so a deep code-block enclosure still scrolls).

- **Placeholder scan:** No "TBD"/"TODO"/"implement later"/"similar to Task N" patterns. Every code step shows the exact code. Every command step shows the exact command and expected output.

- **Type consistency:**
  - Rust struct field names (`match_start`, `match_end`, `line_text`, `files_scanned`, …) match between `search.rs` (Task 2/3), the IPC wrapper (Task 4), the JS test fixtures (Task 5), and the JS DOM glue (Task 7). Serde will serialise snake_case as-is, and the JS access uses the same snake_case keys.
  - JS function names (`derivePanelState`, `groupResults`, `truncateLineText`, `enterSearchMode`, `exitSearchMode`, `initSearchPanel`, `isSearchModeOpen`, `openTabAtLine`) are used consistently across Tasks 5, 7, 8, 9.
  - The IPC arg names in JS (`root`, `query`, `caseSensitive`, `wholeWord`) are the camelCase form Tauri auto-converts to the Rust snake_case (`case_sensitive`, `whole_word`) — confirmed against the existing pattern in `commands.rs` (other commands like `toggle_task` use the same convention).
  - `MIN_QUERY = 2` appears in both halves of `folder_search.js` (Task 5 at top, Task 7 at bottom); flagged in-task as a thing to keep in sync.
