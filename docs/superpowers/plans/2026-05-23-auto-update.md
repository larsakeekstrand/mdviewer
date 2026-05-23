# Auto-update Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a one-click **Update now** button to the existing update banner that downloads, verifies, installs, and (on a **Restart now** click) relaunches into the new version via `tauri-plugin-updater`.

**Architecture:** Consolidate update detection onto the plugin's `check()` (deleting the hand-rolled `updates.rs` + `ureq`/`semver`). The banner becomes a small state machine (available → downloading → installed/error). Relaunch is a one-line custom `restart` command, avoiding `tauri-plugin-process`. Release CI signs the bundle with a minisign key and auto-publishes `latest.json`; the existing draft→publish step is the go-live gate.

**Tech Stack:** Tauri 2.11, `tauri-plugin-updater` 2.x, vanilla JS (no build step; plugin API via `window.__TAURI__.updater`), `node --test` for pure JS helpers, GitHub Actions + `tauri-apps/tauri-action`.

**Spec:** `docs/superpowers/specs/2026-05-23-auto-update-design.md`

---

## File Structure

- `ui/update.js` — **new**. Pure helpers (URL, banner text, progress), no DOM/Tauri. Mirrors `ui/search.js` / `ui/export.js`.
- `ui/update.test.js` — **new**. `node --test` coverage for `update.js`.
- `ui/app.js` — rewrite the `/* ---- Update check ---- */` section into the banner state machine.
- `ui/index.html` — extend the `#update-banner` markup with Update/Restart buttons.
- `src-tauri/src/commands.rs` — remove `check_for_updates`; add `restart`.
- `src-tauri/src/lib.rs` — register the updater plugin; swap the `invoke_handler` entries; drop `mod updates`.
- `src-tauri/src/updates.rs` — **delete**.
- `src-tauri/Cargo.toml` — add `tauri-plugin-updater`; remove `ureq` + `semver`.
- `src-tauri/tauri.conf.json` — `createUpdaterArtifacts`, `plugins.updater` (pubkey + endpoint), version bump.
- `src-tauri/capabilities/default.json` — add `updater:default`.
- `.github/workflows/release.yml` — pass the two signing secrets to `tauri-action`.
- `CLAUDE.md` — architecture note, "things that took hours" note, release-step update, rewritten "Update check internals".

---

## Task 1: Pure update helpers + tests

**Files:**
- Create: `ui/update.js`
- Test: `ui/update.test.js`

- [ ] **Step 1: Write the failing test**

Create `ui/update.test.js`:

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  releaseUrlFor,
  bannerMessage,
  progressPercent,
  progressText,
} from "./update.js";

test("releaseUrlFor builds a v-prefixed tag URL", () => {
  assert.equal(
    releaseUrlFor("larsakeekstrand/mdviewer", "1.5.0"),
    "https://github.com/larsakeekstrand/mdviewer/releases/tag/v1.5.0",
  );
});

test("bannerMessage includes both versions when current is known", () => {
  assert.equal(
    bannerMessage("1.5.0", "1.4.0"),
    "MDViewer 1.5.0 is available — you have 1.4.0.",
  );
});

test("bannerMessage omits the current version when undefined", () => {
  assert.equal(
    bannerMessage("1.5.0", undefined),
    "MDViewer 1.5.0 is available.",
  );
});

test("progressPercent rounds and clamps", () => {
  assert.equal(progressPercent(50, 200), 25);
  assert.equal(progressPercent(199, 200), 100); // 99.5 rounds up
  assert.equal(progressPercent(300, 200), 100); // clamp to 100
});

test("progressPercent returns null when total unknown", () => {
  assert.equal(progressPercent(100, 0), null);
  assert.equal(progressPercent(100, undefined), null);
});

test("progressText degrades without a total", () => {
  assert.equal(progressText(50, 200), "Downloading… 25%");
  assert.equal(progressText(50, 0), "Downloading…");
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test ui/update.test.js`
Expected: FAIL — `Cannot find module './update.js'` (or import error).

- [ ] **Step 3: Write minimal implementation**

Create `ui/update.js`:

```js
// Pure helpers for the auto-update banner. No DOM or Tauri imports, so this
// runs under `node --test` as well as in the WebView (mirrors search.js /
// export.js).

/** GitHub release page URL for a version tag (tags are `v`-prefixed). */
export function releaseUrlFor(repo, version) {
  return `https://github.com/${repo}/releases/tag/v${version}`;
}

/** Banner headline for an available update. `currentVersion` may be undefined
 *  (the updater can omit it); fall back to a shorter sentence. */
export function bannerMessage(version, currentVersion) {
  return currentVersion
    ? `MDViewer ${version} is available — you have ${currentVersion}.`
    : `MDViewer ${version} is available.`;
}

/** Whole-percent download progress, or null when the total size is unknown
 *  (the updater reports contentLength 0/undefined for chunked responses). */
export function progressPercent(downloaded, contentLength) {
  if (!contentLength || contentLength <= 0) return null;
  const pct = Math.round((downloaded / contentLength) * 100);
  return Math.min(100, Math.max(0, pct));
}

/** Progress label for the banner; degrades gracefully without a total. */
export function progressText(downloaded, contentLength) {
  const pct = progressPercent(downloaded, contentLength);
  return pct === null ? "Downloading…" : `Downloading… ${pct}%`;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `node --test ui/update.test.js`
Expected: PASS — all 6 tests pass.

- [ ] **Step 5: Commit**

```bash
git add ui/update.js ui/update.test.js
git commit -m "Add pure helpers for auto-update banner"
```

---

## Task 2: Backend — updater plugin, restart command, remove updates.rs

**Files:**
- Modify: `src-tauri/src/commands.rs:6` (import) and `:78-81` (`check_for_updates`)
- Modify: `src-tauri/src/lib.rs:10` (`mod updates`), `:49` (plugins), `:58` (invoke_handler)
- Delete: `src-tauri/src/updates.rs`
- Modify: `src-tauri/Cargo.toml` (deps)
- Modify: `src-tauri/tauri.conf.json` (bundle + plugins)
- Modify: `src-tauri/capabilities/default.json`

- [ ] **Step 1: Replace `check_for_updates` with `restart` in commands.rs**

In `src-tauri/src/commands.rs`, delete this block (lines ~78-81):

```rust
#[tauri::command]
pub fn check_for_updates() -> Result<updates::UpdateInfo, String> {
    updates::check()
}
```

Replace it with:

```rust
#[tauri::command]
pub fn restart(app: AppHandle) {
    app.restart();
}
```

(`AppHandle` is already imported at `commands.rs:4`. `app.restart()` diverges, so the command never returns — that's fine.)

- [ ] **Step 2: Drop the `updates` import in commands.rs**

In `src-tauri/src/commands.rs:6`, change:

```rust
use crate::{git, markdown, recent, tasklist, tree, updates, AppState};
```

to:

```rust
use crate::{git, markdown, recent, tasklist, tree, AppState};
```

- [ ] **Step 3: Update lib.rs (module, plugin, handler)**

In `src-tauri/src/lib.rs`:

Remove line 10:

```rust
mod updates;
```

After the dialog plugin line (`:49`), add the updater plugin so the block reads:

```rust
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(state)
```

In the `invoke_handler!` list, replace `commands::check_for_updates,` (`:58`) with:

```rust
            commands::restart,
```

- [ ] **Step 4: Delete updates.rs**

Run:

```bash
git rm src-tauri/src/updates.rs
```

- [ ] **Step 5: Verify nothing else references `updates`/`ureq`/`semver`**

Run: `grep -rn "updates::\|ureq\|semver\|check_for_updates" src-tauri/src`
Expected: no matches (empty output).

- [ ] **Step 6: Swap dependencies in Cargo.toml**

In `src-tauri/Cargo.toml`, delete:

```toml
# Update check (GitHub releases API + version comparison)
ureq = { version = "3", features = ["json"] }
semver = "1"
```

Add after the `tauri-plugin-dialog` line:

```toml
tauri-plugin-updater = "2"
```

- [ ] **Step 7: Add updater config to tauri.conf.json**

In `src-tauri/tauri.conf.json`, add `"createUpdaterArtifacts": true,` as the first key inside `"bundle"`:

```jsonc
  "bundle": {
    "active": true,
    "createUpdaterArtifacts": true,
    "targets": "all",
```

Add a top-level `"plugins"` block after the closing `}` of `"bundle"` (sibling of `app`/`build`/`bundle`). Use a **placeholder pubkey for now** — it is replaced with the real key in Task 5; any non-empty string lets the build succeed:

```jsonc
  "plugins": {
    "updater": {
      "pubkey": "REPLACE_WITH_GENERATED_PUBKEY",
      "endpoints": [
        "https://github.com/larsakeekstrand/mdviewer/releases/latest/download/latest.json"
      ]
    }
  }
```

- [ ] **Step 8: Add the updater capability**

In `src-tauri/capabilities/default.json`, add `"updater:default"` to the `permissions` array:

```json
  "permissions": [
    "core:default",
    "core:window:default",
    "core:event:default",
    "core:webview:default",
    "dialog:default",
    "updater:default"
  ]
```

- [ ] **Step 9: Build, lint, format**

Run from `src-tauri/`:

```bash
cargo build
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Expected: build succeeds (the placeholder pubkey is accepted at build time), clippy clean, fmt clean. If `cargo fmt --check` reports diffs, run `cargo fmt` and re-check.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "Wire tauri-plugin-updater and restart command; drop hand-rolled update check"
```

---

## Task 3: Frontend — banner markup for the state machine

**Files:**
- Modify: `ui/index.html:12-30`

- [ ] **Step 1: Replace the banner markup**

In `ui/index.html`, replace the entire `#update-banner` block (lines 12-30) with:

```html
    <div class="update-banner" id="update-banner" hidden role="status">
      <span class="update-banner-text" id="update-banner-text"></span>
      <button
        id="update-banner-update"
        class="update-banner-btn primary"
        type="button"
        hidden
      >
        Update now
      </button>
      <button
        id="update-banner-restart"
        class="update-banner-btn primary"
        type="button"
        hidden
      >
        Restart now
      </button>
      <button
        id="update-banner-view"
        class="update-banner-btn"
        type="button"
        hidden
      >
        View release
      </button>
      <button
        id="update-banner-dismiss"
        class="update-banner-btn"
        type="button"
        hidden
        aria-label="Dismiss"
        title="Dismiss"
      >
        ×
      </button>
    </div>
```

(All four buttons now start `hidden`; the JS state machine in Task 4 shows the right ones per state. No CSS change is needed — buttons reuse `.update-banner-btn`, and the `@media print` rule already hides `#update-banner`.)

- [ ] **Step 2: Commit**

```bash
git add ui/index.html
git commit -m "Add Update now / Restart now buttons to update banner"
```

---

## Task 4: Frontend — rewrite the update section in app.js

**Files:**
- Modify: `ui/app.js:1-9` (imports), `:1755-1820` (update section)

- [ ] **Step 1: Add the update.js import**

In `ui/app.js`, the existing import block (lines 2-9) pulls from `./export.js`. Immediately after that block (after line 9), add:

```js
import {
  releaseUrlFor,
  bannerMessage,
  progressText,
} from "./update.js";
```

- [ ] **Step 2: Replace the update section**

In `ui/app.js`, replace the entire block from `/* ---- Update check ---- */` (line 1755) through the end of `showUpdateBanner` (the line with its closing `}` at ~1820, immediately before the `init()` call) with:

```js
/* ---- Update check & auto-update ---- */

const REPO = "larsakeekstrand/mdviewer";
const DISMISS_KEY = "mdviewer.update.dismissed_version";
const updaterApi = window.__TAURI__.updater;

const updateBanner = document.getElementById("update-banner");
const updateBannerText = document.getElementById("update-banner-text");
const updateBannerUpdate = document.getElementById("update-banner-update");
const updateBannerRestart = document.getElementById("update-banner-restart");
const updateBannerView = document.getElementById("update-banner-view");
const updateBannerDismiss = document.getElementById("update-banner-dismiss");

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

function openReleasePage(version) {
  return async () => {
    try {
      await invoke("open_url", { url: releaseUrlFor(REPO, version) });
    } catch (e) {
      console.error("open_url failed", e);
    }
  };
}

async function checkForUpdates({ silent = true } = {}) {
  let update;
  try {
    update = await updaterApi.check();
  } catch (e) {
    if (silent) {
      // No published release yet, network error, etc.
      console.debug("update check skipped:", e);
      return;
    }
    await dialogApi.message("Couldn't check for updates.\n\n" + e, {
      title: "MDViewer",
      kind: "error",
    });
    return;
  }

  if (update) {
    if (silent) {
      let dismissed = null;
      try {
        dismissed = localStorage.getItem(DISMISS_KEY);
      } catch (_) {}
      if (dismissed === update.version) return;
    }
    showUpdateAvailable(update);
    return;
  }

  if (!silent) {
    await dialogApi.message("You're on the latest version.", {
      title: "MDViewer",
      kind: "info",
    });
  }
}

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

async function runUpdate(update) {
  setUpdateButtons();
  let downloaded = 0;
  let contentLength = 0;
  updateBannerText.textContent = "Downloading…";

  try {
    await update.downloadAndInstall((event) => {
      switch (event.event) {
        case "Started":
          contentLength = event.data.contentLength ?? 0;
          break;
        case "Progress":
          downloaded += event.data.chunkLength;
          updateBannerText.textContent = progressText(downloaded, contentLength);
          break;
        case "Finished":
          updateBannerText.textContent = "Installing…";
          break;
      }
    });
  } catch (e) {
    console.error("update failed", e);
    updateBannerText.textContent = "Update failed: " + e;
    setUpdateButtons({ view: true, dismiss: true });
    updateBannerView.onclick = openReleasePage(update.version);
    updateBannerDismiss.onclick = () => {
      updateBanner.hidden = true;
    };
    return;
  }

  updateBannerText.textContent = "Update installed.";
  setUpdateButtons({ restart: true });
  updateBannerRestart.onclick = async () => {
    try {
      await invoke("restart");
    } catch (e) {
      console.error("restart failed", e);
    }
  };
}
```

(The menu path at `app.js:194` — `listen("menu-check-updates", … checkForUpdates({ silent: false }))` — is unchanged and now routes through `updaterApi.check()`. The `init().then(() => checkForUpdates())` call after this block is also unchanged.)

- [ ] **Step 3: Verify the JS suite still passes and no stale references remain**

Run: `node --test ui/*.test.js`
Expected: PASS (Task 1 tests + existing suites).

Run: `grep -n "check_for_updates\|showUpdateBanner\|updateBannerView.onclick = async" ui/app.js`
Expected: no matches (old command name and old function are gone).

- [ ] **Step 4: Rebuild so the bundled UI is current, then commit**

Frontend changes only take effect after a Rust rebuild (`tauri-codegen` bundles `frontendDist` at compile time). Run from `src-tauri/`:

```bash
cargo build
```

Expected: build succeeds. Then commit:

```bash
git add ui/app.js
git commit -m "Drive update banner from the updater plugin (download, install, restart)"
```

---

## Task 5: Generate the signing key and configure secrets (human-in-the-loop)

> **This task involves a private signing key and GitHub repo secrets that the agent cannot manage. Stop and have the maintainer perform these steps.**

**Files:**
- Modify: `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`)

- [ ] **Step 1: Generate the keypair**

The maintainer runs (requires `cargo install tauri-cli --version "^2"`):

```bash
cargo tauri signer generate -w ~/.tauri/mdviewer.key
```

This prints a **public key** and writes the password-protected private key to `~/.tauri/mdviewer.key` (and the public key to `~/.tauri/mdviewer.key.pub`).

- [ ] **Step 2: Back up the private key**

Store `~/.tauri/mdviewer.key` and its password in a password manager. **If lost, existing installs can no longer accept signed updates** — recovery requires shipping a new pubkey, which existing users can only get via a manual reinstall.

- [ ] **Step 3: Paste the public key into config**

In `src-tauri/tauri.conf.json`, replace `"REPLACE_WITH_GENERATED_PUBKEY"` with the printed public key string (the contents of `~/.tauri/mdviewer.key.pub`, a single base64-ish line).

- [ ] **Step 4: Add GitHub repo secrets**

The maintainer adds two repository secrets (Settings → Secrets and variables → Actions, or via CLI):

```bash
gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/mdviewer.key
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD   # paste the password when prompted
```

- [ ] **Step 5: Verify the build still succeeds with the real key**

Run from `src-tauri/`: `cargo build`
Expected: build succeeds.

- [ ] **Step 6: Commit the pubkey**

```bash
git add src-tauri/tauri.conf.json
git commit -m "Add updater signing public key"
```

---

## Task 6: CI — sign and publish latest.json

**Files:**
- Modify: `.github/workflows/release.yml:81-88`

- [ ] **Step 1: Pass the signing secrets to tauri-action**

In `.github/workflows/release.yml`, change the `Build and bundle` step's `env` block from:

```yaml
      - name: Build and bundle
        uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        with:
```

to:

```yaml
      - name: Build and bundle
        uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        with:
```

With these set and `createUpdaterArtifacts: true`, tauri-action signs the `.app.tar.gz` and auto-generates/attaches `latest.json` (its `includeUpdaterJson` defaults on).

- [ ] **Step 2: Mention auto-update in the release notes**

In `.github/workflows/release.yml`, in the `releaseBody:` heredoc, insert an `## Updating` section immediately before the `## Changes` line (match the existing 12-space indentation):

```yaml
            ## Updating

            MDViewer 1.5.0+ updates itself: when a newer version is published,
            the in-app banner offers **Update now**, which downloads and installs
            in place — no quarantine step needed. The manual DMG above is only
            for a first install or to move a pre-1.5.0 build onto the
            auto-updating track.

            ## Changes
```

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "Sign updater artifacts and publish latest.json in release CI"
```

---

## Task 7: Documentation

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Rewrite "Update check internals"**

In `CLAUDE.md`, replace the first two bullets of the `## Update check internals` section (the `updates::check()` bullet and the dismissal bullet) with:

```markdown
- Update detection is `tauri-plugin-updater`'s `check()` (frontend
  `window.__TAURI__.updater.check()`), which fetches the `latest.json` manifest
  from `releases/latest/download/latest.json`, compares against
  `CARGO_PKG_VERSION`, and verifies a minisign signature on download. There is
  no hand-rolled HTTP probe anymore (the old `updates.rs` + `ureq`/`semver` were
  removed).
- Update banner respects `localStorage.mdviewer.update.dismissed_version` —
  the silent startup check stays hidden for a dismissed version until a newer
  one appears. The menu-driven check ignores this dismissal. The banner is a
  state machine (available → downloading → installed/error); **Update now**
  calls `downloadAndInstall`, **Restart now** calls the `restart` command
  (`app.restart()`), and **View release** opens the reconstructed
  `releases/tag/v<version>` page via `open_url`.
```

(Keep the existing third bullet about `open_url` / `open_path` / `UNSAFE_OPEN_EXTS` unchanged.)

- [ ] **Step 2: Add an architecture-tour bullet**

In `CLAUDE.md`, in the `## Architecture quick-tour` section, add a bullet after the **Update check** bullet:

```markdown
- **Auto-update**: `tauri-plugin-updater` (registered in `lib.rs`, capability
  `updater:default`). The banner's **Update now** downloads the signed
  `.app.tar.gz` in-process, verifies the minisign signature against
  `plugins.updater.pubkey`, swaps the bundle, and **Restart now** relaunches via
  the `restart` command. Because the download is in-process, the new bundle is
  never quarantined — no `xattr` step on update (unlike the first manual DMG
  install).
```

- [ ] **Step 3: Add a "things that took hours" note**

In `CLAUDE.md`, in the `## Things that took hours and shouldn't again` section, add:

```markdown
- **Auto-update signing key is operational state**: the minisign keypair
  (`cargo tauri signer generate`) is separate from Apple signing. The public key
  lives in `tauri.conf.json` `plugins.updater.pubkey`; the private key + password
  are CI secrets `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
  Lose the private key and existing installs reject all future updates (recovery
  = new pubkey = forced manual reinstall). Back it up. `latest/download/latest.json`
  only resolves to the *published* release, so `gh release edit --draft=false` is
  the auto-update go-live trigger. The updater needs a built `.app` — it does
  nothing under `cargo run`. Users on ≤1.4.0 (pre-updater) need one last manual
  DMG hop onto the first updater-enabled release.
```

- [ ] **Step 4: Add the signing step to "Cutting a release"**

In `CLAUDE.md`, under `### Cutting a release`, add a note after the numbered list:

```markdown
The release workflow signs the `.app.tar.gz` and attaches `latest.json` when the
`TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` secrets are
set (one-time setup). Publishing the draft (step 6) is what makes the update
reach existing installs.
```

- [ ] **Step 5: Commit**

```bash
git add CLAUDE.md
git commit -m "Document auto-update architecture and signing-key handling"
```

---

## Task 8: Version bump and final verification

**Files:**
- Modify: `src-tauri/Cargo.toml` (version), `src-tauri/tauri.conf.json` (version)

- [ ] **Step 1: Bump the version to 1.5.0**

In `src-tauri/Cargo.toml`, set `version = "1.5.0"`. In `src-tauri/tauri.conf.json`, set `"version": "1.5.0"`. Then refresh the lockfile:

```bash
cd src-tauri && cargo update -p mdviewer
```

- [ ] **Step 2: Full local verification**

Run from `src-tauri/`:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Then from the repo root:

```bash
node --test ui/*.test.js
```

Expected: all clean / passing.

- [ ] **Step 3: Manual smoke test (built app)**

Build the bundle and confirm the app launches and the banner wiring is intact:

```bash
cd src-tauri && cargo tauri build
```

Open `src-tauri/target/release/bundle/macos/MDViewer.app`. With no newer release published, the banner should stay hidden and **MDViewer ▸ Check for Updates…** should show "You're on the latest version." (End-to-end download/install/restart can only be verified once a higher version's `latest.json` is published — see Step 4.)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/tauri.conf.json src-tauri/Cargo.lock
git commit -m "Bump to 1.5.0"
```

- [ ] **Step 5: End-to-end verification at release time (note, not a code step)**

After tagging `v1.5.0`, letting CI build, and **publishing** the draft (`gh release edit v1.5.0 --draft=false`): install 1.5.0, then later publish a `v1.5.1` (even a trivial bump). Launch 1.5.0 and confirm the banner offers **Update now** → progress → **Restart now** → relaunches into 1.5.1 with **no quarantine prompt**. Negative test: run the app from a read-only location and confirm the **Update failed** state with the **View release** fallback.

---

## Notes for the executor

- **No CSP change** is required: the updater's HTTP requests run in Rust, outside the webview, so `tauri.conf.json` `app.security.csp` does not govern them.
- **Commit style** (per `CLAUDE.md`): imperative subject, **no** `Co-Authored-By` trailer.
- **Lint gate**: `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` from `src-tauri/` must be clean before every Rust commit; CI enforces both with `-D warnings`.
- The updater does nothing under `cargo run`; only a `cargo tauri build` bundle exercises it.
