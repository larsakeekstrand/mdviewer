# Integration Explainers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add explain-before-acting copy so first-time users understand what the CLI-tool install and the Claude Code integration do before they commit.

**Architecture:** Two localized, low-risk changes — a native confirmation dialog before the macOS CLI install in `ui/app.js`, and richer explanatory copy (intro paragraph + per-feature descriptions + footnote styling) in the `ui/claude-integration.html` window. No Rust, no new Tauri commands, no new pure logic.

**Tech Stack:** Vanilla JS frontend (no build step), Tauri `dialogApi` (`window.__TAURI__.dialog`), plain HTML/CSS. Frontend is bundled at compile time, so changes require `cargo build` to appear.

## Global Constraints

- No new Tauri commands and no Rust changes — frontend only.
- CLI install is macOS-only; its menu item is already cfg-gated to macOS. Do not touch the Windows path.
- `dialogApi.ask(message, opts)` returns a boolean; `opts` supports `title`, `kind`, `okLabel`, `cancelLabel`. Existing call sites use `{ title: "MDViewer", kind: ... }`.
- Match existing copy voice: short, plain, second-person ("you can…").
- No persisted "don't ask again" state (YAGNI) — the explainer shows every time.
- Verify with `cargo build` (bundles `ui/`); copy/markup only, so no unit tests.

---

### Task 1: CLI install explainer dialog

**Files:**
- Modify: `ui/app.js` — `installCli()` (currently `ui/app.js:3899-3916`)

**Interfaces:**
- Consumes: `dialogApi.ask(message, { title, kind, okLabel, cancelLabel }) -> Promise<boolean>`; existing `invoke("install_cli")` and the existing success/error `dialogApi.message` calls (unchanged).
- Produces: nothing new for other tasks.

- [ ] **Step 1: Add the confirmation before invoking install_cli**

Replace the opening of `installCli()` so a `dialogApi.ask` gate runs before `invoke("install_cli")`. The full function becomes:

```javascript
async function installCli() {
  const proceed = await dialogApi.ask(
    "This adds an `mdviewer` command to your terminal, so you can open files with `mdviewer file.md` from any shell.\n\nmacOS will ask for your password to link it into /usr/local/bin.",
    {
      title: "Install Command Line Tool",
      kind: "info",
      okLabel: "Install",
      cancelLabel: "Cancel",
    },
  );
  if (!proceed) return;
  let outcome;
  try {
    outcome = await invoke("install_cli");
  } catch (e) {
    await dialogApi.message("Couldn't install the command line tool.\n\n" + e, {
      title: "MDViewer",
      kind: "error",
    });
    return;
  }
  if (outcome === "cancelled") return;
  const msg =
    outcome === "already_installed"
      ? "The mdviewer command line tool is already installed."
      : "Installed. You can now run mdviewer from a terminal.";
  await dialogApi.message(msg, { title: "MDViewer", kind: "info" });
}
```

- [ ] **Step 2: Build to bundle the frontend**

Run: `cd src-tauri && cargo build`
Expected: compiles clean, no warnings.

- [ ] **Step 3: Manual check**

Run the app (`cd src-tauri && cargo run -- ../README.md`), click **MDViewer ▸ Install Command Line Tool…**.
Expected: explainer dialog appears with **Install**/**Cancel**. **Cancel** closes it with no password prompt and no further dialog. (Don't necessarily complete the install; the gate behavior is what's under test.)

- [ ] **Step 4: Commit**

```bash
git add ui/app.js
git commit -m "Explain CLI install before the password prompt"
```

---

### Task 2: Claude Code Integration window copy

**Files:**
- Modify: `ui/claude-integration.html` (intro paragraph, per-feature text, `.touches` CSS)

**Interfaces:**
- Consumes: existing element ids `#project`, `#no-folder`, `#hook-status`, `#hook-btn`, `#mcp-status`, `#mcp-btn` — must remain unchanged (referenced by `ui/claude-integration.js`).
- Produces: nothing new for other tasks.

- [ ] **Step 1: Add the `.touches` footnote style**

In the `<style>` block, after the `.feature-desc` rule (currently `.feature-desc { opacity: 0.8; margin: 2px 0 0; }`), add:

```css
.touches {
  display: block; margin-top: 3px; opacity: 0.55;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 11px;
}
.intro { margin: 0 0 16px; opacity: 0.85; }
```

- [ ] **Step 2: Add the intro paragraph**

Immediately after the `<div class="no-folder" ...>…</div>` block (before the first `<div class="feature">`), insert:

```html
<p class="intro">
  Connect MDViewer to Claude Code so the docs Claude writes open here
  automatically, and Claude can ask you to review them. Optional — set up
  per project.
</p>
```

- [ ] **Step 3: Enrich the Hook description**

Replace the Hook feature's `<p class="feature-desc">…</p>` with:

```html
<p class="feature-desc">
  When Claude Code writes a plan, spec, or design doc in this project,
  MDViewer opens it automatically — so you read it here while Claude keeps
  working.
  <span class="touches">Edits .claude/settings.local.json</span>
</p>
```

- [ ] **Step 4: Enrich the MCP server description**

Replace the MCP server feature's `<p class="feature-desc">…</p>` with:

```html
<p class="feature-desc">
  Lets Claude open documents in MDViewer and request reviews you send back
  inline, without leaving your terminal.
  <span class="touches">Adds an entry to .mcp.json</span>
</p>
```

- [ ] **Step 5: Clarify the Review Mode description**

Replace the Review Mode feature's `<p class="feature-desc">…</p>` with:

```html
<p class="feature-desc">
  Comment on any rendered block, then copy your review or send it straight to
  a waiting Claude session.
  <span class="touches">No setup needed</span>
</p>
```

- [ ] **Step 6: Build to bundle the frontend**

Run: `cd src-tauri && cargo build`
Expected: compiles clean.

- [ ] **Step 7: Manual check (light + dark)**

Run the app, open **MDViewer ▸ Claude Code Integration…**.
Expected: intro paragraph renders under the project line; each feature shows the richer sentence with a dimmed monospace footnote; Install/Update buttons and statuses still work (ids unchanged). Toggle OS/app theme and confirm the footnote stays legible in dark mode.

- [ ] **Step 8: Commit**

```bash
git add ui/claude-integration.html
git commit -m "Explain what each Claude Code integration does"
```

---

## Self-Review

**Spec coverage:**
- Change 1 (CLI explainer, macOS-only, ask before invoke, Cancel returns early, existing dialogs untouched) → Task 1. ✓
- Change 2 (top intro, richer per-feature copy with touched file, `.touches` footnote style) → Task 2 steps 1–5. ✓
- Testing (cargo build, manual light/dark) → Task 1 steps 2–3, Task 2 steps 6–7. ✓
- Non-goals (no welcome window, no info popovers, no nudge change, no Windows CLI explainer) → respected; nothing in either task touches them. ✓

**Placeholder scan:** No TBD/TODO; all copy and code shown in full. ✓

**Type/id consistency:** Element ids (`#hook-btn`, `#mcp-btn`, `#hook-status`, `#mcp-status`, `#project`, `#no-folder`) are preserved — only `<p class="feature-desc">` bodies and surrounding non-id markup change, so `claude-integration.js` keeps working. `dialogApi.ask` options match existing usage plus documented `okLabel`/`cancelLabel`. ✓
