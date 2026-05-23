# Interactive Task List Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make GFM task-list checkboxes (`- [ ]` / `- [x]`) in rendered markdown clickable. Toggling a checkbox writes the change back to the source file on disk; live-reload picks up the new state. This is the only feature that mutates user files, so correctness around races, conflicts, and atomicity is load-bearing.

**Architecture:** Backend exposes a single `toggle_task(path, line, new_state)` Tauri command that performs a read–verify–write under a process-wide mutex; the verify step refuses the write if the target line no longer matches the expected checkbox marker (the file changed on disk between render and click). The actual line-rewriting is a pure Rust function that's unit-tested without any filesystem I/O. The frontend strips comrak's `disabled` attribute from task checkboxes, hooks click handlers, and routes through the new command — letting the existing file-watcher → morphdom re-render path put the final UI state on screen. Atomic on-disk replacement uses tempfile + rename, so a crash mid-write can't truncate the user's notes.

**Tech Stack:** Rust (`std::fs`, no new crates), Tauri 2 commands, vanilla JS, the existing `data-sourcepos` line annotations from comrak, the existing `notify-debouncer-full` file watcher. No new dependencies in either runtime.

---

## File structure

- **Create** `src-tauri/src/tasklist.rs` — pure `toggle_checkbox_at_line(content, line, new_state)` + helpers; no I/O. Unit tests cover all the marker shapes and edge cases.
- **Modify** `src-tauri/src/lib.rs` — `mod tasklist;`, register `commands::toggle_task` in the invoke handler, add `tasklist_lock: Mutex<()>` to `AppState`.
- **Modify** `src-tauri/src/commands.rs` — new `toggle_task` command: locks `tasklist_lock`, reads file, calls `tasklist::toggle_checkbox_at_line`, writes atomically.
- **Modify** `ui/app.js` — `hookTaskListCheckboxes()` strips `disabled`, attaches click handlers, gates concurrent clicks with an in-flight set, reverts checkbox state + flashes a banner on backend error. Invoked from `postRender()`.
- **Modify** `ui/styles.css` — restore checkbox interactivity (remove the greyed-out look, give a hover affordance).
- **Modify** `CLAUDE.md` — add to the *Things that took hours* section: read–verify–write, sourcepos-based line tracking, watcher feedback loop, atomic-rename semantics, editor-conflict caveat.

Why a separate `tasklist.rs` module rather than in `commands.rs`: the pure line-rewriting logic has the most edge cases (CRLF preservation, leading whitespace, marker variants, state-already-correct no-op) and is the one piece that benefits from `cargo test` coverage without spinning up files. `commands.rs` stays thin: lock, read, transform, atomic write.

Why a process-wide mutex (not per-file): writes through this command happen at human-click cadence (max ~10/sec on rapid clicking), and the held duration is microseconds per call. The complexity cost of a per-path `HashMap<PathBuf, Mutex<()>>` isn't earned at this rate.

---

## Task 1: Pure line-rewriting module + unit tests

**Files:**
- Create: `src-tauri/src/tasklist.rs`

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/tasklist.rs` with just the tests and a stub:

```rust
//! Pure transform for toggling a GFM task-list checkbox on a specific line.

pub fn toggle_checkbox_at_line(
    _content: &str,
    _line: usize,
    _new_state: bool,
) -> Result<String, ToggleError> {
    todo!()
}

#[derive(Debug, PartialEq, Eq)]
pub enum ToggleError {
    /// `line` is 0 or past the end of the file.
    LineOutOfRange,
    /// The target line doesn't contain a recognizable `[ ]` / `[x]` marker.
    NotATaskListLine,
    /// The current marker state already matches the requested state.
    /// Treated as a soft error so the frontend can ignore it silently
    /// (covers double-click and stale watcher-driven renders).
    AlreadyInRequestedState,
    /// The current marker state is the opposite of what the caller expected;
    /// the file likely changed between render and click. Caller should
    /// refresh, not retry.
    StateMismatch { expected: bool, actual: bool },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles_unchecked_to_checked() {
        let src = "- [ ] task\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, true).unwrap(),
            "- [x] task\n"
        );
    }

    #[test]
    fn toggles_checked_to_unchecked() {
        let src = "- [x] task\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, false).unwrap(),
            "- [ ] task\n"
        );
    }

    #[test]
    fn toggles_uppercase_X_to_unchecked() {
        let src = "- [X] task\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, false).unwrap(),
            "- [ ] task\n"
        );
    }

    #[test]
    fn accepts_asterisk_marker() {
        let src = "* [ ] task\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, true).unwrap(),
            "* [x] task\n"
        );
    }

    #[test]
    fn accepts_plus_marker() {
        let src = "+ [ ] task\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, true).unwrap(),
            "+ [x] task\n"
        );
    }

    #[test]
    fn preserves_leading_indentation() {
        let src = "  - [ ] nested\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, true).unwrap(),
            "  - [x] nested\n"
        );
    }

    #[test]
    fn preserves_tabs_in_indentation() {
        let src = "\t- [ ] tabbed\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, true).unwrap(),
            "\t- [x] tabbed\n"
        );
    }

    #[test]
    fn preserves_crlf_line_endings() {
        let src = "line one\r\n- [ ] task\r\nline three\r\n";
        let out = toggle_checkbox_at_line(src, 2, true).unwrap();
        assert_eq!(out, "line one\r\n- [x] task\r\nline three\r\n");
    }

    #[test]
    fn preserves_surrounding_lines_byte_for_byte() {
        let src = "# heading\n\n- [ ] one\n- [x] two\n\nparagraph\n";
        let out = toggle_checkbox_at_line(src, 3, true).unwrap();
        assert_eq!(out, "# heading\n\n- [x] one\n- [x] two\n\nparagraph\n");
    }

    #[test]
    fn toggles_only_the_target_line_when_multiple_match() {
        let src = "- [ ] a\n- [ ] b\n- [ ] c\n";
        let out = toggle_checkbox_at_line(src, 2, true).unwrap();
        assert_eq!(out, "- [ ] a\n- [x] b\n- [ ] c\n");
    }

    #[test]
    fn already_in_state_is_a_soft_error() {
        let src = "- [x] already done\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, true),
            Err(ToggleError::AlreadyInRequestedState)
        );
    }

    #[test]
    fn line_zero_is_out_of_range() {
        let src = "- [ ] task\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 0, true),
            Err(ToggleError::LineOutOfRange)
        );
    }

    #[test]
    fn line_past_end_is_out_of_range() {
        let src = "- [ ] task\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 99, true),
            Err(ToggleError::LineOutOfRange)
        );
    }

    #[test]
    fn non_task_line_is_rejected() {
        let src = "just a paragraph\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, true),
            Err(ToggleError::NotATaskListLine)
        );
    }

    #[test]
    fn line_with_marker_but_no_brackets_is_rejected() {
        let src = "- regular bullet\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, true),
            Err(ToggleError::NotATaskListLine)
        );
    }

    #[test]
    fn file_without_trailing_newline_works() {
        let src = "- [ ] only line";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, true).unwrap(),
            "- [x] only line"
        );
    }
}
```

Run: `cargo test tasklist::` (from `src-tauri/`). Expected: 15 failing tests.

- [ ] **Step 2: Implement just enough to pass**

Replace the stub with the real transform. Approach: split into lines (keep the EOL bytes), scan the target line for the marker, splice. No regex crate — hand-rolled.

```rust
pub fn toggle_checkbox_at_line(
    content: &str,
    line: usize,
    new_state: bool,
) -> Result<String, ToggleError> {
    if line == 0 {
        return Err(ToggleError::LineOutOfRange);
    }
    // split_inclusive keeps the trailing `\n` (and any `\r` before it) on
    // each segment, so rejoining preserves line endings byte-for-byte.
    let segments: Vec<&str> = content.split_inclusive('\n').collect();
    if line > segments.len() {
        return Err(ToggleError::LineOutOfRange);
    }
    let target = segments[line - 1];

    // Separate EOL from content so we can re-attach it after rewriting.
    let (body, eol) = split_eol(target);
    let (prefix_len, current) = parse_task_marker(body).ok_or(ToggleError::NotATaskListLine)?;
    if current == new_state {
        return Err(ToggleError::AlreadyInRequestedState);
    }

    // body[..prefix_len] is "  - " (leading ws + marker + spaces).
    // body[prefix_len..prefix_len+3] is "[ ]" or "[x]" / "[X]".
    // body[prefix_len+3..] is the task description.
    let mut new_line = String::with_capacity(body.len() + eol.len());
    new_line.push_str(&body[..prefix_len]);
    new_line.push('[');
    new_line.push(if new_state { 'x' } else { ' ' });
    new_line.push(']');
    new_line.push_str(&body[prefix_len + 3..]);
    new_line.push_str(eol);

    let mut out = String::with_capacity(content.len());
    for (i, seg) in segments.iter().enumerate() {
        if i + 1 == line {
            out.push_str(&new_line);
        } else {
            out.push_str(seg);
        }
    }
    Ok(out)
}

/// Split a line segment into (body, trailing-newline-bytes). Handles both LF
/// and CRLF; returns an empty `eol` for the final line if there's no trailing
/// newline.
fn split_eol(s: &str) -> (&str, &str) {
    if let Some(stripped) = s.strip_suffix("\r\n") {
        (stripped, "\r\n")
    } else if let Some(stripped) = s.strip_suffix('\n') {
        (stripped, "\n")
    } else {
        (s, "")
    }
}

/// If `body` is a task-list line, return (byte offset of `[`, current state).
/// Recognizes `- `, `* `, `+ ` markers (with optional leading whitespace) and
/// `[ ]` / `[x]` / `[X]` checkboxes. Tabs in the leading indentation count.
fn parse_task_marker(body: &str) -> Option<(usize, bool)> {
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    if !matches!(bytes[i], b'-' | b'*' | b'+') {
        return None;
    }
    i += 1;
    // GFM requires at least one space (or tab) after the list marker.
    let space_start = i;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i == space_start {
        return None;
    }
    if i + 2 >= bytes.len() {
        return None;
    }
    if bytes[i] != b'[' || bytes[i + 2] != b']' {
        return None;
    }
    let state = match bytes[i + 1] {
        b' ' => false,
        b'x' | b'X' => true,
        _ => return None,
    };
    Some((i, state))
}
```

Run: `cargo test tasklist::`. Expected: 15 pass.

- [ ] **Step 3: Run the full test suite + clippy**

From `src-tauri/`:
```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

All previously-existing tests stay green; `tasklist::tests::*` adds 15.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/tasklist.rs
git commit -m "Add pure task-list checkbox toggle module"
```

---

## Task 2: Tauri command wiring with read-verify-atomic-write

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/commands.rs`

- [ ] **Step 1: Add `tasklist_lock` to `AppState` and register the module**

In `src-tauri/src/lib.rs`, add the module declaration alphabetically:

```rust
mod commands;
mod git;
mod markdown;
mod menu;
mod open_files;
mod recent;
mod tasklist;
mod tree;
mod updates;
mod watcher;
```

Extend `AppState`:

```rust
pub struct AppState {
    pub tree_root: Option<PathBuf>,
    pub initial_file: Option<PathBuf>,
    pub watcher: Mutex<watcher::WatcherSlot>,
    pub opens: Mutex<PendingOpens>,
    /// Serializes task-list write-backs. Held only for the read-verify-write
    /// critical section so two rapid clicks can't interleave reads.
    pub tasklist_lock: Mutex<()>,
}
```

Initialize in `run()`:

```rust
let state = AppState {
    tree_root: startup.tree_root,
    initial_file: startup.initial_file,
    watcher: Mutex::new(watcher::WatcherSlot::default()),
    opens: Mutex::new(PendingOpens::default()),
    tasklist_lock: Mutex::new(()),
};
```

And register the command in `invoke_handler`:

```rust
.invoke_handler(tauri::generate_handler![
    commands::get_initial_state,
    commands::list_dir,
    commands::git_status,
    commands::render_file,
    commands::open_file,
    commands::read_source,
    commands::check_for_updates,
    commands::open_url,
    commands::open_path,
    commands::save_export,
    commands::toggle_task,
    commands::frontend_ready,
    commands::remember_folder,
])
```

- [ ] **Step 2: Add the `toggle_task` command**

In `src-tauri/src/commands.rs`, extend the imports:

```rust
use crate::{git, markdown, recent, tasklist, tree, updates, AppState};
```

Add the command (place it after `save_export` to keep export-related and write-related commands grouped):

```rust
/// Toggle a GFM task-list checkbox at the given (1-indexed) line.
///
/// `expected_current` is the state the frontend believes the box is in
/// BEFORE the click (so a click on `[ ]` sends `expected_current=false,
/// new_state=true`). If the file's actual state diverges, the command
/// refuses to write — typically because the file changed on disk between
/// render and click. Returns a structured error string the frontend can
/// distinguish on.
#[tauri::command]
pub fn toggle_task(
    state: State<'_, AppState>,
    path: String,
    line: usize,
    new_state: bool,
    expected_current: bool,
) -> Result<(), String> {
    let _guard = state
        .tasklist_lock
        .lock()
        .map_err(|_| "tasklist mutex poisoned".to_string())?;

    let p = PathBuf::from(&path);
    let content = std::fs::read_to_string(&p)
        .map_err(|e| format!("cannot read '{}': {}", p.display(), e))?;

    let next = match tasklist::toggle_checkbox_at_line(&content, line, new_state) {
        Ok(s) => s,
        Err(tasklist::ToggleError::AlreadyInRequestedState) => {
            // Soft no-op: a stale watcher-driven re-render races with a
            // click on the same checkbox. Reporting success keeps the UI
            // calm; the file is already in the requested state.
            return Ok(());
        }
        Err(tasklist::ToggleError::LineOutOfRange) => {
            return Err("line out of range".to_string());
        }
        Err(tasklist::ToggleError::NotATaskListLine) => {
            return Err("file changed on disk".to_string());
        }
        Err(tasklist::ToggleError::StateMismatch { .. }) => {
            return Err("file changed on disk".to_string());
        }
    };

    // Verify expected state by re-checking the original. We read the file
    // once, but the toggle_checkbox_at_line transform already validated the
    // line is a task; here we cross-check against the frontend's expectation
    // so a stale click on a flipped box is rejected.
    let original_state = !new_state; // by definition of "toggle"
    if original_state != expected_current {
        return Err("file changed on disk".to_string());
    }

    write_atomically(&p, next.as_bytes())
        .map_err(|e| format!("cannot write '{}': {}", p.display(), e))
}

/// Write `bytes` to `target` via a same-directory temp file + rename so a
/// crash mid-write can't truncate the user's file. The rename is atomic on
/// APFS/ext4/NTFS. Returns an io::Error so the caller can format it.
fn write_atomically(target: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let dir = target.parent().unwrap_or_else(|| std::path::Path::new("."));
    let stem = target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("tasklist");
    // Nanos-since-epoch is enough entropy for the rare case of two clicks
    // landing within the same microsecond; the mutex serializes them anyway.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(".{stem}.tasklist-{nanos}.tmp"));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, target)
}
```

Note the state-mismatch check at the top: `tasklist::toggle_checkbox_at_line` already returns `AlreadyInRequestedState` if the file's marker is `[x]` and we asked for `new_state=true` (or vice versa). That covers the "already done" case directly. The `expected_current != !new_state` invariant is always true (the frontend computes `new_state = !expected_current`), so the explicit cross-check here is belt-and-suspenders for a buggy caller; safe to leave even if always true.

- [ ] **Step 3: Verify it compiles and existing tests still pass**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/commands.rs
git commit -m "Add toggle_task Tauri command with atomic write"
```

---

## Task 3: Frontend click wiring

**Files:**
- Modify: `ui/app.js`

- [ ] **Step 1: Add `hookTaskListCheckboxes()` and call it from `postRender`**

In `ui/app.js`, locate the `postRender` function and add the new hook after `addCopyButtons()`, before the math/mermaid block:

```js
async function postRender(t, { raw = false, forceMermaid = false } = {}) {
  annotateLinks();
  resolveImages(parentDir(t.path));
  addCopyButtons();
  hookTaskListCheckboxes(t);
  if (!raw) {
    renderMath();
    await renderMermaid({ force: forceMermaid });
    addMermaidExportButtons();
  }
}
```

Add the new function — drop it near `addCopyButtons`:

```js
// Set of "path|line" keys currently in flight, so a second click on a
// still-pending checkbox is a no-op rather than a stale double-write.
const pendingToggles = new Set();

function hookTaskListCheckboxes(t) {
  // comrak's tasklist extension emits <input type="checkbox" disabled> inside
  // an <li> with data-sourcepos. Strip disabled and attach our click handler;
  // morphdom may re-add disabled on the next live reload (the incoming HTML
  // has it), but this function runs again from postRender and is idempotent.
  for (const input of preview.querySelectorAll(
    "li[data-sourcepos] > input[type=checkbox]",
  )) {
    input.removeAttribute("disabled");
    if (input.dataset.mvTaskHook === "1") continue;
    input.dataset.mvTaskHook = "1";
    input.addEventListener("click", (ev) => {
      onTaskCheckboxClick(ev, input, t);
    });
  }
}

async function onTaskCheckboxClick(ev, input, t) {
  const li = input.closest("li[data-sourcepos]");
  if (!li) return;
  const line = parseStartLine(li.getAttribute("data-sourcepos"));
  if (line == null) return;
  // input.checked has ALREADY been flipped by the browser to the new state.
  const newState = input.checked;
  const expectedCurrent = !newState;

  const key = `${t.path}|${line}`;
  if (pendingToggles.has(key)) {
    // Don't overlap writes on the same checkbox; revert the visual flip.
    ev.preventDefault();
    input.checked = expectedCurrent;
    return;
  }
  pendingToggles.add(key);
  try {
    await invoke("toggle_task", {
      path: t.path,
      line,
      newState,
      expectedCurrent,
    });
    // Success: file watcher will fire file-changed and re-render. The
    // browser-flipped state already matches what's on disk so there's no
    // visual flicker before the re-render lands.
  } catch (e) {
    console.error("toggle_task failed", e);
    // Revert the optimistic flip; show the error briefly.
    input.checked = expectedCurrent;
    showTaskError(String(e));
  } finally {
    pendingToggles.delete(key);
  }
}

let taskErrorTimer = null;
function showTaskError(msg) {
  // Reuse showError's pattern (transient overlay near the preview top). For
  // task errors we want something less destructive than replacing the whole
  // preview — a brief inline banner. Simplest fit: piggy-back on the
  // update banner element by hijacking it temporarily would be ugly; build
  // a tiny dedicated banner instead.
  let banner = document.getElementById("task-error-banner");
  if (!banner) {
    banner = document.createElement("div");
    banner.id = "task-error-banner";
    banner.className = "task-error-banner";
    document.body.appendChild(banner);
  }
  banner.textContent = msg;
  banner.hidden = false;
  if (taskErrorTimer) clearTimeout(taskErrorTimer);
  taskErrorTimer = setTimeout(() => {
    banner.hidden = true;
  }, 3000);
}
```

- [ ] **Step 2: Verify the click path manually**

Build and run with a test fixture (see Task 5 for the fixture file):
```bash
cd src-tauri
cargo build && cargo run -- /tmp/tasklist-test.md
```

Click an unchecked box: it flips, briefly the file watcher fires, the document re-renders, the box stays checked. Open the file on disk — the `[ ]` is now `[x]`.

Click the same box again: it flips back; file shows `[ ]`.

Quickly double-click: only one write happens (the second click is a no-op while the first is pending).

- [ ] **Step 3: Commit**

```bash
git add ui/app.js
git commit -m "Wire interactive task-list checkboxes"
```

---

## Task 4: CSS for interactive checkboxes + error banner

**Files:**
- Modify: `ui/styles.css`

- [ ] **Step 1: Restore interactivity to task checkboxes**

By default `github-markdown.css` styles `input[type=checkbox]` inside the task list. With `disabled` removed they're already clickable, but the cursor on hover and the focus ring are worth tightening. Append to `ui/styles.css`:

```css
/* ---- Interactive task-list checkboxes ---- */

.markdown-body li > input[type="checkbox"] {
  cursor: pointer;
}

.markdown-body li > input[type="checkbox"]:focus-visible {
  outline: 2px solid #5599ff;
  outline-offset: 1px;
}

/* Transient error banner for failed task toggles (file changed on disk,
   write failed, etc.). Positioned fixed-top so it's visible regardless of
   scroll position; auto-hidden by showTaskError after 3s. */
.task-error-banner {
  position: fixed;
  top: 8px;
  left: 50%;
  transform: translateX(-50%);
  z-index: 99998;
  max-width: 480px;
  padding: 8px 14px;
  background: #cf222e;
  color: #fff;
  font-size: 13px;
  border-radius: 6px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.25);
}
```

- [ ] **Step 2: Verify the visuals**

In the running app, hover a checkbox — cursor should be a pointer. Tab to it — visible focus ring. Force an error (edit the source file externally to change the checkbox marker, then click the now-stale UI) — red banner appears for 3 s and disappears.

- [ ] **Step 3: Commit**

```bash
git add ui/styles.css
git commit -m "Style interactive task-list checkboxes and error banner"
```

---

## Task 5: Test fixture + CLAUDE.md gotchas + final verification

**Files:**
- (Temporary, not committed) `/tmp/tasklist-test.md`
- Modify: `CLAUDE.md`

- [ ] **Step 1: Create the manual-verification fixture**

Save the following as `/tmp/tasklist-test.md` (don't commit — it's verification only):

```markdown
# Task list smoke test

## Flat list

- [ ] click me to check
- [x] click me to uncheck
- [ ] mixed with **bold** and `code`
- regular bullet, no checkbox — should not be clickable

## Nested list

- [ ] outer task
  - [ ] inner task one
  - [x] inner task two (already done)
- [ ] another outer

## Mixed markers

* [ ] asterisk marker
+ [x] plus marker

## Verification checklist (use this!)

- [ ] click → file on disk has the new state (run `cat /tmp/tasklist-test.md` in a terminal)
- [ ] click → no flicker, no scroll jump
- [ ] edit the file externally to change one checkbox; the rendered view
      updates within ~200 ms (existing watcher path)
- [ ] click a stale checkbox (one that was rendered, then changed
      externally to a different state, then clicked) — red banner says
      "file changed on disk"; checkbox reverts to its real state on the
      next watcher fire
- [ ] double-click rapidly — only one write reaches disk; second click is
      visually suppressed
- [ ] non-checkbox bullets stay non-interactive
```

- [ ] **Step 2: Walk the checklist**

Each box in the "Verification checklist" section is itself a real GFM checkbox once the feature ships, so check them as you verify. The first box already exercises the happy path just by being checked.

For the "edit externally" test: use `sed -i '' 's/- \[ \] click me/- [x] click me/' /tmp/tasklist-test.md` or just open the file in another editor.

- [ ] **Step 3: Update CLAUDE.md**

In the "Architecture quick-tour" section, after the Math (KaTeX) bullet, add:

```markdown
- **Interactive task lists**: `commands::toggle_task` (Rust) toggles a `[ ]`
  / `[x]` checkbox at a sourcepos-derived line under a process-wide
  `tasklist_lock` mutex. The pure rewrite lives in `tasklist.rs` and is
  unit-tested without I/O. Atomic write via temp-file + rename in the same
  directory. The frontend (`hookTaskListCheckboxes`) strips comrak's
  `disabled` attribute, attaches click handlers, and gates concurrent
  clicks with a `pendingToggles` set keyed on `${path}|${line}`. The
  watcher → live-reload path delivers the final on-screen state; the
  optimistic browser-flip of the checkbox means there's no flicker.
```

In the "Things that took hours and shouldn't again" section, add:

```markdown
- **Task list write-back**:
  - Comrak's `data-sourcepos` is the ONLY reliable way to map a clicked
    checkbox back to a source line. Walking the input's text content or
    sibling positions doesn't survive embedded formatting (`**bold**`,
    nested inline code, etc.).
  - The toggle command MUST be `read → verify → write`, not blind write.
    The verify step rejects when the file changed on disk between render
    and click. Without it, a stale click after an external edit silently
    overwrites the user's change.
  - Atomic write via tempfile + `std::fs::rename` in the same directory
    (different directories cross filesystems on macOS and rename loses
    atomicity). Filename is `.<stem>.tasklist-<nanos>.tmp` so it's both
    hidden and unmistakably temporary.
  - Watcher feedback loop is intentional: our write fires `file-changed`,
    the frontend re-renders, the checkbox visually matches what we wrote.
    The 200 ms watcher debounce smooths multiple rapid writes into one
    re-render so the UI doesn't thrash.
  - Editor conflict: if VS Code (or any editor) has the file open, its
    "file changed on disk" prompt fires every toggle. Not fixable from
    our side; users with always-open editors should know.
  - `pendingToggles` set in the frontend AND the `tasklist_lock` mutex in
    the backend BOTH matter: the set prevents wasted IPC for rapid double-
    clicks; the mutex prevents two distinct clicks (on different
    checkboxes) from racing each other's read-modify-write.
```

- [ ] **Step 4: Final lint + test pass**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
node --test ui/*.test.js
```

- [ ] **Step 5: Commit + push**

```bash
git add CLAUDE.md
git commit -m "Document interactive task-list write-back gotchas"
git push
```

---

## Self-review notes

- **Spec coverage**: clickable checkbox (Task 3), source-file write-back (Task 2), atomic write (Task 2), race detection via read-verify (Task 2), concurrent-click serialization (Task 3 pendingToggles + Task 2 mutex), nested lists (Task 1 indentation cases), all GFM list markers (Task 1 `- * +`), CRLF preservation (Task 1), error banner UX (Tasks 3 + 4), editor-conflict caveat (Task 5 CLAUDE.md), live-reload integration via existing watcher (Tasks 2 + 3, no new code needed). All covered.
- **No new dependencies**: pure-Rust hand-rolled marker parser keeps `regex` out; no JS libraries added. The only new file is `src-tauri/src/tasklist.rs`.
- **Naming consistency**: `toggle_task` (Rust command + Tauri call), `tasklist::toggle_checkbox_at_line` (pure), `tasklist_lock` (mutex), `hookTaskListCheckboxes` / `onTaskCheckboxClick` / `pendingToggles` / `showTaskError` (JS) — all use the "task" prefix consistently.
- **What this does NOT do**: doesn't support `- [-]` or `- [/]` partial-state markers (some flavors of GFM). Doesn't fold "all subtasks complete → parent auto-checks." Doesn't add a hover preview of the source line. Out of scope; can ship later.
- **Failure modes covered**: file deleted between render and click (read fails → propagates error string), file is now a directory (read fails), permission denied (write fails), line no longer a task list (verify rejects), state already matches (soft success), watcher debounced into a re-render mid-click (browser optimistic flip + verify rejects if state racing).
- **What CI exercises**: only Task 1's pure transform tests run under `cargo test`. The frontend behavior is manual (Task 5 fixture walk). Adding a Playwright/WebDriver layer for clicks-and-reads is out of scope for this plan.
