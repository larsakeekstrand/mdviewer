# What's New modal for the update banner

**Date:** 2026-05-25
**Status:** Approved (design)

## Problem

When the in-app update banner announces a new release, the user can decide to
**Update now** or **Dismiss**, but has no way to see *what changed* without
leaving the app. The only path to the changelog today is the **View release**
button, which opens the GitHub release page in an external browser. The user
wants to read the changes in-app before deciding whether to update.

## Key facts (from exploration)

- `updaterApi.check()` returns an `Update` object whose `body` field is the
  release notes markdown — populated from the `notes` field of `latest.json`.
  The notes are therefore **already available client-side; no extra network
  request is needed.**
- The published `notes` bundle install instructions, the quarantine command, an
  "Updating" section, **and** a `## Changes` changelog (commit subjects since the
  previous tag). Only the `## Changes` part is relevant for deciding to update.
- `markdown::render_markdown(source: &str, theme: &str) -> String` already exists
  (used by the `render_file` command) — the trusted comrak pipeline with
  `render.unsafe = false`.
- The existing markdown link interception is scoped to `#preview`
  (`preview.addEventListener("click", …)` + `preview.contains(a)`), so a modal
  outside `#preview` needs its own link handling.
- Pure frontend helpers are unit-tested with `node --test` via files like
  `ui/update.test.js`, `ui/export.test.js`, `ui/search.test.js`.

## Decisions (from brainstorming)

1. **Display:** an in-app **modal overlay** with rendered markdown that dims the
   app behind it, carrying its own **Update now** and **Close** buttons.
2. **Content:** show **only the `## Changes` changelog**, stripping the
   install/quarantine/updating boilerplate; fall back to the full notes body if
   the `## Changes` heading is absent.
3. **View release:** **replace** the standalone "View release" banner button;
   the modal instead includes a **"View full release notes →"** link to the
   GitHub release page.

## Approach

Render the changelog server-side through the existing comrak pipeline via a new
thin Tauri command, and present the HTML in a modal. This reuses the same
trusted, escaped rendering and syntax highlighting as the main preview with no
new dependencies.

Rejected alternatives:
- **Render markdown in JS** — no JS markdown renderer is vendored; would add a
  dependency or a hand-rolled parser.
- **Show raw/plain text** — the user chose rendered markdown (modal over the
  native plain-text dialog).

## Components & changes

### Backend
- `src-tauri/src/commands.rs`: add
  `render_notes(source: String, theme: Option<String>) -> Result<String, String>`
  — wraps `markdown::render_markdown(&source, theme.as_deref().unwrap_or("light"))`
  and returns `Ok(html)`.
- `src-tauri/src/lib.rs`: register `render_notes` in the `invoke_handler`.

### Frontend — pure helper (testable)
- `ui/update.js`: add `extractChangelog(body)`:
  - Returns the text **after** the `## Changes` heading (heading line dropped),
    trimmed.
  - If `## Changes` is not found, returns the **full body**, trimmed.
  - If `body` is empty/`null`/`undefined`, returns `""`.
  - No DOM/Tauri imports (stays `node --test`-compatible).

### Frontend — UI
- `ui/index.html`:
  - Replace the banner's `update-banner-view` ("View release") button with
    `update-banner-whatsnew` ("What's new").
  - Add a modal overlay: a dimmed backdrop plus a centered dialog
    (`role="dialog"`, `aria-modal="true"`, labelled by its title) containing:
    - a title element (`What's new in <version>`),
    - a scrollable content area with class `markdown-body`,
    - a footer: **Update now** (primary), **Close**, and a
      **"View full release notes →"** link.
- `ui/styles.css`: overlay + dialog styles; dim/scrim backdrop; centered box
  with `max-width` and `max-height` + internal scroll; light/dark via the
  existing CSS variables. Hidden by default (`hidden`), so it never appears in
  PDF/HTML export (which re-renders `#preview` only).
- `ui/app.js`:
  - Element refs for the modal, its title, content area, Update-now button,
    Close button, the "View full release notes" link, and the new banner
    `What's new` button.
  - `openNotesModal(update)`:
    1. `const md = extractChangelog(update.body);`
    2. if `md` is empty → set content to a "No release notes available." message;
       else `invoke("render_notes", { source: md, theme: currentTheme })` and
       set the content area's HTML to the result.
    3. set the title to `What's new in ${update.version}`.
    4. wire the footer (see below), show the overlay, move focus into the dialog.
  - `closeNotesModal()`: hide the overlay, restore focus to the triggering
    button.
  - Dismissal: **Esc** key and **backdrop click** both call `closeNotesModal()`.
  - Footer wiring:
    - **Update now** → `closeNotesModal()` then `runUpdate(update)` (the banner
      then shows download progress exactly as today).
    - **Close** → `closeNotesModal()` (banner stays).
    - **View full release notes →** → `invoke("open_url", { url:
      releaseUrlFor(REPO, update.version) })`.
  - Modal link interception: a `click` handler on the modal content area that
    intercepts `a[href]`, calls `preventDefault()`, and opens external
    `http(s)` links via `open_url` (the changelog's "Commits since [vX](…)"
    link). Mirrors the `#preview` handler but scoped to the modal.
  - Banner rewiring: `setUpdateButtons({ whatsNew })` replaces the `view` flag.
    Both `showUpdateAvailable` (available state) and the **update-failed** branch
    of `runUpdate` offer **What's new** instead of **View release** (the modal
    carries the GitHub link).
  - `render_notes` is passed the module-level `currentTheme` variable (the same
    value `app.js` already passes to `render_file`).

## Data flow

```
check() → banner: [What's new] [Update now] [Dismiss]
   └ click What's new
       → extractChangelog(update.body)
       → render_notes(source, theme)   (skipped if empty)
       → modal shows rendered changelog
           ├ Update now → closeNotesModal() → runUpdate(update) → banner progress
           ├ Close      → closeNotesModal()
           └ View full release notes → open_url(GitHub release page)
```

## Edge cases

- **Empty/missing notes:** modal shows "No release notes available."; the
  button still opens the modal.
- **`## Changes` absent (format drift):** show the full body rather than
  nothing.
- **Security:** rendered HTML comes from the trusted comrak pipeline
  (`render.unsafe = false`); CSP `script-src 'self'` already neutralizes any
  injected script. **No CSP change required.** External links route through the
  existing `open_url` command (restricted to `http(s)`).

## Testing

- `ui/update.test.js` (`node --test`): `extractChangelog` —
  - extracts the section after `## Changes`,
  - falls back to the full body when `## Changes` is absent,
  - returns `""` for empty/`null`/`undefined`,
  - trims surrounding whitespace.
- Rust: one happy-path test for `render_notes` (it is a thin wrapper over the
  already-exercised `markdown::render_markdown`).
- Lint/build gate before commit: `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, and
  `node --test ui/` for the JS helpers.
- Manual: `cd src-tauri && cargo run -- ../README.md`, trigger the menu-driven
  update check against a published newer release, click **What's new**, verify
  the rendered changelog, the three footer actions, Esc/backdrop dismissal, and
  light/dark styling.

## Out of scope

- Improving the *quality* of the changelog text (it is raw commit subjects). The
  changelog content is produced by `release.yml` from commit messages; making it
  more user-facing is a separate release-process concern.
- Decoupling `latest.json` `notes` from the GitHub release body in the release
  workflow. This design strips boilerplate client-side instead.
