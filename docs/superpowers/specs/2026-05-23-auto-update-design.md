# Auto-update via `tauri-plugin-updater` — design

Date: 2026-05-23
Status: approved, ready for implementation plan

## Goal

The existing startup update banner gains an **Update now** button. Clicking it
downloads the new `.app.tar.gz` in-process, verifies a minisign signature, swaps
the app bundle in place, and — after the user clicks **Restart now** — relaunches
into the new version. No DMG, no manual drag, no `xattr` quarantine dance.

## Why this is feasible without an Apple Developer ID

- macOS `com.apple.quarantine` is only applied to files downloaded by
  browsers/email. A file an app downloads through its **own process** is never
  quarantined, so the updater-downloaded bundle skips the Gatekeeper "damaged
  app" path that the manual-DMG install still requires.
- Tauri's updater integrity is a **minisign signature**, entirely independent of
  Apple notarization. No paid Apple account is needed.
- The release workflow already produces `.app.tar.gz` (the updater format) and
  `updates.rs` already extracts release metadata, so much of the groundwork
  exists.

## Decisions (locked during brainstorming)

1. **Approach**: official `tauri-plugin-updater` (true one-click), not a
   download-and-open-DMG shim.
2. **Single source of truth**: consolidate on the plugin's `check()`. Delete the
   hand-rolled `updates.rs`, the `check_for_updates` command, and the `ureq` +
   `semver` dependencies.
3. **Relaunch UX**: after install, the banner shows **Restart now** — the user
   clicks when ready (avoids silently dropping open tabs). Not auto-relaunch.
4. **Restart mechanism**: a tiny custom `commands::restart` calling
   `app.restart()`, rather than adding `tauri-plugin-process`. One line, no extra
   dependency, no extra capability.

## Components

### 1. Dependencies & plugin registration

`src-tauri/Cargo.toml`:
- **Add** `tauri-plugin-updater = "2"`.
- **Remove** `ureq` and `semver` (only `updates.rs` uses them — verify with a
  grep before removing).

`src-tauri/src/lib.rs`:
- Register: `.plugin(tauri_plugin_updater::Builder::new().build())`.
- Remove `mod updates;`.
- Remove `commands::check_for_updates` from the `invoke_handler!` list.
- Add `commands::restart` to the `invoke_handler!` list.

### 2. Configuration (`src-tauri/tauri.conf.json`)

```jsonc
"bundle": {
  "createUpdaterArtifacts": true,
  ...
},
"plugins": {
  "updater": {
    "pubkey": "<minisign public key — pasted from the generated .pub>",
    "endpoints": [
      "https://github.com/larsakeekstrand/mdviewer/releases/latest/download/latest.json"
    ]
  }
}
```

- `createUpdaterArtifacts: true` makes the bundler emit the signed
  `.app.tar.gz` + `.sig`.
- **No CSP change required**: the updater's HTTP requests run in Rust, outside
  the webview, so `app.security.csp` does not govern them.

### 3. Capabilities (`src-tauri/capabilities/default.json`)

Add `"updater:default"` to the `permissions` array. No process permission is
needed because restart is a custom command (covered by core command dispatch).

### 4. Signing key & CI

One-time, run locally by the maintainer:

```sh
cargo tauri signer generate -w ~/.tauri/mdviewer.key
```

This produces:
- a **public key** → paste into `tauri.conf.json` `plugins.updater.pubkey`.
- a **private key** (password-protected) → store as repo secret
  `TAURI_SIGNING_PRIVATE_KEY`, and its password as
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

> **Back up the private key immediately** (e.g., password manager). If it is
> lost, existing installs can no longer accept signed updates — recovery means
> shipping a new pubkey, which existing users can only get via a *manual*
> reinstall.

`.github/workflows/release.yml`: add the two secrets as `env` on the
`tauri-apps/tauri-action` step:

```yaml
env:
  GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
  TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
```

With these set and `createUpdaterArtifacts: true`, tauri-action signs the bundle
and auto-generates/attaches `latest.json` (its `includeUpdaterJson` defaults on).
The generated `latest.json` maps the platform key `darwin-aarch64` to the
`.app.tar.gz` URL + signature.

**The draft → publish flow stays the go-live gate**: the
`releases/latest/download/latest.json` redirect only resolves to the *published*
release, so `gh release edit --draft=false` is what makes an update reach users.

### 5. Backend deletions

- Delete `src-tauri/src/updates.rs`.
- Delete `commands::check_for_updates` (and its `UpdateInfo` usage if any
  remains in `commands.rs`).
- Remove the `mod updates;` declaration and the `invoke_handler!` entry.

Add the restart command (`src-tauri/src/commands.rs`):

```rust
#[tauri::command]
pub fn restart(app: AppHandle) {
    app.restart();
}
```

(`app.restart()` diverges; the command never returns, which is fine.)

### 6. Frontend

`ui/index.html` — extend the existing `#update-banner` so it can render four
states. Add an **Update now** button and a progress element alongside the
existing **View** / dismiss buttons.

`ui/app.js` — rewrite the `/* ---- Update check ---- */` section around the
global plugin API (`withGlobalTauri` exposes it, exactly like `dialogApi`):

```js
const { check } = window.__TAURI__.updater;
```

Banner state machine:

| State | Shows |
|---|---|
| **available** | "MDViewer `version` is available — you have `currentVersion`." · `[Update now]` `[View]` `[×]` |
| **downloading** | progress text/bar driven by `Started` / `Progress` / `Finished` events |
| **installed** | "Update installed." · `[Restart now]` |
| **error** | "Update failed: …" · `[View]` (manual-DMG fallback) `[×]` |

Behavior:
- `checkForUpdates({ silent })` calls `check()`. A `null` result means up to
  date. On an update, populate the banner from `update.version` /
  `update.currentVersion`.
- **Update now** → `update.downloadAndInstall(onEvent)`, where `onEvent` switches
  on `Started` (capture `contentLength`), `Progress` (accumulate `chunkLength`),
  `Finished`. On success → **installed** state. On throw → **error** state.
- **Restart now** → `invoke("restart")`.
- **View** → reconstruct `https://github.com/larsakeekstrand/mdviewer/releases/tag/v${update.version}`
  and open via the existing `open_url` command.
- Dismissal (`localStorage` key `mdviewer.update.dismissed_version`, keyed on
  `update.version`) is preserved for the silent startup check; the menu
  **Check for Updates…** path ignores dismissal, as today.
- The menu **MDViewer ▸ Check for Updates…** path (`menu-check-updates` event)
  routes through the same `check()`. When `check()` returns `null` and the call
  was not silent, show the existing "you're on the latest version" dialog (the
  current version can come from `window.__TAURI__.app.getVersion()` if a number
  is wanted, or the message can omit it).

`ui/styles.css` — styles for the new button and the progress element, matching
the existing `.update-banner-btn` look.

### 7. Error handling & fallback

- `downloadAndInstall()` failures (network drop mid-download, signature
  mismatch, non-writable install location) land in the **error** state, which
  keeps a **View** button so the user can fall back to the manual DMG.
- Export-style transient errors are not relevant here; the banner itself is the
  surface.

## Edge cases & limitations

- **Users on ≤ 1.4.0 have no updater.** They need one final *manual* DMG update
  to the first updater-enabled release. Auto-update only works going forward.
- **Read-only / non-writable install location** (app still running from the DMG,
  or `/Applications` without write permission): the install throws → error state
  + manual fallback.
- **Dev mode**: like file associations, the updater only functions in a built
  `.app`, never under `cargo run`.
- **Signature pairing**: the `pubkey` in config must match the CI private key, or
  downloads are rejected — this is the integrity guarantee, not a bug.

## Testing

1. Build + install the first updater-enabled version (e.g., 1.5.0) to
   `/Applications`.
2. Publish a higher release (e.g., 1.5.1), or host a hand-crafted test
   `latest.json`, so `check()` sees an update.
3. Launch 1.5.0, confirm the banner appears, click **Update now**, watch the
   progress, click **Restart now**, and confirm it relaunches into 1.5.1 with
   **no quarantine prompt**.
4. Negative test: run from a read-only location and confirm the error state +
   fallback button appear.

## Documentation updates

- `CLAUDE.md`: add an "Auto-update" architecture note (plugin, single-source
  `check()`, custom `restart`, draft→publish go-live gate, the in-process
  download / no-quarantine rationale, and the limitations above). Add the
  signing-key/secrets step to "Cutting a release".
- Release notes / `README`: existing users auto-update from the banner; the
  manual DMG + `xattr` is only for the *first* hop onto the updater-enabled
  release.
