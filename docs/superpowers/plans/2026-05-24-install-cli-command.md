# Install Command Line Tool Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a **MDViewer ▸ Install Command Line Tool…** menu item that symlinks `mdviewer` into `/usr/local/bin` so the app can be launched from a terminal.

**Architecture:** A macOS-only Tauri command `install_cli` resolves the running binary via `std::env::current_exe()`, classifies whatever is currently at `/usr/local/bin/mdviewer` into a `LinkState`, and a pure `decide` function maps that to an `InstallAction`. The command tries an unprivileged symlink first and escalates to an injection-safe `osascript … with administrator privileges` prompt only on a permission/missing-dir error. The menu emits an event; the frontend invokes the command and reports the outcome in a native dialog.

**Tech Stack:** Tauri 2.11, Rust (`serde`, `std::os::unix::fs::symlink`, `std::process::Command`→`osascript`), vanilla JS (no build step), `cargo test` for the pure Rust helper.

**Spec:** `docs/superpowers/specs/2026-05-24-install-cli-command-design.md`

---

## File Structure

- `src-tauri/src/commands.rs` — new `install_cli` command plus the `LinkState` / `InstallAction` / `InstallOutcome` types, the pure `decide` helper, the `classify_link` / `create_cli_symlink` / `install_with_admin` I/O wrappers, the `CLI_LINK_PATH` const, and unit tests for `decide`.
- `src-tauri/src/lib.rs` — register `install_cli` in the `generate_handler!` list.
- `src-tauri/src/menu.rs` — add the `install-cli` menu item to the MDViewer submenu and emit `menu-install-cli` on click.
- `ui/app.js` — listen for `menu-install-cli`, invoke `install_cli`, and show the result via the dialog plugin.
- `README.md` — fix the wrong binary path (`MDViewer` → `mdviewer`), point users at the new menu item, and add a Features bullet.

> **Why the backend is one task, not several:** `commands.rs` items used only by the `#[cfg(test)]` module (`decide`, the enums) are flagged `dead_code` by `clippy --all-targets -- -D warnings` in the non-test compilation until the `install_cli` command that uses them is wired into `generate_handler!`. So the TDD red→green for `decide` happens first within Task 1, but `clippy` is only run after the full command + registration are in place. (Same constraint the restore-tabs plan hit.)

---

## Task 1: Backend `install_cli` command (commands.rs + lib.rs)

**Files:**
- Modify: `src-tauri/src/commands.rs` (add types + helpers + command after `save_session` ~line 313; add tests inside the existing `#[cfg(test)] mod tests` block ~lines 315-360)
- Modify: `src-tauri/src/lib.rs` (the `generate_handler!` list ~lines 51-67)

- [ ] **Step 1: Write the failing tests for `decide`**

In `src-tauri/src/commands.rs`, inside the existing `#[cfg(test)] mod tests { ... }` block (after the last existing test `allows_viewable_files`, before the module's closing `}`), add:

```rust
    #[test]
    fn decide_creates_when_absent() {
        assert_eq!(decide(LinkState::Absent), InstallAction::Create);
    }

    #[test]
    fn decide_creates_when_symlink_points_elsewhere() {
        assert_eq!(decide(LinkState::SymlinkElsewhere), InstallAction::Create);
    }

    #[test]
    fn decide_already_installed_when_symlink_points_to_target() {
        assert_eq!(
            decide(LinkState::SymlinkToTarget),
            InstallAction::AlreadyInstalled
        );
    }

    #[test]
    fn decide_refuses_when_non_symlink_exists() {
        assert_eq!(decide(LinkState::NonSymlink), InstallAction::RefuseNonSymlink);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test decide_ 2>&1 | tail -20`
Expected: compile error — `LinkState`, `InstallAction`, and `decide` are undefined.

- [ ] **Step 3: Add the types and the pure `decide` helper**

In `src-tauri/src/commands.rs`, add this block immediately after the `save_session` command (ends ~line 313) and before the `#[cfg(test)]` module:

```rust
/// Where the CLI symlink lives. `/usr/local/bin` is the first entry in macOS's
/// default `/etc/paths`, so it is already on `$PATH` for every login shell with
/// no profile edits. The directory is root-owned on a stock Mac, so creating
/// the link there usually needs admin rights (handled in `install_with_admin`).
const CLI_LINK_PATH: &str = "/usr/local/bin/mdviewer";

/// What is currently present at `CLI_LINK_PATH`.
#[derive(Debug, PartialEq)]
enum LinkState {
    Absent,
    SymlinkToTarget,
    SymlinkElsewhere,
    NonSymlink,
}

/// What `install_cli` should do given the current `LinkState`.
#[derive(Debug, PartialEq)]
enum InstallAction {
    Create,
    AlreadyInstalled,
    RefuseNonSymlink,
}

/// Pure decision: maps the on-disk state to the action. Unit-tested.
fn decide(state: LinkState) -> InstallAction {
    match state {
        LinkState::Absent | LinkState::SymlinkElsewhere => InstallAction::Create,
        LinkState::SymlinkToTarget => InstallAction::AlreadyInstalled,
        LinkState::NonSymlink => InstallAction::RefuseNonSymlink,
    }
}

/// The outcome reported back to the frontend. Serializes to snake_case strings
/// (`"installed"`, `"already_installed"`, `"cancelled"`) that `app.js` matches.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallOutcome {
    Installed,
    AlreadyInstalled,
    Cancelled,
}
```

- [ ] **Step 4: Run the `decide` tests to verify they pass**

Run: `cd src-tauri && cargo test decide_ 2>&1 | tail -20`
Expected: the four `decide_*` tests pass. **Do NOT run `clippy` yet** — `decide` and the enums are only referenced by tests until Step 5/6 wire in the command, so `clippy --all-targets -D warnings` would trip `dead_code` on the intermediate state.

- [ ] **Step 5: Add the I/O wrappers and the `install_cli` command**

In `src-tauri/src/commands.rs`, immediately after the `InstallOutcome` enum you just added (and before the `#[cfg(test)]` module), add:

```rust
/// Classifies what is at `link` relative to `target`. Uses `symlink_metadata`
/// so a symlink is inspected, not followed (a broken symlink still reads as a
/// symlink; a missing path reads as `Absent`).
fn classify_link(link: &Path, target: &Path) -> LinkState {
    match std::fs::symlink_metadata(link) {
        Err(_) => LinkState::Absent,
        Ok(meta) if meta.file_type().is_symlink() => match std::fs::read_link(link) {
            Ok(dest) if dest.as_path() == target => LinkState::SymlinkToTarget,
            _ => LinkState::SymlinkElsewhere,
        },
        Ok(_) => LinkState::NonSymlink,
    }
}

/// Creates (or replaces) the symlink. Tries unprivileged first so Macs where
/// `/usr/local/bin` is user-writable (e.g. Homebrew on Intel) never see a
/// password prompt; escalates only on a permission or missing-directory error.
fn create_cli_symlink(target: &Path, link: &Path) -> Result<InstallOutcome, String> {
    if link.is_symlink() {
        // Best-effort: on a root-owned dir this fails and we fall through to
        // the elevated `ln -sf`, which replaces the stale link itself.
        let _ = std::fs::remove_file(link);
    }
    match std::os::unix::fs::symlink(target, link) {
        Ok(()) => Ok(InstallOutcome::Installed),
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::NotFound
            ) =>
        {
            install_with_admin(target)
        }
        Err(e) => Err(format!("failed to create symlink: {e}")),
    }
}

/// Elevated path: an AppleScript admin prompt that runs `mkdir -p` + `ln -sf`.
/// The exe path is passed as an `argv` item and shell-quoted by AppleScript's
/// `quoted form of`, so it is never interpolated into a shell string by us — a
/// path containing spaces or quotes cannot break out. The destination is a
/// fixed literal.
fn install_with_admin(target: &Path) -> Result<InstallOutcome, String> {
    let script = format!(
        "do shell script \"mkdir -p /usr/local/bin && ln -sf \" & quoted form of (item 1 of argv) & \" {CLI_LINK_PATH}\" with administrator privileges"
    );
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg("on run argv")
        .arg("-e")
        .arg(&script)
        .arg("-e")
        .arg("end run")
        .arg("--")
        .arg(target)
        .output()
        .map_err(|e| format!("failed to launch osascript: {e}"))?;

    if output.status.success() {
        return Ok(InstallOutcome::Installed);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    // AppleScript reports a dismissed auth dialog as error -128 ("User canceled").
    if stderr.contains("-128") {
        return Ok(InstallOutcome::Cancelled);
    }
    Err(format!(
        "failed to create symlink with administrator privileges: {}",
        stderr.trim()
    ))
}

/// Symlinks the running binary into `/usr/local/bin` so `mdviewer` is runnable
/// from a terminal. The target is always our own `current_exe()`, never a
/// caller-supplied path.
#[tauri::command]
pub fn install_cli() -> Result<InstallOutcome, String> {
    let target =
        std::env::current_exe().map_err(|e| format!("cannot resolve app binary path: {e}"))?;
    let link = Path::new(CLI_LINK_PATH);
    match decide(classify_link(link, &target)) {
        InstallAction::AlreadyInstalled => Ok(InstallOutcome::AlreadyInstalled),
        InstallAction::RefuseNonSymlink => Err(format!(
            "{CLI_LINK_PATH} already exists and is not a symlink; refusing to overwrite it"
        )),
        InstallAction::Create => create_cli_symlink(&target, link),
    }
}
```

(`Path`, `Serialize`, and `AppHandle` are already imported at the top of `commands.rs`; no new `use` lines are needed — `std::os::unix::fs::symlink` and `std::io::ErrorKind` are referenced fully-qualified.)

- [ ] **Step 6: Register the command (lib.rs)**

In `src-tauri/src/lib.rs`, in the `tauri::generate_handler![ ... ]` list, add `commands::install_cli,` after `commands::save_session,`:

```rust
            commands::remember_folder,
            commands::save_session,
            commands::install_cli,
        ])
```

- [ ] **Step 7: Build, lint, test (everything now wired)**

Run from `src-tauri/`:

```bash
cargo build 2>&1 | tail -3
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
cargo fmt --check && echo FMT_OK
cargo test 2>&1 | grep 'test result'
```

Expected: build succeeds; clippy clean (`decide`/enums are now reachable from the registered `install_cli`); `FMT_OK`; tests pass (existing suite + 4 new `decide_*`). If fmt reports diffs, run `cargo fmt` and re-check.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "Add install_cli command (symlink mdviewer into /usr/local/bin)"
```

---

## Task 2: Menu item + frontend handler (menu.rs + app.js)

**Files:**
- Modify: `src-tauri/src/menu.rs` (`check_updates` builder ~line 81-82; the `app_menu` `SubmenuBuilder` ~line 86-96; the `on_menu_event` match ~line 27-32)
- Modify: `ui/app.js` (listener block ~line 200-202; add `installCli` near `checkForUpdates` ~line 1861)

- [ ] **Step 1: Build the menu item (menu.rs)**

In `src-tauri/src/menu.rs`, find the `check_updates` item builder:

```rust
    let check_updates =
        MenuItemBuilder::with_id("check-updates", "Check for Updates…").build(app)?;
```

Add directly after it:

```rust
    let install_cli =
        MenuItemBuilder::with_id("install-cli", "Install Command Line Tool…").build(app)?;
```

- [ ] **Step 2: Add the item to the MDViewer submenu (menu.rs)**

In `src-tauri/src/menu.rs`, the `app_menu` builder currently reads:

```rust
    let app_menu = SubmenuBuilder::new(app, "MDViewer")
        .about(None)
        .item(&github_source)
        .item(&check_updates)
        .separator()
```

Insert `.item(&install_cli)` after `.item(&check_updates)`:

```rust
    let app_menu = SubmenuBuilder::new(app, "MDViewer")
        .about(None)
        .item(&github_source)
        .item(&check_updates)
        .item(&install_cli)
        .separator()
```

- [ ] **Step 3: Emit the event on click (menu.rs)**

In `src-tauri/src/menu.rs`, the `on_menu_event` match has:

```rust
            "check-updates" => {
                let _ = app.emit("menu-check-updates", ());
            }
```

Add a new arm directly after it:

```rust
            "install-cli" => {
                let _ = app.emit("menu-install-cli", ());
            }
```

- [ ] **Step 4: Build + lint the Rust side**

Run from `src-tauri/`:

```bash
cargo build 2>&1 | tail -3
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
cargo fmt --check && echo FMT_OK
```

Expected: build succeeds, clippy clean, `FMT_OK`.

- [ ] **Step 5: Add the frontend listener (app.js)**

In `ui/app.js`, find the existing listener:

```js
  await listen("menu-check-updates", async () => {
    await checkForUpdates({ silent: false });
  });
```

Add directly after it:

```js
  await listen("menu-install-cli", async () => {
    await installCli();
  });
```

- [ ] **Step 6: Add the `installCli` handler (app.js)**

In `ui/app.js`, the `checkForUpdates` function ends (~line 1861) just before `function showUpdateAvailable(update) {`. Insert this function between them:

```js
async function installCli() {
  let outcome;
  try {
    outcome = await invoke("install_cli");
  } catch (e) {
    await dialogApi.message("Couldn't install the command line tool.\n\n" + e, {
      title: "MDViewer",
      kind: "error",
    });
    return;
  }
  if (outcome === "cancelled") return;
  const msg =
    outcome === "already_installed"
      ? "The mdviewer command line tool is already installed."
      : "Installed. You can now run mdviewer from a terminal.";
  await dialogApi.message(msg, { title: "MDViewer", kind: "info" });
}
```

(`installCli` is a hoisted function declaration, so the listener registered earlier in `init` can reference it — the same pattern `checkForUpdates` already uses.)

- [ ] **Step 7: Rebuild the bundle and sanity-check wiring**

Frontend changes only take effect after a Rust rebuild (`tauri-codegen` bundles `ui/` at compile time). Run:

```bash
cd src-tauri && cargo build 2>&1 | tail -2
cd /Users/laek/source/mdviewer && node --test ui/*.test.js 2>&1 | tail -5
grep -n "menu-install-cli\|installCli\|install-cli" ui/app.js src-tauri/src/menu.rs
```

Expected: build succeeds; the JS suite still passes (no new JS tests — the testable logic is the Rust `decide`); grep shows the menu item id + emit in `menu.rs` and the listener + `installCli` definition in `app.js`.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/menu.rs ui/app.js
git commit -m "Add Install Command Line Tool menu item and handler"
```

---

## Task 3: README update

**Files:**
- Modify: `README.md` (the command-line parenthetical in Usage ▸ Launching; the Features list; the Menus list)

- [ ] **Step 1: Fix the wrong binary path and point at the menu item**

In `README.md`, find:

```
  (If `mdviewer` is not on `$PATH`, invoke `/Applications/MDViewer.app/Contents/MacOS/MDViewer` directly or symlink it into `/usr/local/bin/mdviewer`.)
```

Replace with:

```
  (To run `mdviewer` from a terminal, use **MDViewer ▸ Install Command Line Tool…** — it symlinks the app's binary into `/usr/local/bin`, which is already on your `$PATH`, prompting for your password if that directory needs admin rights. To do it by hand instead: `sudo ln -s /Applications/MDViewer.app/Contents/MacOS/mdviewer /usr/local/bin/mdviewer`.)
```

- [ ] **Step 2: Add a Features bullet**

In `README.md`, find the CLI feature bullet:

```
- CLI: `mdviewer [file-or-directory]`
```

Add a new bullet immediately after it:

```
- **Install Command Line Tool** — one menu click symlinks `mdviewer` into `/usr/local/bin` so you can launch it from any terminal
```

- [ ] **Step 3: Document the menu item**

In `README.md`, find the Menus bullet:

```
- **MDViewer ▸ Check for Updates…** — manually checks GitHub for a newer release (the same check also runs silently on startup). **View Source on GitHub** opens the repository.
```

Add a new bullet immediately after it:

```
- **MDViewer ▸ Install Command Line Tool…** — symlinks `mdviewer` into `/usr/local/bin` so you can launch it from a terminal (prompts for your password if the directory needs admin rights).
```

- [ ] **Step 4: Verify and commit**

Run: `grep -n "Install Command Line Tool\|MacOS/mdviewer\|MacOS/MDViewer" README.md`
Expected: the two new menu/feature mentions, the corrected `MacOS/mdviewer` path, and **no** remaining `MacOS/MDViewer` (capitalized) reference.

```bash
git add README.md
git commit -m "Document the Install Command Line Tool menu item"
```

---

## Notes for the executor

- **Commit style** (per `CLAUDE.md`): imperative subject, **no** `Co-Authored-By` trailer.
- **Lint gate**: `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` from `src-tauri/` must be clean before each Rust commit. Run `clippy` only at the points the plan says to (Task 1 Step 7), not after the partial `decide`-only state.
- **No version bump task.** `v1.6.0` is already cut and released, so this is post-1.6.0 work. The maintainer decides release grouping and does the bump + `cargo update -p mdviewer` + tag at release time (see `CLAUDE.md` ▸ "Cutting a release", whose step 1 is the README refresh — Task 3 already covers this feature).
- **Serde enum wire format:** `InstallOutcome` uses `#[serde(rename_all = "snake_case")]`, so the values the JS compares are exactly `"installed"`, `"already_installed"`, and `"cancelled"`. `install_cli` takes no arguments, so there is no camelCase/snake_case arg-naming concern.
- **`current_exe()` under `cargo run` vs. the bundle:** running via `cargo run` makes `current_exe()` the debug binary (`target/debug/mdviewer`), so the menu item would link to that. The feature is only meaningful in a real bundle, so test it from `cargo tauri build` output, not `cargo run`.
- **Manual end-state check (maintainer):** build the bundle (`cd src-tauri && cargo tauri build`), install/run `MDViewer.app`, click **MDViewer ▸ Install Command Line Tool…**. On a stock Mac expect the native password prompt; after entering it, a "Installed" dialog. Then in a new terminal: `which mdviewer` → `/usr/local/bin/mdviewer`, and `mdviewer ~/some.md` opens the file. Click the item again → "already installed" dialog with no prompt. To re-test the prompt path: `sudo rm /usr/local/bin/mdviewer` first.
- **Why try-then-escalate:** most Macs have a root-owned `/usr/local/bin`, so the unprivileged `symlink` fails with `PermissionDenied` and we escalate; but on Homebrew-Intel setups where the dir is user-writable, the symlink succeeds with no password prompt. The `osascript … with administrator privileges` call is the only privileged action and is injection-safe because the exe path travels as an `argv` item quoted by `quoted form of`.
```
