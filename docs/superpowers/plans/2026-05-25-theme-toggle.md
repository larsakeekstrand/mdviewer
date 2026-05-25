# Light/Dark Theme Toggle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a toolbar button (next to **Raw**) that switches the whole app between a light and a dark theme, persisting the choice.

**Architecture:** Make the frontend the single source of truth for the theme. JS resolves the effective theme (stored preference, else OS) and writes it to `document.documentElement.dataset.theme`. All app stylesheets become attribute-driven (`[data-theme="dark"]`) instead of `@media (prefers-color-scheme: …)`, so the JS-set attribute fully controls appearance. Until the user clicks the button the app still live-follows macOS; the first click writes an explicit, sticky preference.

**Tech Stack:** Vanilla HTML/CSS/JS (no build step), `node --test` for pure helpers, Tauri 2 (no Rust changes). Frontend is bundled into the binary at compile time, so a `cargo build` is required to see UI changes.

---

## File structure

- **Create** `ui/theme.js` — pure theme helpers (no DOM/storage): `THEME_KEY`, `isValidTheme`, `resolveTheme`, `nextTheme`, `themeButtonFace`. Single responsibility: theme decision logic, unit-testable like `export.js`/`update.js`.
- **Create** `ui/theme.test.js` — `node --test` coverage for the helpers (auto-run in CI via `node --test ui/*.test.js`).
- **Modify** `ui/index.html` — add the `#toggle-theme` button before `#toggle-raw`.
- **Modify** `ui/app.js` — import helpers; set `data-theme` at module top; `themeBtn` const; `applyTheme`/`updateThemeButton`/`onToggleTheme`/`hasThemePref`; wire button; gate the OS-change listener; snapshot `data-theme` in `exportDocument`.
- **Modify** `ui/styles.css` — convert the four `prefers-color-scheme` blocks (core vars, git badges, ctx-menu, find highlights) to `[data-theme=…]` selectors; small `#toggle-theme` sizing rule.
- **Modify** `ui/github-markdown.css` (vendored) — convert its two `prefers-color-scheme` var blocks to a light base + `[data-theme="dark"]` override.
- **Modify** `README.md` — document the toggle.

**Task order matters:** Task 3 (JS always sets `data-theme`) lands before the CSS conversions (Tasks 4–5). After Task 3 with the old media-query CSS, the chrome still follows the OS (the unused attribute is harmless); full theming is live after Task 5.

---

### Task 1: Pure theme helpers (TDD)

**Files:**
- Create: `ui/theme.js`
- Test: `ui/theme.test.js`

- [ ] **Step 1: Write the failing test**

Create `ui/theme.test.js`:

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  THEME_KEY,
  isValidTheme,
  resolveTheme,
  nextTheme,
  themeButtonFace,
} from "./theme.js";

test("isValidTheme accepts only light and dark", () => {
  assert.equal(isValidTheme("light"), true);
  assert.equal(isValidTheme("dark"), true);
  assert.equal(isValidTheme("system"), false);
  assert.equal(isValidTheme(null), false);
  assert.equal(isValidTheme(""), false);
  assert.equal(isValidTheme(undefined), false);
});

test("resolveTheme prefers a valid stored value", () => {
  assert.equal(resolveTheme("dark", "light"), "dark");
  assert.equal(resolveTheme("light", "dark"), "light");
});

test("resolveTheme falls back to the OS theme when stored is missing/invalid", () => {
  assert.equal(resolveTheme(null, "dark"), "dark");
  assert.equal(resolveTheme("bogus", "light"), "light");
  assert.equal(resolveTheme(undefined, "dark"), "dark");
});

test("nextTheme flips between light and dark", () => {
  assert.equal(nextTheme("light"), "dark");
  assert.equal(nextTheme("dark"), "light");
});

test("themeButtonFace shows the target theme (action convention)", () => {
  assert.equal(themeButtonFace("light").icon, "☾");
  assert.equal(themeButtonFace("light").label, "Switch to dark theme");
  assert.equal(themeButtonFace("dark").icon, "☀");
  assert.equal(themeButtonFace("dark").label, "Switch to light theme");
});

test("THEME_KEY is namespaced", () => {
  assert.equal(THEME_KEY, "mdviewer.theme");
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `node --test ui/theme.test.js`
Expected: FAIL — `Cannot find module './theme.js'` (or import error).

- [ ] **Step 3: Write the implementation**

Create `ui/theme.js`:

```js
// Pure theme helpers (no DOM / localStorage access) so they're unit-testable
// under `node --test`. The DOM/storage wiring lives in app.js.

export const THEME_KEY = "mdviewer.theme";

export function isValidTheme(value) {
  return value === "light" || value === "dark";
}

// Stored preference wins when valid; otherwise fall back to the OS theme.
export function resolveTheme(stored, osTheme) {
  return isValidTheme(stored) ? stored : osTheme;
}

export function nextTheme(current) {
  return current === "dark" ? "light" : "dark";
}

// Button face shows the theme a click switches TO, matching the Raw button's
// "label = what clicking does" convention.
export function themeButtonFace(theme) {
  return theme === "dark"
    ? { icon: "☀", label: "Switch to light theme" }
    : { icon: "☾", label: "Switch to dark theme" };
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `node --test ui/theme.test.js`
Expected: PASS — all tests pass.

- [ ] **Step 5: Commit**

```bash
git add ui/theme.js ui/theme.test.js
git commit -m "Add pure light/dark theme helpers"
```

---

### Task 2: Add the toolbar button

**Files:**
- Modify: `ui/index.html` (the `.toolbar` div, around lines 59-69)
- Modify: `ui/styles.css` (after the `.toolbar-btn[aria-pressed="true"]` rule near line 423)

- [ ] **Step 1: Add the button markup before the Raw button**

In `ui/index.html`, replace:

```html
        <div class="toolbar">
          <button
            id="toggle-raw"
            class="toolbar-btn"
            type="button"
            aria-pressed="false"
            title="Toggle raw / rendered view"
          >
            Raw
          </button>
        </div>
```

with:

```html
        <div class="toolbar">
          <button
            id="toggle-theme"
            class="toolbar-btn"
            type="button"
            title="Switch to dark theme"
            aria-label="Switch to dark theme"
          >
            ☾
          </button>
          <button
            id="toggle-raw"
            class="toolbar-btn"
            type="button"
            aria-pressed="false"
            title="Toggle raw / rendered view"
          >
            Raw
          </button>
        </div>
```

(The static `☾` / "Switch to dark theme" is a placeholder; `updateThemeButton()` in Task 3 sets the correct face on startup.)

- [ ] **Step 2: Add a sizing rule for the icon button**

In `ui/styles.css`, immediately after this existing rule (around line 419-423):

```css
.toolbar-btn[aria-pressed="true"] {
  background: #5599ff;
  color: white;
  border-color: #5599ff;
}
```

add:

```css
#toggle-theme {
  font-size: 14px;
  line-height: 1;
  padding: 3px 8px;
}
```

- [ ] **Step 3: Build and verify the button appears**

Run: `cd src-tauri && cargo build`
Expected: builds cleanly.

Then run: `cd src-tauri && cargo run -- ../README.md`
Expected: the tab bar shows a `☾` button to the left of **Raw**. (It does nothing yet — wired in Task 3.) Close the app.

- [ ] **Step 4: Commit**

```bash
git add ui/index.html ui/styles.css
git commit -m "Add theme toggle button to the toolbar"
```

---

### Task 3: Wire theme state in the frontend

**Files:**
- Modify: `ui/app.js` (import block 10-15; consts 34/38; OS-change listener 209-216; init wiring 218; after `onToggleRaw` 669)

- [ ] **Step 1: Import the theme helpers**

In `ui/app.js`, after the `./update.js` import block (lines 10-15), add:

```js
import {
  THEME_KEY,
  isValidTheme,
  resolveTheme,
  nextTheme,
  themeButtonFace,
} from "./theme.js";
```

- [ ] **Step 2: Add the `themeBtn` element handle**

Replace:

```js
const rawBtn = document.getElementById("toggle-raw");
const splitter = document.getElementById("splitter");
```

with:

```js
const rawBtn = document.getElementById("toggle-raw");
const themeBtn = document.getElementById("toggle-theme");
const splitter = document.getElementById("splitter");
```

- [ ] **Step 3: Resolve and apply the effective theme at module top**

Replace:

```js
let currentTheme = colorScheme();
```

with:

```js
let currentTheme = resolveTheme(localStorage.getItem(THEME_KEY), colorScheme());
// Set as early as the CSP allows (no inline <head> script) to minimize the
// first-paint flash before the rest of the module runs.
document.documentElement.dataset.theme = currentTheme;
```

(`colorScheme` and `resolveTheme` are both available here — `colorScheme` is a hoisted function declaration and `resolveTheme` is an import.)

- [ ] **Step 4: Add the theme functions after `onToggleRaw`**

In `ui/app.js`, after the `onToggleRaw` function (ends around line 669):

```js
function onToggleRaw() {
  const t = activeTab();
  if (!t) return;
  t.raw = !t.raw;
  renderTabBar();
  renderActive({ scrollLock: false });
}
```

add:

```js
function hasThemePref() {
  return isValidTheme(localStorage.getItem(THEME_KEY));
}

function updateThemeButton() {
  const face = themeButtonFace(currentTheme);
  themeBtn.textContent = face.icon;
  themeBtn.title = face.label;
  themeBtn.setAttribute("aria-label", face.label);
}

async function applyTheme(theme) {
  currentTheme = theme;
  document.documentElement.dataset.theme = theme;
  initMermaid();
  updateThemeButton();
  if (activeTab())
    await renderActive({ scrollLock: false, forceMermaid: true });
}

async function onToggleTheme() {
  const next = nextTheme(currentTheme);
  localStorage.setItem(THEME_KEY, next);
  await applyTheme(next);
}
```

- [ ] **Step 5: Gate the OS-change listener and wire the button in `init`**

Replace the OS-change listener (lines 209-216):

```js
  window
    .matchMedia("(prefers-color-scheme: dark)")
    .addEventListener("change", async () => {
      currentTheme = colorScheme();
      initMermaid();
      if (activeTab())
        await renderActive({ scrollLock: false, forceMermaid: true });
    });

  rawBtn.addEventListener("click", onToggleRaw);
```

with:

```js
  window
    .matchMedia("(prefers-color-scheme: dark)")
    .addEventListener("change", async () => {
      // Auto-follow the OS only until the user has made an explicit choice.
      if (hasThemePref()) return;
      await applyTheme(colorScheme());
    });

  rawBtn.addEventListener("click", onToggleRaw);
  themeBtn.addEventListener("click", onToggleTheme);
  updateThemeButton();
```

- [ ] **Step 6: Build and verify toggling works (code + diagrams; chrome lands in Task 4-5)**

Run: `cd src-tauri && cargo build` — expected: clean build.

Run: `cd src-tauri && cargo run -- ../README.md`
Expected: clicking `☾`/`☀` flips the button face and re-renders; the **syntax-highlighted code block** and any **Mermaid diagram** switch between light/dark palettes. The sidebar/tab chrome still follows the OS for now (CSS converted next). Quit the app.

- [ ] **Step 7: Commit**

```bash
git add ui/app.js
git commit -m "Drive theme from a data-theme attribute set by JS"
```

---

### Task 4: Convert app-chrome CSS to attribute-driven

**Files:**
- Modify: `ui/styles.css` (four `prefers-color-scheme` blocks)

- [ ] **Step 1: Convert the core dark variables**

Replace:

```css
@media (prefers-color-scheme: dark) {
  :root {
    --sidebar-bg: #161b22;
    --sidebar-fg: #e6edf3;
    --sidebar-muted: #8b949e;
    --sidebar-border: #30363d;
    --sidebar-hover: #21262d;
    --sidebar-selected: #1f3461;
    --splitter: #30363d;
    --bg: #0d1117;
    --fg: #e6edf3;
    --chev: #8b949e;
  }
}
```

with:

```css
:root[data-theme="dark"] {
  --sidebar-bg: #161b22;
  --sidebar-fg: #e6edf3;
  --sidebar-muted: #8b949e;
  --sidebar-border: #30363d;
  --sidebar-hover: #21262d;
  --sidebar-selected: #1f3461;
  --splitter: #30363d;
  --bg: #0d1117;
  --fg: #e6edf3;
  --chev: #8b949e;
}
```

- [ ] **Step 2: Convert the git-badge dark variables**

Replace:

```css
@media (prefers-color-scheme: dark) {
  :root {
    --git-modified: #e2c08d;
    --git-added: #7bc97a;
    --git-deleted: #f48771;
    --git-untracked: #7bc97a;
    --git-conflict: #f48771;
  }
}
```

with:

```css
:root[data-theme="dark"] {
  --git-modified: #e2c08d;
  --git-added: #7bc97a;
  --git-deleted: #f48771;
  --git-untracked: #7bc97a;
  --git-conflict: #f48771;
}
```

- [ ] **Step 3: Convert the context-menu light variables**

Replace:

```css
@media (prefers-color-scheme: light) {
  :root {
    --ctx-bg: rgba(255, 255, 255, 0.97);
    --ctx-fg: #1f2328;
    --ctx-border: rgba(0, 0, 0, 0.12);
    --ctx-muted: rgba(0, 0, 0, 0.35);
    --ctx-shortcut: rgba(0, 0, 0, 0.45);
    --ctx-sep: rgba(0, 0, 0, 0.08);
  }
}
```

with:

```css
:root[data-theme="light"] {
  --ctx-bg: rgba(255, 255, 255, 0.97);
  --ctx-fg: #1f2328;
  --ctx-border: rgba(0, 0, 0, 0.12);
  --ctx-muted: rgba(0, 0, 0, 0.35);
  --ctx-shortcut: rgba(0, 0, 0, 0.45);
  --ctx-sep: rgba(0, 0, 0, 0.08);
}
```

(The ctx-menu defaults are baked into the `var(--ctx-bg, …dark…)` fallbacks; this light override applies because `data-theme` is always set.)

- [ ] **Step 4: Convert the find-highlight dark rules**

Replace:

```css
@media (prefers-color-scheme: dark) {
  ::highlight(search-match) {
    background-color: rgba(255, 214, 0, 0.3);
  }
  ::highlight(search-current) {
    background-color: #d2691e;
    color: #ffffff;
  }
}
```

with:

```css
[data-theme="dark"] ::highlight(search-match) {
  background-color: rgba(255, 214, 0, 0.3);
}
[data-theme="dark"] ::highlight(search-current) {
  background-color: #d2691e;
  color: #ffffff;
}
```

- [ ] **Step 5: Verify no `prefers-color-scheme` remains in styles.css**

Run: `grep -n "prefers-color-scheme" ui/styles.css`
Expected: no output (the `@media print` block is unrelated and stays).

- [ ] **Step 6: Build and verify the chrome switches**

Run: `cd src-tauri && cargo build` — expected: clean.

Run: `cd src-tauri && cargo run -- ../README.md`
Expected: toggling `☾`/`☀` now switches the **sidebar, tab bar, splitter, and find bar** (⌘F) too. Open a git repo folder and confirm the **git badge colors** switch. Right-click the preview and confirm the **context menu** matches the theme. Quit.

- [ ] **Step 7: Commit**

```bash
git add ui/styles.css
git commit -m "Make app-chrome CSS attribute-driven for the theme toggle"
```

---

### Task 5: Convert the markdown-body CSS (vendored)

**Files:**
- Modify: `ui/github-markdown.css` (the two `prefers-color-scheme` blocks, lines 13-124)

The two blocks span ~55 lines of variables each; only the wrapper/braces change, the variable lines are left as-is (CSS ignores the extra indentation; there is no CSS formatter in the repo).

- [ ] **Step 1: Unwrap the dark block (opening)**

Replace:

```css
@media (prefers-color-scheme: dark) {
  .markdown-body, [data-theme="dark"] {
```

with:

```css
[data-theme="dark"] .markdown-body, [data-theme="dark"].markdown-body {
```

- [ ] **Step 2: Remove the dark block's extra closing brace**

Replace:

```css
    --color-prettylights-syntax-sublimelinter-gutter-mark: #3d444d;
  }
}
```

with:

```css
    --color-prettylights-syntax-sublimelinter-gutter-mark: #3d444d;
}
```

- [ ] **Step 3: Unwrap the light block to an unconditional base (opening)**

Replace:

```css
@media (prefers-color-scheme: light) {
  .markdown-body, [data-theme="light"] {
```

with:

```css
.markdown-body {
```

- [ ] **Step 4: Remove the light block's extra closing brace**

Replace:

```css
    --color-prettylights-syntax-sublimelinter-gutter-mark: #818b98;
  }
}
```

with:

```css
    --color-prettylights-syntax-sublimelinter-gutter-mark: #818b98;
}
```

- [ ] **Step 5: Verify the file is balanced and media-free**

Run: `grep -n "prefers-color-scheme" ui/github-markdown.css`
Expected: no output.

Run: `node -e "const c=require('fs').readFileSync('ui/github-markdown.css','utf8');const o=(c.match(/{/g)||[]).length,x=(c.match(/}/g)||[]).length;console.log('open',o,'close',x);if(o!==x)process.exit(1)"`
Expected: `open N close N` (equal counts; non-zero exit means unbalanced braces — fix before continuing).

- [ ] **Step 6: Build and verify the document body switches**

Run: `cd src-tauri && cargo build` — expected: clean.

Run: `cd src-tauri && cargo run -- ../README.md`
Expected: toggling now also switches the **markdown body** — text color, blockquote/table borders, inline `code` backgrounds, and headings. Force a theme opposite to your OS and confirm it holds (e.g. on a dark Mac, switch to light and the body goes fully light). Quit.

- [ ] **Step 7: Commit**

```bash
git add ui/github-markdown.css
git commit -m "Make github-markdown.css attribute-driven over a light base"
```

---

### Task 6: Keep exports light under the new scheme

**Files:**
- Modify: `ui/app.js` (`exportDocument`, lines 882-915)

HTML export already produces light output (the exported doc has no `data-theme`, so the restructured body CSS resolves to its light base). PDF export renders the on-screen DOM through `@media print`, so it must temporarily clear `data-theme` to `light` — otherwise a dark in-app theme would carry into the PDF.

- [ ] **Step 1: Snapshot and force `data-theme` during export**

In `exportDocument`, replace:

```js
  const prevTheme = currentTheme;
  const prevRaw = t.raw;
  const prevScroll = previewScroll.scrollTop;
  try {
    currentTheme = "light";
    t.raw = false;
```

with:

```js
  const prevTheme = currentTheme;
  const prevDataTheme = document.documentElement.dataset.theme;
  const prevRaw = t.raw;
  const prevScroll = previewScroll.scrollTop;
  try {
    currentTheme = "light";
    document.documentElement.dataset.theme = "light";
    t.raw = false;
```

- [ ] **Step 2: Restore `data-theme` in the `finally`**

Replace:

```js
    exportInProgress = false;
    currentTheme = prevTheme;
    t.raw = prevRaw;
```

with:

```js
    exportInProgress = false;
    currentTheme = prevTheme;
    document.documentElement.dataset.theme = prevDataTheme;
    t.raw = prevRaw;
```

- [ ] **Step 3: Build and verify both exports are light in dark mode**

Run: `cd src-tauri && cargo build` — expected: clean.

Run: `cd src-tauri && cargo run -- ../README.md`
With the app toggled to **dark**: File ▸ **Export as HTML…**, open the `.html` in a browser → it is light. File ▸ **Export as PDF…**, open the PDF → it is light (no dark background, code blocks light). Confirm the app returns to dark after each export. Quit.

- [ ] **Step 4: Commit**

```bash
git add ui/app.js
git commit -m "Force light data-theme during export so PDFs stay light"
```

---

### Task 7: Full verification pass

**Files:** none (verification only)

- [ ] **Step 1: Run the JS unit tests**

Run: `node --test ui/*.test.js`
Expected: all tests pass (theme, export, search, update).

- [ ] **Step 2: Run the Rust lints (must be clean for CI)**

Run: `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: no output / no warnings (no Rust changed, but CI enforces this).

- [ ] **Step 3: Manual persistence + follow-then-stick check**

Run: `cd src-tauri && cargo run -- ../README.md`
- **Persistence:** toggle to the non-OS theme, quit, relaunch → it reopens in the chosen theme.
- **Stick after choice:** with a theme chosen, change macOS appearance (System Settings ▸ Appearance) → the app does **not** change. (The pre-choice auto-follow path is covered by the `resolveTheme`/`hasThemePref` unit logic and the listener gate.)
- **Math:** confirm the KaTeX `$…$` in README stays legible in both themes.

Quit the app.

- [ ] **Step 4: No commit** (verification only). If any step failed, return to the relevant task.

---

### Task 8: Update the README

**Files:**
- Modify: `README.md` (feature bullet line 28; the "Raw vs rendered view" area around lines 89-91)

- [ ] **Step 1: Update the theme feature bullet**

Replace:

```markdown
- Auto light + dark theme via OS `prefers-color-scheme`
```

with:

```markdown
- **Light / dark theme toggle** — a toolbar button (next to **Raw**) switches the whole app between light and dark; the app follows the macOS appearance until you choose, then remembers your choice across launches
```

- [ ] **Step 2: Add a "Theme" subsection**

After this block (around lines 89-91):

```markdown
### Raw vs rendered view

Each tab can be viewed rendered (default) or raw. Toggle with the **Raw** button at the top-right of the tab bar, or via the **Actions ▸ Toggle Raw** menu item, or via the right-click context menu. The toggle is per tab.
```

add:

```markdown
### Theme

Switch between light and dark with the **☾ / ☀** button at the top-right of the tab bar (left of **Raw**); the icon shows the theme you'll switch to. Until you press it, MDViewer follows your macOS appearance live; once you choose, that choice is remembered across launches and the app stops auto-following the OS. The theme applies everywhere — file tree, tabs, rendered markdown, syntax-highlighted code, Mermaid diagrams, and math. (Exports are always light regardless of the in-app theme.)
```

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "Document the light/dark theme toggle"
```

---

## Self-review notes

- **Spec coverage:** two-state toggle (Tasks 1,3), follow-then-stick persistence (Task 3 listener gate + `localStorage`), JS-owned `data-theme` (Task 3), attribute-driven chrome + body CSS (Tasks 4,5), target-icon button (Tasks 1,2,3), export stays light incl. PDF fix (Task 6), README (Task 8), FOUC mitigation via early `data-theme` set (Task 3 Step 3). All spec sections mapped.
- **No backend changes:** `render_file`/`render_notes` already take a `theme` arg fed by `currentTheme`.
- **Type consistency:** helper names (`THEME_KEY`, `isValidTheme`, `resolveTheme`, `nextTheme`, `themeButtonFace`) and DOM handle (`themeBtn`) are used identically across tasks.
