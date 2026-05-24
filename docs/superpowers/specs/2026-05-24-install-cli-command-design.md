# Install Command Line Tool — design

Date: 2026-05-24
Status: approved, ready for implementation plan

## Goal

Give users a one-click way to put `mdviewer` on their `$PATH` so they can run
`mdviewer [file-or-directory]` from a terminal. Today the binary only exists
inside the bundle (`/Applications/MDViewer.app/Contents/MacOS/mdviewer`) and the
README tells users to symlink it by hand — and even points at the wrong
(capitalized) binary name. A menu item that creates the symlink for them removes
the manual step entirely.

## Why a symlink into /usr/local/bin

- `/usr/local/bin` is the **first** entry in macOS's default `/etc/paths`, so it
  is already on `$PATH` for every login shell with **no profile edits**. This is
  the same approach VS Code's "Install 'code' command in PATH" uses.
- The symlink target is `std::env::current_exe()`, not a hard-coded
  `/Applications/...` path, so the command keeps working if the user installed
  the app somewhere else.
- The directory is root-owned (`root:wheel`, mode 755) on a stock Mac, so
  creating the link usually needs admin rights — handled below.

## Behavior

- New menu item **MDViewer ▸ Install Command Line Tool…**, placed directly after
  **Check for Updates…** in the app submenu.
- Clicking it ensures `/usr/local/bin/mdviewer` is a symlink to the running app
  binary, then reports the outcome in a native dialog.
- Idempotent: if the correct symlink already exists, it reports "already
  installed" without writing or prompting.
- macOS-only, like the rest of the app.

## Decision logic (pure, unit-tested)

The classify-then-act core is a pure function so every branch is testable
without touching the filesystem or triggering a password prompt:

```rust
enum LinkState {
    Absent,           // nothing at /usr/local/bin/mdviewer
    SymlinkToTarget,  // already our symlink
    SymlinkElsewhere, // a symlink, but to some other path
    NonSymlink,       // a regular file or directory
}

enum InstallAction {
    Create,            // make the symlink (Absent or SymlinkElsewhere)
    AlreadyInstalled,  // no-op success
    RefuseNonSymlink,  // refuse to clobber an unrelated file
}

fn decide(state: LinkState) -> InstallAction
```

- `Absent` / `SymlinkElsewhere` → `Create` (a stale symlink — e.g. an old
  install location — is replaced).
- `SymlinkToTarget` → `AlreadyInstalled`.
- `NonSymlink` → `RefuseNonSymlink`. We will not overwrite a real file named
  `mdviewer` that someone else may have placed there (could be an unrelated
  tool). This is the conservative, data-safe choice.

## I/O wrapper

A thin function computes `LinkState`, then performs the action:

1. Resolve `target = std::env::current_exe()`.
2. `LinkState` from `std::fs::symlink_metadata(link)`:
   - error / not found → `Absent`
   - is symlink → compare `std::fs::read_link(link)` against `target`
     (`SymlinkToTarget` vs `SymlinkElsewhere`)
   - otherwise → `NonSymlink`
3. Dispatch on `decide(state)`:
   - `AlreadyInstalled` → return that outcome.
   - `RefuseNonSymlink` → return an error outcome with an explanatory message.
   - `Create`:
     - **Try unprivileged first**: if a symlink is already present, remove it,
       then `std::os::unix::fs::symlink(target, link)`. Succeeds on Macs where
       `/usr/local/bin` is user-writable (e.g. Homebrew on Intel) with **no**
       password prompt.
     - On `PermissionDenied` / missing-directory errors, **escalate** (below).

The command returns `Result<InstallOutcome, String>`, where `InstallOutcome`
is a small serializable enum/struct the frontend maps to a dialog
(`Installed` / `AlreadyInstalled` / `Cancelled`). The error branch carries the
refuse/other failure message (Tauri serializes it to a JS rejection).

## Privilege escalation (injection-safe)

When the unprivileged symlink fails, run `osascript` with administrator
privileges, which shows the **native macOS password dialog**. Crucially, the
exe path is passed as an AppleScript `argv` item and shell-quoted by
`quoted form of` — it is never string-interpolated into a shell command by us:

```
osascript \
  -e 'on run argv' \
  -e 'do shell script "mkdir -p /usr/local/bin && ln -sf " & quoted form of (item 1 of argv) & " /usr/local/bin/mdviewer" with administrator privileges' \
  -e 'end run' \
  -- "<current_exe path>"
```

- Invoked via `std::process::Command::new("osascript").args([...])` — **no shell
  on our side**, so a target path containing spaces or quotes cannot break out.
  The destination (`/usr/local/bin/mdviewer`) is a fixed literal.
- `mkdir -p` covers the fresh-Apple-Silicon case where `/usr/local/bin` does not
  yet exist (creating it also needs admin).
- `ln -sf` atomically replaces a stale symlink under the elevated path.
- osascript exits non-zero on cancel; the **`-128` "User canceled"** code is
  mapped to the `Cancelled` outcome (a gentle no-op, not an error dialog).
- The command is **synchronous**. Tauri runs non-async commands off the main
  thread, so blocking on the password dialog does not freeze the UI (unlike
  `export_pdf`, which must be async for a different reason — its print runs on
  the main runloop).

## Menu wiring & feedback

Mirrors the existing `check-updates` flow (menu → event → frontend):

- `menu.rs`: add `MenuItemBuilder::with_id("install-cli", "Install Command Line
  Tool…")` to the MDViewer submenu after `check_updates`; the handler emits
  `app.emit("menu-install-cli", ())`.
- `app.js`: a listener for `menu-install-cli` calls `invoke("install_cli")` and
  shows the result with the dialog plugin's `message(...)`:
  - `Installed` → "MDViewer command line tool installed. You can now run
    `mdviewer` from the terminal."
  - `AlreadyInstalled` → "The `mdviewer` command line tool is already installed."
  - `Cancelled` → no dialog (silent no-op).
  - rejection (refuse / other failure) → error dialog with the message.

## Files touched

- `src-tauri/src/commands.rs` — `install_cli` command, `LinkState` /
  `InstallAction` / `InstallOutcome` types, `decide`, the I/O wrapper, the
  osascript escalation, and unit tests for `decide`.
- `src-tauri/src/lib.rs` — register `install_cli` in the `invoke_handler`.
- `src-tauri/src/menu.rs` — the `install-cli` menu item + event emit.
- `ui/app.js` — `menu-install-cli` listener that invokes the command and shows
  the result dialog.
- `README.md` — fix the wrong binary path (`MDViewer` → `mdviewer`), replace the
  "symlink it yourself" parenthetical with a pointer to the menu item, add a
  Features bullet.

## Testing

- Rust unit tests in `commands.rs` for `decide`: one per branch
  (`Absent` → `Create`, `SymlinkToTarget` → `AlreadyInstalled`,
  `SymlinkElsewhere` → `Create`, `NonSymlink` → `RefuseNonSymlink`).
- The privileged `osascript` path and the real symlink I/O are **not**
  unit-tested — they need a password prompt and writable system paths,
  consistent with how the rest of the codebase keeps untestable FFI/IO thin and
  tests the pure decision logic instead.

## Out of scope (YAGNI)

- An **uninstall** command. (Removing the symlink later is a one-line `rm`; add
  it only if asked.)
- A `~/.local/bin` fallback or any shell-profile editing.
- A Homebrew cask. (Separate distribution channel; would overlap the in-app
  auto-updater.)
- Non-macOS platforms (the app is macOS-only).
