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

1. **The JS `check()` always uses the endpoint the plugin was registered with at
   startup.** GitHub serves static manifests, so the response can't be varied
   per-user by header or query string. The only lever is *which endpoint the
   plugin holds*. Therefore per-user channel selection is fixed at startup
   (plugin registration), and a channel change applies on relaunch.
2. **Immediate in-place switching would require replacing the JS
   `check()`/`downloadAndInstall` path** with Rust `updater_builder().endpoints()`
   + Rust download/install commands + event-based progress. That reworks the
   hardened update path and is explicitly out of scope. We accept
   relaunch-to-apply instead.
3. **Semver ordering is on our side:** `1.15.1 < 1.16.0-rc.1 < 1.16.0`. A beta
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

### 3. Updater registration (`lib.rs`)

Where the updater plugin is registered (currently
`.plugin(tauri_plugin_updater::Builder::new().build())`), read the persisted
channel and choose the endpoint:

```rust
const STABLE_URL: &str = ".../releases/latest/download/latest.json";
const BETA_URL:   &str = ".../releases/download/beta/latest.json";

let channel = recent::load_channel(&handle);
let endpoint = if channel == UpdateChannel::Beta { BETA_URL } else { STABLE_URL };
tauri_plugin_updater::Builder::new()
    .endpoints(vec![endpoint.parse().expect("valid updater endpoint")])?
    .build()
```

Registration must happen where an `AppHandle` (or `&App`) is available so
`recent::load_channel` can resolve `app_data_dir()` — i.e. inside `setup` (the
documented pattern: `app.handle().plugin(...)`), not the bare builder chain, if
the current chain has no handle. The stable URL stays the default in
`tauri.conf.json` `endpoints` as documentation/fallback; `.endpoints()`
overrides it at runtime. Everything downstream — JS `check()`, the banner,
progress, *What's new*, install — is **unchanged**.

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
- A read-only **"Current version: X.Y.Z"** line (from `CARGO_PKG_VERSION`, via an
  existing or tiny new command / the version is also available to the frontend).

**Menu.** Add `MDViewer ▸ Settings…` with accelerator `CmdOrCtrl+,`. In
`menu.rs` it emits an event (or directly opens the window). Opening reuses the
window if already present (focus it) rather than spawning duplicates.

**Toggle flow.** Changing the checkbox calls a Rust command
`set_update_channel(channel)` → `recent::save_channel`. After persisting, the UI
surfaces **"Relaunch now to apply?"** — confirm → `restart` (the process-plugin
restart already used by the banner's *Restart now*); decline → the change takes
effect on the next launch. A `get_update_channel` command (or bundling channel +
version into one `get_preferences` command) feeds the window's initial state.

### 5. Documentation

- **README**: a short "Beta updates" subsection under the updates docs — how to
  opt in (Settings → checkbox), what to expect, how to switch back.
- **CLAUDE.md**: document the channel architecture (rolling `beta` release,
  superset model, startup-fixed endpoint, Preferences window) and add a
  beta-release recipe alongside the existing "Cutting a release" steps.

## Out of scope (YAGNI)

- Immediate in-place channel switching (no relaunch) — explicitly rejected;
  relaunch-to-apply is acceptable.
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
- **Endpoint registration needs an AppHandle** — must register the updater where
  `app_data_dir()` is resolvable.
- **Don't widen CSP** for the new window; external JS only.
</content>
</invoke>
