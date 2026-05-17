# mdviewer

[![CI](https://github.com/larsakeekstrand/mdviewer/actions/workflows/ci.yml/badge.svg)](https://github.com/larsakeekstrand/mdviewer/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A markdown viewer with a VS Code–style file tree and a beautifully rendered preview, built in Rust on Tauri 2.

## Features

- VS Code–style file tree (lazy expansion, respects `.gitignore`, hides dotfiles)
- GitHub-flavored markdown rendering with syntax-highlighted code blocks
- Live reload when the open file changes on disk
- Tabs with VS Code–style sticky/preview behavior (single-click replaces preview, double-click sticks)
- Per-tab raw / rendered toggle
- File menu with Open File…, Open Folder…, and Open Recent (persisted)
- Custom right-click context menu (Copy / Copy Source / Show Raw·Rendered)
- Light + dark theme via OS `prefers-color-scheme`
- CLI: `mdviewer [file-or-directory]`

## Install (macOS)

Download the `.dmg` for your Mac from the [latest release](https://github.com/larsakeekstrand/mdviewer/releases/latest):

- Apple Silicon (M1 / M2 / M3 / M4): `*_aarch64.dmg`
- Intel: `*_x64.dmg`

Open the `.dmg` and drag `mdviewer.app` to `Applications`.

### First launch — Gatekeeper

Builds are ad-hoc signed but not code-signed by an Apple Developer ID. macOS will refuse to open the app because the browser sets a quarantine flag on the download. Remove it from Terminal once after installing:

```sh
sudo xattr -dr com.apple.quarantine /Applications/mdviewer.app
```

Then double-click the app — it'll open normally from then on.

> On macOS 15 (Sequoia) and later, the old right-click → Open workaround no longer works for browser-downloaded unsigned apps; macOS flat-out reports the app as "damaged". The `xattr` command above is the supported way to bypass this for software you trust.

## Build from source

```sh
cd src-tauri
cargo build --release
```

The release binary is at `src-tauri/target/release/mdviewer`. To produce a `.app` / `.dmg` bundle:

```sh
cargo install tauri-cli --version "^2"
cd src-tauri
cargo tauri build
```

Bundles end up under `src-tauri/target/release/bundle/`.

## Run from source

```sh
# from the project root
./src-tauri/target/release/mdviewer            # tree rooted at CWD
./src-tauri/target/release/mdviewer README.md  # opens the file; tree rooted at its parent
```

During development:

```sh
cd src-tauri
cargo run -- ../README.md
```

## Releasing

Push a `v*` tag (e.g. `v0.1.0`) to trigger the release workflow. It builds for both `aarch64-apple-darwin` and `x86_64-apple-darwin`, then attaches the `.dmg` and `.app.tar.gz` artifacts to a draft GitHub Release that you publish manually.

```sh
git tag v0.1.0
git push origin v0.1.0
```

## License

[MIT](LICENSE)
