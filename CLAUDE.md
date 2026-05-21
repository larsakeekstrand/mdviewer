# CLAUDE.md — mdviewer project guide

A macOS markdown viewer in Rust on Tauri 2. VS Code–style file tree + tabbed
preview, GitHub-flavored markdown rendering, Mermaid diagrams, live reload, an
Open Recent menu, an in-app update check against GitHub Releases, a custom
right-click context menu, and a default-app file association for markdown files.

Repo: https://github.com/larsakeekstrand/mdviewer

## Stack

- **Backend** (`src-tauri/`): Tauri 2.11, Rust edition 2021, MSRV 1.80.
  - `comrak` (GFM markdown) + `syntect` (server-side syntax highlighting).
  - `notify` + `notify-debouncer-full` for file watching.
  - `ignore` for gitignore-aware directory listing.
  - `ureq` + `semver` for the update check (GitHub API).
  - `tauri-plugin-dialog` for Open File / Open Folder native pickers and the
    Check-for-Updates result dialogs.
- **Frontend** (`ui/`): vanilla HTML / CSS / JS, no build step, no framework.
  Vendored `morphdom` for scroll-preserving diffs, vendored `mermaid` for
  diagram rendering, and vendored `github-markdown.css` for typography.
  `withGlobalTauri: true` in `tauri.conf.json` exposes the IPC API at
  `window.__TAURI__`.

## File layout (each file's purpose in one line)

```
src-tauri/
  src/
    main.rs       — CLI parse (argv[1] → tree root / initial file)
    lib.rs        — Tauri builder, AppState, command registration, setup hook;
                    app.run handles macOS RunEvent::Opened (files from Finder)
    commands.rs   — #[tauri::command]: list_dir, render_file, open_file,
                    read_source, check_for_updates, open_url, open_path,
                    frontend_ready (drains the Finder-open buffer)
    open_files.rs — file:// URL → markdown path; RunEvent::Opened handler:
                    emit open-file + focus window, or buffer until ready
    markdown.rs   — comrak + syntect; sourcepos for scroll anchoring;
                    mermaid fences → <pre class="mermaid"> (codefence renderer)
    tree.rs       — ignore::WalkBuilder depth-1, hides node_modules / target
    watcher.rs    — notify-debouncer-full, 200 ms debounce, watches PARENT dir
    menu.rs       — native menu bar; on_menu_event handler emits JS events
    recent.rs     — JSON-persisted recent-folders list + last_folder (app_data_dir)
    updates.rs    — GitHub /releases/latest probe, semver compare
  tauri.conf.json — productName MDViewer, withGlobalTauri true, ad-hoc signing
  Cargo.toml      — bin name "mdviewer" (lowercase, CLI convention)
  icons/          — 32, 128, 128@2x PNG + icon.icns (built from icon.svg)
ui/
  index.html      — banner + sidebar + splitter + tab-bar + preview-scroll
  app.js          — tabs model, tree, IPC, scroll-anchor, link interception,
                    mermaid render (renderMermaid) + live-reload preservation
  styles.css      — grid layout, CSS variables for light/dark, pre.mermaid
  github-markdown.css, morphdom-umd.min.js, mermaid.min.js  — vendored
icon.svg          — source for icon regeneration
.github/workflows/
  ci.yml          — fmt, clippy -D warnings, test, debug build on push/PR
  release.yml     — tag v* → build aarch64 .dmg, attach to draft Release
                    with auto-generated changelog
```

## Architecture quick-tour

- **Tab model**: `tabs[]` of `{ path, sticky, raw }` + `activeIdx`. Single-click
  on a tree file replaces the non-sticky "preview" tab (or creates one);
  double-click promotes to sticky. Each tab tracks its own raw/rendered state.
- **Watcher**: rewires on tab switch (`setActiveTab` → `open_file`). Watches the
  **parent directory** with `RecursiveMode::NonRecursive` because editors like
  VS Code and IntelliJ do atomic-save rename, which orphans path-level
  watchers on macOS.
- **Live reload**: backend emits `file-changed`; JS re-renders the active tab
  and restores scroll position via comrak's `data-sourcepos` attributes.
- **Mermaid**: `markdown.rs` emits ` ```mermaid ` fences as
  `<pre class="mermaid">` (a comrak `codefence_renderers` entry, not syntect);
  the frontend's `renderMermaid()` turns them into SVG after each morphdom
  patch — theme-aware, with unchanged diagrams preserved across live reloads
  and a per-diagram inline error fallback. Skipped in raw view.
- **File associations / open from Finder**: `tauri.conf.json`
  `bundle.fileAssociations` declares the markdown extensions
  (`CFBundleDocumentTypes`, Viewer role) so MDViewer is selectable as the
  default `.md` app. Finder opens arrive as `RunEvent::Opened` (not argv);
  `open_files::handle_opened` emits `open-file` + focuses the window when the UI
  is ready, else buffers into `AppState.opens`. On startup the frontend calls
  the `frontend_ready` command, which drains the buffer under the same lock:
  cold double-click opens the file (sidebar → its folder); warm opens add a tab
  and keep the current folder.
- **Last directory restore**: `recent.json` carries a `last_folder` alongside
  the recent list. The frontend calls the `remember_folder` command whenever it
  sets the sidebar root (`setTreeRoot`, and the cold-Finder branch of `init`).
  On a plain launch (`Startup.tree_root == None`), `get_initial_state` resolves
  the root as explicit argv → `last_folder` (if still a dir) → cwd, persisting
  all but the bare cwd fallback. `recent::clear` keeps `last_folder`.
- **Menu actions** fire as Tauri events into the frontend:
  `edit-action` (copy / copy-source / toggle-raw), `open-file`, `open-folder`,
  `menu-check-updates`.
- **Update check** runs after `init()` on every launch (silent on failure /
  current). The menu entry **MDViewer ▸ Check for Updates…** triggers the same
  function with `silent: false` so it surfaces a native dialog when current.

## Things that took hours and shouldn't again

- **`Edit` submenu**: macOS auto-injects Writing Tools, AutoFill, Start
  Dictation, and Emoji & Symbols into ANY submenu titled exactly `Edit`,
  regardless of items. That's why our menu is titled **Actions**. Do not rename
  it back without an alternative way to suppress the auto-inserts.
- **Bundle identifier `com.mdviewer.app`**: don't change. The recent-folders
  store, localStorage update-dismissal flag, and any future persistent state
  are keyed off `app_data_dir()` which is bundle-id-based. Renaming would
  silently orphan all of it.
- **CSS `[hidden]` vs `display: flex`**: `.preview-empty { display: flex }`
  beats the implicit `[hidden] { display: none }` in WebKit. We have an
  explicit `[hidden] { display: none !important }` rule for this. If you add a
  flex/grid element you intend to toggle via `.hidden = true`, this rule
  must stay.
- **Frontend changes need `cargo build`**: Tauri bundles `frontendDist` at
  compile time via `tauri-codegen`. Editing `ui/*` without rebuilding shows
  stale UI.
- **`withGlobalTauri: true`** in `tauri.conf.json` is what makes
  `window.__TAURI__` exist. Without it, the JS IPC code throws on load.
- **Link clicks** must be intercepted with `preventDefault()`. Default
  WebView behavior navigates to `tauri://localhost/<href>`, which either 404s
  or — worse — falls through to index.html with broken relative URLs and
  re-runs `init()` cold.
- **Comrak quirks** at the pinned version:
  - `Options::render.r#unsafe` is a raw identifier (`unsafe` is a keyword).
  - `extension.header_id_prefix` (was `header_ids` pre-0.30).
  - `Options<'static>` lifetime is required because `Options` borrows the
    header-id prefix string slice.
  - `comrak::Plugins` is deprecated; use `comrak::options::Plugins`.
- **Mermaid** (mostly frontend; pinned mermaid 11.x):
  - Backend uses comrak's per-language `codefence_renderers` for `mermaid`, NOT
    a `SyntaxHighlighterAdapter` wrapper — comrak appends `</code></pre>` after
    the highlighter path (stray tag). A codefence renderer returns early, so we
    emit the exact `<pre class="mermaid">…escaped source…</pre>`.
  - Only lowercase ` ```mermaid ` matches (comrak preserves the info string),
    consistent with GitHub.
  - `mermaid.min.js` is the single self-contained `dist/mermaid.min.js` IIFE
    whose last line sets `window.mermaid`. The split ESM build pulls in
    `./chunks/*` and can't be vendored without a bundler. Loaded as a classic
    `<script>` BEFORE the `app.js` module so `window.mermaid` exists at `init()`.
  - `mermaid.initialize({ securityLevel: "strict" })` (markdown may be
    untrusted); SVG is inserted via `DOMParser` + `replaceChildren`, never as a
    raw HTML string.
  - Live reload preserves an already-rendered diagram when its source is
    unchanged (morphdom `onBeforeElUpdated`); a `forceMermaid` flag re-renders
    all diagrams on theme change.
- **File associations / Finder open** are two separate problems:
  - Declaring `bundle.fileAssociations` (→ `CFBundleDocumentTypes`) is what
    lets macOS offer MDViewer as the default; it exists ONLY in a
    `cargo tauri build` bundle, never under `cargo run`.
  - Finder opens a file via an Apple Event, surfaced as `RunEvent::Opened`
    (macOS-gated), NOT `argv` — so `lib.rs` uses `.build()? + app.run(cb)` to
    catch it. `main.rs`'s `argv` path still works for the CLI.
  - The cold double-click fires `Opened` before the webview is ready, so files
    are buffered in `Mutex<PendingOpens>` and drained by the `frontend_ready`
    command. Set `ready` and drain under the SAME lock `handle_opened` takes,
    or a file can be lost between the ready-check and the push.
  - Testing needs the built `.app` + Launch Services: copy to `/Applications`,
    `lsregister -f …/MDViewer.app`, then set the default via Finder Get Info →
    Open With → Change All. A locally-built `.app` is NOT quarantined
    (quarantine only marks downloaded files), so the `xattr` step is only for
    downloaded DMGs.
  - The bundler warns that `com.mdviewer.app` ends in `.app` — ignore it; the
    id must not change (see the bundle-identifier note above).
- **Icons**: macOS Big Sur+ uses a "squircle" (superellipse). We approximate
  with `rx=230` on a 1024×1024 (~22.5 %). Regenerate from `icon.svg` with
  `rsvg-convert` (see "Icon regeneration").
- **Gatekeeper / quarantine**: builds are ad-hoc signed
  (`bundle.macOS.signingIdentity: "-"`) but unsigned by Apple Developer ID.
  Users must `sudo xattr -dr com.apple.quarantine /Applications/MDViewer.app`
  once after install. Right-click→Open no longer works on Sequoia+.
- **GitHub macos-13 (Intel) runners** routinely queue 30–60+ min. We dropped
  x86_64 from `release.yml`. Don't add it back without a queue-management
  plan.

## Build / develop / release

```sh
# develop
cd src-tauri
cargo run -- ../README.md

# release build (binary only, no bundle)
cd src-tauri
cargo build --release

# bundle .app + .dmg (needs cargo-tauri once)
cargo install tauri-cli --version "^2"
cd src-tauri
cargo tauri build
```

### Cutting a release

1. Bump version in `src-tauri/Cargo.toml` AND `src-tauri/tauri.conf.json`.
2. `cd src-tauri && cargo update -p mdviewer` to refresh `Cargo.lock`.
3. Commit (`Bump to 0.x.y` or include the user-facing change).
4. `git tag v0.x.y && git push && git push origin v0.x.y`.
5. Release workflow auto-builds aarch64, generates the changelog from
   commits since the previous tag, attaches `.dmg` + `.app.tar.gz` to a
   **draft** Release.
6. `gh release edit v0.x.y --draft=false` to publish.

### Icon regeneration

```sh
rsvg-convert -w 1024 -h 1024 icon.svg -o /tmp/icon_1024.png
# Then run the iconset/iconutil block from commit 64c1ab2 to rebuild
# src-tauri/icons/icon.icns and the three referenced PNGs.
```

## Conventions

- **Commit messages**: no `Co-Authored-By: Claude` trailer (user has this in
  global CLAUDE.md). Imperative subject, body explains *why* not *what* when
  it isn't obvious from the diff.
- **Lint must be clean before commit**: `cargo fmt --check` and
  `cargo clippy --all-targets -- -D warnings` from `src-tauri/`. CI runs both
  with `-D warnings`, so a slip blocks merge.
- **No comments in code unless the why is non-obvious** (CLAUDE.md global says
  this; project follows it).
- **Tauri commands** return `Result<T, String>` — Tauri serializes the error
  branch to a JS rejection. Use `format!("…: {e}")` rather than `?` straight
  to keep error messages informative.
- **JS error handling**: failed `invoke()` calls log to console; user-facing
  errors go through the existing `showError`/banner/dialog flow.

## Update check internals

- `updates::check()` hits `https://api.github.com/repos/larsakeekstrand/mdviewer/releases/latest`
  and parses tag_name. `env!("CARGO_PKG_VERSION")` is the baseline.
- Update banner respects `localStorage.mdviewer.update.dismissed_version` —
  if the user dismissed v0.1.x, the silent startup check stays hidden until
  a newer version appears. The menu-driven check ignores this dismissal.
- `open_url` (Rust) is restricted to `http(s)://` schemes; `open_path` opens
  any existing local path via the macOS `open` command.

## When in doubt

- Run `git log --oneline -20` for recent context.
- The release notes on each published GitHub Release link the commits in
  that release; useful for "when did X get added".
- `gh run watch <run-id>` follows a workflow run; `gh run view --log-failed
  --job=<job-id>` for failures.
