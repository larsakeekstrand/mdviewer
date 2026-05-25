# What's New Modal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user read a new release's changelog inside the app, in a modal opened from the update banner, before deciding to update.

**Architecture:** The release notes are already available client-side as `update.body` (from `updaterApi.check()`). A pure JS helper extracts just the `## Changes` section; a thin Rust command renders that markdown to HTML through the existing comrak pipeline; the frontend shows it in a modal overlay. The banner's "View release" button is replaced by a "What's new" button; the modal carries a link to the full GitHub release page.

**Tech Stack:** Tauri 2 (Rust commands), vanilla JS (ES modules, `node --test`), comrak (`markdown::render_markdown`), CSS variables for light/dark.

**Spec:** `docs/superpowers/specs/2026-05-25-whats-new-modal-design.md`

---

## File structure

- `ui/update.js` — add the pure `extractChangelog(body)` helper (alongside the existing pure update helpers).
- `ui/update.test.js` — `node --test` cases for `extractChangelog`.
- `src-tauri/src/commands.rs` — add the `render_notes` command + a unit test.
- `src-tauri/src/lib.rs` — register `render_notes` in the invoke handler.
- `ui/index.html` — swap the banner button, add the modal markup.
- `ui/styles.css` — modal overlay + dialog styling.
- `ui/app.js` — element refs, `openNotesModal`/`closeNotesModal`, dismissal + link handlers, banner rewiring.
- `README.md`, `CLAUDE.md` — document the new behavior; fix the now-stale "View release" claim.

---

## Task 1: `extractChangelog` pure helper

**Files:**
- Modify: `ui/update.js`
- Test: `ui/update.test.js`

- [ ] **Step 1: Write the failing tests**

First extend the existing top-of-file import in `ui/update.test.js`. Change:

```javascript
import {
  releaseUrlFor,
  bannerMessage,
  progressPercent,
  progressText,
} from "./update.js";
```

to:

```javascript
import {
  releaseUrlFor,
  bannerMessage,
  progressPercent,
  progressText,
  extractChangelog,
} from "./update.js";
```

Then append these tests to the end of `ui/update.test.js`:

```javascript
test("extractChangelog returns the section after '## Changes'", () => {
  const body = "## Install\n\nblah\n\n## Changes\n\n- a (h1)\n- b (h2)\n";
  assert.equal(extractChangelog(body), "- a (h1)\n- b (h2)");
});

test("extractChangelog falls back to the full body when '## Changes' is absent", () => {
  const body = "# Notes\n\n- only this\n";
  assert.equal(extractChangelog(body), "# Notes\n\n- only this");
});

test("extractChangelog returns empty string for empty/null/undefined", () => {
  assert.equal(extractChangelog(""), "");
  assert.equal(extractChangelog(null), "");
  assert.equal(extractChangelog(undefined), "");
});

test("extractChangelog trims surrounding whitespace", () => {
  assert.equal(extractChangelog("## Changes\n\n\n- only\n\n\n"), "- only");
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `node --test ui/update.test.js`
Expected: FAIL — `extractChangelog` is not exported (import resolves to `undefined`, the new tests throw).

- [ ] **Step 3: Implement the helper**

Append to `ui/update.js`:

```javascript
/** The release-notes body is the full GitHub release text (install steps,
 *  quarantine note, an Updating section, then a "## Changes" changelog). For
 *  the in-app "What's new" modal we want only the changelog. Returns the text
 *  after the "## Changes" heading (heading dropped, trimmed); falls back to the
 *  whole body when that heading is absent; "" for empty input. */
export function extractChangelog(body) {
  if (!body) return "";
  const lines = body.split("\n");
  const idx = lines.findIndex((l) => /^##\s+Changes\s*$/.test(l));
  if (idx === -1) return body.trim();
  return lines.slice(idx + 1).join("\n").trim();
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `node --test ui/update.test.js`
Expected: PASS — all existing tests plus the four new ones.

- [ ] **Step 5: Commit**

```bash
git add ui/update.js ui/update.test.js
git commit -m "Add extractChangelog helper for release-notes modal"
```

---

## Task 2: `render_notes` Tauri command

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs:51-68` (invoke handler list)
- Test: `src-tauri/src/commands.rs` (existing `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test**

In `src-tauri/src/commands.rs`, inside the existing `#[cfg(test)] mod tests { … }`
block (it already has `use super::*;`), add:

```rust
#[test]
fn render_notes_renders_markdown_to_html() {
    let html = render_notes("# Hello".to_string(), None).unwrap();
    assert!(html.contains("<h1"), "expected an h1, got: {html}");
    assert!(html.contains("Hello"));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test --lib render_notes_renders`
Expected: FAIL to compile — `cannot find function render_notes in this scope`.

- [ ] **Step 3: Implement the command**

In `src-tauri/src/commands.rs`, add immediately after the `render_file`
function (after its closing `}` near line 80):

```rust
#[tauri::command]
pub fn render_notes(source: String, theme: Option<String>) -> Result<String, String> {
    let theme = theme.as_deref().unwrap_or("light");
    Ok(markdown::render_markdown(&source, theme))
}
```

- [ ] **Step 4: Register the command**

In `src-tauri/src/lib.rs`, add `commands::render_notes,` to the
`tauri::generate_handler!` list (after `commands::render_file,` on line 55):

```rust
            commands::render_file,
            commands::render_notes,
            commands::open_file,
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --lib render_notes_renders`
Expected: PASS.

- [ ] **Step 6: Lint**

Run: `cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "Add render_notes command for in-app release notes"
```

---

## Task 3: Banner button swap, modal markup, styling, and wiring

This task touches three frontend files together because they are interdependent:
removing the `update-banner-view` button without updating `app.js`'s references
to it would throw at load. There is no JS DOM test harness in this repo, so this
task is verified by build + a manual end-to-end check (Step 11).

**Files:**
- Modify: `ui/index.html:12-48` (banner) and before the `<script>` tags (modal)
- Modify: `ui/styles.css` (append modal styles)
- Modify: `ui/app.js` (import, refs, functions, wiring)

- [ ] **Step 1: Swap the banner button in `ui/index.html`**

Replace this block (lines 30-37):

```html
      <button
        id="update-banner-view"
        class="update-banner-btn"
        type="button"
        hidden
      >
        View release
      </button>
```

with:

```html
      <button
        id="update-banner-whatsnew"
        class="update-banner-btn"
        type="button"
        hidden
      >
        What's new
      </button>
```

- [ ] **Step 2: Add the modal markup in `ui/index.html`**

Immediately before the `<script src="morphdom-umd.min.js"></script>` line
(line 138), insert:

```html
    <div class="notes-modal" id="notes-modal" hidden>
      <div
        class="notes-dialog"
        id="notes-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="notes-modal-title"
      >
        <header class="notes-modal-header">
          <h2 class="notes-modal-title" id="notes-modal-title"></h2>
          <button
            id="notes-modal-x"
            class="notes-modal-x"
            type="button"
            aria-label="Close"
            title="Close"
          >
            ×
          </button>
        </header>
        <div class="notes-modal-body markdown-body" id="notes-modal-body"></div>
        <footer class="notes-modal-footer">
          <a id="notes-modal-link" class="notes-modal-fulllink" href="#">
            View full release notes →
          </a>
          <span class="notes-modal-spacer"></span>
          <button id="notes-modal-update" class="update-banner-btn primary" type="button">
            Update now
          </button>
          <button id="notes-modal-close" class="update-banner-btn" type="button">
            Close
          </button>
        </footer>
      </div>
    </div>

```

- [ ] **Step 3: Append modal styles to `ui/styles.css`**

Add at the end of `ui/styles.css`:

```css
.notes-modal {
  position: fixed;
  inset: 0;
  z-index: 100000;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.5);
  padding: 24px;
}

.notes-dialog {
  display: flex;
  flex-direction: column;
  width: min(680px, 100%);
  max-height: min(80vh, 720px);
  background: var(--bg);
  color: var(--fg);
  border: 1px solid var(--sidebar-border);
  border-radius: 10px;
  box-shadow: 0 12px 48px rgba(0, 0, 0, 0.4);
  overflow: hidden;
}

.notes-modal-header {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 16px;
  border-bottom: 1px solid var(--sidebar-border);
}

.notes-modal-title {
  flex: 1;
  margin: 0;
  font-size: 15px;
  font-weight: 600;
  border: none;
  padding: 0;
}

.notes-modal-x {
  font: inherit;
  font-size: 18px;
  line-height: 1;
  border: none;
  background: transparent;
  color: var(--sidebar-muted);
  cursor: pointer;
  padding: 2px 8px;
  border-radius: 4px;
}

.notes-modal-x:hover {
  background: var(--sidebar-hover);
}

#notes-modal-body {
  flex: 1;
  overflow: auto;
  padding: 16px;
  margin: 0;
  max-width: none;
  min-width: 0;
  background: var(--bg);
}

.notes-modal-footer {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 16px;
  border-top: 1px solid var(--sidebar-border);
}

.notes-modal-fulllink {
  font-size: 12px;
  color: #4477dd;
  text-decoration: none;
}

.notes-modal-fulllink:hover {
  text-decoration: underline;
}

.notes-modal-spacer {
  flex: 1;
}

.notes-modal-footer .update-banner-btn {
  color: var(--fg);
  border-color: var(--sidebar-border);
}

.notes-modal-footer .update-banner-btn:hover {
  background: var(--sidebar-hover);
}

.notes-modal-footer .update-banner-btn.primary {
  background: #4477dd;
  color: #fff;
  border-color: #4477dd;
}

.notes-modal-footer .update-banner-btn.primary:hover {
  background: #5599ff;
}
```

Note: `.notes-modal-title` and `#notes-modal-body` explicitly reset `border`/
`max-width`/`margin`/`padding` because `github-markdown.css`'s `.markdown-body`
and heading rules would otherwise add a 980px max-width, auto margins, and
heading borders inside the modal.

- [ ] **Step 4: Add `extractChangelog` to the `app.js` import**

In `ui/app.js`, change the update.js import (lines 10-14):

```javascript
import {
  releaseUrlFor,
  bannerMessage,
  progressText,
} from "./update.js";
```

to:

```javascript
import {
  releaseUrlFor,
  bannerMessage,
  progressText,
  extractChangelog,
} from "./update.js";
```

- [ ] **Step 5: Replace the banner element refs with banner + modal refs**

In `ui/app.js`, replace these two lines (1804-1805):

```javascript
const updateBannerView = document.getElementById("update-banner-view");
const updateBannerDismiss = document.getElementById("update-banner-dismiss");
```

with:

```javascript
const updateBannerWhatsNew = document.getElementById("update-banner-whatsnew");
const updateBannerDismiss = document.getElementById("update-banner-dismiss");

const notesModal = document.getElementById("notes-modal");
const notesDialog = document.getElementById("notes-dialog");
const notesModalTitle = document.getElementById("notes-modal-title");
const notesModalBody = document.getElementById("notes-modal-body");
const notesModalLink = document.getElementById("notes-modal-link");
const notesModalUpdate = document.getElementById("notes-modal-update");
const notesModalClose = document.getElementById("notes-modal-close");
const notesModalX = document.getElementById("notes-modal-x");
let notesModalTrigger = null;
```

- [ ] **Step 6: Update `setUpdateButtons` (view → whatsNew)**

Replace the whole `setUpdateButtons` function (1807-1817):

```javascript
function setUpdateButtons({
  update = false,
  restart = false,
  view = false,
  dismiss = false,
} = {}) {
  updateBannerUpdate.hidden = !update;
  updateBannerRestart.hidden = !restart;
  updateBannerView.hidden = !view;
  updateBannerDismiss.hidden = !dismiss;
}
```

with:

```javascript
function setUpdateButtons({
  update = false,
  restart = false,
  whatsNew = false,
  dismiss = false,
} = {}) {
  updateBannerUpdate.hidden = !update;
  updateBannerRestart.hidden = !restart;
  updateBannerWhatsNew.hidden = !whatsNew;
  updateBannerDismiss.hidden = !dismiss;
}
```

- [ ] **Step 7: Replace `openReleasePage` with the modal functions + listeners**

Replace the whole `openReleasePage` function (1819-1827):

```javascript
function openReleasePage(version) {
  return async () => {
    try {
      await invoke("open_url", { url: releaseUrlFor(REPO, version) });
    } catch (e) {
      console.error("open_url failed", e);
    }
  };
}
```

with (note: the rendered HTML is adopted via `DOMParser` + `replaceChildren`,
never assigned through `innerHTML` — this matches the existing mermaid insertion
pattern and avoids a raw-HTML-string assignment):

```javascript
function openNotesModal(update) {
  notesModalTrigger = document.activeElement;
  notesModalTitle.textContent = `What's new in ${update.version}`;
  notesModalLink.href = releaseUrlFor(REPO, update.version);

  notesModalUpdate.onclick = () => {
    closeNotesModal();
    runUpdate(update);
  };

  const md = extractChangelog(update.body);
  if (!md) {
    notesModalBody.textContent = "No release notes available.";
    revealNotesModal();
    return;
  }
  notesModalBody.textContent = "Loading…";
  revealNotesModal();
  invoke("render_notes", { source: md, theme: currentTheme })
    .then((html) => {
      const doc = new DOMParser().parseFromString(html, "text/html");
      notesModalBody.replaceChildren(...doc.body.childNodes);
    })
    .catch((e) => {
      console.error("render_notes failed", e);
      notesModalBody.textContent = md;
    });
}

function revealNotesModal() {
  notesModal.hidden = false;
  notesModalX.focus();
}

function closeNotesModal() {
  notesModal.hidden = true;
  if (notesModalTrigger && typeof notesModalTrigger.focus === "function") {
    notesModalTrigger.focus();
  }
  notesModalTrigger = null;
}

notesModalClose.addEventListener("click", closeNotesModal);
notesModalX.addEventListener("click", closeNotesModal);
notesModal.addEventListener("click", (ev) => {
  if (ev.target === notesModal) closeNotesModal();
});
document.addEventListener("keydown", (ev) => {
  if (ev.key === "Escape" && !notesModal.hidden) closeNotesModal();
});
notesDialog.addEventListener("click", (ev) => {
  const a = ev.target.closest("a[href]");
  if (!a || !notesDialog.contains(a)) return;
  ev.preventDefault();
  const href = a.getAttribute("href");
  if (href && isExternalUrl(href)) {
    invoke("open_url", { url: href }).catch((e) =>
      console.error("open_url failed", e),
    );
  }
});
```

(`isExternalUrl` is a hoisted function declaration defined later in `app.js`, so
calling it inside these runtime handlers is fine.)

- [ ] **Step 8: Rewire `showUpdateAvailable`**

Replace the whole `showUpdateAvailable` function (1886-1903):

```javascript
function showUpdateAvailable(update) {
  updateBannerText.textContent = bannerMessage(
    update.version,
    update.currentVersion,
  );
  setUpdateButtons({ update: true, view: true, dismiss: true });

  updateBannerUpdate.onclick = () => runUpdate(update);
  updateBannerView.onclick = openReleasePage(update.version);
  updateBannerDismiss.onclick = () => {
    try {
      localStorage.setItem(DISMISS_KEY, update.version);
    } catch (_) {}
    updateBanner.hidden = true;
  };

  updateBanner.hidden = false;
}
```

with:

```javascript
function showUpdateAvailable(update) {
  updateBannerText.textContent = bannerMessage(
    update.version,
    update.currentVersion,
  );
  setUpdateButtons({ update: true, whatsNew: true, dismiss: true });

  updateBannerUpdate.onclick = () => runUpdate(update);
  updateBannerWhatsNew.onclick = () => openNotesModal(update);
  updateBannerDismiss.onclick = () => {
    try {
      localStorage.setItem(DISMISS_KEY, update.version);
    } catch (_) {}
    updateBanner.hidden = true;
  };

  updateBanner.hidden = false;
}
```

- [ ] **Step 9: Rewire the `runUpdate` failure branch**

Replace this block inside `runUpdate` (1927-1937):

```javascript
  } catch (e) {
    updateInProgress = false;
    console.error("update failed", e);
    updateBannerText.textContent = "Update failed: " + e;
    setUpdateButtons({ view: true, dismiss: true });
    updateBannerView.onclick = openReleasePage(update.version);
    updateBannerDismiss.onclick = () => {
      updateBanner.hidden = true;
    };
    return;
  }
```

with:

```javascript
  } catch (e) {
    updateInProgress = false;
    console.error("update failed", e);
    updateBannerText.textContent = "Update failed: " + e;
    setUpdateButtons({ whatsNew: true, dismiss: true });
    updateBannerWhatsNew.onclick = () => openNotesModal(update);
    updateBannerDismiss.onclick = () => {
      updateBanner.hidden = true;
    };
    return;
  }
```

- [ ] **Step 10: Verify no stale references remain, then build**

Run: `grep -n "update-banner-view\|updateBannerView\|openReleasePage" ui/app.js ui/index.html`
Expected: NO matches (all removed).

Run: `cd src-tauri && cargo build`
Expected: builds successfully (this re-bundles the frontend; required because
Tauri embeds `ui/*` at compile time).

- [ ] **Step 11: Manual end-to-end check**

The updater only shows the banner when a newer published release exists. Simulate
by temporarily downgrading the local version so the published 1.8.0 is "newer":

1. In `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`, temporarily set the
   version to `1.6.0`. Run `cd src-tauri && cargo run -- ../README.md`.
2. The blue update banner should appear with **What's new**, **Update now**, **×**.
3. Click **What's new** → the modal opens centered, dimming the app, titled
   "What's new in 1.8.0", showing the rendered changelog (a bullet list), with a
   **View full release notes →** link and **Update now** / **Close** buttons.
4. Verify: the "Commits since [v1.7.0]" link and the footer link open the browser
   (do NOT navigate the app); **Close**, the **×**, **Esc**, and a backdrop click
   all dismiss the modal; the banner remains. Toggle macOS dark/light to confirm
   the modal colors follow the theme. Do NOT click **Update now** (it would
   really download/replace the running build).
5. Revert the version bump: `git checkout src-tauri/tauri.conf.json src-tauri/Cargo.toml`.

- [ ] **Step 12: Commit**

```bash
git add ui/index.html ui/styles.css ui/app.js
git commit -m "Show in-app What's New modal from the update banner"
```

---

## Task 4: Documentation

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update the README**

In `README.md`, replace the auto-update feature bullet (line 31):

```markdown
- **One-click auto-update** — when a newer release is published, a dismissible banner downloads, installs, and restarts the signed update in-app
```

with:

```markdown
- **One-click auto-update** — when a newer release is published, a dismissible banner downloads, installs, and restarts the signed update in-app; a **What's new** button on the banner shows that release's changelog in an in-app window before you decide
```

- [ ] **Step 2: Fix the stale CLAUDE.md claim**

In `CLAUDE.md` (the "Update check internals" section, lines 396-397), replace:

```
  (`app.restart()`), and **View release** opens the reconstructed
  `releases/tag/v<version>` page via `open_url`.
```

with:

```
  (`app.restart()`), and **What's new** opens an in-app modal
  (`openNotesModal`) showing the release changelog — extracted from the
  `## Changes` section client-side (`extractChangelog` in `update.js`), rendered
  via the `render_notes` command (comrak) and inserted with `DOMParser` +
  `replaceChildren`, with a link out to the `releases/tag/v<version>` page via
  `open_url`.
```

- [ ] **Step 3: Commit**

```bash
git add README.md CLAUDE.md
git commit -m "Document the What's New release-notes modal"
```

---

## Task 5: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Run the full gate**

```bash
node --test ui/*.test.js
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

Expected: JS tests pass; `fmt --check` clean; clippy clean; all Rust tests pass.

- [ ] **Step 2: Confirm the branch state**

Run: `git status --short` (expected: clean) and `git log --oneline -6`
(expected: the four feature commits + the spec commit).

---

## Notes

- **HTML insertion is safe + consistent:** the modal HTML comes from the trusted
  comrak pipeline (`render.unsafe = false`) and is inserted via `DOMParser` +
  `replaceChildren` (never a raw `innerHTML` string), matching the mermaid
  pattern. `script-src 'self'` in the CSP additionally neutralizes any injected
  script. External links route through the existing `open_url` (http(s) only).
  **No CSP change required.**
- **Releasing is out of scope** for this plan and is a separate, user-initiated
  step (version bump + tag + publish per CLAUDE.md "Cutting a release"). Do not
  bump the version or tag as part of this work.
