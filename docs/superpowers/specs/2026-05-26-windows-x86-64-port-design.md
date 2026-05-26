# Windows x86_64 Port — Design

**Status**: Design approved, ready for implementation plan.
**Date**: 2026-05-26.
**Target release**: v1.13.0.

## Goal

Ship MDViewer as a native Windows x86_64 application alongside the existing
macOS aarch64 build, distributed through the same GitHub Releases channel and
covered by the same `tauri-plugin-updater` auto-update flow. The first Windows
release achieves feature parity with macOS *except* PDF export (HTML export
works on both).

## Decisions

| Question | Decision |
|---|---|
| Architecture | `x86_64-pc-windows-msvc` only. No 32-bit, no ARM64. |
| PDF export on Windows | Deferred. The existing stub returns an error; the menu item is hidden on Windows. HTML export works. |
| Installer formats | NSIS `.exe` **and** MSI, both produced from `targets: "all"`, both attached to each Release. |
| Code signing (Windows) | Unsigned. README documents the SmartScreen "More info → Run anyway" workaround, parallel to the existing macOS Gatekeeper note. |
| WebView2 distribution | Default `downloadBootstrapper` — small installer, fetches WebView2 at install time only if missing. |
| "Install MDViewer CLI" menu | Hidden on Windows. NSIS's optional "Add to PATH" covers the equivalent use case. |
| PR-time CI | Matrix on both `macos-14` and `windows-latest`. Catches cross-platform breakage on every PR. |
| Phasing | Single PR / single release. Refactor + bundle + CI + docs in one `windows-port` branch, cut as v1.13.0. |

## Architecture & cross-platform model

This is a port, not a rewrite. The existing single-process Tauri 2 model
already runs on Windows. The gaps are platform-specific Rust modules and the
bundle/CI pipeline. We extend cleanly without introducing a new abstraction
layer.

Existing patterns preserved:

- **cfg-gated modules** (`open_files.rs`, `export.rs::macos`)
- **cfg-gated inline expressions** (`lib.rs:90`, the `app.run` macOS-only event handler)

New conventions:

- **Cross-platform shell-out** uses the `opener` crate (replacing two
  `Command::new("open")` call sites in `commands.rs`). Same fire-and-forget
  spawn semantics; on Windows it shells out via `start`, on macOS via `open`.
- **Dangerous-extension denylist** is a single cross-platform union
  (`UNSAFE_OPEN_EXTS`). macOS-dangerous types (`.app`, `.command`, `.scpt`)
  are harmless to deny on Windows; Windows-dangerous types
  (`.exe`, `.bat`, `.cmd`, `.ps1`, …) are harmless to deny on macOS.

What we are **not** doing:

- Not introducing a platform abstraction trait or `cfg_if!`. Two new
  conditionals don't justify it.
- Not implementing Windows PDF export. The stub stays with a clearer message.
- Not implementing a Windows equivalent of the CLI symlink. The NSIS
  installer's optional "Add to PATH" checkbox is the equivalent affordance.

## Code & config changes

### `src-tauri/Cargo.toml`

Add one cross-platform dependency:

```toml
[dependencies]
opener = "0.7"
```

The existing `[target.'cfg(target_os = "macos")']` block (objc2 family)
stays untouched — Windows builds won't pull those in. No
`[target.'cfg(target_os = "windows")']` block is needed; WebView2 is hidden
behind Tauri.

### `src-tauri/src/commands.rs`

1. **`open_url`** — replace `Command::new("open").arg(&url).spawn()` with
   `opener::open(&url)`. Same `Result<(), String>` signature. The
   `http(s)://` prefix check stays in front of the call.
2. **`open_path`** — same swap. The `is_unsafe_to_open` check stays in
   front of the call.
3. **`UNSAFE_OPEN_EXTS`** — append Windows-dangerous extensions. The list
   becomes "things we refuse to launch via the OS shell on any platform":

   ```text
   // Windows executables / scripts / shortcuts
   "exe", "bat", "cmd", "com", "ps1", "psm1", "psc1",
   "vbs", "vbe", "js", "jse", "wsf", "wsh", "msc",
   "scr", "msi", "msp", "msh", "msh1", "msh2",
   "mshxml", "msh1xml", "msh2xml",
   "lnk", "pif", "hta", "cpl", "reg", "inf",
   "scf", "appref-ms", "appx", "appxbundle",
   ```

   `.url` is already in the list (macOS .webloc analog) — keep, don't
   duplicate.
4. **`install_cli`** + its `install_with_admin`, `create_cli_symlink`,
   `CLI_LINK_PATH`, `LinkState`, `InstallAction`, `InstallOutcome` —
   wrap the entire macOS-specific block in `#[cfg(target_os = "macos")]`.
   Add a non-mac stub command:

   ```rust
   #[cfg(not(target_os = "macos"))]
   #[tauri::command]
   pub fn install_cli() -> Result<InstallOutcome, String> {
       Err("CLI install is only supported on macOS".to_string())
   }
   ```

   `InstallOutcome` must be available cross-platform for serialization;
   move it out of the cfg block or duplicate the type for the stub. The
   frontend never calls `install_cli` on Windows (menu item hidden) but
   the stub keeps `invoke_handler!` registration uniform.

### `src-tauri/src/export.rs`

Update the existing non-mac branch's message:

```rust
#[cfg(not(target_os = "macos"))]
{
    let _ = (window, path);
    Err("PDF export is not yet supported on Windows".to_string())
}
```

### `src-tauri/src/menu.rs`

Two cfg-gates:

- The **Install MDViewer CLI** `MenuItem` and its `.item(&install_cli)`
  registration: `#[cfg(target_os = "macos")]`.
- The **Export as PDF…** `MenuItem` and its registration: same.

HTML export stays on both platforms.

### `src-tauri/src/lib.rs`

No change. The existing `#[cfg(target_os = "macos")]` around the
`RunEvent::Opened` handler in `app.run` already handles Windows correctly
(Windows opens files via argv, parsed in `main.rs`).

### `src-tauri/src/commands.rs` — new `platform()` command

Add a small cross-platform command returning the current OS:

```rust
#[tauri::command]
pub fn platform() -> &'static str {
    std::env::consts::OS  // "macos" | "windows" | "linux" | ...
}
```

Register it in `invoke_handler!`. The frontend uses this as the single
source of truth for platform-conditional UI (replaces the
`navigator.platform` heuristic).

### `src-tauri/tauri.conf.json`

Add Windows bundle block:

```json
"bundle": {
  "active": true,
  "category": "Productivity",
  "targets": "all",
  "windows": {
    "webviewInstallMode": { "type": "downloadBootstrapper" },
    "nsis": {
      "installMode": "perUser",
      "installerIcon": "icons/icon.ico"
    },
    "wix": {
      "language": "en-US"
    }
  },
  "fileAssociations": [ /* unchanged — works cross-platform */ ]
}
```

Notes:

- `targets: "all"` already produces both NSIS and MSI on Windows; no
  change there.
- `perUser` NSIS install avoids requiring admin for the typical case
  (matches the macOS drag-to-Applications experience).
- `webviewInstallMode: downloadBootstrapper` is the Tauri default —
  installer is small (~5 MB), fetches WebView2 at install time only if
  missing (it usually isn't on Windows 10 1903+ / Windows 11).
- `fileAssociations` block is already cross-platform; Tauri writes the
  appropriate Windows registry entries (`HKCU\Software\Classes\…`) on
  install.

### `src-tauri/icons/icon.ico`

Add a multi-resolution `.ico` derived from `icon.svg`. Generation
(one-time, added to the icon-regen recipe in CLAUDE.md):

```sh
magick icon.svg \
  -define icon:auto-resize=256,128,64,48,32,16 \
  src-tauri/icons/icon.ico
```

If ImageMagick isn't available locally, `cargo tauri icon icon.svg` also
generates the `.ico` along with the platform PNGs.

Reference the new file in `tauri.conf.json` `bundle.icon` array.

### `ui/app.js` and `ui/export.js`

- Replace `navigator.platform.toLowerCase().includes("mac")` with a call
  to the new `platform()` Tauri command at startup. Store the result on
  a module-level `IS_MAC` / `IS_WINDOWS`.
- Menu visibility for "Install CLI" and "Export as PDF" uses these
  constants. (The menu items are server-side; the frontend only reacts
  to events. The relevant guard is in the `edit-action` handler.)
- `export.js` doesn't change — it invokes `save_export` and `export_pdf`
  by name; the PDF path is unreachable on Windows because the menu item
  isn't built.

### What stays unchanged

- All `ui/*.css`, `ui/index.html`, `ui/morphdom-*`, `ui/mermaid.min.js`,
  `ui/katex/`, `ui/github-markdown.css`, the entire rendering pipeline.
- `recent.rs`, `tree.rs`, `watcher.rs`, `git.rs`, `markdown.rs`,
  `tasklist.rs`, `open_files.rs`, `main.rs`.
- The Content-Security-Policy block.

## CI

### `.github/workflows/ci.yml` — PR-time matrix

Convert the existing single-runner job into a matrix:

```yaml
strategy:
  fail-fast: false
  matrix:
    os: [macos-14, windows-latest]
runs-on: ${{ matrix.os }}
```

`fmt --check`, `clippy -D warnings`, `cargo test`, and a debug
`cargo build` run on both. `fail-fast: false` keeps one OS's failure
from masking the other's. Cached via `Swatinem/rust-cache@v2`
(already used). Estimated CI time impact: ~6–8 min added per PR on
top of existing macOS time.

What Windows CI catches:

- cfg-gated module gaps (e.g., a function used outside its cfg block)
- behavioral differences between `opener` and the old `Command::new("open")`
- new Rust deps that don't build with MSVC

### `.github/workflows/release.yml` — Windows build job

Add a `build-windows` job alongside `build-macos`. Both jobs:

1. Resolve the tag the same way (`steps.tag`).
2. Run `tauri-action@v0` with `projectPath: src-tauri`, same `tagName` /
   `releaseName`.
3. Upload to the same draft Release via `GITHUB_TOKEN`.

Differences:

- `runs-on: windows-latest`
- No `targets: aarch64-apple-darwin` in the toolchain step
- Only the **macOS job** sets `releaseBody`. The Windows job leaves it
  empty so it doesn't clobber the macOS body. (The `releaseBody`
  itself is updated to include a Windows section — see Distribution
  below.)

The Windows runner produces both `.exe` and `.msi` from
`targets: "all"`; `tauri-action` uploads each, plus the per-platform
`latest.json` fragment.

## Auto-updater wiring

`tauri-plugin-updater` already targets both NSIS (`nsis`) and MSI
(`msi`) artifact types in the per-platform `latest.json`. When
`tauri-action` builds with `TAURI_SIGNING_PRIVATE_KEY` set, it
generates a `latest.json` per build and *merges* them at upload time
on the GitHub Release. We rely on this merge behavior — same as how
the macOS build today produces `latest.json` mentioning
`darwin-aarch64` only; after the Windows job runs, the published
`latest.json` will list `darwin-aarch64`, `windows-x86_64-nsis`,
and `windows-x86_64-msi`.

**Known risk**: the merge step has historically been finicky in
`tauri-action`. If the second job overwrites instead of merging,
Windows users would see "no update available" until a hot-fix.

**Mitigation**: smoke-test on a pre-release tag (e.g.,
`v1.13.0-rc1`) before publishing the final v1.13.0 draft. Inspect
the uploaded `latest.json` with `gh release view v1.13.0-rc1
--json assets`.

## Distribution & install instructions

### `release.yml` `releaseBody` — add Windows section

```text
## Install (Windows x86_64)

Download either:
- `MDViewer_<version>_x64-setup.exe` (NSIS, smaller, per-user install)
- `MDViewer_<version>_x64_en-US.msi` (MSI, enterprise / GPO deploy)

### First-run SmartScreen warning

Builds are unsigned. Windows Defender SmartScreen will show
"Windows protected your PC". Click **More info** → **Run anyway**
once. After install, MDViewer launches normally from the Start
Menu.

### WebView2

The installer auto-downloads Microsoft Edge WebView2 Runtime if
it isn't already present (it usually is on Windows 10 1903+ and
Windows 11). Internet access required during install in that case.
```

### `README.md` updates

- Install section: add a Windows subsection mirroring the macOS one
  (download, SmartScreen note, WebView2 note).
- Build-from-source section: note that `cargo tauri build` on Windows
  produces both `.exe` and `.msi`; mention `magick` / `cargo tauri
  icon` for `.ico` regeneration.
- Feature list / project description: broaden any "macOS app" claims
  to "macOS and Windows app."

### `CLAUDE.md` updates

Add a "Platform support" section listing:

- macOS-only code paths (PDF export, CLI symlink, `RunEvent::Opened`,
  AppleScript admin elevation).
- `UNSAFE_OPEN_EXTS` is a cross-platform union list — when adding a
  new dangerous type, include both OS families.
- Windows-specific gotchas (WebView2 bootstrap, NSIS per-user vs.
  per-machine, ICO icon regeneration).
- Add `magick`/`cargo tauri icon` recipe alongside the existing
  `rsvg-convert` icon-regen recipe.

## Error handling

- `opener::open` failure (no default handler registered) surfaces
  through the existing `showError` banner — matches the macOS
  failure mode today.
- WebView2 bootstrap failure shows a Windows-native dialog from the
  bootstrapper; not our concern.
- Auto-updater on Windows: download interruption uses the same in-app
  banner error state used on macOS. Minisign signature verification
  is identical across platforms.
- The non-mac stubs for `install_cli` and `export_pdf` return
  `Err(String)` which Tauri serializes to a JS rejection. The
  frontend never invokes them on Windows (menus hidden), but the
  defensive stubs prevent surprises if the menu gate is ever bypassed.

## Testing

### Automated (CI)

- Rust unit tests (`tasklist.rs`, etc.) run on both `macos-14` and
  `windows-latest` via the new matrix.
- Frontend `node --test` tests for `ui/export.js`, `ui/theme.js`,
  `ui/filetype.js` are OS-agnostic and run as part of existing CI.
- `fmt --check`, `clippy -D warnings`, debug `cargo build` on both
  runners verify cross-platform compilation cleanliness.

### Manual (first Windows artifact)

The user does local dev on macOS. Validation of Windows artifacts
goes through a pre-release tag:

1. Push `v1.13.0-rc1` tag. `release.yml` produces both platforms'
   artifacts on the draft Release.
2. Download `MDViewer_<ver>_x64-setup.exe` and the MSI to a real
   Windows machine.
3. Verify:
   - Installer runs (SmartScreen workaround, WebView2 bootstrap if
     needed).
   - App launches from Start Menu.
   - Drag-and-drop a `.md` file from Explorer.
   - Right-click `.md` → Open with → MDViewer appears as an option;
     setting as default works.
   - Tree navigation, tab open/close, mermaid rendering, KaTeX
     rendering, live reload via VS Code save.
   - HTML export works (round-trips images and KaTeX).
   - Auto-update banner appears when a newer version is published.
   - "Install CLI" menu item is **not** present.
   - "Export as PDF" menu item is **not** present.
   - The macOS Gatekeeper / quarantine flow is unaffected.
4. Inspect the uploaded `latest.json` to confirm both platform
   targets are listed (the auto-updater merge-step smoke test).
5. If clean, push the final `v1.13.0` tag and `gh release edit
   v1.13.0 --draft=false` to publish.

## Out of scope (explicit non-goals)

- Windows PDF export (separate future task; will use WebView2
  `PrintToPdfAsync` via COM bindings).
- Linux support (separate future task — most of the same refactor
  applies, but file associations, packaging via AppImage/deb/rpm,
  and CI runner additions are their own design).
- Windows code signing with an Authenticode certificate (separate
  future task once a certificate is acquired; CI hook is trivial
  to add).
- ARM64 Windows (Snapdragon X laptops). Possible but not in this
  scope.
- 32-bit (i686) Windows. Not in this scope.
- Windows-specific CLI install equivalent (NSIS "Add to PATH"
  checkbox is considered sufficient).
- A platform abstraction layer / `cfg_if!` macro. Two conditionals
  don't justify it.
