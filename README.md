# mdviewer

[![CI](https://github.com/larsakeekstrand/mdviewer/actions/workflows/ci.yml/badge.svg)](https://github.com/larsakeekstrand/mdviewer/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A markdown viewer with a VS Code–style file tree and a beautifully rendered preview, built in Rust on Tauri 2.

![MDViewer rendering Markdown — prose, a syntax-highlighted Rust code block, and a Mermaid flowchart — in dark mode](docs/screenshot.png)

## Features

- VS Code–style file tree (lazy expansion, shows every file on disk)
- **Git status decoration** — `M` / `A` / `U` / `D` badges on modified, added, untracked, and deleted files when the folder is a git repo, with directory roll-up
- GitHub-flavored markdown rendering with syntax-highlighted code blocks
- **Mermaid diagrams** rendered inline, with hover-revealed **SVG / PNG export** buttons on each diagram (Retina-quality PNG with white background)
- **LaTeX math** via KaTeX — inline `$…$` and display `$$…$$`, with the strict GFM delimiter rules (so `$5 and $10` stays as text)
- **Copy button** on every fenced code block (hover to reveal)
- **Interactive task lists** — click a `- [ ]` / `- [x]` checkbox in the rendered view and the change is written back to the source file atomically
- **In-document find** (⌘F) with case-sensitive and whole-word toggles, match count, and next/previous navigation
- **Document export** — export the rendered page to **self-contained HTML** (CSS, fonts, and local images inlined; always light-themed) or **PDF** (native WebKit print pipeline)
- Live reload when the open file changes on disk
- Tabs with VS Code–style sticky/preview behavior (single-click replaces preview, double-click sticks)
- **Session restore** — the last folder you opened and your open tabs are reopened on the next launch
- Per-tab raw / rendered toggle
- **Open from Finder** — set MDViewer as the default app for `.md` files and double-click to open them
- File menu with **Open File…**, **Open Folder…**, and **Open Recent** (persisted)
- Custom right-click context menu (Copy / Copy Source / Show Raw·Rendered)
- Auto light + dark theme via OS `prefers-color-scheme`
- CLI: `mdviewer [file-or-directory]`
- **Install Command Line Tool** — one menu click symlinks `mdviewer` into `/usr/local/bin` so you can launch it from any terminal
- **One-click auto-update** — when a newer release is published, a dismissible banner downloads, installs, and restarts the signed update in-app; a **What's new** button on the banner shows that release's changelog in an in-app window before you decide

## Install on macOS

The app currently ships only as a macOS Apple Silicon bundle (M1 / M2 / M3 / M4). Builds are not signed by an Apple Developer ID, so you have to remove macOS's quarantine flag once after installing.

### 1. Download

Grab `MDViewer_<version>_aarch64.dmg` from the [latest release](https://github.com/larsakeekstrand/mdviewer/releases/latest).

### 2. Install

Open the `.dmg` and drag `MDViewer.app` to `Applications`.

### 3. Remove the quarantine flag (required)

When you download an unsigned app through a browser, macOS attaches a quarantine attribute. On macOS 15 (Sequoia) and newer, Gatekeeper then refuses to launch it with **"mdviewer" is damaged and cannot be opened**. That message is misleading — the app is fine; macOS is just blocking it. Clear the flag once from Terminal:

```sh
sudo xattr -dr com.apple.quarantine /Applications/MDViewer.app
```

You'll be prompted for your password. After this, double-click `mdviewer` in Applications — it'll open normally, and you won't have to repeat this step on future launches.

> The older right-click → Open workaround that some guides mention no longer works on Sequoia+ for browser-downloaded apps. The `xattr` command is the supported way to bypass Gatekeeper for software you trust.

> You only need the `xattr` step for this initial manual install. Later releases arrive through the in-app **one-click auto-update** banner, which downloads and swaps the bundle in-process — those updates are never quarantined, so you won't have to clear the flag again.

## Usage

### Launching

- **From Applications**: double-click `mdviewer`. The tree is rooted at the last folder you had open (the current working directory on a first-ever launch), and the tabs from your previous session are reopened.
- **By double-clicking a `.md` file in Finder**: once MDViewer is set as the default app for Markdown (Finder ▸ *Get Info* ▸ *Open With* ▸ select MDViewer ▸ *Change All…*), double-clicking any `.md` file opens it in MDViewer with the tree rooted at the file's folder.
- **From the command line**: pass a file or a directory. Files open rendered with the tree rooted at the file's parent; directories just root the tree there.

  ```sh
  mdviewer ~/notes/today.md      # opens the file, tree at ~/notes
  mdviewer ~/notes               # tree at ~/notes, nothing pre-opened
  mdviewer                       # tree at current working directory
  ```

  (To run `mdviewer` from a terminal, use **MDViewer ▸ Install Command Line Tool…** — it symlinks the app's binary into `/usr/local/bin`, which is already on your `$PATH`, prompting for your password if that directory needs admin rights. To do it by hand instead: `sudo ln -s /Applications/MDViewer.app/Contents/MacOS/mdviewer /usr/local/bin/mdviewer`.)

### File tree

- Click a folder to expand or collapse it.
- **Single-click** a file → opens it in the *preview* tab (italic title). Single-clicking another file replaces it.
- **Double-click** a file → opens it as a *sticky* tab (regular title) that won't be replaced by future single-clicks.
- Every file on disk is shown — including dotfiles, entries matched by `.gitignore`, and `node_modules` / `target`.

### Tabs

- Single-click a tab to activate it.
- Double-click a tab to promote a preview tab to sticky.
- Click the **×** on the tab or middle-click the tab to close it.
- The active tab's file is watched on disk; saves elsewhere live-reload the preview while preserving scroll position.

### Raw vs rendered view

Each tab can be viewed rendered (default) or raw. Toggle with the **Raw** button at the top-right of the tab bar, or via the **Actions ▸ Toggle Raw** menu item, or via the right-click context menu. The toggle is per tab.

### Menus

- **MDViewer ▸ Check for Updates…** — manually checks GitHub for a newer release (the same check also runs silently on startup). **View Source on GitHub** opens the repository.
- **MDViewer ▸ Install Command Line Tool…** — symlinks `mdviewer` into `/usr/local/bin` so you can launch it from a terminal (prompts for your password if the directory needs admin rights).
- **File ▸ Open File…** (⌘O) — opens any markdown file. The tree stays where it is; the file opens as a sticky tab.
- **File ▸ Open Folder…** (⇧⌘O) — re-roots the tree at any folder.
- **File ▸ Open Recent** — the last 10 folders you've opened (persisted across launches). The bottom **Clear Recent** entry wipes the list.
- **File ▸ Export as HTML…** / **Export as PDF…** — exports the active tab's rendered document. HTML is fully self-contained; both are always rendered light-themed regardless of your OS appearance.
- **Actions** — Copy (⌘C), Find… (⌘F), Copy Source, Toggle Raw.

### Right-click

Right-clicking anywhere in the preview shows a compact menu with Copy / Copy Source / Show Raw·Rendered. macOS's default text menu (Look Up, Translate, Writing Tools, Speech, …) is suppressed.

## Build from source

```sh
cd src-tauri
cargo build --release
```

The release binary lands at `src-tauri/target/release/mdviewer`.

To produce a `.app` / `.dmg` bundle:

```sh
cargo install tauri-cli --version "^2"
cd src-tauri
cargo tauri build
```

Bundles end up under `src-tauri/target/release/bundle/`.

## Develop

```sh
cd src-tauri
cargo run -- ../README.md
```

CI (`.github/workflows/ci.yml`) runs `cargo fmt --check`, `cargo clippy -- -D warnings`, and `cargo test` on every push and PR.

## Cut a release

Push a `v*` tag to trigger `.github/workflows/release.yml`. It builds for `aarch64-apple-darwin` and attaches the `.dmg` and `.app.tar.gz` artifacts to a draft GitHub Release that you publish manually.

```sh
git tag v0.1.0
git push origin v0.1.0
```

The same workflow can also be re-run from the Actions tab via **Run workflow** by entering an existing tag name — useful if one of the arch builds failed and you want to retry without re-tagging.

## License

[MIT](LICENSE)
