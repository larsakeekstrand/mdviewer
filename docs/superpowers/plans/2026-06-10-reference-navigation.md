# Reference Navigation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make file references in a rendered markdown doc clickable — bare `path:line` text and markdown links — opening markdown targets in mdviewer with a line jump and handing code targets to a configurable editor at the line.

**Architecture:** Pure detection helpers in `ui/refs.js` (`findReferences`, `parseHrefLine`) and a pure Rust `editor::build_editor_argv`, all unit-tested. A `postRender` hook `linkifyReferences()` scans text nodes, batches one `resolve_references` IPC (resolve + existence-check + root containment), and wraps verified hits in `<a class="ref-link">`. A single `navigateToRef()` routes markdown → existing `openTabAtLine`, code → `open_in_editor` (argv-substituted, no shell). The editor command is a persisted setting surfaced in Preferences.

**Tech Stack:** Rust (Tauri 2, `serde_json`), `cargo test` + `node --test`, vanilla JS. No new crates.

---

## File structure

- **Create `ui/refs.js`** — pure: `findReferences(text)`, `parseHrefLine(href)`.
- **Create `ui/refs.test.js`** — `node --test`.
- **Create `src-tauri/src/editor.rs`** — pure `build_editor_argv` + `#[cfg(test)]`.
- **Modify `src-tauri/src/commands.rs`** — `resolve_refs` core + `resolve_references` command, `open_in_editor` command, `Preferences.editor_command`, `set_editor_command`.
- **Modify `src-tauri/src/recent.rs`** — persist `editor_command` (+ default const + load/save).
- **Modify `src-tauri/src/lib.rs`** — `mod editor;` + register 3 commands.
- **Modify `ui/app.js`** — import refs.js; `linkifyReferences()` postRender hook; `navigateToRef()`; ref-link + `:line` handling in the existing preview click handler.
- **Modify `ui/preferences.html` / `ui/preferences.js`** — editor-command field.
- **Modify `ui/styles.css`** — `.ref-link` (light + dark).

## Conventions

- JS tests: `node --test ui/*.test.js`. Rust tests: `cd src-tauri && cargo test`. Filesystem tests use `std::env::temp_dir().join(format!("mdv-…-{}", std::process::id()))` (the `fs_ops.rs` convention) and clean up.
- Lint gate before commit: `cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings`.
- Tauri commands return `Result<T, String>` (or a plain value); use `format!("…: {e}")`.
- Frontend + Rust changes need `cargo build` to rebundle.
- No `Co-Authored-By` trailer. Commit after each task.

---

## Task 1: `refs.js::findReferences`

**Files:**
- Create: `ui/refs.js`
- Create: `ui/refs.test.js`

- [ ] **Step 1: Write the failing test**

Create `ui/refs.test.js`:

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { findReferences } from "./refs.js";

test("finds a path with a line number", () => {
  const r = findReferences("see src/foo.rs:42 here");
  assert.equal(r.length, 1);
  assert.deepEqual(
    { raw: r[0].raw, path: r[0].path, line: r[0].line },
    { raw: "src/foo.rs:42", path: "src/foo.rs", line: 42 },
  );
  assert.equal("see src/foo.rs:42 here".slice(r[0].start, r[0].end), "src/foo.rs:42");
});

test("finds a bare filename with a known extension and no line", () => {
  const r = findReferences("edit foo.rs please");
  assert.equal(r.length, 1);
  assert.equal(r[0].path, "foo.rs");
  assert.equal(r[0].line, null);
});

test("finds a slashed path with no extension", () => {
  const r = findReferences("under docs/superpowers/plans matters");
  assert.equal(r.length, 1);
  assert.equal(r[0].path, "docs/superpowers/plans");
});

test("ignores timestamps, ratios, bare words, and prose colons", () => {
  assert.equal(findReferences("at 12:34 today").length, 0);
  assert.equal(findReferences("ratio 3:1 ok").length, 0);
  assert.equal(findReferences("just a word").length, 0);
  assert.equal(findReferences("key: value").length, 0);
});

test("ignores URLs", () => {
  assert.equal(findReferences("visit http://example.com/x now").length, 0);
});

test("finds two references in one string in order", () => {
  const r = findReferences("a/b.md:1 and c/d.ts:9");
  assert.deepEqual(r.map((x) => x.path), ["a/b.md", "c/d.ts"]);
  assert.deepEqual(r.map((x) => x.line), [1, 9]);
});

test("ignores a :col after :line (line only)", () => {
  const r = findReferences("foo.rs:42:8");
  assert.equal(r[0].line, 42);
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `node --test ui/refs.test.js`
Expected: FAIL — `Cannot find module './refs.js'`.

- [ ] **Step 3: Implement**

Create `ui/refs.js`:

```js
// Pure helpers for reference navigation. DOM-free so they unit-test under
// `node --test`; the DOM linkifying and IPC live in app.js.

const KNOWN_EXT =
  /\.(rs|ts|tsx|js|jsx|mjs|cjs|json|toml|yaml|yml|py|go|rb|java|kt|kts|swift|c|h|cc|cpp|hpp|cs|php|sh|bash|zsh|lua|ex|exs|sql|css|scss|html|htm|xml|md|markdown|mdown|mkd|mkdn|txt|cfg|ini|conf)$/i;

/** Find file references in plain text. A reference is a path-like token —
 *  containing a `/` OR ending in a known extension — optionally followed by
 *  `:line` (and an ignored `:col`). URL-ish tokens are skipped. Existence is
 *  NOT checked here (the backend does that); this only proposes candidates.
 *  Returns [{ raw, path, line, start, end }] in document order. */
export function findReferences(text) {
  const out = [];
  const re = /[A-Za-z0-9._~@\-/]+(?::\d+){0,2}/g;
  let m;
  while ((m = re.exec(text)) !== null) {
    const raw = m[0];
    if (raw.includes("://")) continue;
    const parts = raw.split(":");
    const path = parts[0];
    if (!path) continue;
    if (!path.includes("/") && !KNOWN_EXT.test(path)) continue;
    const line = parts.length > 1 && /^\d+$/.test(parts[1]) ? parseInt(parts[1], 10) : null;
    out.push({ raw, path, line, start: m.index, end: m.index + raw.length });
  }
  return out;
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `node --test ui/refs.test.js`
Expected: PASS (7 tests).

- [ ] **Step 5: Commit**

```bash
git add ui/refs.js ui/refs.test.js
git commit -m "Add refs.js findReferences (detect file references in text)"
```

---

## Task 2: `refs.js::parseHrefLine`

**Files:**
- Modify: `ui/refs.js`, `ui/refs.test.js`

- [ ] **Step 1: Write the failing test**

Append to `ui/refs.test.js`:

```js
import { parseHrefLine } from "./refs.js";

test("parseHrefLine splits a trailing :line", () => {
  assert.deepEqual(parseHrefLine("docs/spec.md:88"), { path: "docs/spec.md", line: 88 });
});

test("parseHrefLine returns null line when absent", () => {
  assert.deepEqual(parseHrefLine("docs/spec.md"), { path: "docs/spec.md", line: null });
});

test("parseHrefLine keeps the rightmost colon as the separator", () => {
  assert.deepEqual(parseHrefLine("a:b.md:5"), { path: "a:b.md", line: 5 });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `node --test ui/refs.test.js`
Expected: FAIL — `parseHrefLine is not a function`.

- [ ] **Step 3: Implement**

Append to `ui/refs.js`:

```js
/** Split a trailing `:line` off a (relative) link href. `"a.md:42"` →
 *  `{ path: "a.md", line: 42 }`; no trailing line → `{ path, line: null }`. */
export function parseHrefLine(href) {
  const m = /^(.*):(\d+)$/.exec(href);
  if (m) return { path: m[1], line: parseInt(m[2], 10) };
  return { path: href, line: null };
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `node --test ui/refs.test.js`
Expected: PASS (10 tests).

- [ ] **Step 5: Commit**

```bash
git add ui/refs.js ui/refs.test.js
git commit -m "Add refs.js parseHrefLine"
```

---

## Task 3: `editor.rs::build_editor_argv`

**Files:**
- Create: `src-tauri/src/editor.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod editor;`)

- [ ] **Step 1: Write the failing test + module stub**

Create `src-tauri/src/editor.rs`:

```rust
//! Building the editor launch argv for code references. Pure + unit-tested; the
//! command that reads settings and spawns lives in commands.rs.

pub fn build_editor_argv(_template: &str, _file: &str, _line: u32) -> Vec<String> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_file_and_line_into_one_argv_element() {
        assert_eq!(
            build_editor_argv("code -g {file}:{line}", "/a/b.rs", 42),
            vec!["code", "-g", "/a/b.rs:42"],
        );
    }

    #[test]
    fn keeps_a_malicious_path_as_one_inert_element() {
        // A path with shell metacharacters must NOT split or inject — it stays a
        // single argv element because the template is split BEFORE substitution.
        assert_eq!(
            build_editor_argv("code -g {file}:{line}", "; rm -rf ~", 42),
            vec!["code", "-g", "; rm -rf ~:42"],
        );
    }

    #[test]
    fn supports_other_editor_templates() {
        assert_eq!(
            build_editor_argv("idea --line {line} {file}", "/a/b.rs", 7),
            vec!["idea", "--line", "7", "/a/b.rs"],
        );
    }

    #[test]
    fn template_without_line_placeholder_is_fine() {
        assert_eq!(
            build_editor_argv("subl {file}", "/a/b.rs", 7),
            vec!["subl", "/a/b.rs"],
        );
    }
}
```

Add `mod editor;` to `src-tauri/src/lib.rs` among the `mod` declarations (lines 1-13). Because there is no production caller yet and `-D warnings` promotes `dead_code`, temporarily annotate it:

```rust
#[allow(dead_code)]
mod editor;
```

(Task 6 adds the caller and removes the `#[allow(dead_code)]`.)

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test build_editor_argv 2>&1 | tail -15`
Expected: FAIL — `not yet implemented` panic.

- [ ] **Step 3: Implement**

Replace the stub in `editor.rs`:

```rust
/// Build the editor launch argv from a user template. The template is split on
/// whitespace into [program, args…] FIRST, then `{file}` / `{line}` are
/// substituted within each token — so an untrusted path (even with spaces or
/// shell metacharacters) lands in a single argv element and is never
/// shell-interpreted by the caller (`Command::new(program).args(rest)`).
pub fn build_editor_argv(template: &str, file: &str, line: u32) -> Vec<String> {
    let line = line.to_string();
    template
        .split_whitespace()
        .map(|tok| tok.replace("{file}", file).replace("{line}", &line))
        .collect()
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd src-tauri && cargo test build_editor_argv 2>&1 | tail -15`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/editor.rs src-tauri/src/lib.rs
git commit -m "Add editor::build_editor_argv (injection-safe editor launch argv)"
```

---

## Task 4: persist `editor_command` in settings

**Files:**
- Modify: `src-tauri/src/recent.rs`

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` in `recent.rs`:

```rust
    #[test]
    fn editor_command_round_trips_and_preserves_other_fields() {
        let mut s = Store {
            last_folder: Some(PathBuf::from("/keep")),
            ..Default::default()
        };
        s.editor_command = Some("subl {file}:{line}".to_string());
        let json = serde_json::to_string(&s).unwrap();
        let back: Store = serde_json::from_str(&json).unwrap();
        assert_eq!(back.editor_command.as_deref(), Some("subl {file}:{line}"));
        assert_eq!(back.last_folder, Some(PathBuf::from("/keep")));
    }

    #[test]
    fn default_editor_command_is_vscode() {
        assert_eq!(DEFAULT_EDITOR_COMMAND, "code -g {file}:{line}");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test editor_command 2>&1 | tail -15`
Expected: FAIL — no field `editor_command` / `DEFAULT_EDITOR_COMMAND` not found.

- [ ] **Step 3: Implement**

Add the constant near the top of `recent.rs` (after the imports):

```rust
pub const DEFAULT_EDITOR_COMMAND: &str = "code -g {file}:{line}";
```

Add the field to `struct Store` (alongside `channel`):

```rust
    #[serde(default)]
    editor_command: Option<String>,
```

Add load/save functions near `load_channel`/`save_channel`:

```rust
pub fn load_editor_command(app: &AppHandle) -> String {
    load_store(app)
        .editor_command
        .unwrap_or_else(|| DEFAULT_EDITOR_COMMAND.to_string())
}

/// Persists the editor command, preserving every other field.
pub fn save_editor_command(app: &AppHandle, command: String) {
    let mut store = load_store(app);
    store.editor_command = Some(command);
    write_store(app, &store);
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd src-tauri && cargo test editor_command 2>&1 | tail -15`
Expected: PASS (2 new tests; existing recent tests still pass).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/recent.rs
git commit -m "Persist editor_command setting in the recent store"
```

---

## Task 5: `resolve_references` (resolve + existence + containment)

**Files:**
- Modify: `src-tauri/src/commands.rs`

The testable core is a free function `resolve_refs(root, doc_dir, candidates)`; the `#[tauri::command]` wrapper supplies `root` from `current_root`.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` in `commands.rs` (it already has `use tauri::Manager;` etc.; add what you need):

```rust
    #[test]
    fn resolve_refs_resolves_existing_in_root_and_marks_markdown() {
        use std::fs;
        let root = std::env::temp_dir().join(format!("mdv-refs-{}", std::process::id()));
        let docs = root.join("docs");
        fs::create_dir_all(&docs).unwrap();
        fs::write(root.join("code.rs"), "x").unwrap();
        fs::write(docs.join("plan.md"), "# p").unwrap();

        // doc_dir is the docs/ folder; "plan.md" resolves via doc_dir,
        // "code.rs" via the root, "nope.rs" doesn't exist, "../escape" is out.
        fs::write(std::env::temp_dir().join(format!("mdv-refs-escape-{}", std::process::id())), "x").ok();
        let cands = vec![
            "plan.md".to_string(),
            "code.rs".to_string(),
            "nope.rs".to_string(),
        ];
        let got = resolve_refs(&root, &docs, cands);
        let by: std::collections::HashMap<_, _> =
            got.iter().map(|r| (r.raw.clone(), r)).collect();

        assert!(by.contains_key("plan.md"));
        assert!(by["plan.md"].is_markdown);
        assert!(by.contains_key("code.rs"));
        assert!(!by["code.rs"].is_markdown);
        assert!(!by.contains_key("nope.rs"));
        assert!(by["plan.md"].abs_path.ends_with("plan.md"));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn resolve_refs_rejects_paths_outside_root() {
        use std::fs;
        let root = std::env::temp_dir().join(format!("mdv-refs-out-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        // A real file that exists but is OUTSIDE root (temp_dir itself).
        let outside = std::env::temp_dir().join(format!("mdv-refs-out-file-{}", std::process::id()));
        fs::write(&outside, "x").unwrap();
        let rel = format!("../mdv-refs-out-file-{}", std::process::id());
        let got = resolve_refs(&root, &root, vec![rel]);
        assert_eq!(got.len(), 0);
        fs::remove_dir_all(&root).ok();
        fs::remove_file(&outside).ok();
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test resolve_refs 2>&1 | tail -15`
Expected: FAIL — `cannot find function resolve_refs` / `ResolvedRef`.

- [ ] **Step 3: Implement**

Add to `commands.rs` (near the other commands). `ResolvedRef`, the core, and the command:

```rust
#[derive(Serialize)]
pub struct ResolvedRef {
    /// The candidate path string the frontend sent (its lookup key).
    pub raw: String,
    pub abs_path: String,
    pub is_markdown: bool,
}

/// Resolve candidate paths against `doc_dir` then `root`, keeping only those
/// that exist as a file and canonicalize to within `root`. Pure of Tauri state
/// so it is unit-testable.
fn resolve_refs(root: &Path, doc_dir: &Path, candidates: Vec<String>) -> Vec<ResolvedRef> {
    let mut out = Vec::new();
    for raw in candidates {
        let abs = [doc_dir.join(&raw), root.join(&raw)]
            .into_iter()
            .find_map(|p| std::fs::canonicalize(&p).ok())
            .filter(|c| c.is_file());
        let abs = match abs {
            Some(a) => a,
            None => continue,
        };
        if !fs_ops::within_root(&abs, root) {
            continue;
        }
        let is_markdown = abs
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| {
                matches!(
                    e.to_ascii_lowercase().as_str(),
                    "md" | "markdown" | "mdown" | "mkd" | "mkdn"
                )
            })
            .unwrap_or(false);
        out.push(ResolvedRef {
            raw,
            abs_path: abs.to_string_lossy().to_string(),
            is_markdown,
        });
    }
    out
}

/// Resolve file references found in a rendered doc. Returns only references that
/// exist as a file within the open tree root; the line number stays on the
/// frontend (it is not part of the candidate path).
#[tauri::command]
pub fn resolve_references(
    state: State<'_, AppState>,
    doc_dir: String,
    candidates: Vec<String>,
) -> Vec<ResolvedRef> {
    let root = match current_root(&state) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    resolve_refs(&root, Path::new(&doc_dir), candidates)
}
```

Note: `Path`, `PathBuf`, `Serialize`, `fs_ops`, `current_root`, `State`, `AppState` are already in scope in `commands.rs`.

- [ ] **Step 4: Run to verify it passes**

Run: `cd src-tauri && cargo test resolve_refs 2>&1 | tail -15`
Expected: PASS (2 tests). Then `cargo clippy --all-targets -- -D warnings` clean. (`resolve_references` is registered in Task 6; until then it may warn as unused — if `-D warnings` blocks the commit, do Task 6 before committing, or temporarily `#[allow(dead_code)]` the command and remove it in Task 6. Prefer doing Task 6 next.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "Add resolve_references: existence + root-contained reference resolution"
```

---

## Task 6: `open_in_editor` command + Preferences wiring + registration

**Files:**
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`

- [ ] **Step 1: Add the `open_in_editor` command**

In `commands.rs`:

```rust
/// Open a code reference in the user's configured editor at `line`. The path is
/// confined within the open tree root; the editor argv is built without a shell
/// (`editor::build_editor_argv`), so an untrusted path cannot inject a command.
#[tauri::command]
pub fn open_in_editor(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    line: u32,
) -> Result<(), String> {
    let root = current_root(&state)?;
    if !fs_ops::within_root(Path::new(&path), &root) {
        return Err("reference is outside the open folder".to_string());
    }
    let template = recent::load_editor_command(&app);
    let argv = crate::editor::build_editor_argv(&template, &path, line);
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| "editor command is empty".to_string())?;
    std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to launch editor '{program}': {e}"))?;
    Ok(())
}
```

- [ ] **Step 2: Add `editor_command` to Preferences + a setter**

In `commands.rs`, add the field to the `Preferences` struct (it is `#[serde(rename_all = "camelCase")]`, so this serializes as `editorCommand`):

```rust
    pub editor_command: String,
```

In `get_preferences`, set it:

```rust
        editor_command: recent::load_editor_command(&app),
```

Add the setter command:

```rust
#[tauri::command]
pub fn set_editor_command(app: AppHandle, command: String) {
    recent::save_editor_command(&app, command);
}
```

- [ ] **Step 3: Register the three commands + drop the editor dead_code allow**

In `src-tauri/src/lib.rs` `tauri::generate_handler![ … ]`, add:

```rust
            commands::resolve_references,
            commands::open_in_editor,
            commands::set_editor_command,
```

And change `#[allow(dead_code)] mod editor;` back to plain `mod editor;` (it now has a caller via `open_in_editor`).

- [ ] **Step 4: Verify build + tests + lint**

Run: `cd src-tauri && cargo build 2>&1 | tail -3`
Expected: `Finished`.
Run: `cargo test 2>&1 | grep "test result" | head -1` (all pass) and `cargo clippy --all-targets -- -D warnings 2>&1 | tail -3` (clean).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "Add open_in_editor + editor-command preference; register reference commands"
```

---

## Task 7: Frontend — linkify, navigate, and extend the link handler

**Files:**
- Modify: `ui/app.js`

- [ ] **Step 1: Import the helpers**

After the existing `import { validateName } from "./treeops.js";` (and the `review.js` import added by an earlier feature), add:

```js
import { findReferences, parseHrefLine } from "./refs.js";
```

- [ ] **Step 2: Add `linkifyReferences` and `navigateToRef`**

Add near the other postRender hooks (e.g. after `renderReviewMarkers`/`copyReview`):

```js
async function linkifyReferences(t) {
  const candidates = new Set();
  const hits = [];
  const walker = document.createTreeWalker(preview, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      for (let el = node.parentElement; el && el !== preview; el = el.parentElement) {
        if (el.tagName === "PRE" || el.tagName === "A") return NodeFilter.FILTER_REJECT;
      }
      return NodeFilter.FILTER_ACCEPT;
    },
  });
  let n;
  while ((n = walker.nextNode())) {
    const refs = findReferences(n.nodeValue);
    if (refs.length) {
      hits.push({ node: n, refs });
      for (const r of refs) candidates.add(r.path);
    }
  }
  if (candidates.size === 0) return;

  let resolved;
  try {
    resolved = await invoke("resolve_references", {
      docDir: parentDir(t.path),
      candidates: [...candidates],
    });
  } catch (e) {
    console.error("resolve_references failed", e);
    return;
  }
  const byPath = new Map(resolved.map((r) => [r.raw, r]));
  if (byPath.size === 0) return;

  for (const { node, refs } of hits) {
    const valid = refs.filter((r) => byPath.has(r.path));
    if (!valid.length) continue;
    const text = node.nodeValue;
    const frag = document.createDocumentFragment();
    let cursor = 0;
    for (const r of valid) {
      if (r.start < cursor) continue; // skip overlaps
      if (r.start > cursor) frag.appendChild(document.createTextNode(text.slice(cursor, r.start)));
      const info = byPath.get(r.path);
      const a = document.createElement("a");
      a.className = "ref-link";
      a.href = "#";
      a.textContent = r.raw;
      a.dataset.path = info.abs_path;
      a.dataset.line = r.line != null ? String(r.line) : "";
      a.dataset.kind = info.is_markdown ? "md" : "code";
      a.title = info.abs_path + (r.line != null ? `:${r.line}` : "");
      frag.appendChild(a);
      cursor = r.end;
    }
    if (cursor < text.length) frag.appendChild(document.createTextNode(text.slice(cursor)));
    node.parentNode.replaceChild(frag, node);
  }
}

async function navigateToRef(absPath, line, isMarkdown) {
  if (isMarkdown) {
    if (line != null) await openTabAtLine(absPath, line);
    else await openPreview(absPath);
  } else {
    try {
      await invoke("open_in_editor", { path: absPath, line: line ?? 1 });
    } catch (e) {
      showTransientError("Couldn't open in editor: " + e);
    }
  }
}
```

- [ ] **Step 3: Hook into `postRender`**

In `postRender` (`ui/app.js:1517`), inside the `if (!raw)` block, after the existing hooks (e.g. after `renderReviewMarkers(t);`), add:

```js
    await linkifyReferences(t);
```

- [ ] **Step 4: Handle ref-links and `:line` in the existing preview click handler**

The handler at `ui/app.js:2410` begins:

```js
preview.addEventListener("click", async (ev) => {
  const a = ev.target.closest("a[href]");
  if (!a || !preview.contains(a)) return;
  const href = a.getAttribute("href");
  if (!href) return;

  // Always intercept; default would navigate the WebView and destroy app state.
  ev.preventDefault();
```

Immediately after that `ev.preventDefault();`, add the ref-link branch:

```js
  // Auto-linked file reference (data-* carries a resolved absolute path).
  if (a.classList.contains("ref-link")) {
    const refLine = a.dataset.line ? parseInt(a.dataset.line, 10) : null;
    await navigateToRef(a.dataset.path, refLine, a.dataset.kind === "md");
    return;
  }
```

Then, in the **relative path** branch lower down — currently:

```js
  // Relative path: resolve against the active tab's directory.
  const tab = activeTab();
  if (!tab) return;
  const resolved = resolveRelative(parentDir(tab.path), href);

  if (MD_EXT.test(resolved)) {
    if (ev.metaKey || ev.ctrlKey) {
      await openSticky(resolved);
    } else {
      await openPreview(resolved);
    }
  } else if (ev.metaKey || ev.ctrlKey) {
    try {
      await invoke("open_path", { path: resolved });
    } catch (e) {
      console.error("open_path failed", e);
    }
  }
```

replace it with (parse a trailing `:line`, and jump for markdown / editor for code):

```js
  // Relative path: resolve against the active tab's directory. Honor a
  // trailing :line on the href (e.g. [x](docs/spec.md:88)).
  const tab = activeTab();
  if (!tab) return;
  const { path: hrefPath, line: hrefLine } = parseHrefLine(href);
  const resolved = resolveRelative(parentDir(tab.path), hrefPath);

  if (MD_EXT.test(resolved)) {
    if (hrefLine != null) {
      await openTabAtLine(resolved, hrefLine);
    } else if (ev.metaKey || ev.ctrlKey) {
      await openSticky(resolved);
    } else {
      await openPreview(resolved);
    }
  } else if (hrefLine != null) {
    await navigateToRef(resolved, hrefLine, false);
  } else if (ev.metaKey || ev.ctrlKey) {
    try {
      await invoke("open_path", { path: resolved });
    } catch (e) {
      console.error("open_path failed", e);
    }
  }
```

- [ ] **Step 5: Verify build + JS tests**

Run: `node --test ui/*.test.js 2>&1 | grep -E "# (pass|fail)"`
Expected: PASS (no regressions; refs.js tests included).
Run: `cd src-tauri && cargo build 2>&1 | tail -3`
Expected: `Finished`.

- [ ] **Step 6: Commit**

```bash
git add ui/app.js
git commit -m "Linkify file references; route md to in-app jump, code to editor"
```

---

## Task 8: Preferences — editor command field

**Files:**
- Modify: `ui/preferences.html`, `ui/preferences.js`

- [ ] **Step 1: Add the input to `preferences.html`**

After the existing beta-toggle `.row` (the `<div class="row">…</div>` containing `#beta-toggle`), add:

```html
    <div class="row">
      <label for="editor-command">
        Editor command for code references
        <div class="hint">
          Launched for non-markdown references. Use <code>{file}</code> and
          <code>{line}</code> placeholders (default: <code>code -g {file}:{line}</code>).
        </div>
      </label>
    </div>
    <input type="text" id="editor-command" style="width: 100%; box-sizing: border-box" />
```

- [ ] **Step 2: Wire it in `preferences.js`**

In `ui/preferences.js`, add a reference and load/save. After `const versionEl = …`:

```js
const editorInput = document.getElementById("editor-command");
```

In `load()`, after setting the toggle:

```js
  editorInput.value = prefs.editorCommand || "";
```

After the existing toggle listener, add:

```js
editorInput.addEventListener("change", async () => {
  try {
    await invoke("set_editor_command", { command: editorInput.value });
  } catch (e) {
    console.error("set_editor_command failed", e);
  }
});
```

- [ ] **Step 3: Verify build**

Run: `cd src-tauri && cargo build 2>&1 | tail -3`
Expected: `Finished`.

- [ ] **Step 4: Commit**

```bash
git add ui/preferences.html ui/preferences.js
git commit -m "Add editor-command field to Preferences"
```

---

## Task 9: `.ref-link` styling

**Files:**
- Modify: `ui/styles.css`

- [ ] **Step 1: Add styles**

Append to `ui/styles.css`:

```css
/* ---- Reference navigation ---- */

.ref-link {
  color: #0969da;
  text-decoration: underline dotted;
  cursor: pointer;
}

.ref-link:hover {
  text-decoration: underline;
}

[data-theme="dark"] .ref-link {
  color: #2f81f7;
}
```

- [ ] **Step 2: Commit**

```bash
git add ui/styles.css
git commit -m "Style .ref-link (light + dark)"
```

---

## Task 10: Build + manual GUI smoke test

**Files:** none (verification only)

- [ ] **Step 1: Full gates**

Run: `cd src-tauri && cargo test 2>&1 | grep "test result" | head -1` (all pass) ·
`cargo fmt --check && cargo clippy --all-targets -- -D warnings 2>&1 | tail -1` (clean) ·
`cd .. && node --test ui/*.test.js 2>&1 | grep -E "# (pass|fail)"` (pass).

- [ ] **Step 2: Create a scratch doc with references and run the app**

```bash
cat > /Users/laek/source/mdviewer/scratch-refs.md <<'EOF'
# Reference test

- Code ref: `src-tauri/src/commands.rs:42`
- Bare file: `ui/app.js`
- Markdown ref: see [the spec](docs/superpowers/specs/2026-06-10-reference-navigation-design.md:1)
- Fake ref (should stay plain): `nope/does-not-exist.rs:9`
- Not a ref: meeting at 12:34, ratio 3:1
EOF
cd src-tauri && cargo run -- ..
```

Open `scratch-refs.md` in the app.

- [ ] **Step 3: Verify behavior (light mode)**

- `src-tauri/src/commands.rs:42` and `ui/app.js` render as dotted-underline links; `nope/does-not-exist.rs:9`, `12:34`, `3:1` stay plain text.
- Click `ui/app.js` → opens in mdviewer (rendered). Click `commands.rs:42` → your editor opens at line 42 (set the editor in **MDViewer ▸ Settings…** first if not VS Code).
- Click the markdown ref → opens the design spec tab and jumps/pulses near the top.

- [ ] **Step 4: Verify Settings + dark mode**

- **MDViewer ▸ Settings…** shows the editor-command field with `code -g {file}:{line}`; change it (e.g. to `cursor -g {file}:{line}`), reopen Settings → persisted.
- Toggle theme (☾) → `.ref-link` color stays legible.

- [ ] **Step 5: Clean up + commit any fixes**

```bash
rm -f /Users/laek/source/mdviewer/scratch-refs.md
git add -A && git commit -m "Polish reference navigation after smoke test"   # skip if no fixes
```

---

## Self-review notes (for the implementer)

- **Spec coverage:** Part 1 behavior → Tasks 7 (linkify/navigate), 9 (style), 8 (setting). Part 2 detection/verification/resolution → Task 1 (`findReferences`), Task 5 (`resolve_references`). Part 3 architecture → all. Part 4 editor safety → Task 3 (`build_editor_argv`) + Task 6 (`open_in_editor`). Part 5 settings → Tasks 4 + 6 + 8. Part 6 testing → Tasks 1-5 + Task 10 smoke. Out-of-scope items (col numbers, fenced-block refs, out-of-root, multi-editor, verifying explicit md links) are all honored.
- **Type consistency:** `ResolvedRef { raw, abs_path, is_markdown }` defined Task 5, consumed in Task 7 (`info.abs_path`, `info.is_markdown`, keyed by `r.raw`/`r.path`). `findReferences` → `{ raw, path, line, start, end }` (Task 1) used in Task 7. `parseHrefLine` → `{ path, line }` (Task 2) used in Task 7's handler. `build_editor_argv(template, file, line)` (Task 3) called in Task 6. `navigateToRef(absPath, line, isMarkdown)` defined + called consistently in Task 7. Preference serializes camelCase `editorCommand` (Task 6) and is read as `prefs.editorCommand` (Task 8).
- **IPC argument naming:** `resolve_references` takes `docDir`/`candidates`; `open_in_editor` takes `path`/`line`; `set_editor_command` takes `command` — matching the `#[tauri::command]` parameter names (Tauri maps camelCase JS keys to snake_case Rust params, so `docDir` → `doc_dir`).
- **Known v1 limitations (accepted, per spec):** explicit markdown links are not existence-verified (a broken `[x](missing.md)` errors on open); an editor program path containing spaces isn't supported (template split on whitespace); `:col` is parsed away but not used.
