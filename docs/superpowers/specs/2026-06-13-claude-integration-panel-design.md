# Claude Code Integration panel + first-run nudge

**Date:** 2026-06-13
**Status:** Approved (brainstorming) — pending implementation plan

## Goal

Make mdviewer's Claude Code integration **discoverable** and give users
**confidence it worked**. Today the hook and MCP server are installed from two
buried items in the MDViewer app menu, the payoff happens in a different app
(the terminal), and nothing in mdviewer signals whether either is set up. This
feature adds a dedicated **Claude Code Integration** window (explanation +
per-project install state + Install/Update buttons) and a conservative
**first-run nudge** that surfaces it when you open a git project without the
integration installed.

This is the fifth Claude-companion increment, after Review Mode, the hook
installer, reveal-in-tree, and the MCP server.

## Decisions (locked during brainstorming)

| Question | Decision |
|---|---|
| Surface | A new dedicated `claude-integration` webview window (same mechanism as the Settings window), opened by **MDViewer ▸ Claude Code Integration…**. |
| Status depth | **Install-state only** — does the current root have the hook / MCP entry. The live "Claude connected" indicator is deferred (the proxy connects per tool-call, not persistently). |
| Existing menu items | **Keep** Install Claude Code Hook… / Install MCP Server… working as-is; the panel is the richer primary surface. Consolidating them away is out of scope. |
| Nudge trigger | `isGitRepo && hook==not_installed && mcp==not_installed && !globallyDismissed`. The git signal is the "this is a code project" gate that keeps the nudge out of markdown-reader use. |
| Nudge dismissal | **Global, permanent** — one `localStorage` flag (the update-dismissal pattern). Dismiss once → never shown anywhere again; the menu item is always available. |
| Button semantics | **Install** when `not_installed`, **Update** when `installed` — mirrors the existing idempotent `merge_hook`/`merge_mcp_config` Installed/Updated behavior. |

**Out of scope (YAGNI):** live session/connection indicator; menu-item
state checkmarks on the two standalone installers; removing/consolidating those
two items; any change to what the hook or MCP server actually do.

---

## Part 1 — The window

A new webview window `claude-integration` (registered in
`capabilities/default.json` alongside `main`/`preferences`), built from
`ui/claude-integration.html` + `ui/claude-integration.js`, mirroring the
`preferences.*` pattern (external module only — CSP `script-src 'self'`).
Opened by a new menu item **MDViewer ▸ Claude Code Integration…**
(un-gated — shows on both platforms; placed in the MDViewer app submenu near
the existing install items).

Layout:

- **Target line** at the top: `Project: <current root path>`, so it's
  unambiguous which folder an install writes to. When no folder is open, the
  install rows are disabled with the note: *"Open a folder to set up
  integration for a project."*
- **Three explanatory rows**, each one plain-language sentence:
  - **Hook** — "Plans, specs, and designs Claude Code writes here open
    automatically." → status text + **Install**/**Update** button.
  - **MCP server** — "Lets Claude open documents in MDViewer and request
    reviews you send back inline." → status text + **Install**/**Update**
    button.
  - **Review Mode** — "Comment on any block, then copy your review or send it
    straight to a waiting Claude session." → explanatory only, no button
    (always available, nothing to install).
- After an Install/Update click, the row re-queries `integration_status` and
  refreshes its status text + button label in place.

The window calls the **existing** `install_claude_hook` / `install_mcp_server`
commands (which resolve the project via the process-global
`AppState.current_root`, so the separate window targets the same folder the
main window shows). On success it emits `integration-changed` (see Part 3).

---

## Part 2 — Install-state detection

A new read-only command:

```
integration_status() -> { hook: Status, mcp: Status }   // Status = "installed" | "not_installed"
```

computed against the current root:

- **hook:** read `<root>/.claude/settings.local.json`; `installed` if any
  `PostToolUse` hook `command` contains `--claude-hook` (the marker
  `merge_hook` keys on).
- **mcp:** read `<root>/.mcp.json`; `installed` if `mcpServers.mdviewer`
  exists.

Detection lives in **pure functions** beside the existing merge logic so it is
unit-testable without I/O:

- `claude_hook::hook_installed(settings: &Value) -> bool`
- `mcp::mcp_installed(config: &Value) -> bool`

The command reads the files (missing / empty / unparseable / wrong-typed all
read as `not_installed` — never an error), parses to `serde_json::Value`, and
calls the pure predicates. If no folder is open, the command returns both as
`not_installed` (the window shows the disabled state regardless).

Button label is derived (pure, frontend `statusButtonLabel(status)`):
`not_installed` → "Install", `installed` → "Update". Re-running an install when
already present updates the stored exe path — exactly today's behavior.

---

## Part 3 — The first-run nudge

When the frontend sets the sidebar root (`setTreeRoot`, and the cold-Finder
branch of `init`), it evaluates:

```
shouldNudge(isGitRepo, hookStatus, mcpStatus, dismissed) =
    isGitRepo && hookStatus === "not_installed"
              && mcpStatus === "not_installed" && !dismissed
```

- `isGitRepo` reuses the frontend's existing `gitRepoRoot` tracking (non-null
  when the current root is inside a git working tree).
- `hookStatus`/`mcpStatus` come from an `integration_status` call after the
  root is set.
- `dismissed` is a global `localStorage` flag
  (`mdviewer.integration.nudge_dismissed`), same pattern as the update
  dismissal.

`shouldNudge` is a **pure function** (`ui/integration.js`), unit-tested.

When true, show a banner in the existing top-banner area:

> 💬 Using Claude Code in this project? Set up MDViewer integration —
> **[Set up]** **[Dismiss]**

- **Set up** opens the `claude-integration` window.
- **Dismiss** sets the global flag → the nudge never appears again, anywhere.
- Installing from the window **or** the standalone menu items emits an
  `integration-changed` event; the main window re-evaluates `shouldNudge`
  (now false, since a piece is installed) and hides the banner.

The nudge cannot fire with no folder open (no git root). It is independent of
the per-version update banner and uses its own dismissal key.

---

## Part 4 — File layout & testing

**Rust**
- `claude_hook.rs` — add pure `hook_installed(&Value) -> bool` (+ unit tests).
- `mcp.rs` — add pure `mcp_installed(&Value) -> bool` (+ unit tests).
- `commands.rs` — add `integration_status` command (reads the two files for
  `current_root`, calls the predicates, returns the two statuses).
- `lib.rs` — register `integration_status`. The `integration-changed` event
  must fire after **every** successful install, regardless of entry point (the
  new window and both standalone menu items). Emitting it from within the
  `install_claude_hook` / `install_mcp_server` commands themselves is the
  cleaner seam (covers all callers uniformly); the implementation plan may
  instead emit from each frontend caller if that proves simpler — but coverage
  of all three paths is the requirement.
- `menu.rs` — new item id `claude-integration` titled "Claude Code
  Integration…", emitting a `menu-claude-integration` event; opens the window
  (mirrors `open_settings`).
- `capabilities/default.json` — add `claude-integration` to `windows`.

**Frontend**
- `ui/claude-integration.html` + `ui/claude-integration.js` — the window
  (mirrors `preferences.*`): loads `integration_status`, renders the three
  rows + target line, wires Install/Update buttons to the existing commands,
  refreshes on success, emits `integration-changed`.
- `ui/integration.js` — pure helpers `statusButtonLabel(status)` and
  `shouldNudge(isGitRepo, hookStatus, mcpStatus, dismissed)`.
- `ui/integration.test.js` — `node --test` units for both helpers.
- `ui/app.js` — open the window on `menu-claude-integration`; evaluate +
  render/hide the nudge banner on root change; listen for
  `integration-changed`.

**Tests**
- Rust unit: `hook_installed` and `mcp_installed` across installed / absent /
  empty / unparseable / wrong-typed inputs.
- JS unit: `statusButtonLabel` (both statuses); `shouldNudge` (the gating
  matrix — git vs non-git, each status, dismissed flag).
- Manual GUI smoke (both light/dark themes): nudge appears in a fresh git repo
  with nothing installed; **Set up** opens the window; Install flips the row
  status to Installed/Update and clears the banner; **Dismiss** silences the
  nudge and it stays gone after reopening folders; no-folder disabled state;
  the target line shows the right path.
