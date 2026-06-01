# Beta update channel — design

Date: 2026-06-01

## Goal

Let MDViewer publish release-candidate (beta) builds that flow through the
existing `tauri-plugin-updater` auto-update system, and let a user **opt in** to
receive them. Stable users must be unaffected — they keep getting only final
releases.

## Background / current state

- Auto-update is `tauri-plugin-updater` (2.10). The frontend calls
  `window.__TAURI__.updater.check()`, which fetches the manifest from the single
  endpoint in `tauri.conf.json` `plugins.updater.endpoints`:
  `https://github.com/larsakeekstrand/mdviewer/releases/latest/download/latest.json`.
- That URL resolves to the newest **non-prerelease** GitHub release. So if betas
  are published as GitHub *prereleases*, stable users ignore them automatically —
  this property is the foundation of the whole design.
- The minisign keypair that signs update artifacts is operational state; the
  public key is in `plugins.updater.pubkey`. The **same** key will sign both
  channels — no pubkey changes.
- `release.yml` already tolerates `vX.Y.Z-rcN` tags: the `polish-release` job
  derives the bare semver with `sed -E 's/^v([0-9]+\.[0-9]+\.[0-9]+).*/\1/'` for
  installer filenames.
- Settings today live in localStorage (theme, dismissed-update version) and
  native menu items. There is no preferences window yet.
- `recent.rs` persists a JSON `Store` in `recent.json` under `app_data_dir()`,
  readable synchronously at startup (this is where the channel preference will
  live).

## Key technical constraints that shaped the approach

1. **The plugin's JS `check()` reads endpoints from immutable managed state**
   seeded from `tauri.conf.json` at plugin setup. The plugin `Builder` exposes
   `pubkey`/`installer_args` but **not** `endpoints`, so there is no
   startup-fixed way to point the bundled `check()` at a beta URL. (This
   invalidated an earlier plan of overriding endpoints at plugin registration.)
2. **The runtime path *can* choose endpoints.**
   `webview.updater_builder().endpoints(vec![url])?` builds an updater against an
   arbitrary endpoint at call time. A small custom `check_update` command uses
   this, picking the URL from the stored channel. Crucially, the resulting
   `Update` is added to the webview resource table and its `rid` is returned —
   and the **existing, hardened** `plugin:updater|download_and_install` command
   looks an update up by `rid`. So only `check` is custom; download + progress +
   install are reused verbatim.
3. **Channel choice is read per-check, so no relaunch is needed.** Toggling the
   channel persists it and re-runs the normal `checkForUpdates()`; the next check
   uses the new endpoint immediately.
4. **Semver ordering is on our side:** `1.15.1 < 1.16.0-rc.1 < 1.16.0`. A beta
   tester rolls rc → rc → final stable with no special "downgrade" logic and
   without `allowDowngrades`.

## Design

### 1. Channel infrastructure (GitHub / CI)

**Versioning.** A beta build carries a semver *prerelease* version in BOTH
`src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json` — e.g. `1.16.0-rc.1`.
This is load-bearing: the updater embeds `CARGO_PKG_VERSION`, and the semver
prerelease ordering is what makes the channel progression work. The git tag is
`vX.Y.Z-rc.N`.

> Note: existing CLAUDE.md prose references `vX.Y.Z-rc1` (no dot). We standardize
> on `-rc.N` (with dot) going forward; the `polish-release` regex
> (`s/^v([0-9]+\.[0-9]+\.[0-9]+).*/.../`) strips everything after the semver core
> regardless, so both forms still yield the correct bare version for filenames.

**Rolling `beta` release.** `release.yml` gains a prerelease path. When the tag
matches a prerelease (`vX.Y.Z-rc.N`), the workflow publishes to a single
fixed-tag GitHub release **`beta`** (`prerelease: true`), **clearing its old
assets first**, then attaching the dmg/exe/msi + signed `latest.json`. The beta
updater endpoint is therefore the permanently-stable URL:
`https://github.com/larsakeekstrand/mdviewer/releases/download/beta/latest.json`.

**Superset model (important).** The beta channel must always offer the *newest*
build — stable or beta — or a tester on `1.16.0-rc.2` would be stranded behind
stable `1.16.0`. So:

- **Beta tag (`vX.Y.Z-rc.N`)** → refresh the rolling `beta` manifest only. Do
  NOT touch the stable `latest.json`.
- **Stable tag (`vX.Y.Z`)** → existing behavior (draft release + its own
  `latest.json`) **and** also refresh the rolling `beta` manifest so beta users
  roll onto the stable build.

The same minisign key signs both channels (the CI secrets are unchanged).

**Asset hygiene.** Because the `beta` release is rolling, each beta build must
clear stale assets before upload (a previous `1.16.0` payload must not linger
when `1.17.0-rc.1` ships). Approach: delete + recreate the `beta` release (or
clear its assets) at the start of the beta job so the manifest and its
referenced `.app.tar.gz` always belong to the current release.

### 2. Persistence (Rust, `recent.rs`)

Extend `Store` with a channel field:

```rust
#[derive(Default, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
enum UpdateChannel { #[default] Stable, Beta }

struct Store {
    // ...existing fields...
    #[serde(default)]
    channel: UpdateChannel,
}
```

Add `load_channel(app) -> UpdateChannel` and `save_channel(app, channel)`,
mirroring the `last_folder` accessors, preserving all other fields on write.
Legacy `recent.json` files without the field deserialize to `Stable` (covered by
`#[serde(default)]`, same pattern as the existing legacy-store tests). Add unit
tests: round-trip with channel, and legacy-store-without-channel → `Stable`.

### 3. Channel-aware update check (`commands.rs` + frontend)

The bundled updater plugin stays registered as-is (its `tauri.conf.json` stable
endpoint becomes a harmless default — our command always sets endpoints
explicitly). We add **one** custom command that mirrors the plugin's `check` plus
an endpoint override, and a thin JS shim.

**Rust `check_update` command (`commands.rs`):** picks the endpoint from the
stored channel, builds an updater against it via `webview.updater_builder()
.endpoints(...)`, runs `check()`, and — exactly as the plugin's own `check`
does — adds the resulting `Update` to `webview.resources_table()` and returns its
`rid` alongside the version/body. Constants:

```rust
const STABLE_URL: &str = "https://github.com/larsakeekstrand/mdviewer/releases/latest/download/latest.json";
const BETA_URL:   &str = "https://github.com/larsakeekstrand/mdviewer/releases/download/beta/latest.json";
```

`tauri::Url` is the re-export of `url::Url` (no new dependency). The exact borrow
order (read `version`/`body`/`current_version` off the `Update` before moving it
into the resource table) is spelled out in the plan.

**Frontend shim (`ui/app.js`):** replace `updaterApi.check()` in
`checkForUpdates()` with `invoke("check_update")`, wrapping the returned metadata
in an object exposing the same surface the banner already uses (`.version`,
`.currentVersion`, `.body`, `.downloadAndInstall(cb)`):

```js
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

`download_and_install` is the same plugin command the banner installs with today
(permission already granted via `updater:default`), so its behavior is unchanged.
Because `check_update` reads the channel per call, switching channels takes effect
on the next check — **no relaunch**. Everything downstream of `check` — banner
state machine, progress, *What's new*, install, restart — is untouched.

### 4. Preferences window (frontend + small Rust surface)

**Window.** New static files `ui/preferences.html` + `ui/preferences.js`
(external JS only — CSP `script-src 'self'` forbids inline scripts; same rule as
`index.html`). The window is created on demand via `WebviewWindowBuilder`
(NOT declared in `tauri.conf.json`, so it isn't shown at launch), small and
non-resizable-ish, titled "Settings".

**Contents.**
- A checkbox **"Receive beta (pre-release) updates"**, reflecting the persisted
  channel.
- A one-line explainer (betas may be unstable; you can switch back any time).
- A read-only **"Current version: X.Y.Z"** line, supplied by the
  `get_preferences` command (returns `{ channel, version }` where `version` is
  `env!("CARGO_PKG_VERSION")`).

**Menu.** Add `MDViewer ▸ Settings…` with accelerator `CmdOrCtrl+,`. The
`menu.rs` handler opens the window directly (Rust `WebviewWindowBuilder`), and if
a window with that label already exists it focuses it rather than spawning a
duplicate.

**Toggle flow.** Changing the checkbox calls `set_update_channel(channel)` →
`recent::save_channel`. No relaunch: because `check_update` reads the channel per
call, the change takes effect at the next update check. The prefs window may
optionally emit an event so the main window kicks off an immediate
`checkForUpdates()`, but that is a nicety, not required for correctness.
`get_preferences` feeds the window's initial checkbox + version state.

### 5. Documentation

- **README**: a short "Beta updates" subsection under the updates docs — how to
  opt in (Settings → checkbox), what to expect, how to switch back.
- **CLAUDE.md**: document the channel architecture (rolling `beta` release,
  superset model, the custom `check_update` command + JS shim, Preferences
  window) and add a beta-release recipe alongside the existing "Cutting a
  release" steps.

## Out of scope (YAGNI)

- Replacing the plugin's download/install path — reused verbatim via the `rid`
  handed to `plugin:updater|download_and_install`.
- Migrating the theme toggle into Preferences — the toolbar `☾/☀` stays.
- A general preferences framework — the window holds only the beta toggle (+
  version line) for now; it can grow later.
- Per-build "you are on a beta" chrome/badge in the main window.
- Auto-downgrade when opting out — a user who opts out simply stops receiving new
  betas and stays on their current build until stable surpasses it.

## Testing

- Rust unit tests in `recent.rs`: channel round-trip; legacy store (no channel)
  → `Stable`; `save_channel` preserves other fields.
- Manual / smoke: cut a `vX.Y.Z-rc.N` tag, confirm the rolling `beta` release is
  produced with a valid signed `latest.json`; a beta-opted build sees it; a
  stable build does not. Verify a subsequent stable release reaches a
  beta-opted install (superset). Use a throwaway `-rc` tag and
  `gh release view beta --json assets` before relying on it.
- `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` clean.

## Risks / watch-items

- **Rolling-release asset staleness** — the beta job MUST clear old assets or the
  manifest can reference a deleted/old payload. Primary failure mode.
- **`latest.json` URL rewriting** — the existing `polish-release` job rewrites
  Windows asset URLs and restores `notes`; the beta path needs the equivalent so
  the in-app *What's new* modal and Windows installer URLs resolve on the `beta`
  release too.
- **`check_update` borrow order** — read `version`/`body`/`current_version` off
  the `Update` *before* moving it into `webview.resources_table().add(...)`; the
  add consumes/owns it and yields the `rid`.
- **New window needs capability scope** — the `preferences` window label must be
  added to `capabilities/default.json` `windows` (or a new capability), or its
  webview gets no `core`/`event` permissions and `invoke` fails.
- **Don't widen CSP** for the new window; external JS only.
