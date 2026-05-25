# Light/Dark theme toggle button

**Date:** 2026-05-25
**Status:** Approved (design)

## Problem

The app follows the macOS appearance setting and offers no way to override it.
A user on a dark desktop who wants to read a document in light mode (or vice
versa) has to change their whole OS appearance. The user wants an in-app toggle
— "a button next to the Raw button" — to switch between light and dark.

## Key facts (from exploration)

- The theme currently lives in **three** places, all keyed off
  `prefers-color-scheme`:
  1. **App chrome** (`ui/styles.css`): `:root` light defaults plus
     `@media (prefers-color-scheme: dark)` blocks for the core vars, the git
     decoration badges, and the find highlights; plus one
     `@media (prefers-color-scheme: light)` block for the context-menu vars
     (whose defaults are dark).
  2. **Markdown body** (`ui/github-markdown.css`, vendored): ~60 color vars
     defined inside `@media (prefers-color-scheme: dark|light)` blocks. The
     blocks already list `[data-theme="dark"]` / `[data-theme="light"]`
     selectors, but **nested inside the media query**, so they only *match* the
     OS setting — they cannot *override* it.
  3. **JS** (`ui/app.js`): `currentTheme = colorScheme()` (from `matchMedia`),
     passed to the backend `render_file` command for **syntect** server-side
     highlighting (`InspiredGitHub` light vs `base16-ocean.dark`), used for the
     **Mermaid** theme (`mermaidTheme`/`initMermaid`), and for `render_notes`.
     A `matchMedia("(prefers-color-scheme: dark)")` `change` listener re-renders
     the active tab on OS appearance change.
- The toolbar holds one button today, `#toggle-raw`, whose label follows a
  "label = what clicking does" convention (`textContent = t.raw ? "Rendered" :
  "Raw"`). Theme is **global**, not per-tab (unlike `raw`).
- Persisted UI preferences use `localStorage` (e.g.
  `localStorage["mdviewer.update.dismissed_version"]`).
- **Export** inlines only `github-markdown.css` (via `forceLightCss`) + KaTeX
  CSS + `EXPORT_PAGE_CSS`; it does **not** include `styles.css`.
  `exportDocument` already snapshots and forces `currentTheme = "light"` for the
  re-render, restoring in a `finally`. HTML export's `<html>` carries no
  `data-theme`. PDF export reuses the on-screen render through `@media print`.
- CSP is `script-src 'self'` — no inline `<script>`, so the usual
  inline-head theme bootstrap is not available.

## Decisions (from brainstorming)

1. **Two-state toggle (Light ⇄ Dark).** No "System" entry in the cycle.
2. **Follow-then-stick.** Until the user clicks the button, the app follows
   macOS appearance live (current behavior). The first click writes an explicit
   preference; from then on the app ignores OS appearance changes.
3. **Single source of truth = JS + a `data-theme` attribute.** JS owns the
   effective theme and writes it to `document.documentElement.dataset.theme`.
   All CSS becomes attribute-driven; no `prefers-color-scheme` query remains in
   the app's own stylesheets.
4. **Attribute-driven CSS, not a layered override.** Convert the existing media
   blocks to `[data-theme=...]` selectors rather than duplicating ~70 variable
   values on top of the media queries (rejected: unmaintainable in two no-build
   CSS files).
5. **Button presentation.** An icon-only `toolbar-btn` left of Raw, showing the
   *target* theme (moon ☾ while light → switch to dark; sun ☀ while dark →
   switch to light), with `title` / `aria-label` ("Switch to dark theme" /
   "Switch to light theme"). Not an `aria-pressed` toggle (theme is a 2-way
   switch, not on/off), so it does not take the pressed blue styling.

## Design

### Frontend (`ui/app.js`)

- Add `THEME_KEY = "mdviewer.theme"`.
- `storedTheme()` → returns `"light"`/`"dark"` from `localStorage` if valid,
  else `null`.
- Effective theme on startup: `storedTheme() ?? colorScheme()`. Set
  `currentTheme` to it and write `document.documentElement.dataset.theme` as the
  **very first statement** in the module (before `init()`), to minimize the
  initial flash (see Trade-offs).
- `applyTheme(theme)`: set `currentTheme = theme`, write
  `document.documentElement.dataset.theme = theme`, call `initMermaid()`, and if
  a tab is active `renderActive({ scrollLock: false, forceMermaid: true })`.
  This mirrors exactly what the existing OS-change listener does.
- New `#toggle-theme` click handler: `const next = currentTheme === "dark" ?
  "light" : "dark"; localStorage.setItem(THEME_KEY, next); applyTheme(next);`
  then update the button face.
- Button face update (called from `renderTabBar` alongside the Raw button, or a
  small `updateThemeButton()` helper): set icon (☾ when light, ☀ when dark) and
  `title`/`aria-label` to the target.
- **`matchMedia` change listener (existing):** gate it on
  `storedTheme() === null` — i.e. only auto-follow the OS while the user has not
  chosen. When a stored pref exists, ignore OS changes. Inside, it now calls
  `applyTheme(colorScheme())` (which also keeps `data-theme` in sync).
- **`exportDocument`:** add `data-theme` to the existing snapshot/force/restore:
  save `prevDataTheme = document.documentElement.dataset.theme`, set it to
  `"light"` in the `try` (alongside `currentTheme = "light"`), restore in the
  `finally`. This makes the PDF (which renders on-screen through `@media print`)
  light for dark-mode users — a fix for a latent bug.

### App chrome CSS (`ui/styles.css`)

Replace selectors only; **no color values change**:

- `@media (prefers-color-scheme: dark) { :root { … } }` (core vars) →
  `:root[data-theme="dark"] { … }`.
- `@media (prefers-color-scheme: dark) { :root { … } }` (git badge vars) →
  `:root[data-theme="dark"] { … }`.
- `@media (prefers-color-scheme: light) { :root { … } }` (context-menu vars) →
  `:root[data-theme="light"] { … }`.
- `@media (prefers-color-scheme: dark) { ::highlight(...) { … } }` (find
  highlights) → `[data-theme="dark"] ::highlight(...) { … }`.
- The `@media print` block is unrelated to color scheme and stays as-is.

Light remains the unconditional `:root` base.

### Markdown body CSS (`ui/github-markdown.css`, vendored — edit required)

Convert the two media blocks so dark is attribute-gated over a light base:

- `@media (prefers-color-scheme: dark) { .markdown-body, [data-theme="dark"] { … } }`
  → `[data-theme="dark"] .markdown-body, [data-theme="dark"].markdown-body { … }`
  (descendant covers the `#preview` / notes-modal bodies under `<html
  data-theme="dark">`; the compound covers a `.markdown-body` that is itself the
  themed element, for safety).
- `@media (prefers-color-scheme: light) { .markdown-body, [data-theme="light"] { … } }`
  → unwrap to an unconditional `.markdown-body { … }` (light is the base). The
  `[data-theme="dark"]` rule wins by specificity (0,1,1 > 0,1,0) when present.

Result: with no `data-theme` (e.g. exported HTML) the body is light; with
`data-theme="dark"` on `<html>` it is dark.

### Toolbar markup (`ui/index.html`)

Add `#toggle-theme` as a `toolbar-btn` before `#toggle-raw` inside `.toolbar`,
icon-only, with an initial `title`/`aria-label`.

### Backend

No Rust changes. `render_file` / `render_notes` already accept a `theme`
parameter; `currentTheme` continues to feed them.

### Docs

Update `README.md` (Features + the toolbar/menus description) to cover the new
theme toggle, per the project's release discipline.

## Trade-offs / risks

- **FOUC:** with media queries gone, the first paint before the deferred
  `app.js` module runs uses the `:root` light base. If the effective theme is
  dark, there is a sub-frame flash. CSP forbids an inline-head bootstrap;
  mitigation is setting `data-theme` as the first module statement. The initial
  screen is the empty-state chrome (no document rendered yet), so the flash is
  minimal. A `:root:not([data-theme])` media first-paint fallback is a possible
  future add if it proves noticeable; not included now to keep the CSS DRY.
- **Editing a vendored file** (`github-markdown.css`): acceptable because the
  file is pinned and not re-vendored frequently, and the change is mechanical
  (two selector rewrites). Documented in CLAUDE.md's spirit of "things that took
  hours."

## Testing

- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` clean
  (no Rust changes expected, but CI runs them).
- `cargo build` (frontend is bundled at compile time; required to see UI
  changes).
- Manual: toggle light↔dark and confirm chrome, markdown body, syntect code
  blocks, Mermaid diagrams, KaTeX, git badges, context menu, find highlights,
  and the notes modal all switch together; preference persists across relaunch;
  pre-choice OS-appearance change still live-follows; HTML and PDF exports are
  light regardless of the in-app theme.

## Out of scope

- A "System" / auto option in the cycle (explicitly decided against).
- Per-tab themes (theme is global).
- A menu-bar entry or keyboard shortcut for theme (toolbar button only).
