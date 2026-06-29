# Explain-before-acting for CLI tool & Claude integration

**Date:** 2026-06-29
**Status:** Approved

## Problem

First-time users don't understand what **MDViewer ▸ Install Command Line
Tool…** and **MDViewer ▸ Claude Code Integration…** do before they act:

- The CLI install fires immediately. On macOS it pops an admin password
  prompt with no explanation of what it installs or why.
- The Integration window shows only a terse one-line description per feature
  and no overall "what is this / who is it for" framing.

## Goal

Add lightweight, in-context "explain-before-acting" copy. No new windows, no
new Tauri commands, no Rust logic changes.

## Change 1 — CLI install explainer (macOS-only)

The **Install Command Line Tool…** menu item is already cfg-gated to macOS,
so this lives entirely in `installCli()` (`ui/app.js`).

Before `invoke("install_cli")`, show a native confirmation via
`dialogApi.ask(...)`:

- Body: what it does ("adds an `mdviewer` command to your terminal, so you can
  open files with `mdviewer file.md` from any shell") plus the heads-up that
  macOS will ask for a password to link it into `/usr/local/bin`.
- Buttons: **Install** (ok) / **Cancel**. Cancel → return early; no password
  prompt, no further dialogs.

The existing success / already-installed / error `dialogApi.message` calls are
unchanged. Shown every time the item is clicked — it's a rare, once-ever
action, so no persisted "don't ask again" state (YAGNI).

## Change 2 — Claude Code Integration window copy

Pure HTML/CSS in `ui/claude-integration.html`. No change to
`claude-integration.js` or any command.

- **Top intro paragraph** under the project line: one short blurb framing the
  integration as optional and per-project — e.g. "Connect MDViewer to Claude
  Code so the docs Claude writes open here automatically, and Claude can ask
  you to review them. Optional — set up per project."
- **Richer per-feature descriptions**, each a clear sentence plus the concrete
  file it touches, styled as a dimmed footnote:
  - **Hook** — auto-opens plans/specs/designs Claude writes; *edits
    `.claude/settings.local.json`*.
  - **MCP server** — lets Claude open docs and request inline reviews without
    leaving the terminal; *adds an entry to `.mcp.json`*.
  - **Review Mode** — always on, no setup; comment on any block and copy or
    send the review back to a waiting Claude session; *no setup needed*.
- Minor CSS: a `.touches` class (small, monospace, dimmed) for the file-path
  footnote so it reads as a note, not body text.

## Non-goals

- No welcome/onboarding window.
- No inline `ⓘ` info popovers.
- No changes to the first-run nudge banner (it already points to the window).
- No Windows-side CLI explainer (Windows uses the NSIS "Add to PATH" checkbox;
  there is no in-app CLI install on Windows).

## Testing

- `cargo build` to bundle the frontend (Tauri bundles `ui/` at compile time).
- Manual: click **Install Command Line Tool…** → explainer appears → Cancel
  aborts cleanly; Install proceeds to the existing flow. Open **Claude Code
  Integration…** → intro + richer copy render in both light and dark.
- No new unit tests (copy/markup only; no new pure logic).
