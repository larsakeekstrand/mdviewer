# Folder content search — design

**Date:** 2026-05-31
**Status:** Approved, pending implementation plan
**Issue:** [#3](https://github.com/larsakeekstrand/mdviewer/issues/3) — "File tree text search"

## Goal

When a folder is visible in the file tree, let the user right-click it and
search the **contents** of every file inside that folder for a given string.
Results group by file and show the matched line; clicking a result opens the
file and jumps the preview to that line. Modelled on VS Code's "Find in
Folder."

## Non-goals

- Filename search (this is content search; filename matching is out).
- Regular-expression search (literal substring with case-sensitive and
  whole-word toggles only — mirrors `ui/search.js`).
- Search-and-replace.
- Global multi-folder search; scope is the right-clicked folder and its
  descendants.
- Honoring `.gitignore` or any other ignore file — the search is unfiltered,
  matching the existing tree behaviour (`tree.rs::list_directory` lists
  everything). The only files skipped are detected binaries and files larger
  than 10 MB.
- Live re-running the query when files change on disk (matches VS Code; user
  re-runs the query manually).
- Searching open tabs that aren't backed by disk (e.g. unsaved synthetic
  buffers — there are none in this app today).
- Surfacing results across tabs; results live in the sidebar takeover only.

## Approach

### UI placement: sidebar takeover

The sidebar (currently the `<nav id="tree">`) gains a sibling `<section
id="search-panel" hidden>`. When entering search mode, a class on the sidebar
hides the tree and shows the panel; the panel contains:

- A "← Files" button (top-left) that exits search mode.
- A title showing the relative path of the searched folder (e.g.
  "Search in: docs/").
- A search input + case-sensitive (`Aa`) and whole-word (`ab`) toggle
  buttons, styled to match the in-document find bar.
- A scrollable results list, grouped by file (collapsible `<details>` per
  file).
- A footer line showing scan stats ("237 files · 12 matches" or
  "Showing first 5000 — refine your query").

Rejected alternatives: bottom results panel (adds a new resizable region we
don't otherwise have), synthetic results tab in the tab bar (conflates
markdown documents with results), modal dialog (obscures the preview while
searching, feels less integrated).

### Backend: `walkdir` + manual matcher

A new Rust module `src-tauri/src/search.rs`:

- `walkdir::WalkDir::new(root).follow_links(false)` for the recursive walk.
- Per-file binary sniff: read up to the first 8 KB; if any byte is `0x00`,
  classify as binary and skip. Otherwise read the file fully (still subject
  to the 10 MB cap below).
- Per-file scan: split the contents into lines (`split('\n')`), run the
  literal-substring matcher (mirror of `ui/search.js::findMatches`) over
  each line, emit one `Match` per occurrence with `line`, `column`,
  `line_text`, `match_start`, `match_end`.

Rejected alternatives:

- **`ignore` crate (ripgrep's walker):** designed around `.gitignore` and
  parallel filtering — most of its value is disabled by the unfiltered
  scope. Pulls in `globset` / `ignore` / `same-file` transitively for no
  gain in this design.
- **External `rg` binary:** fastest, but breaks the self-contained-app
  property, adds per-platform shipping headaches, and depends on the user
  having it installed.

### Matcher parity with the in-document find bar

The Rust matcher mirrors `ui/search.js::findMatches` semantically:

- Literal substring, no regex.
- Case-sensitive toggle (default off → lowercase both the line and the
  query via Rust's `str::to_lowercase()`, then substring-search; same shape
  as `ui/search.js` which does `text.toLowerCase()` / `query.toLowerCase()`).
  Both `str::to_lowercase()` in Rust and JS `toLowerCase()` are
  Unicode-aware, so non-ASCII case-insensitive matches behave the same in
  both halves of the app.
- Whole-word toggle: a match `[start, end)` is a whole word iff the chars
  immediately before `start` and at `end` are NOT word characters, where a
  word character is `\p{L} | \p{N} | _` — same as `ui/search.js::isWordChar`.
  Implementation uses Rust's `char::is_alphanumeric()` (which is Unicode-aware)
  plus an explicit `_` check, giving the same set in practice.
- Non-overlapping matches, in order, advancing `from = i + 1` after each
  found match (same loop shape as the JS).

A pure function `match_line(line: &str, query: &str, opts: SearchOpts) ->
Vec<(usize, usize)>` is extracted so it can be unit-tested without any
filesystem. The cases in `ui/search.test.js` are ported one-for-one.

### Limits and back-pressure

| Limit | Value | Rationale |
| --- | --- | --- |
| Min query length | 2 chars | Avoids "e" matching everything. |
| Per-file match cap | 200 | One pathological file shouldn't dominate. |
| Total match cap | 5000 | UI stays responsive; truncation flag → footer hint. |
| Per-file size cap | 10 MB | Reading a 1 GB log into memory would freeze. |
| Binary sniff window | 8 KB | Matches GNU grep / git's heuristic. |
| Query debounce | 150 ms | Snappy without thrashing IPC. |

When the total cap is reached, the walker stops early and the response sets
`truncated = true`. When a file hits the per-file cap, scanning continues
into the next file. Files skipped for size are not counted as binary.

### Frontend integration

A new module `ui/folder_search.js`:

```js
enterSearchMode(folderPath)   // capture path, swap sidebar UI, focus input
exitSearchMode()              // restore tree, clear state
onInputChanged()              // debounce 150ms → invoke with seq number
renderResults({matches, truncated, files_scanned, ...})
openResult(path, line)        // open tab non-sticky, jump-to-line
```

Each `invoke("search_in_folder", …)` call carries a monotonically increasing
sequence number; the response handler discards results whose seq isn't the
latest. (Tauri's IPC has no cancellation; this is the same pattern any other
debounced UI uses.)

Results are rendered as nested `<details>` (file group) → `<button>` (match
row). The matched span inside the line text is wrapped in `<mark>`. Long
lines (`>` ~300 chars) are truncated with an ellipsis on either side,
centred on the first match.

### Context-menu wiring

In `ui/app.js`, the existing tree-row branch of the contextmenu handler
(currently adds "Copy Relative Path" and "Copy Absolute Path") gains a
third item — **"Search in Folder…"** — shown only when
`treeRow.dataset.isDir === "1"`. Click → `enterSearchMode(absolutePath)`.

### Jump-to-line in the preview

Comrak already annotates every block-level element with
`data-sourcepos="L1:C1-L2:C2"` (used today by scroll-anchor preservation
during live reload, per CLAUDE.md). `openResult(path, line)`:

1. Reuses the existing single-click tab-replace path to open the file
   (preview, non-sticky).
2. After the next `postRender`, locates the first element whose
   `data-sourcepos` start-line `≤ line ≤` end-line.
3. Scrolls that element into view (centered).
4. Registers a `Range` over its text with `CSS.highlights.set("search-jump",
   …)` and clears it 1.5 s later.

Double-click on a result promotes the tab to sticky, mirroring the tree's
single/double-click semantics.

## Architecture

```
context-menu (Search in Folder…)
  ├─→ folder_search.js  enterSearchMode(path)
  │     └─→ UI: hide tree, show search-panel, focus input
  │
  ├─→ on input (debounced)
  │     └─→ invoke("search_in_folder", {root, query, opts}) [seq=N]
  │           └─→ Rust search.rs::search_in_folder
  │                 └─→ walkdir → match_line → SearchResults
  │           ←──── results (if seq still latest)
  │     └─→ render
  │
  ├─→ on click result
  │     └─→ tab open (non-sticky) → preview render
  │           └─→ jump-to-line via data-sourcepos
  │                 └─→ CSS.highlights "search-jump" (1.5s)
  │
  └─→ on "← Files" or Esc
        └─→ exitSearchMode → restore tree
```

## Components

### Backend

**`src-tauri/src/search.rs`** (new)

```rust
pub struct SearchOpts {
    pub case_sensitive: bool,
    pub whole_word: bool,
}

pub struct Match {
    pub path: String,
    pub line: u32,
    pub column: u32,
    pub line_text: String,
    pub match_start: u32,
    pub match_end: u32,
}

pub struct SearchResults {
    pub matches: Vec<Match>,
    pub truncated: bool,
    pub files_scanned: usize,
    pub files_skipped_binary: usize,
    pub files_skipped_too_large: usize,
    pub files_unreadable: usize,
}

pub fn search_in_folder(
    root: &Path,
    query: &str,
    opts: SearchOpts,
) -> Result<SearchResults, String>;

// Extracted pure helpers (no I/O):
pub fn match_line(line: &str, query: &str, opts: SearchOpts) -> Vec<(usize, usize)>;
pub fn is_binary(sample: &[u8]) -> bool;
pub fn is_word_char(c: char) -> bool;
```

**`src-tauri/src/commands.rs`** — add a thin `#[tauri::command]` wrapper
(the `#[tauri::command]` lives here, matching the existing convention; the
pure logic lives in `search.rs`):

```rust
#[tauri::command]
pub fn search_in_folder(
    root: String,
    query: String,
    case_sensitive: bool,
    whole_word: bool,
) -> Result<SearchResults, String>
```

**`src-tauri/src/lib.rs`** — register the module and command in the
`invoke_handler` list.

**`src-tauri/Cargo.toml`** — add `walkdir = "2"`.

### Frontend

**`ui/folder_search.js`** (new) — exports `enterSearchMode`, `exitSearchMode`,
plus a pure reducer `derivePanelState({query, results, error, busy})` that
returns the renderable shape so the rendering can be unit-tested without a
DOM, matching the `ui/export.js` test pattern.

**`ui/folder_search.test.js`** (new) — `node --test`-style:
- Grouping helper turns flat `Match[]` into ordered `{path, matches}[]`.
- Line truncation helper centers on first match, respects 300-char width.
- `derivePanelState` returns the right shape across query/result combos.

**`ui/app.js`** changes:
- Add the "Search in Folder…" item to the tree-row branch of the contextmenu
  handler (around line 1664), gated on `isDir`.
- Expose a small seam (`openTabAtLine(path, line)`) that the search panel
  calls — implemented as "the existing single-click open path, plus a
  pending jump-target stored on the tab object that `postRender` consumes."
- One extra step in `postRender` (or a follow-up function it calls) that
  consumes the pending jump-target on the active tab and runs the
  scroll/highlight.

**`ui/index.html`** — add `<section id="search-panel" hidden>` inside the
sidebar, structured as described above.

**`ui/styles.css`** — sidebar takeover styles (the panel uses the same grid
slot as the tree); result row styles; `mark` styling for the matched span;
`::highlight(search-jump)` rule.

## Error handling

| Condition | Behaviour |
| --- | --- |
| Empty / whitespace query | "Type to search" hint, no IPC |
| Query length < 2 | "Type at least 2 characters" hint, no IPC |
| `root` missing / not a dir | Reject IPC; transient banner ("Folder not found") |
| Per-entry walkdir error | Increment `files_unreadable`, continue |
| `read_to_string` I/O error | Increment `files_unreadable`, continue |
| File > 10 MB | Increment `files_skipped_too_large`, continue |
| Binary file | Increment `files_skipped_binary`, continue |
| Stale (superseded) response | Frontend drops via seq number check |
| User exits search mid-search | Latest results may still arrive; frontend ignores them after `exitSearchMode` clears state |

Backend errors use the same `Result<T, String>` pattern as other commands
(per CLAUDE.md). Frontend errors use `showTransientError` (not `showError`,
which clears the preview).

## Testing

### Pure Rust unit tests (`search.rs::tests`)

- `match_line` — port every case in `ui/search.test.js` (empty query,
  whole-word at boundaries, case-sensitive on/off, overlapping pattern,
  Unicode word characters).
- `is_binary` — NUL in first byte, NUL at byte 7999, NUL at byte 8001 (not
  detected), pure ASCII, pure UTF-8 with multibyte chars.
- `is_word_char` — letters, digits, underscore, hyphen, punctuation,
  whitespace, common multibyte (e.g. `å`, `日`).

### Integration tests (`search.rs::tests`, filesystem)

Use the `unique_temp_dir()` pattern from `tree.rs`:

- Folder with `.md`, `.txt`, a binary file (contains NUL), and a nested
  subdir → results include the right paths, binaries skipped.
- File at exactly 10 MB → not skipped; file at 10 MB + 1 byte → skipped,
  counter incremented.
- One file with 300 matches → only first 200 emitted, scanning continues
  to next file.
- Total match cap: many files with many matches → `truncated=true`,
  matches.len() ≤ 5000.
- Case-sensitive and whole-word options both flow through.
- Symlink cycle does not infinite-loop (walkdir handles this with
  `follow_links(false)`).

### Pure JS tests (`folder_search.test.js`, `node --test`)

- `derivePanelState` reducer across (no-query, busy, results, truncated,
  error) inputs.
- Result grouping preserves walker order (file appearance order).
- Long-line truncation: centers on first match, total length ≤ ~300, adds
  ellipsis on either side as needed.

### Manual smoke (per CLAUDE.md "verify in the real app")

- Right-click folder vs. file: "Search in Folder…" appears only on dirs.
- Search a term that exists → result list groups correctly; click jumps to
  the right line in preview and the jump-highlight pulses.
- Search a folder containing `.git/` → binary sniff skips pack files;
  `.git/HEAD`, `.git/config` still match (matches "skip nothing" choice).
- Search a folder containing a `node_modules/` → also searched (matches
  "skip nothing"), demonstrating the trade-off the user explicitly chose.
- Pathological query (e.g. `e`) on a big tree → blocked by min-length 2;
  query `the` → truncation kicks in, footer shows hint, UI doesn't freeze.
- Live reload during search: file changes mid-results — old results stay
  on screen (no auto-refresh), user re-runs query if needed.
- Esc in the input → exits search mode and restores the tree, current tab
  unchanged.
- Theme switch while panel is open: result styling restyles correctly,
  no FOUC.
- Stale-response check: rapid typing `f`, `fo`, `foo` → only `foo` results
  rendered, even if `f`/`fo` responses arrive later.

## Files touched

| File | Change |
| --- | --- |
| `src-tauri/src/search.rs` | NEW — walker, matcher, command body, tests |
| `src-tauri/src/lib.rs` | Register module + `search_in_folder` command |
| `src-tauri/src/commands.rs` | `#[tauri::command] search_in_folder` wrapper delegating to `search::search_in_folder` |
| `src-tauri/Cargo.toml` | Add `walkdir = "2"` |
| `ui/folder_search.js` | NEW — panel state, IPC, rendering, reducer |
| `ui/folder_search.test.js` | NEW — `node --test` unit tests |
| `ui/app.js` | Context-menu item for dirs; `openTabAtLine` seam |
| `ui/index.html` | `<section id="search-panel" hidden>` inside sidebar |
| `ui/styles.css` | Sidebar takeover, result rows, `mark`, `::highlight(search-jump)` |
| `CLAUDE.md` | Append a "Folder content search" subsection |

## Things that could go wrong (and why this design handles them)

- **Searching `.git/` text files surfaces ref/config noise.** That's the
  trade-off of the "skip nothing" choice and is documented in the smoke
  tests. If it becomes painful in practice, the future fix is to add a
  fixed denylist (`.git`) as a single-line walker filter, not a redesign.
- **Big repos freeze the UI.** Mitigated by 10 MB per-file cap, 5000 total
  cap, 150 ms debounce, binary sniff, and min-query-length 2. The walk
  still runs synchronously on the IPC worker thread — that's fine in
  Tauri (the main thread isn't blocked).
- **`data-sourcepos` doesn't cover a target line because comrak ranges are
  block-level, not line-level.** Mitigation: pick the first element whose
  start-line ≤ target ≤ end-line; if no element matches (e.g. line is
  inside a code block whose sourcepos spans many lines), scroll to the
  enclosing block and the highlight covers the whole block. Acceptable.
- **Jump-to-line conflicts with the live-reload scroll-anchor preservation
  in `postRender`.** Mitigation: when a tab has `pendingJumpLine` set, the
  jump-to-line step runs and `restoreAnchor` is skipped for that one
  render; `pendingJumpLine` is cleared after consumption so subsequent
  live-reload renders fall back to normal anchor restoration.
- **The new `search_in_folder` command is open to abuse from `tauri://`
  origin scripts? No — Tauri commands are not callable from arbitrary
  origins; the `app.security.csp` `script-src 'self'` ensures no injected
  scripts can call IPC anyway. (CLAUDE.md note.)
- **The walkdir crate adds a dep.** It's tiny (~1 file, no transitive deps
  in the default feature set), and it's the de-facto Rust standard for
  this use case. Worth it.
