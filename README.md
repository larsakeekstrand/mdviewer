# mdviewer

A markdown viewer with a VS Code–style file tree and a beautifully rendered preview, built in Rust on Tauri 2.

## Features

- VS Code–style file tree (lazy expansion, respects `.gitignore`, hides dotfiles)
- GitHub-flavored markdown rendering with syntax-highlighted code blocks
- Live reload when the open file changes on disk
- Light + dark theme via OS `prefers-color-scheme`
- Non-markdown files open as plain text

## Build

```sh
cd src-tauri
cargo build --release
```

The release binary is at `src-tauri/target/release/mdviewer`.

## Run

```sh
# from the project root
./src-tauri/target/release/mdviewer            # tree rooted at CWD
./src-tauri/target/release/mdviewer README.md  # opens README.md, tree at its parent dir
```

During development:

```sh
cd src-tauri
cargo run -- ../README.md
```
