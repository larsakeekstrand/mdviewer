# Beta Update Channel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let MDViewer publish release-candidate (beta) builds through the existing auto-updater and let users opt in to receive them, without affecting stable users.

**Architecture:** Betas publish as GitHub *prereleases* to a single rolling `beta` release (stable URL `releases/download/beta/latest.json`). A persisted per-user `channel` preference (in `recent.json`) selects the endpoint at *check time* via a custom `check_update` Rust command that reuses the plugin's hardened `download_and_install`. A small Preferences window (opened from `MDViewer ▸ Settings…`) toggles the channel. A stable release also promotes its manifest onto the `beta` release so testers roll forward (superset).

**Tech Stack:** Rust / Tauri 2.11, `tauri-plugin-updater` 2.10, vanilla JS frontend, GitHub Actions + `tauri-action`.

**Reference spec:** `docs/superpowers/specs/2026-06-01-beta-update-channel-design.md`

---

## File map

- `src-tauri/src/recent.rs` — add `UpdateChannel` enum, `Store.channel`, `load_channel`/`save_channel` (+ tests).
- `src-tauri/src/commands.rs` — add `check_update`, `get_preferences`, `set_update_channel` commands + channel-URL constants.
- `src-tauri/src/lib.rs` — register the three new commands in `generate_handler!`.
- `src-tauri/capabilities/default.json` — add the `preferences` window label.
- `ui/preferences.html` + `ui/preferences.js` — the Settings window (new files).
- `src-tauri/src/menu.rs` — add `Settings…` menu item + open/focus the window.
- `ui/app.js` — swap `updaterApi.check()` for `invoke("check_update")` + `wrapUpdate` shim.
- `.github/workflows/release.yml` — branch stable vs. rolling-`beta` publishing.
- `.github/workflows/promote-beta.yml` — on stable publish, copy its manifest onto `beta` (new file).
- `README.md`, `CLAUDE.md` — docs.

---

## Task 1: Channel persistence in `recent.rs`

**Files:**
- Modify: `src-tauri/src/recent.rs`
- Test: `src-tauri/src/recent.rs` (inline `#[cfg(test)]` module, matching existing pattern)

- [ ] **Step 1: Write the failing tests**

Add these tests inside the existing `mod tests { ... }` block at the bottom of `src-tauri/src/recent.rs` (after the last test, before the closing `}`):

```rust
    #[test]
    fn store_defaults_channel_to_stable() {
        let s = Store::default();
        assert_eq!(s.channel, UpdateChannel::Stable);
    }

    #[test]
    fn deserializes_legacy_store_without_channel() {
        let back: Store = serde_json::from_str(r#"{"folders":["/a"]}"#).unwrap();
        assert_eq!(back.channel, UpdateChannel::Stable);
    }

    #[test]
    fn channel_round_trips_as_lowercase() {
        let s = Store {
            channel: UpdateChannel::Beta,
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"channel\":\"beta\""), "got: {json}");
        let back: Store = serde_json::from_str(&json).unwrap();
        assert_eq!(back.channel, UpdateChannel::Beta);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test --lib recent::tests 2>&1 | tail -20`
Expected: FAIL — compile errors ("no field `channel`", "cannot find type `UpdateChannel`").

- [ ] **Step 3: Add the enum, the field, and the accessors**

In `src-tauri/src/recent.rs`, after the `use` lines at the top, add the enum:

```rust
/// Which release stream the auto-updater follows. Persisted in `recent.json`;
/// read at update-check time to pick the manifest endpoint.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Beta,
}
```

Add the field to `Store` (after `active_tab`):

```rust
    #[serde(default)]
    channel: UpdateChannel,
```

Add the accessors next to `load_last`/`save_last`:

```rust
pub fn load_channel(app: &AppHandle) -> UpdateChannel {
    load_store(app).channel
}

/// Persists the update channel, preserving every other field.
pub fn save_channel(app: &AppHandle, channel: UpdateChannel) {
    let mut store = load_store(app);
    store.channel = channel;
    write_store(app, &store);
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --lib recent::tests 2>&1 | tail -20`
Expected: PASS (all recent tests, including the 3 new ones).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/recent.rs
git commit -m "Add persisted update-channel preference to recent store"
```

---

## Task 2: Channel-aware commands in `commands.rs`

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

These commands need a live webview + network, so they aren't unit-tested; they're verified by `cargo build` + clippy here and manual smoke later (Task 9 / release).

- [ ] **Step 1: Add the constants and structs**

At the top of `src-tauri/src/commands.rs`, after the existing `use` block, add:

```rust
const STABLE_UPDATE_URL: &str =
    "https://github.com/larsakeekstrand/mdviewer/releases/latest/download/latest.json";
const BETA_UPDATE_URL: &str =
    "https://github.com/larsakeekstrand/mdviewer/releases/download/beta/latest.json";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMeta {
    rid: tauri::ResourceId,
    version: String,
    current_version: String,
    body: Option<String>,
}

#[derive(Serialize)]
pub struct Preferences {
    pub channel: recent::UpdateChannel,
    pub version: String,
}
```

- [ ] **Step 2: Add the three commands**

Add to `src-tauri/src/commands.rs` (e.g. just after the `restart` command):

```rust
#[tauri::command]
pub fn get_preferences(app: AppHandle) -> Preferences {
    Preferences {
        channel: recent::load_channel(&app),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

#[tauri::command]
pub fn set_update_channel(app: AppHandle, channel: recent::UpdateChannel) {
    recent::save_channel(&app, channel);
}

/// Checks for an update on the user's selected channel. Mirrors the updater
/// plugin's own `check`, but overrides the endpoint per the stored channel and
/// returns the resource id so the frontend can hand it to the plugin's
/// (unchanged) `download_and_install`.
#[tauri::command]
pub async fn check_update(
    app: AppHandle,
    webview: tauri::Webview,
) -> Result<Option<UpdateMeta>, String> {
    use tauri::Manager;
    use tauri_plugin_updater::UpdaterExt;

    let url = match recent::load_channel(&app) {
        recent::UpdateChannel::Beta => BETA_UPDATE_URL,
        recent::UpdateChannel::Stable => STABLE_UPDATE_URL,
    };
    let endpoint = tauri::Url::parse(url).map_err(|e| format!("bad updater endpoint: {e}"))?;

    let updater = webview
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|e| format!("updater endpoints: {e}"))?
        .build()
        .map_err(|e| format!("updater build: {e}"))?;

    let update = updater
        .check()
        .await
        .map_err(|e| format!("update check failed: {e}"))?;

    match update {
        Some(update) => {
            // Read fields before moving `update` into the resource table.
            let version = update.version.clone();
            let current_version = update.current_version.clone();
            let body = update.body.clone();
            let rid = webview.resources_table().add(update);
            Ok(Some(UpdateMeta {
                rid,
                version,
                current_version,
                body,
            }))
        }
        None => Ok(None),
    }
}
```

> Note: `tauri::Url` is the re-export of `url::Url` (tauri lib.rs: `pub use url::Url;`) — no new dependency. `webview.resources_table().add(update)` is exactly what the plugin's own `check` does.

- [ ] **Step 3: Register the commands in `lib.rs`**

In `src-tauri/src/lib.rs`, inside `tauri::generate_handler![ ... ]`, add three entries (e.g. after `commands::restart,`):

```rust
            commands::get_preferences,
            commands::set_update_channel,
            commands::check_update,
```

- [ ] **Step 4: Build + clippy to verify it compiles cleanly**

Run: `cd src-tauri && cargo build 2>&1 | tail -20 && cargo clippy --all-targets -- -D warnings 2>&1 | tail -20`
Expected: builds; clippy clean (no warnings).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "Add channel-aware check_update and preferences commands"
```

---

## Task 3: Grant the preferences window IPC permissions

**Files:**
- Modify: `src-tauri/capabilities/default.json`

A window not listed in any capability has no `core` permissions, so its `invoke` calls fail. Add the `preferences` label.

- [ ] **Step 1: Add the window label**

In `src-tauri/capabilities/default.json`, change:

```json
  "windows": [
    "main"
  ],
```

to:

```json
  "windows": [
    "main",
    "preferences"
  ],
```

- [ ] **Step 2: Verify JSON is valid**

Run: `python3 -m json.tool src-tauri/capabilities/default.json > /dev/null && echo OK`
Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add src-tauri/capabilities/default.json
git commit -m "Grant preferences window IPC capabilities"
```

---

## Task 4: Preferences window UI

**Files:**
- Create: `ui/preferences.html`
- Create: `ui/preferences.js`

- [ ] **Step 1: Create `ui/preferences.html`**

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Settings</title>
    <style>
      :root {
        color-scheme: light dark;
        font-family:
          -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
      }
      body {
        margin: 0;
        padding: 20px;
        font-size: 13px;
        line-height: 1.5;
      }
      h1 {
        font-size: 15px;
        margin: 0 0 16px;
      }
      .row {
        display: flex;
        align-items: flex-start;
        gap: 8px;
        margin-bottom: 12px;
      }
      .row input {
        margin-top: 2px;
      }
      .hint {
        opacity: 0.7;
        margin: 4px 0 0 24px;
      }
      .version {
        margin-top: 20px;
        opacity: 0.7;
      }
    </style>
  </head>
  <body>
    <h1>Settings</h1>
    <div class="row">
      <input type="checkbox" id="beta-toggle" />
      <label for="beta-toggle">
        Receive beta (pre-release) updates
        <div class="hint">
          Beta builds may be unstable. You can switch back to stable releases at
          any time; the change takes effect at the next update check.
        </div>
      </label>
    </div>
    <div class="version" id="version">Current version: …</div>
    <script type="module" src="preferences.js"></script>
  </body>
</html>
```

- [ ] **Step 2: Create `ui/preferences.js`**

```js
// Settings window logic. External module only (CSP script-src 'self').
const { invoke } = window.__TAURI__.core;

const toggle = document.getElementById("beta-toggle");
const versionEl = document.getElementById("version");

async function load() {
  const prefs = await invoke("get_preferences");
  toggle.checked = prefs.channel === "beta";
  versionEl.textContent = `Current version: ${prefs.version}`;
}

toggle.addEventListener("change", async () => {
  const channel = toggle.checked ? "beta" : "stable";
  try {
    await invoke("set_update_channel", { channel });
  } catch (e) {
    console.error("set_update_channel failed", e);
    // Revert the visual state so it reflects what was actually persisted.
    toggle.checked = !toggle.checked;
  }
});

load().catch((e) => console.error("loading preferences failed", e));
```

- [ ] **Step 3: Commit**

```bash
git add ui/preferences.html ui/preferences.js
git commit -m "Add preferences window UI"
```

---

## Task 5: Settings menu item that opens the window

**Files:**
- Modify: `src-tauri/src/menu.rs`

- [ ] **Step 1: Extend the menu imports**

In `src-tauri/src/menu.rs`, change the import line:

```rust
use tauri::{AppHandle, Emitter, Wry};
```

to:

```rust
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, Wry};
```

- [ ] **Step 2: Add the open-settings helper**

Add this function to `src-tauri/src/menu.rs` (e.g. above `prompt_open_file`):

```rust
fn open_settings(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("preferences") {
        let _ = win.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(
        app,
        "preferences",
        WebviewUrl::App("preferences.html".into()),
    )
    .title("Settings")
    .inner_size(440.0, 230.0)
    .resizable(false)
    .build();
}
```

- [ ] **Step 3: Build the menu item and add it to the app menu**

In `rebuild`, after the `check_updates` item is built (around the `MenuItemBuilder::with_id("check-updates", ...)` line), add:

```rust
    let settings = MenuItemBuilder::with_id("settings", "Settings…")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;
```

Then in the `app_menu_builder` chain, insert the settings item. Change:

```rust
    let app_menu_builder = SubmenuBuilder::new(app, "MDViewer")
        .about(None)
        .item(&github_source)
        .item(&check_updates);
    #[cfg(target_os = "macos")]
    let app_menu_builder = app_menu_builder.item(&install_cli);
    let app_menu = app_menu_builder
        .separator()
```

to:

```rust
    let app_menu_builder = SubmenuBuilder::new(app, "MDViewer")
        .about(None)
        .item(&github_source)
        .item(&check_updates);
    #[cfg(target_os = "macos")]
    let app_menu_builder = app_menu_builder.item(&install_cli);
    let app_menu = app_menu_builder
        .separator()
        .item(&settings)
        .separator()
```

- [ ] **Step 4: Handle the menu event**

In `install`, inside the `match id.as_str()` block, add an arm (e.g. after the `"check-updates"` arm):

```rust
            "settings" => open_settings(app),
```

- [ ] **Step 5: Build + clippy**

Run: `cd src-tauri && cargo build 2>&1 | tail -20 && cargo clippy --all-targets -- -D warnings 2>&1 | tail -20`
Expected: builds; clippy clean.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/menu.rs
git commit -m "Add Settings menu item that opens the preferences window"
```

---

## Task 6: Point the frontend update check at the channel-aware command

**Files:**
- Modify: `ui/app.js`

- [ ] **Step 1: Add the `wrapUpdate` shim**

In `ui/app.js`, replace this line (the `updaterApi` definition, ~line 2162):

```js
const updaterApi = window.__TAURI__.updater;
```

with the shim (we no longer use the plugin's JS `check`, but still reuse its `download_and_install`):

```js
/** Wrap the metadata returned by the `check_update` command into the same
 *  surface the banner already consumes. `downloadAndInstall` reuses the updater
 *  plugin's own command via the resource id. */
function wrapUpdate(meta) {
  if (!meta) return null;
  return {
    version: meta.version,
    currentVersion: meta.currentVersion,
    body: meta.body,
    async downloadAndInstall(onEvent) {
      const channel = new window.__TAURI__.core.Channel();
      if (onEvent) channel.onmessage = onEvent;
      await invoke("plugin:updater|download_and_install", {
        rid: meta.rid,
        onEvent: channel,
      });
    },
  };
}
```

- [ ] **Step 2: Swap the check call**

In `checkForUpdates` (~line 2260), replace:

```js
    update = await updaterApi.check();
```

with:

```js
    update = wrapUpdate(await invoke("check_update"));
```

- [ ] **Step 3: Rebuild and sanity-check (no stale-UI trap)**

Frontend changes require a Rust rebuild (Tauri bundles `frontendDist` at compile time).

Run: `cd src-tauri && cargo build 2>&1 | tail -5`
Expected: builds. (Runtime verification happens in Task 9 / against a real release.)

- [ ] **Step 4: Commit**

```bash
git add ui/app.js
git commit -m "Use channel-aware check_update command in the update banner"
```

---

## Task 7: Branch the release workflow for the rolling beta channel

**Files:**
- Modify: `.github/workflows/release.yml`

Replace the whole file with the version below. Changes vs. current: a `meta` job computes channel facts; a `prepare-beta` job resets the rolling `beta` release/tag (serialized, no asset-clearing race); the build + polish jobs parameterize tag/name/prerelease/draft from `meta` and tolerate the skipped `prepare-beta` on stable tags.

- [ ] **Step 1: Overwrite `.github/workflows/release.yml`**

```yaml
name: Release

on:
  push:
    tags:
      - "v*"
  workflow_dispatch:
    inputs:
      tag:
        description: "Tag to release as (e.g. v0.1.0 or v0.1.0-rc.1). Must already exist as a git tag."
        required: true
        type: string

permissions:
  contents: write

env:
  CARGO_TERM_COLOR: always
  FORCE_JAVASCRIPT_ACTIONS_TO_NODE24: "true"

jobs:
  meta:
    name: Resolve release facts
    runs-on: ubuntu-latest
    outputs:
      tag: ${{ steps.r.outputs.tag }}
      release_tag: ${{ steps.r.outputs.release_tag }}
      release_name: ${{ steps.r.outputs.release_name }}
      is_prerelease: ${{ steps.r.outputs.is_prerelease }}
      is_draft: ${{ steps.r.outputs.is_draft }}
    steps:
      - name: Resolve
        id: r
        env:
          DISPATCH_TAG: ${{ inputs.tag }}
          REF_TAG: ${{ github.ref_name }}
          EVENT_NAME: ${{ github.event_name }}
        run: |
          set -euo pipefail
          if [ "$EVENT_NAME" = "workflow_dispatch" ]; then
            TAG="$DISPATCH_TAG"
          else
            TAG="$REF_TAG"
          fi
          echo "tag=$TAG" >> "$GITHUB_OUTPUT"
          # A "-" in the semver tag (e.g. v1.16.0-rc.1) marks a prerelease.
          case "$TAG" in
            *-*)
              echo "release_tag=beta" >> "$GITHUB_OUTPUT"
              echo "release_name=Beta channel" >> "$GITHUB_OUTPUT"
              echo "is_prerelease=true" >> "$GITHUB_OUTPUT"
              echo "is_draft=false" >> "$GITHUB_OUTPUT"
              ;;
            *)
              echo "release_tag=$TAG" >> "$GITHUB_OUTPUT"
              echo "release_name=$TAG" >> "$GITHUB_OUTPUT"
              echo "is_prerelease=false" >> "$GITHUB_OUTPUT"
              echo "is_draft=true" >> "$GITHUB_OUTPUT"
              ;;
          esac

  prepare-beta:
    name: Reset rolling beta release
    needs: meta
    if: ${{ needs.meta.outputs.is_prerelease == 'true' }}
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
        with:
          ref: ${{ needs.meta.outputs.tag }}
          fetch-depth: 0
      - name: Delete old beta release, move beta tag to this commit
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          set -euo pipefail
          # Drop the old release (keep its tag for a moment), then repoint the
          # rolling `beta` tag at the current prerelease commit. The build jobs
          # recreate the release fresh against this tag.
          gh release delete beta --yes || true
          git tag -f beta
          git push -f origin beta

  build-macos:
    name: Build macOS (aarch64)
    needs: [meta, prepare-beta]
    if: ${{ always() && needs.meta.result == 'success' && needs.prepare-beta.result != 'failure' }}
    runs-on: macos-14
    outputs:
      changelog: ${{ steps.changelog.outputs.value }}
    steps:
      - uses: actions/checkout@v6
        with:
          ref: ${{ needs.meta.outputs.tag }}
          fetch-depth: 0

      - name: Build changelog
        id: changelog
        env:
          CURRENT: ${{ needs.meta.outputs.tag }}
        run: |
          set -euo pipefail
          PREV=$(git describe --tags --abbrev=0 "${CURRENT}^" 2>/dev/null || true)
          echo "current=$CURRENT" >&2
          echo "previous=$PREV" >&2

          if [ -z "$PREV" ]; then
            HEADER="Initial release."
            LOG=$(git log HEAD --pretty=format:'- %s (%h)' --no-merges --reverse)
          else
            HEADER="Commits since [${PREV}](https://github.com/${GITHUB_REPOSITORY}/releases/tag/${PREV}):"
            LOG=$(git log "${PREV}..HEAD" --pretty=format:'- %s (%h)' --no-merges --reverse)
          fi

          {
            echo "value<<CHANGELOG_EOF"
            echo "$HEADER"
            echo ""
            echo "$LOG"
            echo "CHANGELOG_EOF"
          } >> "$GITHUB_OUTPUT"

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: aarch64-apple-darwin

      - name: Cache cargo
        uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri

      - name: Build and bundle
        uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        with:
          projectPath: src-tauri
          tagName: ${{ needs.meta.outputs.release_tag }}
          releaseName: ${{ needs.meta.outputs.release_name }}
          releaseBody: |
            ## Install (Apple Silicon)

            Download `MDViewer_<version>_macOS_aarch64.dmg`, open it, and drag
            `MDViewer.app` to Applications.

            ### Required: remove the quarantine flag

            Builds are ad-hoc signed but not signed by an Apple Developer ID.
            On macOS 15 (Sequoia) and later, browser-downloaded unsigned apps
            are reported as "damaged" by Gatekeeper. Run this once after
            installing:

                sudo xattr -dr com.apple.quarantine /Applications/MDViewer.app

            Then double-click the app in Applications. It opens normally
            from then on.

            ## Updating

            MDViewer 1.5.0+ updates itself: when a newer version is published,
            the in-app banner offers **Update now**, which downloads and installs
            in place — no quarantine step needed. The manual DMG above is only
            for a first install or to move a pre-1.5.0 build onto the
            auto-updating track.

            ## Install (Windows x86_64)

            Download either:

            - `MDViewer_<version>_Windows_x64-setup.exe` (NSIS, smaller, per-user install)
            - `MDViewer_<version>_Windows_x64_en-US.msi` (MSI, enterprise / GPO deploy)

            ### First-run SmartScreen warning

            Builds are unsigned. Windows Defender SmartScreen will show
            "Windows protected your PC". Click **More info** → **Run anyway**
            once. After install, MDViewer launches normally from the Start Menu.

            ### WebView2

            The installer auto-downloads Microsoft Edge WebView2 Runtime if
            it isn't already present (it usually is on Windows 10 1903+
            and Windows 11). Internet access required during install in
            that case.

            ## Changes

            ${{ steps.changelog.outputs.value }}
          releaseDraft: ${{ needs.meta.outputs.is_draft }}
          prerelease: ${{ needs.meta.outputs.is_prerelease }}
          args: --target aarch64-apple-darwin

  build-windows:
    name: Build Windows (x86_64)
    needs: [meta, prepare-beta]
    if: ${{ always() && needs.meta.result == 'success' && needs.prepare-beta.result != 'failure' }}
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v6
        with:
          ref: ${{ needs.meta.outputs.tag }}
          fetch-depth: 0

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: x86_64-pc-windows-msvc

      - name: Cache cargo
        uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri

      - name: Build and bundle
        uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        with:
          projectPath: src-tauri
          tagName: ${{ needs.meta.outputs.release_tag }}
          releaseName: ${{ needs.meta.outputs.release_name }}
          releaseDraft: ${{ needs.meta.outputs.is_draft }}
          prerelease: ${{ needs.meta.outputs.is_prerelease }}

  polish-release:
    name: Rename installers for clarity
    needs: [meta, build-macos, build-windows]
    if: ${{ always() && needs.build-macos.result == 'success' && needs.build-windows.result == 'success' }}
    runs-on: ubuntu-latest
    steps:
      - name: Rename installers and patch latest.json
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          REPO: ${{ github.repository }}
          TAG: ${{ needs.meta.outputs.tag }}
          RELEASE_TAG: ${{ needs.meta.outputs.release_tag }}
          CHANGELOG: ${{ needs.build-macos.outputs.changelog }}
        run: |
          set -euo pipefail

          # Tauri's bundler embeds the bare semver (no leading "v", no pre-release
          # suffix) in installer filenames. Derive it from the tag so this works
          # for both vX.Y.Z and vX.Y.Z-rc.N tags.
          VERSION=$(echo "$TAG" | sed -E 's/^v([0-9]+\.[0-9]+\.[0-9]+).*/\1/')
          # Release operations target the *published* release tag, which is the
          # rolling "beta" for prereleases and the actual tag for stable.
          RELEASE_ID=$(gh api "repos/$REPO/releases" --jq ".[] | select(.tag_name == \"$RELEASE_TAG\") | .id")

          rename_asset() {
            local old="$1"; local new="$2"
            local id
            id=$(gh api "repos/$REPO/releases/$RELEASE_ID/assets" --jq ".[] | select(.name == \"$old\") | .id")
            if [ -z "$id" ]; then
              echo "::warning::Asset not found: $old (skipping)"
              return 0
            fi
            gh api -X PATCH "repos/$REPO/releases/assets/$id" -f name="$new" > /dev/null
            echo "Renamed: $old -> $new"
          }

          rename_asset "MDViewer_${VERSION}_aarch64.dmg"          "MDViewer_${VERSION}_macOS_aarch64.dmg"
          rename_asset "MDViewer_${VERSION}_x64-setup.exe"        "MDViewer_${VERSION}_Windows_x64-setup.exe"
          rename_asset "MDViewer_${VERSION}_x64-setup.exe.sig"    "MDViewer_${VERSION}_Windows_x64-setup.exe.sig"
          rename_asset "MDViewer_${VERSION}_x64_en-US.msi"        "MDViewer_${VERSION}_Windows_x64_en-US.msi"
          rename_asset "MDViewer_${VERSION}_x64_en-US.msi.sig"    "MDViewer_${VERSION}_Windows_x64_en-US.msi.sig"

          LATEST_ID=$(gh api "repos/$REPO/releases/$RELEASE_ID/assets" --jq '.[] | select(.name == "latest.json") | .id')
          if [ -z "$LATEST_ID" ]; then
            echo "::warning::latest.json not found on the release; skipping URL patch"
            exit 0
          fi

          gh api -H "Accept: application/octet-stream" "repos/$REPO/releases/assets/$LATEST_ID" > latest.json
          sed -i -E "
            s|MDViewer_${VERSION}_x64-setup\.exe|MDViewer_${VERSION}_Windows_x64-setup.exe|g
            s|MDViewer_${VERSION}_x64_en-US\.msi|MDViewer_${VERSION}_Windows_x64_en-US.msi|g
          " latest.json

          NOTES=$(printf '## Changes\n\n%s\n' "$CHANGELOG")
          jq --arg notes "$NOTES" '.notes = $notes' latest.json > latest.json.tmp
          mv latest.json.tmp latest.json

          python3 -m json.tool latest.json > /dev/null

          gh api -X DELETE "repos/$REPO/releases/assets/$LATEST_ID"
          gh release upload "$RELEASE_TAG" latest.json -R "$REPO"
```

- [ ] **Step 2: Lint the YAML**

Run: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/release.yml')); print('OK')"`
Expected: `OK` (install pyyaml first if missing: `python3 -m pip install --quiet pyyaml`).

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "Publish rc tags to a rolling beta release"
```

---

## Task 8: Promote stable releases onto the beta channel (superset)

**Files:**
- Create: `.github/workflows/promote-beta.yml`

When a stable release is published (`released` fires only for non-prerelease, non-draft), copy its `latest.json` onto the rolling `beta` release. The manifest's asset URLs are absolute and point at the stable release, so beta testers roll onto stable without re-uploading binaries.

- [ ] **Step 1: Create `.github/workflows/promote-beta.yml`**

```yaml
name: Promote stable to beta channel

on:
  release:
    types: [released]

permissions:
  contents: write

jobs:
  promote:
    name: Copy stable latest.json onto the beta release
    if: ${{ !github.event.release.prerelease && github.event.release.tag_name != 'beta' }}
    runs-on: ubuntu-latest
    steps:
      - name: Mirror manifest
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          REPO: ${{ github.repository }}
          STABLE_TAG: ${{ github.event.release.tag_name }}
        run: |
          set -euo pipefail

          STABLE_ID=$(gh api "repos/$REPO/releases" --jq ".[] | select(.tag_name == \"$STABLE_TAG\") | .id")
          SRC_ID=$(gh api "repos/$REPO/releases/$STABLE_ID/assets" --jq '.[] | select(.name == "latest.json") | .id')
          if [ -z "$SRC_ID" ]; then
            echo "::warning::stable release has no latest.json; nothing to promote"
            exit 0
          fi
          gh api -H "Accept: application/octet-stream" "repos/$REPO/releases/assets/$SRC_ID" > latest.json
          python3 -m json.tool latest.json > /dev/null

          # The beta release must already exist (it does once any rc has shipped).
          BETA_ID=$(gh api "repos/$REPO/releases" --jq '.[] | select(.tag_name == "beta") | .id')
          if [ -z "$BETA_ID" ]; then
            echo "::warning::no beta release yet; skipping promotion"
            exit 0
          fi
          OLD_ID=$(gh api "repos/$REPO/releases/$BETA_ID/assets" --jq '.[] | select(.name == "latest.json") | .id')
          if [ -n "$OLD_ID" ]; then
            gh api -X DELETE "repos/$REPO/releases/assets/$OLD_ID"
          fi
          gh release upload beta latest.json -R "$REPO"
          echo "Promoted $STABLE_TAG manifest onto the beta channel."
```

- [ ] **Step 2: Lint the YAML**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/promote-beta.yml')); print('OK')"`
Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/promote-beta.yml
git commit -m "Promote published stable manifest onto the beta channel"
```

---

## Task 9: Documentation

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`

- [ ] **Step 1: Add a README "Beta updates" subsection**

Find the README section that documents updating (search for "Update now" or "auto-update"). Add, immediately after it:

```markdown
### Beta updates

MDViewer can track a beta (pre-release) channel. Open **MDViewer ▸ Settings…**
(⌘,) and tick **Receive beta (pre-release) updates** to opt in. From then on the
in-app update check follows the beta channel, which always offers the newest
build — including stable releases — so a beta tester is never left behind a
final release. Untick the box to return to stable-only updates; the change takes
effect at the next update check (no restart needed). Beta builds may be less
stable than final releases.
```

Run to find the anchor: `grep -n "Update now\|Updating\|auto-update" README.md | head`

- [ ] **Step 2: Add CLAUDE.md architecture note**

In `CLAUDE.md`, in the "Architecture quick-tour" list, add a bullet after the **Auto-update** bullet:

```markdown
- **Beta channel**: a persisted `channel` (`stable`/`beta`) in `recent.json`
  selects the updater endpoint. The bundled updater plugin can't switch
  endpoints at runtime, so a custom `commands::check_update` builds
  `webview.updater_builder().endpoints(...)` from the stored channel, adds the
  resulting `Update` to the webview resource table, and returns its `rid`; the
  frontend `wrapUpdate` shim (`app.js`) hands that `rid` to the unchanged
  `plugin:updater|download_and_install`. Channel is read per check, so toggling
  it in **MDViewer ▸ Settings…** (`ui/preferences.html`/`.js`, a `preferences`
  window listed in `capabilities/default.json`) takes effect at the next check —
  no relaunch. Betas publish as GitHub *prereleases* to a single rolling `beta`
  release (`releases/download/beta/latest.json`); `release.yml` branches on a
  `-` in the tag, and `promote-beta.yml` copies a published stable `latest.json`
  onto the `beta` release so testers roll onto stable (superset model).
```

- [ ] **Step 3: Add a CLAUDE.md beta-release recipe**

In `CLAUDE.md`, under "### Cutting a release", add after the numbered list:

```markdown
**Cutting a beta:** bump `Cargo.toml` + `tauri.conf.json` to a prerelease
version (e.g. `1.16.0-rc.1`), `cargo update -p mdviewer`, commit, then
`git tag v1.16.0-rc.1 && git push origin v1.16.0-rc.1`. The release workflow
detects the `-` and publishes to the rolling `beta` release (prerelease,
non-draft) instead of a draft — beta-opted installs pick it up automatically.
When the matching stable `vX.Y.Z` is later published, `promote-beta.yml` rolls
beta testers onto it. Smoke-test the manifest with
`gh release view beta --json assets` after the run.
```

- [ ] **Step 4: Commit**

```bash
git add README.md CLAUDE.md
git commit -m "Document the beta update channel"
```

---

## Task 10: Full verification sweep

**Files:** none (verification only)

- [ ] **Step 1: Rust format + lint + tests**

Run:
```bash
cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
Expected: fmt clean (no diff), clippy clean, all tests pass (including the 3 new `recent` tests).

- [ ] **Step 2: Frontend pure-helper tests still pass**

Run: `node --test ui/*.test.js 2>&1 | tail -15` (from repo root; the exact command `ci.yml` runs).
Expected: PASS, no regressions. (No new node tests are required — the new frontend code is DOM/IPC glue, not pure helpers.)

- [ ] **Step 3: Release build**

Run: `cd src-tauri && cargo build --release 2>&1 | tail -5`
Expected: builds.

- [ ] **Step 4: Manual smoke checklist (record results, do not skip)**

This needs a bundled app + a real `-rc` tag, so it is the human/operator gate:

1. `cargo tauri build`, launch, open **MDViewer ▸ Settings…** (⌘,). The window
   shows the checkbox unchecked and the correct current version.
2. Tick the box; reopen Settings → still ticked (persisted). Check
   `recent.json` contains `"channel":"beta"`.
3. Push a throwaway `vX.Y.Z-rc.1` tag; confirm the `beta` release is created
   (prerelease) with a signed `latest.json` and renamed Windows installers
   (`gh release view beta --json assets`).
4. A beta-opted install offers the update; a stable install does not.
5. Click **Update now** → downloads + installs (verifies the `rid` handoff).
6. Publish a stable `vX.Y.Z`; confirm `promote-beta.yml` updated the `beta`
   release's `latest.json` and the beta install rolls onto stable.

- [ ] **Step 5: Final commit (if any verification fixups were needed)**

```bash
git add -A
git commit -m "Beta channel verification fixups"
```
(Skip if nothing changed.)

---

## Notes for the implementer

- **`-rc.N` vs `-rcN`:** existing CLAUDE.md prose uses `-rc1`; this plan
  standardizes on `-rc.N`. Either still satisfies the `*-*` prerelease detection
  and the `polish-release` version regex. Use `-rc.N`.
- **Rolling `beta` tag commit is cosmetic:** `prepare-beta` force-moves the
  `beta` tag to the rc commit for tidiness; the updater resolves assets by
  absolute URL, not by tag commit, so correctness doesn't depend on it.
- **No CSP widening:** the preferences window uses external JS only; do not add
  `'unsafe-inline'` to `script-src`. (Its small `<style>` block is fine —
  `style-src` already allows `'unsafe-inline'`.)
- **Don't change the bundle identifier** (`com.mdviewer.app`): the channel pref
  lives in `recent.json` under `app_data_dir()`, which is bundle-id-keyed.
```
</content>
