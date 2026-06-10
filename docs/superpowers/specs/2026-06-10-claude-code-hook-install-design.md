# Install Claude Code Hook: auto-open Claude's plan files in MDViewer

**Date:** 2026-06-10
**Status:** Approved (brainstorming) — pending implementation plan

## Goal

Let a user, with one menu click, configure the currently-open project so that
when Claude Code writes a plan/spec/design markdown file there, the file opens
automatically in MDViewer. This closes the read-in-mdviewer / act-in-terminal
loop alongside the existing **Review Mode**: Claude writes a plan → it pops open
in MDViewer → you read and annotate it → **Copy Review** back to Claude.

The integration is a *product feature* of the app (a menu item), not a manual
config step. It composes two seams MDViewer already has: the cross-platform CLI
binary, and the `RunEvent::Opened` warm-open path that adds a tab to the running
instance.

## Decisions (locked during brainstorming)

| Question | Decision |
|---|---|
| What triggers an open | **Filename-based, anywhere**: any markdown whose filename stem (case-insensitive) contains `plan`, `spec`, or `design`. Works in any project, not just superpowers layouts. |
| Where the hook is written | **`<project>/.claude/settings.local.json`** — personal, machine-specific, gitignored by Claude Code's defaults. Never the committed `settings.json`. |
| How the hook does its work | **Self-hook**: the hook command is the absolute path to MDViewer's own binary + `--claude-hook`. MDViewer reads the PostToolUse JSON from stdin, matches, and opens the file. No `jq`, no shell script, no `chmod`; cross-platform; logic is pure/testable Rust. |
| Hook event / matcher | **`PostToolUse`** matching the **`Write`** tool only (fires on file creation, not every edit). |
| Idempotency | Re-running updates the existing MDViewer hook's path and reports "already installed"; never duplicates; preserves unrelated hooks. |
| Platform | Menu item on **both** macOS and Windows. macOS adds a tab (warm open); Windows opens a new window (pre-existing non-single-instance behavior). |

**Out of scope (YAGNI):** an uninstall menu item (removal is a manual edit;
re-install is idempotent), configurable match patterns, opening non-plan files,
committed-`settings.json` installs, Windows single-instance/tab behavior.

---

## Why these decisions

**Self-hook over shell script.** A `.sh` + `jq` hook is the common Claude Code
idiom, but it depends on `jq`, puts match logic in untested shell, needs
`chmod`, and a `.sh` does not run on the Windows build — so the menu item would
install a broken hook there. Routing through MDViewer's own binary removes every
one of those problems and makes the match logic a pure, unit-tested Rust
function, consistent with `fs_ops.rs` / `tasklist.rs`.

**Absolute exe path, not a PATH symlink.** Using `std::env::current_exe()` means
the hook works whether or not the user has run *Install Command Line Tool…*, and
survives the app being located anywhere.

**`settings.local.json`, not `settings.json`.** The hook command launches *this
machine's* MDViewer via `open -a`; it is inherently a personal preference and
must not be imposed on collaborators (or break on their Linux box) by being
committed.

**`Write` matcher only.** Plans are created once via `Write`; later tweaks come
through `Edit`. Matching only `Write` opens the tab on creation without
re-popping on every edit. Re-opening an already-open path focuses the existing
tab (`findTab`), so even a second `Write` is harmless.

---

## Part 1 — User-facing behavior

A new menu item **MDViewer ▸ Install Claude Code Hook…**, placed next to
*Install Command Line Tool…* in the app menu. On click (mirroring the
`menu-install-cli` → frontend → command flow):

1. The frontend reads the current sidebar root (`treeRoot`). If none is open, it
   shows a dialog: *"Open a folder first to install the hook there."* and stops.
2. Otherwise it calls `invoke("install_claude_hook", { root })`.
3. The command writes/merges the hook and returns an `Outcome`.
4. The frontend shows a result dialog:
   - Installed: *"Claude Code hook installed in `<folder>`. Plan, spec, and
     design files Claude writes here will now open in MDViewer."*
   - Updated/already present: *"Already installed — updated the MDViewer path."*
   - Error: the error message (e.g. malformed existing settings).

## Part 2 — Architecture: two halves

New file **`src-tauri/src/claude_hook.rs`** holds the pure, unit-tested pieces;
`commands.rs` and `main.rs` are thin wrappers.

### Half A — the installer

`commands::install_claude_hook(root: String) -> Result<HookOutcome, String>`
(registered in `lib.rs`, like the other commands):

1. Validate `root` is an existing directory (else `Err`).
2. `current_exe()` → quote it → `command = "\"<exe>\" --claude-hook"`.
3. Ensure `<root>/.claude/` exists (`create_dir_all`).
4. Read `<root>/.claude/settings.local.json` → parse to `serde_json::Value`; if
   the file is absent, start from `Value::Object({})`; if present but unparseable,
   return `Err` (do **not** overwrite a file we can't understand).
5. `let (merged, outcome) = claude_hook::merge_hook(settings, &command);`
6. Serialize `merged` pretty-printed and write atomically via the shared
   `write_atomically` helper.
7. Return `outcome` (`Installed` | `Updated`).

`HookOutcome` serializes to the frontend so the dialog text can branch.

### Half B — the hook runtime (`--claude-hook`)

In `main.rs`, **before** building/launching the Tauri app, detect the flag:

```rust
if std::env::args().any(|a| a == "--claude-hook") {
    claude_hook::run_hook();   // reads stdin, opens if matched, never returns to GUI
    return;
}
```

`claude_hook::run_hook()`:

1. Read all of stdin to a `String`.
2. `extract_file_path(&stdin)` → `Option<String>`. `None` → return (exit 0).
3. `is_plan_file(&path)` false → return (exit 0).
4. Open the file in MDViewer:
   - macOS: `open -a <MDViewer.app> "<path>"` — derive the `.app` bundle from
     `current_exe()` (strip `/Contents/MacOS/<bin>`). Falls back to `open -b
     com.mdviewer.app` if the bundle path can't be derived.
   - Windows: launch the binary with the path as argv (the path `main.rs`
     already handles).
5. Any failure is swallowed (log to stderr, exit 0). The hook must **never**
   exit non-zero or block — that would disrupt Claude's tool call.

## Part 3 — The hook config written

Merged into `<project>/.claude/settings.local.json`:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write",
        "hooks": [
          {
            "type": "command",
            "command": "\"/Applications/MDViewer.app/Contents/MacOS/mdviewer\" --claude-hook"
          }
        ]
      }
    ]
  }
}
```

`merge_hook(settings, command)` rules (pure function on `serde_json::Value`):

- Navigate/create `settings.hooks.PostToolUse` as an array, preserving any other
  keys and any existing hook entries.
- Scan all `PostToolUse[].hooks[]` for a `command` string containing
  `--claude-hook`:
  - **Found** → set that command to the new `command` value, return `Updated`.
  - **Not found** → append a new matcher object
    `{ "matcher": "Write", "hooks": [{ "type": "command", "command": <command> }] }`,
    return `Installed`.
- Return `(merged_value, outcome)`.

## Part 4 — Cross-platform behavior

The match + stdin parsing are platform-neutral. Only the open step differs:

- **macOS:** `open -a` → `RunEvent::Opened` → adds a tab to the running window
  (or cold-launches). This is the warm-open path already built and documented.
- **Windows:** launching with argv opens the file; because the app is not
  single-instance on Windows, each open spawns a **new window**. Accepted as-is
  for v1; noted so it isn't mistaken for a regression.

## Part 5 — Pure functions & testing

`#[cfg(test)]` unit tests in `claude_hook.rs` (no I/O), matching the project's
pure-helper convention:

- `is_plan_file(path: &str) -> bool` — true when the filename stem
  (case-insensitive) contains `plan`, `spec`, or `design` **and** the extension
  is `md` or `markdown`. Cases:
  - `migration-plan.md` → true
  - `auth-design.md` → true
  - `api-spec.markdown` → true
  - `SPEC.MD` → true (case-insensitive)
  - `docs/plans/2026-01-01-foo.md` → false (stem `2026-01-01-foo` has none of the
    keywords) — **NOTE:** filename-based, so a superpowers plan named without a
    keyword won't match; this is the accepted trade-off of choosing filename
    matching over folder matching.
  - `README.md` → false
  - `plan.txt` → false (wrong extension)
- `extract_file_path(stdin_json: &str) -> Option<String>` — parses JSON, returns
  `tool_input.file_path` as a `String`; `None` on malformed JSON or missing key.
  Cases: valid PostToolUse Write payload → `Some(path)`; `{}` → `None`; non-JSON
  → `None`.
- `merge_hook(settings: Value, command: &str) -> (Value, HookOutcome)` — cases:
  - empty `{}` → adds full `hooks.PostToolUse` chain, `Installed`.
  - settings with an unrelated `PostToolUse` entry → appended, original kept,
    `Installed`.
  - settings already containing a `--claude-hook` command → that command's path
    updated, no new entry, `Updated`.
  - settings with unrelated top-level keys (e.g. `permissions`) → preserved.

Integration (manual, can't be unit-tested): install in a scratch project, have
Claude write a `*-plan.md`, confirm the tab opens in the running MDViewer;
re-run the menu item and confirm the "already installed" dialog and no duplicate
entry in `settings.local.json`.

## Part 6 — File layout & touch points

```
src-tauri/src/
  claude_hook.rs   — NEW: is_plan_file, extract_file_path, merge_hook (pure,
                     tested) + run_hook (stdin → open) + HookOutcome enum
  main.rs          — early --claude-hook branch before GUI launch
  commands.rs      — install_claude_hook command (thin wrapper over merge_hook
                     + atomic write); reuse write_atomically
  lib.rs           — register install_claude_hook; add claude_hook module
  menu.rs          — "Install Claude Code Hook…" item + emit menu-install-claude-hook
ui/
  app.js           — listen("menu-install-claude-hook") → invoke install_claude_hook
                     with treeRoot, show result/error dialog (mirror install-cli)
```

## Build reminder

Frontend (`ui/app.js`) and Rust changes both require `cargo build` to rebundle.
After implementing, smoke-test by installing into a real project and watching a
Claude-written plan open.
