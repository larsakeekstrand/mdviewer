# Default app for `.md` files (file associations) — design

**Date:** 2026-05-20
**Status:** Approved, ready for planning

## Problem

MDViewer cannot be set as the default app for `.md` files in Finder, and
double-clicking a `.md` wouldn't open it even if it could. Two root causes:

1. **No file-type declaration.** macOS Launch Services only offers an app in
   "Open With → Change All…" when its bundle `Info.plist` has
   `CFBundleDocumentTypes` for that type. `tauri.conf.json` has no
   `bundle.fileAssociations`, so nothing is declared.
2. **Finder opens aren't read.** `main.rs` takes the file only from `argv[1]`.
   When Finder opens a file *with* an app it sends an Apple Event, surfaced by
   Tauri as `RunEvent::Opened { urls }`. The app uses the plain `.run(...)` form
   and ignores it, so a double-clicked file launches MDViewer showing the
   working directory, not the file.

## Goal & scope

- Make MDViewer registerable as the default handler for markdown files, and
  actually open the file when launched/triggered from Finder.
- Extensions: **all five** the app already treats as markdown — `md`,
  `markdown`, `mdown`, `mkd`, `mkdn` (matches the in-app `MD_EXT` set).
- Cold launch from a double-click: set the sidebar to the file's folder and open
  it (same as today's `argv` behavior).
- Open while already running: open the file as a new **sticky tab** in the
  existing window and bring the window to front; **keep the current sidebar
  folder**.
- Keep `argv`/CLI launching working unchanged.

Out of scope: Linux/Windows associations; changing the bare-launch (no file)
default tree root (still the process cwd); deep-link URL schemes; editor role.

## Approach (chosen)

Native Tauri `RunEvent::Opened` + `bundle.fileAssociations`, with a small
readiness handshake so the cold-launch double-click is race-free. No new
dependencies.

Rejected: `tauri-plugin-deep-link` (extra dependency, mostly for URL schemes,
still needs the readiness handling); associations-without-buffering (the cold
double-click — the primary case — is exactly when `Opened` fires before the JS
listeners exist).

## Confirmed Tauri 2.11 API facts

- `tauri::RunEvent::Opened { urls: Vec<url::Url> }` — **`#[cfg(target_os = "macos" | "ios" | "android")]`**, so the match arm must be `#[cfg(target_os = "macos")]`.
- `tauri::Url` is re-exported (`pub use url::Url`); `Url::scheme()` and `Url::to_file_path()` are available. `to_file_path` decodes percent-encoding and does no filesystem access.
- Built `App::run<F: FnMut(&AppHandle, RunEvent) + 'static>(self, callback)` — the event-loop callback form. `Builder::build(context) -> tauri::Result<App>`.
- CI builds on `macos-14`, so the macOS-gated code compiles and tests run there. The app is macOS-only; non-macOS builds are not a target.

## Data flow

```
Finder double-click → macOS Apple Event → RunEvent::Opened { urls }
   → open_files::handle_opened
       → markdown_paths(urls)            (file:// + markdown ext + exists)
       → if frontend ready: emit "open-file" per path + focus window
         else:              buffer paths in AppState.opens

Frontend init():
   get_initial_state → register listeners → frontend_ready() (drains buffer)
   → choose tree root → render once → open argv file + drained files as tabs
   → later "open-file" events (warm opens) → openSticky (existing listener)
```

## Components

### 1. `src-tauri/tauri.conf.json` — declare associations

Add to `bundle`:

```json
"fileAssociations": [
  {
    "ext": ["md", "markdown", "mdown", "mkd", "mkdn"],
    "name": "Markdown Document",
    "description": "Markdown document",
    "role": "Viewer"
  }
]
```

Tauri's bundler generates the `CFBundleDocumentTypes` Info.plist entries from
this. Present only in a `cargo tauri build` bundle, not `cargo run`.

### 2. `src-tauri/src/lib.rs` — state + event-loop

- Add a small struct and an `AppState` field, replacing any need for a separate
  atomic (single lock avoids a check-then-push race):

  ```rust
  #[derive(Default)]
  pub struct PendingOpens {
      pub ready: bool,
      pub files: Vec<PathBuf>,
  }
  ```
  `AppState` gains `pub opens: Mutex<PendingOpens>`.

- Add `mod open_files;`.
- Register the new `commands::frontend_ready` in `invoke_handler!`.
- Change the tail of `run()` from `.run(generate_context!())` to:

  ```rust
  let app = tauri::Builder::default()
      … (plugin, manage, invoke_handler, setup) …
      .build(tauri::generate_context!())
      .expect("error while building mdviewer");

  app.run(move |_handle, _event| {
      #[cfg(target_os = "macos")]
      {
          if let tauri::RunEvent::Opened { urls } = _event {
              open_files::handle_opened(_handle, urls);
          }
      }
  });
  ```
  (`_handle`/`_event` names keep non-macOS builds warning-free; the cfg block
  uses them on macOS.)

### 3. `src-tauri/src/open_files.rs` — pure helper + handler

```rust
pub fn markdown_path(url: &tauri::Url) -> Option<std::path::PathBuf> {
    if url.scheme() != "file" {
        return None;
    }
    let path = url.to_file_path().ok()?;
    crate::markdown::is_markdown_path(&path).then_some(path)
}

#[cfg(target_os = "macos")]
pub fn markdown_paths(urls: &[tauri::Url]) -> Vec<std::path::PathBuf> {
    urls.iter()
        .filter_map(markdown_path)
        .filter(|p| p.is_file())
        .collect()
}

#[cfg(target_os = "macos")]
pub fn handle_opened(handle: &tauri::AppHandle, urls: Vec<tauri::Url>) {
    use tauri::{Emitter, Manager};
    let paths = markdown_paths(&urls);
    if paths.is_empty() {
        return;
    }
    let state = handle.state::<crate::AppState>();
    let mut guard = state.opens.lock().unwrap();
    if guard.ready {
        drop(guard);
        for p in &paths {
            let _ = handle.emit("open-file", p.to_string_lossy().into_owned());
        }
        if let Some(w) = handle.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
    } else {
        guard.files.extend(paths);
    }
}
```

`markdown_path` is pure (no filesystem) and unit-tested. `markdown_paths` adds
the existence check; `handle_opened` is the macOS event handler. (These two are
`#[cfg(target_os = "macos")]`; the app targets only macOS, so they are never
dead code in a real build.)

### 4. `src-tauri/src/commands.rs` — handshake command

```rust
#[tauri::command]
pub fn frontend_ready(state: tauri::State<AppState>) -> Vec<String> {
    let mut guard = state.opens.lock().unwrap();
    guard.ready = true;
    guard
        .files
        .drain(..)
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}
```

Setting `ready` and draining under the same lock that `handle_opened` takes
makes the buffer hand-off race-free: a file is either drained here (returned to
the frontend) or, if it arrives after `ready` is set, emitted live — never both,
never lost.

### 5. `ui/app.js` — reorder `init()`

Render the tree exactly once, after the handshake, so there's no flash of `/`:

1. `get_initial_state()`.
2. Register all event listeners (so live `open-file` events aren't missed).
3. `pending = await frontend_ready()` (drained Finder files; `[]` on a bare
   launch).
4. Tree root = the argv root, unless there was **no** argv file and `pending`
   is non-empty (cold Finder launch) — then the first pending file's parent
   (`parentDir`). Render the tree once.
5. Open the argv file (if any) and each pending file via `openSticky`.

Warm opens after startup arrive on the existing `open-file` listener →
`openSticky` (new tab, current folder kept); the window is focused from Rust.

## Error handling

Non-`file://` URLs, unparsable URLs, non-markdown extensions, and
non-existent paths are filtered out (`markdown_path` / `markdown_paths`). An
`Opened` batch with nothing valid is a no-op. `frontend_ready` failing in JS is
caught and logged; the app still renders normally.

## Testing

- **Rust unit tests** (`open_files.rs`, run in CI on macos-14): `markdown_path`
  returns `Some` for a `file://…/x.md` URL (incl. a percent-encoded
  `my%20notes.md` → `/…/my notes.md`), and `None` for a `.txt` URL and for a
  non-`file` scheme. No filesystem or event loop touched.
- **Manual (end-to-end, requires a built bundle):**
  1. `cd src-tauri && cargo tauri build`.
  2. Confirm associations landed: `plutil -p target/release/bundle/macos/MDViewer.app/Contents/Info.plist | grep -A 20 CFBundleDocumentTypes`.
  3. Copy the app to `/Applications`; `sudo xattr -dr com.apple.quarantine /Applications/MDViewer.app`.
  4. In Finder, right-click a `.md` → Get Info → "Open with:" → MDViewer → "Change All…".
  5. Double-click a `.md` with the app **closed** → it opens with the file's
     folder in the sidebar.
  6. With the app **running**, double-click another `.md` → new tab, window
     comes to front, sidebar folder unchanged.

## Notes / risks

- **Only testable from a built `.app`.** `cargo run` has no Info.plist, so
  Launch Services can't see the associations. Registration is normally automatic
  once the app is in `/Applications` and launched once; force with
  `…/LaunchServices.framework/Support/lsregister -f /Applications/MDViewer.app`
  if needed.
- **Quarantine:** the ad-hoc-signed/unsigned build may need the existing
  `xattr -dr com.apple.quarantine` step before Launch Services behaves.
- **UTI conformance:** Tauri generates the document-type entries from `ext`. If
  macOS is fussy about a system markdown UTI (`net.daringfireball.markdown`),
  inspect the generated Info.plist during manual verification; extension-based
  `CFBundleDocumentTypes` is sufficient to be selectable as default.
- **Cold-launch race** is eliminated by the single-lock handshake (component 4).
- Bare launch with no file is unchanged (sidebar shows the process cwd); out of
  scope.
```
