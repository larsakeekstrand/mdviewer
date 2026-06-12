# CLAUDE.md — mdviewer project guide

A markdown viewer and editor for macOS and Windows, in Rust on Tauri 2. VS Code–style file tree + tabbed
preview, GitHub-flavored markdown rendering, Mermaid diagrams, KaTeX math,
copy-button on code blocks, git status decoration in the tree, live reload,
an Open Recent menu, an in-app update check against GitHub Releases, a custom
right-click context menu, a default-app file association for markdown files,
in-app source editing with live preview, file management from the tree, and a
Review Mode that annotates rendered blocks and copies a structured review to the
clipboard for pasting into an AI coding assistant, and a one-click installer for
a Claude Code hook that auto-opens the plan/spec files Claude writes, and an MCP server that lets Claude Code open documents in the viewer and request in-app reviews.

Repo: https://github.com/larsakeekstrand/mdviewer

## Stack

- **Backend** (`src-tauri/`): Tauri 2.11, Rust edition 2021, MSRV 1.80.
  - `comrak` (GFM markdown) + `syntect` (server-side syntax highlighting).
  - `notify` + `notify-debouncer-full` for file watching.
  - `ureq` + `semver` for the update check (GitHub API).
  - `tauri-plugin-dialog` for Open File / Open Folder native pickers and the
    Check-for-Updates result dialogs.
- **Frontend** (`ui/`): vanilla HTML / CSS / JS, no build step, no framework.
  Vendored `morphdom` for scroll-preserving diffs, vendored `mermaid` for
  diagram rendering, vendored `katex` for math (`ui/katex/`, ~600K including
  woff2 fonts), vendored `github-markdown.css` for typography, and vendored
  **CodeMirror 5** for the source editor (`ui/codemirror/`).
  `withGlobalTauri: true` in `tauri.conf.json` exposes the IPC API at
  `window.__TAURI__`.
- **`trash` crate** (Rust): moves files to the system Trash on delete (cross-platform recycle).

## File layout (each file's purpose in one line)

```
src-tauri/
  src/
    main.rs       — CLI parse (argv[1] → tree root / initial file)
    lib.rs        — Tauri builder, AppState, command registration, setup hook;
                    app.run handles macOS RunEvent::Opened (files from Finder)
    commands.rs   — #[tauri::command]: list_dir, render_file, render_preview,
                    open_file, read_source, save_file, open_url, open_path,
                    frontend_ready (drains the Finder-open buffer),
                    create_file, create_folder, rename_path, duplicate_file,
                    delete_to_trash
    open_files.rs — file:// URL → markdown path; RunEvent::Opened handler:
                    emit open-file + focus window, or buffer until ready
    claude_hook.rs— Claude Code hook: pure is_plan_file / extract_file_path /
                    merge_hook / hook_command (unit-tested) + run_hook runtime
                    for `--claude-hook` (stdin JSON → open plan in MDViewer)
    mcp.rs        — MCP server: pure JSON-RPC dispatch/tool defs/validation/
                    .mcp.json merge (unit-tested) + run_proxy runtime for
                    `--mcp` (stdio ↔ local socket relay, launches GUI)
    mcp_server.rs — GUI-side socket listener; McpPending ack-confirmed map
                    routes tool calls webview-ward and replies socket-ward
    markdown.rs   — comrak + syntect; sourcepos for scroll anchoring;
                    mermaid fences → <pre class="mermaid"> (codefence renderer)
    tree.rs       — std::fs::read_dir depth-1, unfiltered (shows all files)
    watcher.rs    — notify-debouncer-full, 200 ms debounce, watches PARENT dir
    menu.rs       — native menu bar; on_menu_event handler emits JS events
    recent.rs     — JSON-persisted recent-folders list + last_folder (app_data_dir)
    fs_ops.rs     — validate_name, duplicate_candidate, within_root, create_file,
                    create_folder, rename_path, duplicate_file; pure + IO helpers
                    for file-tree operations, all unit-tested
  tauri.conf.json — productName MDViewer, withGlobalTauri true, ad-hoc signing
  Cargo.toml      — bin name "mdviewer" (lowercase, CLI convention)
  icons/          — 32, 128, 128@2x PNG + icon.icns (built from icon.svg)
ui/
  index.html      — banner + sidebar + splitter + tab-bar + preview-scroll
  app.js          — tabs model, tree, IPC, scroll-anchor, link interception,
                    mermaid render (renderMermaid) + live-reload preservation;
                    CodeMirror editor wiring (enter/exit edit, save, conflict)
  editor.js       — pure helpers: isDirty, classifyFileChange (unit-tested)
  treeops.js      — pure helpers: validateName (inline-rename) + treeAncestors
                    (folders to expand to reveal a file); unit-tested
  review.js       — pure helpers: quoteBlock, formatReview, reanchorReviews
                    for Review Mode (unit-tested); DOM wiring lives in app.js
  mcp.js          — pure helpers: reviewButtonLabel, mcpHintText, reviewBusy,
                    viewerState for the MCP review loop (unit-tested)
  styles.css      — grid layout, CSS variables for light/dark, pre.mermaid
  github-markdown.css, morphdom-umd.min.js, mermaid.min.js  — vendored
  codemirror/     — vendored CodeMirror 5: codemirror.min.js + .css,
                    xml.min.js, markdown.min.js; loaded as classic scripts
icon.svg          — source for icon regeneration
.github/workflows/
  ci.yml          — fmt, clippy -D warnings, test, debug build on push/PR
  release.yml     — tag v* → build aarch64 .dmg, attach to draft Release
                    with auto-generated changelog
```

## Architecture quick-tour

- **Tab model**: `tabs[]` of `{ path, sticky, raw, editing, dirty, savedContent, editBuffer, reviewMode, reviews, generalNote, orphanedReviews }` + `activeIdx`. The last four are Review Mode state (ephemeral; never serialized in session restore). Single-click
  on a tree file replaces the non-sticky "preview" tab (or creates one);
  double-click promotes to sticky. Each tab tracks its own raw/rendered state
  and its own in-progress editor buffer (`editBuffer`), so switching tabs
  preserves unsaved edits.
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
  `renderMermaid` → `renderReviewMarkers`. Order matters: math/mermaid change
  element heights and must run before `restoreAnchor`. New post-render hooks go
  here.
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
- **Theme (light/dark)**: JS is the single source of truth. `app.js` resolves
  the effective theme (`resolveTheme(localStorage["mdviewer.theme"], OS)`) and
  writes it to `document.documentElement.dataset.theme`; all CSS is
  attribute-driven (`:root[data-theme="dark"]` in `styles.css`,
  `[data-theme="dark"] .markdown-body` in `github-markdown.css`) — no
  `prefers-color-scheme` in our stylesheets. The toolbar **☾/☀** button
  (`#toggle-theme`, pure helpers in `ui/theme.js`) flips and persists the
  choice; the `matchMedia` listener only auto-follows the OS until a pref
  exists (`hasThemePref`). `currentTheme` still feeds syntect/Mermaid/render_notes
  and stays in lockstep with `data-theme` via `applyTheme`. FOUC tradeoff: CSP
  `script-src 'self'` forbids an inline `<head>` bootstrap, so `data-theme` is
  set as the first statement of the deferred `app.js` module (sub-frame flash
  possible if a stored pref differs from the OS).
- **Image files**: a frontend-only feature (no Rust). `isImagePath` (`ui/filetype.js`,
  unit-tested) detects image extensions; `renderActive` short-circuits to
  `renderImage` *before* the `render_file` IPC — which is essential because the
  backend does `read_to_string` and would fail on binary data. `renderImage`
  builds an `<img>` at natural size via `convertFileSrc` (asset protocol; `#preview`
  takes the `image-view` class, not `markdown-body`, so the prose-width rules don't
  shrink it). Live reload bumps a per-path `imageVersions` counter → `?v=N`
  cache-bust. The Raw button is hidden and Copy Source / Export are guarded for
  image tabs.
- **Menu actions** fire as Tauri events into the frontend:
  `edit-action` (copy / copy-source / toggle-raw / toggle-edit / save),
  `open-file`, `open-folder`, `menu-check-updates`, `menu-install-claude-hook`,
  `menu-install-mcp-server`.
- **Update check** runs after `init()` on every launch and then on a
  `setInterval` of `UPDATE_CHECK_INTERVAL_MS` (1 h) for the lifetime of the
  process (silent on failure / current; the silent path also respects the
  `DISMISS_KEY` localStorage flag, so re-checks won't resurrect a banner the
  user dismissed for the current latest version). The menu entry **MDViewer ▸
  Check for Updates…** triggers the same function with `silent: false` so it
  surfaces a native dialog when current.
- **Auto-update**: `tauri-plugin-updater` (registered in `lib.rs`, capability
  `updater:default`). The banner's **Update now** downloads the signed
  `.app.tar.gz` in-process, verifies the minisign signature against
  `plugins.updater.pubkey`, swaps the bundle, and **Restart now** relaunches via
  the `restart` command. Because the download is in-process, the new bundle is
  never quarantined — no `xattr` step on update (unlike the first manual DMG
  install).
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
- **Folder content search**: right-click a folder in the tree → "Search in
  Folder…", or right-click the sidebar background, or **Actions ▸ Search
  Files…** (⌘⇧F) to search the whole open tree. All three open a sidebar
  takeover (`<section id="search-panel">` sibling to `<ul id="tree">`,
  toggled by `.searching` on the sidebar). Backend is
  `src-tauri/src/search.rs` (uses ripgrep's `ignore` crate walker; with
  `standard_filters(false)` it behaves like walkdir, and with
  `git_ignore`/`git_exclude`/`git_global`/`ignore` enabled it honors all
  the standard ignore sources). The substring matcher mirrors
  `ui/search.js::findMatches` — non-overlapping matches, case-sensitive +
  whole-word options, Unicode-aware case-insensitive via
  `str::to_lowercase`. Detected binaries (NUL in first 8 KB) and files
  >10 MB are skipped, plus a per-file cap of 200 matches and a total cap
  of 5000 (truncation flag surfaced in the footer). The "Include
  .gitignored files" toggle is OFF by default — IPC sends
  `respectGitignore=true` unless the user flips it. `SearchOpts::default()`
  in Rust is the *opposite* (all flags false, "least filtering") so tests
  stay deterministic regardless of any host's global gitignore; the
  user-facing default lives in `ui/folder_search.js`. The frontend
  debounces input at 150 ms and uses a sequence number to drop stale IPC
  responses (Tauri's `invoke` has no abort). Clicking a result calls
  `openTabAtLine(path, line)` which stashes `pendingJumpLine` on the tab;
  the next `postRender` consumes it, scrolls the matching `data-sourcepos`
  element into view, and pulses `CSS.highlights["search-jump"]` for
  1.5 s. `restoreAnchor` is skipped on that one render so the jump's
  scroll position survives.
- **Split-view editor**: the **Edit** toolbar button (and **Actions ▸ Toggle Edit** /
  `edit-action:"toggle-edit"` event) enters a side-by-side split — CodeMirror 5
  (markdown mode, line numbers, line-wrap) on the left, the live-rendered
  preview on the right, re-rendering via `render_preview` (renders the editor
  buffer, not disk) debounced ~150 ms. Hidden/disabled for image tabs. CodeMirror
  ships only a light `default` theme, so dark mode is done with attribute-driven
  CSS overrides (`[data-theme="dark"] .editor-pane .CodeMirror …` in `styles.css`,
  recoloring surface + markdown tokens) rather than a vendored theme — toggling the
  theme while editing restyles the editor with no JS. Entering
  edit calls `read_source` → primes `savedContent` + `editBuffer`; the editor
  follows the active tab by sharing a single CodeMirror instance re-initialized
  per `setActiveTab`. **Save** (⌘S / Actions ▸ Save) calls `save_file(path,
  contents, expected)`: read-verify-write — refuses if disk diverged from
  `expected` (the last-saved content), unless forced. Atomic write via
  `write_atomically` (shared with `toggle_task`). Unsaved tabs show a ● dirty
  dot (`tab.dirty`). Closing a dirty tab prompts to discard.
- **Editor conflict handling**: `file-changed` events while editing are
  classified by `classifyFileChange` (`ui/editor.js`, pure/unit-tested) into
  `"self"` (disk equals `savedContent` — our own write, ignore), `"reload"` (not
  editing or editor is clean — auto-reload as before), or `"conflict"` (disk
  diverged AND dirty — show banner offering **Reload from disk** / **Keep my
  version**). The `"self"` path suppresses the watcher feedback loop that
  follows every `save_file` write.
- **File operations**: tree right-click menu (file/folder row and sidebar
  background) offers **New File…**, **New Folder…**, **Rename…**, **Duplicate**
  (files), **Delete**. Backend: `src-tauri/src/fs_ops.rs` (pure helpers
  `validate_name`, `duplicate_candidate`, `within_root` + IO `create_file`,
  `create_folder`, `rename_path`, `duplicate_file`) + thin command wrappers in
  `commands.rs` (`create_file`, `create_folder`, `rename_path`, `duplicate_file`,
  `delete_to_trash`). Delete uses the `trash` crate (recoverable). **Inline
  rename**: the row becomes a `<input>` (VS Code style); Enter commits, Esc
  cancels; `validateName` (`ui/treeops.js`, mirrors `fs_ops::validate_name`)
  rejects on the fly. Open tabs are retargeted on rename (descendants too) and
  closed on delete.
- **Security containment for file ops**: all five file-op commands resolve paths
  against `AppState.current_root` via `fs_ops::within_root` (canonicalizes the
  nearest existing ancestor, component-wise `starts_with`). `current_root` is a
  `Mutex<Option<PathBuf>>` on `AppState`, seeded from `Startup.tree_root` in
  `get_initial_state` and updated by the `remember_folder` command whenever the
  frontend changes the sidebar root.
- **Review Mode**: a frontend-only feature (no Rust, no IPC). The **💬 Review**
  toolbar toggle (`#toggle-review`, gated like Raw/Edit — hidden for raw/edit/
  image tabs) is the whole lifecycle: clicking it enters review (`reviewMode`
  true, label becomes **✓ Finish & Copy**); clicking again calls `finishReview`.
  `renderReviewMarkers(t)` is a
  `postRender` hook (so markers survive live-reload like copy buttons): it
  strips its own prior nodes, then injects a left-gutter **+** on each
  annotatable block (`ANNOTATABLE_TAGS`: P/H1-6/LI/PRE/BLOCKQUOTE, excluding
  `pre.mermaid`), a comment card on annotated blocks, and a top **review bar**
  (an instructional hint + general-note textarea). Comments are
  `{ sourcepos, quotedText, comment }`; the anchor key is `blockText()` =
  `quoteBlock(textContent, Infinity)` of the block with injected UI stripped —
  the SAME normalization on both save (`openCommentBox`) and re-anchor, or
  matching breaks. On every render `renderReviewMarkers` re-anchors via
  `reanchorReviews` (pure, `ui/review.js`): matches by `quotedText`, refreshes
  `sourcepos`, and moves non-matches into `orphanedReviews` (surfaced with a
  "⚠ this block changed" tag — never silently dropped; count is conserved, so
  re-anchor alone can't empty both arrays). **Finish & Copy** (toggling review
  off) → `finishReview`: commits any open comment box (clicks its Save button),
  and if there's content, `formatReview` (pure) builds the clipboard markdown
  (relative path, general note, `---` divider only when comments follow, blocks
  in document order), `copyText` copies it (returns a bool — on failure a toast
  warns and the review is **kept** for retry), a success toast confirms
  (`showTransientMessage`, neutral `.info` reuse of the transient banner), then
  all review state is cleared and the mode exits. The comment box commits via
  Save/Enter and dismisses via Cancel/Esc (shared `commit`/`dismiss` closures). `exportDocument` forces
  `reviewMode` off during its re-render (like `prevRaw`) so review chrome never
  leaks into HTML/PDF. State is ephemeral — excluded from session restore and
  reset on the `openPreview` tab-reuse path.
- **Claude Code hook install**: **MDViewer ▸ Install Claude Code Hook…**
  (`menu.rs` emits `menu-install-claude-hook`, un-gated so it shows on both
  platforms) → frontend `installClaudeHook()` (guards on an open `treeRoot`) →
  `commands::install_claude_hook`. The command resolves the open root via
  `current_root`, builds the hook command with `claude_hook::hook_command()`
  (POSIX single-quote / Windows double-quote escaping of `current_exe()`), and
  merges a `Write` `PostToolUse` hook into `<root>/.claude/settings.local.json`
  via the pure `claude_hook::merge_hook` (idempotent: updates an existing
  `--claude-hook` entry's path → `Updated`, else appends → `Installed`; refuses
  to clobber wrong-typed or unparseable settings), written with the shared
  `write_atomically`. The hook command is mdviewer's own binary + `--claude-hook`;
  `main.rs` checks `args().nth(1) == "--claude-hook"` *before* GUI launch and
  calls `run_claude_hook` → `claude_hook::run_hook`: read PostToolUse JSON from
  stdin → `extract_file_path` → `is_plan_file` (md/markdown whose stem contains
  plan/spec/design OR under a `plans`/`specs`/`designs` dir) → `open_in_mdviewer`
  (macOS `open -a <bundle>` → warm tab, or the dev binary directly when not a
  `.app`; Windows re-spawns the exe → new window; child stdio detached). Errors
  and non-matches exit 0 silently so the hook never disrupts Claude. No new IPC
  beyond the one command; no recursion risk (opening a file is a viewer action,
  not a Write).
- **MCP server**: **MDViewer ▸ Install MCP Server…** merges
  `{"mcpServers":{"mdviewer":{"command":<exe>,"args":["--mcp"]}}}` into
  `<root>/.mcp.json` (`mcp::merge_mcp_config`, idempotent like `merge_hook`).
  Claude Code spawns `mdviewer --mcp` (checked in `main.rs` next to
  `--claude-hook`): a hand-rolled stdio JSON-RPC proxy (`mcp.rs`, no SDK) that
  relays `tools/call` over a local socket (`interprocess` crate: per-user named
  pipe on Windows, `$TMPDIR/mdviewer-mcp.sock` on macOS; `MDVIEWER_MCP_SOCKET`
  overrides for tests) to the GUI's listener thread (`mcp_server.rs`),
  launching the GUI via `claude_hook::launch_mdviewer(None)` if the connect
  fails. Tools: `open_document(path, line?)` → `openTabAtLine`/`openSticky`;
  `get_viewer_state`; blocking `request_review(path, instructions?)` → tab
  opens in Review Mode (raw view forced off) with an MCP banner (instructions
  + Decline), toolbar reads **✓ Finish & Send**, and `finishReview` routes the
  `formatReview` markdown to `mcp_review_result` instead of the clipboard
  (proxy dead → clipboard fallback toast; decline/tab-close →
  `{"declined": true}`, a success so Claude proceeds). The proxy emits
  `notifications/progress` every 10 s to hold client timeouts open and exits
  when stdin or stdout closes. Validation is GUI-side (`mcp_server::validate`):
  extension allowlist (markdown+images; markdown only for reviews) +
  existence; the proxy absolutizes relative paths against its cwd (= Claude's
  project root). One review at a time (`reviewBusy` + a post-await re-check —
  the review tab is resolved by path, not focus). Stale socket on startup:
  probe, back off if a live instance answers, else reclaim. The blocking
  request_review call returns through `McpPending::resolve`, which waits for
  the connection thread's socket-write ack — so "Review sent" is only shown
  when the proxy actually received it.

## Platform support

Targets: macOS aarch64 (.dmg) and Windows x86_64 (NSIS .exe + MSI).
Linux is out of scope.

What's macOS-only (and how the Windows build handles it):

- **PDF export** (`export.rs::macos`). Uses WKWebView's print pipeline.
  The non-mac stub in `export.rs` returns
  `"PDF export is not yet supported on Windows"`. The **File ▸ Export as PDF…**
  menu item is cfg-gated to macOS in `menu.rs` so Windows users don't see
  it. HTML export works on both platforms.
- **Install CLI** (`commands.rs::install_cli` + `osascript` admin
  elevation + `/usr/local/bin` symlink). The non-mac stub returns an
  error; the **MDViewer ▸ Install Command Line Tool…** menu item is
  cfg-gated to macOS. The NSIS installer's optional "Add to PATH"
  checkbox covers the equivalent affordance on Windows.
- **`RunEvent::Opened`** (`open_files.rs`, `lib.rs`). macOS surfaces
  Finder file-open as an Apple Event. Windows opens via argv — already
  handled by `main.rs`.

What's intentionally cross-platform:

- `UNSAFE_OPEN_EXTS` (`commands.rs`) is a single union list of dangerous
  extensions for BOTH platforms. macOS-dangerous types (`.app`,
  `.command`, `.scpt`) are harmless to deny on Windows and vice versa.
  When adding a new dangerous extension, add to the single list — don't
  cfg-split.
- `opener::open` (the `opener` crate) replaces the old
  `Command::new("open")` call sites. It dispatches to `start` on
  Windows, `open` on macOS.
- `platform()` (`commands.rs`) is the single Rust-side source of truth
  for the frontend's `IS_MAC` / `IS_WINDOWS` constants in `app.js`.

Windows-specific gotchas:

- **WebView2 dependency.** Bundle config uses `webviewInstallMode:
  downloadBootstrapper` (the default). Installer is small;
  bootstrapper fetches WebView2 only if missing (rare on
  Win 10 1903+ / Win 11).
- **NSIS install mode.** `currentUser` (Tauri 2.x schema; equivalent
  of per-user no-admin install). Mirrors the macOS drag-to-Applications
  experience. Note: the plan originally called this `perUser`, which is
  the Tauri 1.x field name — 2.x renamed it.
- **MSI rejects `-rc.N` versions.** The WiX/MSI `ProductVersion` accepts
  only a *numeric* pre-release identifier, so a tag like `v1.17.0-rc.1`
  fails the `msi` target (`pre-release identifier must be numeric-only`)
  while NSIS builds fine. `release.yml`'s Windows job therefore passes
  `args: --bundles nsis` when `is_prerelease` is true (stable keeps the
  config default `"all"` → NSIS + MSI). Don't strip the suffix to a
  numeric version instead — that desyncs the per-platform `latest.json`
  `version` from macOS's and confuses the updater's compare.
- **`.ico` regeneration.** `cargo tauri icon icon.svg` rebuilds the
  full icon set, including `src-tauri/icons/icon.ico`. The `.ico` must
  remain in the `bundle.icon` array in `tauri.conf.json`.
- **Code signing.** Windows builds are unsigned, like macOS. README
  documents the SmartScreen "More info → Run anyway" workaround.
- **CI** runs on `macos-14` AND `windows-latest` matrix entries in
  `ci.yml` for every PR. The release workflow has separate
  `build-macos` and `build-windows` jobs; only the macOS job sets
  `releaseBody` so the Windows job doesn't clobber it.
- **`latest.json` merge.** `tauri-action` merges per-platform
  `latest.json` fragments on the GitHub Release. The Windows job runs
  without `releaseBody`, so its merge blanks the manifest's `notes` field —
  and the in-app **What's new** modal reads `notes` (via the updater's
  `update.body`), NOT the GitHub release-page body. The `polish-release` job
  restores it: `build-macos` exposes its `changelog` step as a job output,
  and `polish-release` writes that into `.notes` with `jq --arg` (passed via
  env, never shell-interpolated) when it rewrites the Windows asset URLs.
  Without this the modal shows "No release notes available" (regressed in the
  v1.13.0 Windows port; worked on the macOS-only v1.11/v1.12). Smoke-test
  with a `vX.Y.Z-rc1` tag and `gh release view … --json assets` before
  publishing the final draft.

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
  - Atomic write via `write_atomically` (shared with `save_file`) + `std::fs::rename`
    in the same directory (a different directory crosses filesystems on macOS
    and rename loses atomicity). Filename is `.<stem>.mdviewer-<nanos>.tmp`
    so it's both hidden and unmistakably temporary.
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
- **Export stays light via attribute-driven CSS**: `github-markdown.css` is
  attribute-driven — the light color variables are the unconditional
  `.markdown-body` base, dark lives under `[data-theme="dark"] .markdown-body`.
  The exported standalone HTML sets **no** `data-theme` on its `<html>`, so the
  light base always wins; for PDF, `exportDocument` forces `data-theme="light"`
  on the live `<html>` during the print re-render (restored in `finally`). The
  `forceLightCss` helper is now a defensive no-op on this file — it only strips
  `@media (prefers-color-scheme: …)` blocks, which no longer exist here — kept
  in case the vendored CSS is ever re-vendored with media-query themes. KaTeX
  fonts must still be inlined as `data:` URLs, or the `.html` references font
  files that don't travel with it.
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
2. Add a `## [X.Y.Z] - <date>` section to `CHANGELOG.md` with short,
   user-facing bullets (no commit hashes, no internal/test/bump commits) —
   this is what the GitHub release page and the in-app "What's new" modal
   show. If omitted, the workflow falls back to the raw commit log.
3. Bump version in `src-tauri/Cargo.toml` AND `src-tauri/tauri.conf.json`.
4. `cd src-tauri && cargo update -p mdviewer` to refresh `Cargo.lock`.
5. Commit (`Bump to 0.x.y` or include the user-facing change).
6. `git tag v0.x.y && git push && git push origin v0.x.y`.
7. Release workflow auto-builds aarch64, extracts the matching `CHANGELOG.md`
   section for the release notes (falling back to commits since the previous
   tag when absent), attaches `.dmg` + `.app.tar.gz` to a **draft** Release.
8. `gh release edit v0.x.y --draft=false` to publish.

The release workflow signs the `.app.tar.gz` and attaches `latest.json` when the
`TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` secrets are
set (one-time setup). Publishing the draft (step 7) is what makes the update
reach existing installs.

**Cutting a beta:** bump `Cargo.toml` + `tauri.conf.json` to a prerelease
version (e.g. `1.16.0-rc.1`), `cargo update -p mdviewer`, commit, then
`git tag v1.16.0-rc.1 && git push origin v1.16.0-rc.1`. The release workflow
detects the `-` and publishes to the rolling `beta` release (prerelease,
non-draft) instead of a draft — beta-opted installs pick it up automatically.
When the matching stable `vX.Y.Z` is later published, `promote-beta.yml` rolls
beta testers onto it. Smoke-test the manifest with
`gh release view beta --json assets` after the run. A prerelease tag gets
curated notes only if `CHANGELOG.md` has a matching `## [1.16.0-rc.1]` section;
otherwise it falls back to the commit log (fine for testers).

### Icon regeneration

```sh
# All platforms (preferred — generates .ico, .icns, and PNGs from one source):
cargo tauri icon icon.svg

# macOS-only manual fallback (the old recipe, kept for reference):
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
  (`app.restart()`), and **What's new** opens an in-app modal
  (`openNotesModal`) showing the release changelog — extracted from the
  `## Changes` section client-side (`extractChangelog` in `update.js`), rendered
  via the `render_notes` command (comrak) and inserted with `DOMParser` +
  `replaceChildren`, with a link out to the `releases/tag/v<version>` page via
  `open_url`.
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
