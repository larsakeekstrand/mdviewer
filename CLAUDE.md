# CLAUDE.md — mdviewer project guide

A macOS markdown viewer in Rust on Tauri 2. VS Code–style file tree + tabbed
preview, GitHub-flavored markdown rendering, Mermaid diagrams, KaTeX math,
copy-button on code blocks, git status decoration in the tree, live reload,
an Open Recent menu, an in-app update check against GitHub Releases, a custom
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
  diagram rendering, vendored `katex` for math (`ui/katex/`, ~600K including
  woff2 fonts), and vendored `github-markdown.css` for typography.
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
- **Math (KaTeX)**: `markdown.rs` enables comrak's `extension.math_dollars`,
  which emits `<span data-math-style="inline|display">SRC</span>` from `$..$`
  / `$$..$$` using the strict GFM delimiter rules (no whitespace inside, no
  digit after closing `$`, code spans excluded, `\$` escapes). The frontend's
  `renderMath()` hands each span to `katex.render()` with `throwOnError:false`
  (parse errors render in red inline). Same morphdom preservation pattern as
  mermaid (`data-math-state` / `data-math-src`). Skipped in raw view.
- **Export (HTML)**: `ui/export.js` holds pure helpers (filename derivation,
  `forceLightCss`, `inlineFontUrls`, `buildHtmlDocument`, `documentNeedsKatex`),
  unit-tested under `node --test`. The frontend's `exportDocument()` snapshots
  view state, forces a **light** render through `renderActive` (so code/math/
  Mermaid are light and theme-stable), serializes `#preview` (stripping injected
  buttons, disabling task checkboxes, inlining local images and the
  github-markdown/KaTeX CSS + woff2 fonts as `data:` URLs), and writes via the
  existing `save_export` command. Triggered by **File ▸ Export as HTML…**
  (`menu.rs` emits an `export` event with the format). Export errors use the
  transient banner (`showTransientError`), not `showError` (which clears the
  preview). PDF reuses the same `exportDocument` light re-render, then calls the
  native `export_pdf` command (`src-tauri/src/export.rs`, macOS-only):
  `with_webview` → `WKWebView.printOperationWithPrintInfo` with a save-to-file
  `NSPrintInfo` and the print/progress panels suppressed (objc2 0.3 bindings) —
  WebKit's paginated print engine written straight to the dialog-chosen path.
  The `@media print` block in `styles.css` hides the app chrome and reflows the
  preview; because the native print renders through the print pipeline, those
  rules apply during capture and never flash on screen.
- **`postRender()`**: single seam that runs after every morphdom patch —
  `annotateLinks` → `resolveImages` → `addCopyButtons` → `renderMath` →
  `renderMermaid`. Order matters: math/mermaid change element heights and
  must run before `restoreAnchor`. New post-render hooks go here.
- **Interactive task lists**: `commands::toggle_task` (Rust) toggles a `[ ]`
  / `[x]` checkbox at a sourcepos-derived line under a process-wide
  `tasklist_lock` mutex. The pure rewrite lives in `tasklist.rs` and is
  unit-tested without I/O. Atomic write via temp-file + rename in the same
  directory. The frontend (`hookTaskListCheckboxes`) strips comrak's
  `disabled` attribute, attaches click handlers, and gates concurrent
  clicks with a `pendingToggles` set keyed on `${path}|${line}`. The
  watcher → live-reload path delivers the final on-screen state; the
  optimistic browser-flip of the checkbox means there's no flicker.
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
- **Auto-update**: `tauri-plugin-updater` (registered in `lib.rs`, capability
  `updater:default`). The banner's **Update now** downloads the signed
  `.app.tar.gz` in-process, verifies the minisign signature against
  `plugins.updater.pubkey`, swaps the bundle, and **Restart now** relaunches via
  the `restart` command. Because the download is in-process, the new bundle is
  never quarantined — no `xattr` step on update (unlike the first manual DMG
  install).

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
  - **Export (SVG/PNG)**: `renderMermaidForExport` re-renders the source with
    `theme:"default"` (so embedded PNGs aren't dark) AND `htmlLabels:false`
    for flowchart/sequence/class/state — labels become plain `<text>` instead
    of `<foreignObject>`. WebKit can't reliably rasterize foreignObject content
    to canvas; with HTML labels enabled, `canvas.toBlob` silently produced
    null (button flashed "Failed" with no further signal). Restored to the
    on-screen config in a `finally` block so future renders use the user's
    theme again.
  - **PNG rasterizer uses a `data:` URL, not `blob:`** for the `<img>` step.
    The CSP's `img-src` allows `data:` but not `blob:`; loading `blob:`
    rejects via `img.onerror` and the whole export silently fails. Adding
    `blob:` to img-src would also work but unnecessarily widens the CSP.
  - PNG canvas is filled white before `drawImage` — transparent PNGs render
    against black/checker in many viewers, making the diagram unreadable.
- **Task list write-back**:
  - Comrak's `data-sourcepos` is the ONLY reliable way to map a clicked
    checkbox back to a source line. Walking the input's text content or
    sibling positions doesn't survive embedded formatting (`**bold**`,
    nested inline code, etc.).
  - The toggle command MUST be `read → verify → write`, not blind write.
    The verify step rejects when the file changed on disk between render
    and click. Without it, a stale click after an external edit silently
    overwrites the user's change.
  - Atomic write via tempfile + `std::fs::rename` in the same directory
    (a different directory crosses filesystems on macOS and rename loses
    atomicity). Filename is `.<stem>.tasklist-<nanos>.tmp` so it's both
    hidden and unmistakably temporary.
  - Watcher feedback loop is intentional: our write fires `file-changed`,
    the frontend re-renders, the checkbox visually matches what we wrote.
    The 200 ms watcher debounce smooths multiple rapid writes into one
    re-render so the UI doesn't thrash.
  - Editor conflict: if VS Code (or any editor) has the file open, its
    "file changed on disk" prompt fires on every toggle. Not fixable from
    our side; users with always-open editors should know.
  - `pendingToggles` set in the frontend AND the `tasklist_lock` mutex in
    the backend BOTH matter: the set prevents wasted IPC for rapid double-
    clicks; the mutex prevents two distinct clicks (on different
    checkboxes) from racing each other's read-modify-write.
- **Content-Security-Policy** lives in `tauri.conf.json` `app.security.csp`
  (must NOT be `null` — that disables it). `script-src 'self'` is the
  load-bearing defense: the app ships no inline `<script>` or `on*=` handlers,
  so injected markup can't run JS even if comrak's escaping
  (`render.unsafe = false`) is ever bypassed. Do NOT add
  `'unsafe-inline'`/`'unsafe-eval'` to `script-src`. `style-src` DOES need
  `'unsafe-inline'` — syntect emits inline `style=` on code blocks and mermaid
  injects a `<style>`; drop it and code blocks go monochrome and diagrams
  break. Mermaid 11 needs NO `'unsafe-eval'` (verified at runtime). Tauri
  auto-injects the nonces/sources its IPC needs, so don't hand-add IPC origins.
  `img-src` allows `http(s):`/`data:` so remote images render; tighten to
  `'self' data:` to block tracking pixels in untrusted docs.
- **Local images** in markdown (`![](docs/x.png)`, absolute paths) are served
  through Tauri's **asset protocol** — a bare path can't be fetched from the
  `tauri://localhost` origin. Three pieces must stay in sync: (1)
  `tauri.conf.json` `app.security.assetProtocol` (`enable: true` + `scope`,
  currently `["**"]`); (2) the `protocol-asset` feature on the `tauri` crate in
  `Cargo.toml` (the build hard-errors without it); (3) `asset:` in the CSP
  `img-src`. The frontend's `resolveImages()` rewrites local `<img src>` to
  `convertFileSrc(path)` after each render (remote / `data:` / already-`asset:`
  srcs are left alone), and morphdom preserves an already-resolved image so
  live reload doesn't re-fetch it.
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
- **Export must force light CSS**: `github-markdown.css` gates its light color
  variables behind `@media (prefers-color-scheme: light)`. Simply deleting the
  dark block leaves a dark-OS viewer with *no* variables (broken colors). The
  export's `forceLightCss` both removes the dark block AND unwraps the light
  block so its rules apply unconditionally. KaTeX fonts must be inlined as
  `data:` URLs too, or the `.html` references font files that don't travel with
  it.
- **PDF export is native objc2 FFI** (`export.rs`), and the print invocation is
  the load-bearing part:
  - `WKWebView` print rendering is **asynchronous**. `[op runOperation]` captures
    before it finishes and produces a giant pile of BLANK pages (we shipped a
    230 MB / ~889k-page un-openable PDF this way). Use
    `runOperationModalForWindow:delegate:didRunSelector:contextInfo:` instead
    (with panels off it shows no UI; a nil delegate is fine), and set
    `op.view().setFrame(paperSize)` first — without the frame it blanks/crashes.
    (See Apple DTS forum thread 705138 / WebKit bug 151386.)
  - That makes the print **async on the main runloop**, so `export_pdf` must be
    an **async** command (runs off-main) that does NOT block the main thread:
    the `with_webview` closure only *starts* the print; completion is detected by
    polling the output file for a trailing `%%EOF` (which also gates the
    frontend's view-restore until capture is done). A sync command blocking on
    the closure's result would deadlock.
  - `printOperationWithPrintInfo` (paginated, save disposition via
    `NSPrintSaveJob` + `NSPrintJobSavingURL`), NOT `createPDF` (screen media,
    captures chrome, one oversized page).
  - objc2 crates are pinned to the 0.3 framework generation Tauri 2.11 uses
    (core `objc2` is 0.6); mismatched versions break the `inner()`-pointer cast.
  - `@media print` (not a screen class) hides the chrome and flattens the app's
    `100vh`/`height:100%` shell to natural flow, so there's no on-screen flash;
    flatten the WHOLE chain (`html, body, .preview-pane, .preview-scroll,
    #preview`), not just `body`.
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

1. Update `README.md` to cover any user-facing features added since the last
   release (Features list, Usage, Menus), and fix any now-stale claims. The
   README is the user-facing source of truth and drifts silently otherwise —
   every release must leave it accurate.
2. Bump version in `src-tauri/Cargo.toml` AND `src-tauri/tauri.conf.json`.
3. `cd src-tauri && cargo update -p mdviewer` to refresh `Cargo.lock`.
4. Commit (`Bump to 0.x.y` or include the user-facing change).
5. `git tag v0.x.y && git push && git push origin v0.x.y`.
6. Release workflow auto-builds aarch64, generates the changelog from
   commits since the previous tag, attaches `.dmg` + `.app.tar.gz` to a
   **draft** Release.
7. `gh release edit v0.x.y --draft=false` to publish.

The release workflow signs the `.app.tar.gz` and attaches `latest.json` when the
`TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` secrets are
set (one-time setup). Publishing the draft (step 7) is what makes the update
reach existing installs.

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
- `open_url` (Rust) is restricted to `http(s)://` schemes. `open_path` opens an
  existing local path via the macOS `open` command, but **refuses launchable /
  executable types** (`UNSAFE_OPEN_EXTS` in `commands.rs`: `.app`, `.command`,
  `.webloc`/`.inetloc` redirect files, `.pkg`, AppleScript, shells, loadable
  bundles, …). Markdown is untrusted, and a Cmd-clicked relative link to a
  co-located payload would otherwise be local code execution. Keep the denylist
  if you add new open targets.

## When in doubt

- Run `git log --oneline -20` for recent context.
- The release notes on each published GitHub Release link the commits in
  that release; useful for "when did X get added".
- `gh run watch <run-id>` follows a workflow run; `gh run view --log-failed
  --job=<job-id>` for failures.
