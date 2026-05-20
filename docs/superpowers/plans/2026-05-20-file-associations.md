# `.md` File Associations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let MDViewer be set as the default macOS app for markdown files and open the file when launched/triggered from Finder.

**Architecture:** Declare `bundle.fileAssociations` in `tauri.conf.json` (Tauri generates the `CFBundleDocumentTypes` Info.plist entries). Handle `RunEvent::Opened { urls }` in `lib.rs`: convert URLs to existing markdown paths, and either emit the existing `open-file` event + focus the window (if the frontend is ready) or buffer them. A `frontend_ready` IPC command drains the buffer under the same lock, making the cold-launch double-click race-free.

**Tech Stack:** Rust + Tauri 2.11 (`RunEvent::Opened`, `tauri::Url`, `App::run` callback); vanilla JS frontend.

---

## File structure

- `src-tauri/src/open_files.rs` — **create.** Pure `markdown_path(&Url)` helper (+ tests), the macOS-only `markdown_paths` and `handle_opened`.
- `src-tauri/src/lib.rs` — **modify.** `PendingOpens` struct + `AppState.opens` field; register `frontend_ready`; switch the tail from `.run(ctx)` to `.build(ctx)? + app.run(callback)` handling `Opened`; `mod open_files;`.
- `src-tauri/src/commands.rs` — **modify.** Add the `frontend_ready` command.
- `src-tauri/tauri.conf.json` — **modify.** Add `bundle.fileAssociations`.
- `ui/app.js` — **modify.** Reorder `init()`: listeners → `frontend_ready()` handshake → pick tree root → render once → open files.

Confirmed Tauri 2.11 facts:
- `tauri::RunEvent::Opened { urls: Vec<url::Url> }` is `#[cfg(target_os = "macos"|"ios"|"android")]` — the match arm must be `#[cfg(target_os = "macos")]`.
- `tauri::Url` is re-exported; `Url::scheme()` and `Url::to_file_path()` exist; `to_file_path` decodes percent-encoding, no filesystem access.
- Built `App::run<F: FnMut(&AppHandle, RunEvent) + 'static>(self, callback)`; `Builder::build(ctx) -> tauri::Result<App>`.
- CI builds on `macos-14`, so macOS-gated code compiles and tests run there. App is macOS-only.
- `pub fn` items in this lib crate are not `dead_code`-linted, so a public helper with only test callers won't fail `clippy -D warnings`.

---

## Task 1: `open_files::markdown_path` pure helper (TDD)

**Files:**
- Create: `src-tauri/src/open_files.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod open_files;`)
- Test: `src-tauri/src/open_files.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Create the file with tests and a stub**

Create `src-tauri/src/open_files.rs`:

```rust
pub fn markdown_path(_url: &tauri::Url) -> Option<std::path::PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tauri::Url;

    #[test]
    fn accepts_markdown_file_url() {
        let u = Url::parse("file:///tmp/notes.md").unwrap();
        assert_eq!(markdown_path(&u), Some(PathBuf::from("/tmp/notes.md")));
    }

    #[test]
    fn rejects_non_markdown_extension() {
        let u = Url::parse("file:///tmp/notes.txt").unwrap();
        assert_eq!(markdown_path(&u), None);
    }

    #[test]
    fn rejects_non_file_scheme() {
        let u = Url::parse("https://example.com/x.md").unwrap();
        assert_eq!(markdown_path(&u), None);
    }

    #[test]
    fn decodes_percent_encoded_path() {
        let u = Url::parse("file:///tmp/my%20notes.md").unwrap();
        assert_eq!(markdown_path(&u), Some(PathBuf::from("/tmp/my notes.md")));
    }
}
```

- [ ] **Step 2: Declare the module**

In `src-tauri/src/lib.rs`, the module declarations are:

```rust
mod commands;
mod markdown;
mod menu;
mod recent;
mod tree;
mod updates;
mod watcher;
```

Add `mod open_files;` after `mod menu;`:

```rust
mod commands;
mod markdown;
mod menu;
mod open_files;
mod recent;
mod tree;
mod updates;
mod watcher;
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd src-tauri && cargo test --lib open_files`
Expected: `accepts_markdown_file_url` and `decodes_percent_encoded_path` FAIL (stub returns `None`); the two `rejects_*` PASS.

- [ ] **Step 4: Implement the real helper**

Replace the stub `markdown_path` in `src-tauri/src/open_files.rs` with:

```rust
pub fn markdown_path(url: &tauri::Url) -> Option<std::path::PathBuf> {
    if url.scheme() != "file" {
        return None;
    }
    let path = url.to_file_path().ok()?;
    crate::markdown::is_markdown_path(&path).then_some(path)
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --lib open_files`
Expected: 4 passed.

- [ ] **Step 6: Lint**

Run: `cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings`
Expected: clean, no fmt diff.

- [ ] **Step 7: Commit**

```bash
cd /Users/laek/source/mdviewer && git add src-tauri/src/open_files.rs src-tauri/src/lib.rs && git commit -m "Add open_files::markdown_path helper for file:// URLs"
```
(No `Co-Authored-By` trailer.)

---

## Task 2: `PendingOpens` state + `frontend_ready` command

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/commands.rs`

- [ ] **Step 1: Add `PendingOpens` and the `AppState` field**

In `src-tauri/src/lib.rs`, the current types are:

```rust
pub struct Startup {
    pub tree_root: PathBuf,
    pub initial_file: Option<PathBuf>,
}

pub struct AppState {
    pub tree_root: PathBuf,
    pub initial_file: Option<PathBuf>,
    pub watcher: Mutex<watcher::WatcherSlot>,
}
```

Replace the `AppState` block (and add `PendingOpens` above it) so it reads:

```rust
pub struct Startup {
    pub tree_root: PathBuf,
    pub initial_file: Option<PathBuf>,
}

#[derive(Default)]
pub struct PendingOpens {
    pub ready: bool,
    pub files: Vec<PathBuf>,
}

pub struct AppState {
    pub tree_root: PathBuf,
    pub initial_file: Option<PathBuf>,
    pub watcher: Mutex<watcher::WatcherSlot>,
    pub opens: Mutex<PendingOpens>,
}
```

- [ ] **Step 2: Initialize the field in `run()`**

In `src-tauri/src/lib.rs`, the state is built as:

```rust
    let state = AppState {
        tree_root: startup.tree_root,
        initial_file: startup.initial_file,
        watcher: Mutex::new(watcher::WatcherSlot::default()),
    };
```

Replace with:

```rust
    let state = AppState {
        tree_root: startup.tree_root,
        initial_file: startup.initial_file,
        watcher: Mutex::new(watcher::WatcherSlot::default()),
        opens: Mutex::new(PendingOpens::default()),
    };
```

- [ ] **Step 3: Add the `frontend_ready` command**

In `src-tauri/src/commands.rs`, append:

```rust
#[tauri::command]
pub fn frontend_ready(state: State<'_, AppState>) -> Vec<String> {
    let mut guard = state.opens.lock().unwrap();
    guard.ready = true;
    guard
        .files
        .drain(..)
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}
```

(`State` and `AppState` are already imported at the top of `commands.rs`.)

- [ ] **Step 4: Register the command**

In `src-tauri/src/lib.rs`, the handler list ends:

```rust
            commands::open_url,
            commands::open_path,
        ])
```

Add `commands::frontend_ready`:

```rust
            commands::open_url,
            commands::open_path,
            commands::frontend_ready,
        ])
```

- [ ] **Step 5: Build + lint**

Run: `cd src-tauri && cargo build && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: builds clean, no warnings.

- [ ] **Step 6: Commit**

```bash
cd /Users/laek/source/mdviewer && git add src-tauri/src/lib.rs src-tauri/src/commands.rs && git commit -m "Add pending-opens buffer and frontend_ready handshake command"
```

---

## Task 3: Handle `RunEvent::Opened` (run-loop + handler)

**Files:**
- Modify: `src-tauri/src/open_files.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add `markdown_paths` and `handle_opened`**

In `src-tauri/src/open_files.rs`, append (after `markdown_path`, before the `#[cfg(test)]` module):

```rust
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

- [ ] **Step 2: Bind the builder to `app`**

In `src-tauri/src/lib.rs`, the builder chain starts:

```rust
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
```

Change the first line to bind it:

```rust
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
```

- [ ] **Step 3: Replace `.run(...)` with `.build(...)?` + `app.run(callback)`**

In `src-tauri/src/lib.rs`, the chain currently ends:

```rust
        .run(tauri::generate_context!())
        .expect("error while running mdviewer");
}
```

Replace with:

```rust
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
}
```

- [ ] **Step 4: Build + test + lint**

Run: `cd src-tauri && cargo build && cargo test && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: builds clean; existing tests (incl. `open_files`) pass; no warnings.

- [ ] **Step 5: Commit**

```bash
cd /Users/laek/source/mdviewer && git add src-tauri/src/open_files.rs src-tauri/src/lib.rs && git commit -m "Handle RunEvent::Opened: open or buffer files from Finder"
```

---

## Task 4: Declare file associations

**Files:**
- Modify: `src-tauri/tauri.conf.json`

- [ ] **Step 1: Add `fileAssociations` to the bundle**

In `src-tauri/tauri.conf.json`, the bundle starts:

```json
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
```

Insert `fileAssociations` between `"targets"` and `"icon"`:

```json
  "bundle": {
    "active": true,
    "targets": "all",
    "fileAssociations": [
      {
        "ext": ["md", "markdown", "mdown", "mkd", "mkdn"],
        "name": "Markdown Document",
        "description": "Markdown document",
        "role": "Viewer"
      }
    ],
    "icon": [
```

- [ ] **Step 2: Verify the config parses**

Run: `cd src-tauri && cargo build`
Expected: builds successfully (the build runs `tauri-build`, which validates `tauri.conf.json` against its schema; a malformed config fails here).

- [ ] **Step 3: Commit**

```bash
cd /Users/laek/source/mdviewer && git add src-tauri/tauri.conf.json && git commit -m "Declare markdown file associations (Viewer role)"
```

---

## Task 5: Frontend — reorder `init()` for the handshake

**Files:**
- Modify: `ui/app.js`

- [ ] **Step 1: Replace the `init()` function**

In `ui/app.js`, replace the entire current `init()` function:

```js
async function init() {
  initMermaid();
  const initial = await invoke("get_initial_state");
  treeRoot = initial.tree_root;
  treeTitle.textContent = basename(treeRoot) || treeRoot;
  treeTitle.title = treeRoot;

  await renderRoot();

  if (initial.initial_file) {
    await openSticky(initial.initial_file);
  }

  await listen("file-changed", async (ev) => {
    const tab = activeTab();
    if (tab && ev.payload === tab.path) {
      await renderActive({ scrollLock: true });
    }
  });

  await listen("open-file", async (ev) => {
    await openSticky(ev.payload);
  });

  await listen("open-folder", async (ev) => {
    await openExternalFolder(ev.payload);
  });

  await listen("edit-action", async (ev) => {
    await runEditAction(ev.payload);
  });

  await listen("menu-check-updates", async () => {
    await checkForUpdates({ silent: false });
  });

  window
    .matchMedia("(prefers-color-scheme: dark)")
    .addEventListener("change", async () => {
      currentTheme = colorScheme();
      initMermaid();
      if (activeTab())
        await renderActive({ scrollLock: false, forceMermaid: true });
    });

  rawBtn.addEventListener("click", onToggleRaw);
}
```

with:

```js
async function init() {
  initMermaid();
  const initial = await invoke("get_initial_state");

  // Register listeners before the readiness handshake so a file opened the
  // instant the app becomes ready isn't missed.
  await listen("file-changed", async (ev) => {
    const tab = activeTab();
    if (tab && ev.payload === tab.path) {
      await renderActive({ scrollLock: true });
    }
  });

  await listen("open-file", async (ev) => {
    await openSticky(ev.payload);
  });

  await listen("open-folder", async (ev) => {
    await openExternalFolder(ev.payload);
  });

  await listen("edit-action", async (ev) => {
    await runEditAction(ev.payload);
  });

  await listen("menu-check-updates", async () => {
    await checkForUpdates({ silent: false });
  });

  window
    .matchMedia("(prefers-color-scheme: dark)")
    .addEventListener("change", async () => {
      currentTheme = colorScheme();
      initMermaid();
      if (activeTab())
        await renderActive({ scrollLock: false, forceMermaid: true });
    });

  rawBtn.addEventListener("click", onToggleRaw);

  // Drain files Finder buffered during a cold launch; afterwards, files opened
  // while running arrive live via the "open-file" listener above.
  let pending = [];
  try {
    pending = await invoke("frontend_ready");
  } catch (e) {
    console.error("frontend_ready failed", e);
  }

  // A cold Finder launch (no argv file) starts the sidebar at the file's folder.
  treeRoot =
    !initial.initial_file && pending.length
      ? parentDir(pending[0])
      : initial.tree_root;
  treeTitle.textContent = basename(treeRoot) || treeRoot;
  treeTitle.title = treeRoot;

  await renderRoot();

  if (initial.initial_file) await openSticky(initial.initial_file);
  for (const p of pending) await openSticky(p);
}
```

- [ ] **Step 2: Syntax-check and build**

Run: `cd /Users/laek/source/mdviewer && node --check ui/app.js && (cd src-tauri && cargo build)`
Expected: `app.js` parses with no output; build succeeds.

- [ ] **Step 3: Smoke-test the non-Finder paths still work**

Run: `cd /Users/laek/source/mdviewer/src-tauri && cargo run -- ../README.md`
Expected: the app opens with `README.md` rendered and its folder in the sidebar (argv path unaffected by the reorder). Close the window.

- [ ] **Step 4: Commit**

```bash
cd /Users/laek/source/mdviewer && git add ui/app.js && git commit -m "Frontend: drain Finder-opened files via frontend_ready handshake"
```

---

## Task 6: Manual end-to-end verification (built bundle)

File associations only exist in a built `.app`, so this must use `cargo tauri build`, not `cargo run`. Requires `cargo-tauri` (install once: `cargo install tauri-cli --version "^2"`).

- [ ] **Step 1: Build the bundle**

Run: `cd /Users/laek/source/mdviewer/src-tauri && cargo tauri build`
Expected: produces `target/release/bundle/macos/MDViewer.app` (and a `.dmg`).

- [ ] **Step 2: Confirm the association landed in Info.plist**

Run: `plutil -p /Users/laek/source/mdviewer/src-tauri/target/release/bundle/macos/MDViewer.app/Contents/Info.plist | grep -A 25 CFBundleDocumentTypes`
Expected: a `CFBundleDocumentTypes` array referencing the markdown extensions and `CFBundleTypeRole = Viewer`.

- [ ] **Step 3: Install + de-quarantine + register**

```bash
cp -R /Users/laek/source/mdviewer/src-tauri/target/release/bundle/macos/MDViewer.app /Applications/
sudo xattr -dr com.apple.quarantine /Applications/MDViewer.app
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f /Applications/MDViewer.app
```

- [ ] **Step 4: Set MDViewer as the default for `.md`**

In Finder: right-click any `.md` → Get Info → "Open with:" → choose MDViewer → click "Change All…".

- [ ] **Step 5: Verify cold launch**

Quit MDViewer if running. Double-click a `.md` in Finder.
Expected: MDViewer launches and shows that file, with the file's folder in the sidebar.

- [ ] **Step 6: Verify warm launch**

With MDViewer already open (on some folder), double-click a different `.md`.
Expected: it opens as a new tab, the window comes to the front, and the sidebar folder is unchanged.

- [ ] **Step 7: Verify a non-default open path**

Right-click a `.md` → Open With → MDViewer (without changing the default).
Expected: same behavior as Step 5/6.

(No commit — verification only. If a step needs a code fix, commit it with a descriptive message and re-verify.)

---

## Notes for the implementer

- **Do not change `main.rs`** — `argv` launching stays as-is and must keep working (smoke-tested in Task 5 Step 3).
- The `frontend_ready` command sets `ready` and drains under the same `opens` mutex that `handle_opened` takes — this is what prevents a file being lost between "check ready" and "buffer it". Don't split this into a separate atomic flag.
- The `Opened` arm must stay `#[cfg(target_os = "macos")]`; the variant doesn't exist on other targets. `_handle`/`_event` are underscore-named so non-macOS builds don't warn about unused bindings.
- `cargo run` cannot exercise the Finder path (no Info.plist); that's why Task 6 is a separate built-bundle verification.
- If macOS doesn't offer MDViewer for `.md` after install, re-run the `lsregister -f` command from Task 6 Step 3 and confirm the app isn't quarantined.
```
