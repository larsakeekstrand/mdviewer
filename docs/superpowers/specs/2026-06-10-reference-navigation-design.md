# Reference Navigation: click file references in a rendered doc to open/jump

**Date:** 2026-06-10
**Status:** Approved (brainstorming) — pending implementation plan

## Goal

Make file references inside a rendered markdown document navigable. When reading
a Claude Code plan/report in mdviewer, a reference like `src-tauri/src/commands.rs:42`
or a link `[spec](docs/spec.md:88)` becomes clickable: markdown/text targets open
in mdviewer and jump to the line; code targets hand off to the user's editor at
the line. This is the third Claude-companion feature (after Review Mode and the
hook installer) and the "navigation" option scoped — but not built — during the
Review Mode brainstorm.

It reuses two existing seams: the folder-search **jump-to-line** machinery
(`openTabAtLine` → `pendingJumpLine` → `jumpToLine` → `findElementForLine`) and
the existing **relative-link click handling** in `#preview`.

## Decisions (locked during brainstorming)

| Question | Decision |
|---|---|
| Target routing | **Markdown/text** target → open in mdviewer + jump to line (reuse `openTabAtLine`). **Code** target → hand off to the editor at the line. |
| Editor selection | **Configurable command** in Settings, default `code -g {file}:{line}` (VS Code). `{file}`/`{line}` substituted as discrete **argv** elements (never shell-interpolated). |
| What is clickable | **Both** bare `path:line` text (autolinked) **and** markdown links (extended to honor a trailing `:line`). |
| False-positive control | **Verify the file exists** before linkifying a bare reference; only verified, in-root refs become links. |
| Resolution base | Resolve relative to the **document's directory** first, then the **tree root** (repo root). |
| Containment | Resolved paths confined **within the open tree root** (canonicalized, component-wise) — out-of-root refs are not linkified or opened. |

**Out of scope (YAGNI):** column numbers (`file:42:8` — parse line, ignore col);
linkifying refs inside fenced code blocks; absolute/out-of-root paths;
per-extension or multiple editors; existence-verifying explicit markdown links.

---

## Why these decisions

**Markdown jumps in-app, code hands off.** mdviewer renders and line-maps
markdown (via `data-sourcepos`); it is not a code editor. So markdown targets
get the full open-and-jump treatment the app already supports, while code
references go where the user actually works — their editor — at the right line.

**Configurable editor, argv substitution.** A single template covers any editor
(`code`, `cursor`, `subl`, `idea`). Substituting `{file}`/`{line}` as discrete
argv elements (not into a shell string) means an untrusted path from a Claude
doc cannot inject a command — the security-critical property of this feature.

**Verify existence before linkifying bare refs.** Bare-text autolinking risks
false positives. Requiring the file to exist (and a path-shaped token) keeps the
document clean — only real references light up. Explicit markdown links are
author-intentional, so they are not existence-gated (open-then-error as today).

**Containment within the tree root.** Markdown may be untrusted. Confining
resolved references to the open tree root (the file-op security model) prevents
a doc from turning `/etc/passwd:1` into a one-click open.

---

## Part 1 — User-facing behavior

In a rendered markdown tab:

- **Bare references** in prose and inline `<code>` spans — `src/commands.rs:42`,
  `ui/app.js`, `tasklist.rs:128` — are detected and, if the file exists, rendered
  as links (a subtle `.ref-link` style).
- **Markdown links** keep working and honor a trailing `:line`:
  `[spec](docs/spec.md:88)`.
- **Click routing:**
  - Markdown/text target → `openTabAtLine(absPath, line)` opens (or focuses) the
    tab and jumps+pulses the line. No line → plain open (current behavior).
  - Code target → `open_in_editor(absPath, line)` launches the configured editor.
- **Settings** gains *"Editor command for code references"* (default
  `code -g {file}:{line}`).
- Failures use the transient banner (`showTransientError`), never `showError`
  (which clears the preview) and never a crash.

## Part 2 — Detection, verification, resolution

- **Pattern (pure):** `ui/refs.js::findReferences(text)` returns
  `[{ raw, path, line, start, end }]`. A candidate is a token that **contains a
  `/` OR ends in a known file extension**, optionally followed by `:<digits>`.
  Rejects timestamps (`12:34`), URL schemes (`http://…`), ratios (`3:1`), and
  bare words. A trailing `:col` after `:line` is consumed but ignored.
- **Scan scope:** text nodes within prose and inline `<code>`; **excluded:**
  inside `<pre>` (fenced code blocks) and inside existing `<a>`.
- **Verification (one IPC per render):** the frontend collects candidates and
  calls `resolve_references(docDir, root, candidates)` once; the backend resolves,
  checks existence, confines within root, and returns
  `ResolvedRef { raw, abs_path, is_markdown }` for the survivors. Only these are
  linkified. Idempotent across live-reload (re-runs each `postRender`, like the
  copy-button / review-marker hooks).
- **Resolution base:** for each candidate, try `docDir.join(path)` then
  `root.join(path)`; first existing wins. Markdown detection mirrors the existing
  `MD_EXT`/`isImagePath` convention (`.md`/`.markdown` → markdown, else code).

## Part 3 — Architecture & file layout

```
ui/
  refs.js        — NEW pure: findReferences(text), parseHrefLine(href) (node --test)
  app.js         — linkifyReferences() postRender hook; .ref-link click handler;
                   navigateToRef(); extend existing md-link handler to honor :line
  preferences.html / preferences.js — "Editor command" field (load/save)
  styles.css     — .ref-link styling (light + dark)
src-tauri/src/
  editor.rs      — NEW pure: build_editor_argv(template, file, line) -> Vec<String>
                   (+ #[cfg(test)] injection-safety tests)
  commands.rs    — resolve_references(...) -> Vec<ResolvedRef>; open_in_editor(path, line)
  recent.rs      — persist editor_command (alongside channel / last_folder), with default
  lib.rs         — register the two commands
```

**Flow:** `postRender(t)` → `linkifyReferences(t)`:
1. Walk `#preview` text nodes, skipping `<pre>` and `<a>` subtrees.
2. `findReferences` on each → candidate list (dedup raw strings).
3. `invoke("resolve_references", { docDir: parentDir(t.path), root: treeRoot, candidates })`.
4. For each text node, replace verified candidate substrings with
   `<a class="ref-link" data-path="<abs>" data-line="<n|''>" data-kind="md|code">`.

`navigateToRef(absPath, line, isMarkdown)` (single funnel):
- `isMarkdown` → `openTabAtLine(absPath, line)` (or `openPreview(absPath)` if no line).
- else → `invoke("open_in_editor", { path: absPath, line: line ?? 1 })`.

Both the `.ref-link` click handler and the extended markdown-link handler
resolve to `(absPath, line, isMarkdown)` and call `navigateToRef`. The markdown
handler uses `parseHrefLine` to split a trailing `:line` before resolving.

## Part 4 — Editor launch (injection safety)

`commands::open_in_editor(path, line)`:
1. Confine `path` within the tree root (reject otherwise) — defense in depth.
2. Read the editor template from settings (default `code -g {file}:{line}`).
3. `editor::build_editor_argv(template, file, line)`:
   - Split the template on whitespace → `[program, args…]`.
   - Substitute `{file}`/`{line}` **within each token** so `{file}:{line}`
     becomes one argv element `"/abs/path:42"`.
   - Return `Vec<String>`; the command is run `Command::new(program).args(rest)`
     with stdio detached, fire-and-forget. **No shell.**
4. A spawn failure surfaces via the transient banner.

`build_editor_argv` is pure and the security seam, so it is unit-tested,
including: a malicious `file = "; rm -rf ~"` with template `code -g {file}:{line}`
yields `["code", "-g", "; rm -rf ~:42"]` — the junk remains one inert argv
element (no split, no shell), proving injection safety.

## Part 5 — Settings

- A persisted `editor_command: String` is added to the settings store
  (`recent.json`, alongside `channel` and `last_folder`), defaulting to
  `code -g {file}:{line}` when absent.
- `preferences.html`/`preferences.js` gain a labeled text input that loads the
  current value and saves on change (mirroring the existing channel control).
- `commands::open_in_editor` reads the value per call (no relaunch needed).

## Part 6 — Pure functions & testing

`node --test` (mirrors `editor.js` / `search.js` / `review.js`):

- `findReferences(text)` — yes: `src/foo.rs:42`, `foo.rs`, `a/b/c.md:1`,
  `tasklist.rs:128`; no: `12:34`, `http://x.com`, `3:1`, `word`, `key: value`.
  Asserts `{raw, path, line, start, end}` with correct offsets and that two refs
  in one string are both found in order.
- `parseHrefLine(href)` — `"spec.md:42"` → `{path:"spec.md", line:42}`;
  `"spec.md"` → `{path:"spec.md", line:null}`; `"a/b.md:0"` and trailing-colon
  edge cases.

Rust `#[cfg(test)]`:

- `editor::build_editor_argv` — substitution, `{file}:{line}` one-element rule,
  injection-safety case, a template with a different editor
  (`idea --line {line} {file}`), and a template lacking `{line}`.
- `commands::resolve_references` — temp-dir tests: resolves via doc-dir then
  root; marks markdown vs code by extension; drops non-existent; drops
  out-of-root (e.g. `../../etc/x`); returns absolute canonical paths.

Manual GUI smoke test (the part tests can't cover): a doc containing a real
markdown ref with `:line`, a real code ref with `:line`, a bare existing file,
and a fake `nope.rs:9`. Confirm: md ref jumps in-app; code ref opens the editor
at the line; bare file opens; fake ref stays plain text. Dark-mode the
`.ref-link` styling.

## Build reminder

Frontend (`ui/*`) and Rust changes both require `cargo build` to rebundle.
Smoke-test by opening a doc with real references against this repo.
