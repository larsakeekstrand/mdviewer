use std::path::PathBuf;

use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Wry};
use tauri_plugin_dialog::DialogExt;

use crate::recent;

const RECENT_PREFIX: &str = "recent-folder-";
const SOURCE_URL: &str = "https://github.com/larsakeekstrand/mdviewer";

/// Installs the menu and registers the global menu event handler.
/// Subsequent recent-list changes call `rebuild` to update the bar.
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    rebuild(app)?;
    app.on_menu_event(move |app, event| {
        let id = event.id().as_ref().to_string();
        match id.as_str() {
            "open-file" => prompt_open_file(app.clone()),
            "open-folder" => prompt_open_folder(app.clone()),
            "export-html" => {
                let _ = app.emit("export", "html");
            }
            "export-pdf" => {
                let _ = app.emit("export", "pdf");
            }
            "check-updates" => {
                let _ = app.emit("menu-check-updates", ());
            }
            "install-cli" => {
                let _ = app.emit("menu-install-cli", ());
            }
            "github-source" => {
                let _ = crate::commands::open_url(SOURCE_URL.to_string());
            }
            "edit-copy" => {
                let _ = app.emit("edit-action", "copy");
            }
            "edit-find" => {
                let _ = app.emit("edit-action", "find");
            }
            "edit-copy-source" => {
                let _ = app.emit("edit-action", "copy-source");
            }
            "edit-toggle-raw" => {
                let _ = app.emit("edit-action", "toggle-raw");
            }
            "clear-recent" => {
                recent::clear(app);
                let _ = rebuild(app);
            }
            other if other.starts_with(RECENT_PREFIX) => {
                if let Some(idx) = other
                    .strip_prefix(RECENT_PREFIX)
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    let folders = recent::load(app);
                    if let Some(path) = folders.get(idx).cloned() {
                        choose_recent_folder(app.clone(), path);
                    }
                }
            }
            _ => {}
        }
    });
    Ok(())
}

/// Builds a fresh menu (reflecting the current recent-folders list) and
/// applies it. Called on startup and after any change to the recent list.
fn rebuild(app: &AppHandle) -> tauri::Result<()> {
    let open_file = MenuItemBuilder::with_id("open-file", "Open File…")
        .accelerator("CmdOrCtrl+O")
        .build(app)?;
    let open_folder = MenuItemBuilder::with_id("open-folder", "Open Folder…")
        .accelerator("CmdOrCtrl+Shift+O")
        .build(app)?;

    let recent_submenu = build_recent_submenu(app)?;

    let export_html = MenuItemBuilder::with_id("export-html", "Export as HTML…").build(app)?;
    let export_pdf = MenuItemBuilder::with_id("export-pdf", "Export as PDF…").build(app)?;

    let check_updates =
        MenuItemBuilder::with_id("check-updates", "Check for Updates…").build(app)?;
    let install_cli =
        MenuItemBuilder::with_id("install-cli", "Install Command Line Tool…").build(app)?;
    let github_source =
        MenuItemBuilder::with_id("github-source", "View Source on GitHub").build(app)?;

    let app_menu = SubmenuBuilder::new(app, "MDViewer")
        .about(None)
        .item(&github_source)
        .item(&check_updates)
        .item(&install_cli)
        .separator()
        .item(&PredefinedMenuItem::hide(app, None)?)
        .item(&PredefinedMenuItem::hide_others(app, None)?)
        .item(&PredefinedMenuItem::show_all(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::quit(app, None)?)
        .build()?;

    let file_menu = SubmenuBuilder::new(app, "File")
        .item(&open_file)
        .item(&open_folder)
        .item(&recent_submenu)
        .separator()
        .item(&export_html)
        .item(&export_pdf)
        .separator()
        .close_window()
        .build()?;

    let edit_copy = MenuItemBuilder::with_id("edit-copy", "Copy")
        .accelerator("CmdOrCtrl+C")
        .build(app)?;
    let edit_find = MenuItemBuilder::with_id("edit-find", "Find…")
        .accelerator("CmdOrCtrl+F")
        .build(app)?;
    let edit_copy_source =
        MenuItemBuilder::with_id("edit-copy-source", "Copy Source").build(app)?;
    let edit_toggle_raw = MenuItemBuilder::with_id("edit-toggle-raw", "Toggle Raw").build(app)?;

    // Title intentionally not "Edit": macOS auto-injects Writing Tools,
    // AutoFill, Start Dictation, and Emoji & Symbols into any submenu
    // titled "Edit" regardless of which items we put in it.
    let edit_menu = SubmenuBuilder::new(app, "Actions")
        .item(&edit_copy)
        .item(&edit_find)
        .separator()
        .item(&edit_copy_source)
        .item(&edit_toggle_raw)
        .build()?;

    let view_menu = SubmenuBuilder::new(app, "View").fullscreen().build()?;

    let window_menu = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .build()?;

    let menu = MenuBuilder::new(app)
        .item(&app_menu)
        .item(&file_menu)
        .item(&edit_menu)
        .item(&view_menu)
        .item(&window_menu)
        .build()?;

    app.set_menu(menu)?;
    Ok(())
}

fn build_recent_submenu(app: &AppHandle) -> tauri::Result<tauri::menu::Submenu<Wry>> {
    let folders = recent::load(app);
    let mut builder = SubmenuBuilder::new(app, "Open Recent");
    if folders.is_empty() {
        let empty = MenuItemBuilder::new("(no recent folders)")
            .enabled(false)
            .build(app)?;
        builder = builder.item(&empty);
    } else {
        // Build all items first so they outlive the SubmenuBuilder chain.
        let items: Vec<_> = folders
            .iter()
            .enumerate()
            .map(|(i, p)| {
                MenuItemBuilder::with_id(format!("{RECENT_PREFIX}{i}"), recent::display(p))
                    .build(app)
            })
            .collect::<Result<Vec<_>, _>>()?;
        for item in &items {
            builder = builder.item(item);
        }
        let clear = MenuItemBuilder::with_id("clear-recent", "Clear Recent").build(app)?;
        builder = builder.separator().item(&clear);
    }
    builder.build()
}

fn prompt_open_file(app: AppHandle) {
    app.dialog()
        .file()
        .add_filter("Markdown", &["md", "markdown", "mdown", "mkd", "mkdn"])
        .add_filter("All files", &["*"])
        .pick_file(move |chosen| {
            if let Some(file_path) = chosen {
                if let Some(p) = file_path.as_path() {
                    let _ = app.emit("open-file", p.to_string_lossy().to_string());
                }
            }
        });
}

fn prompt_open_folder(app: AppHandle) {
    app.dialog().file().pick_folder(move |chosen| {
        if let Some(file_path) = chosen {
            if let Some(p) = file_path.as_path() {
                let pb = PathBuf::from(p);
                recent::push(&app, &pb);
                let _ = rebuild(&app);
                let _ = app.emit("open-folder", pb.to_string_lossy().to_string());
            }
        }
    });
}

fn choose_recent_folder(app: AppHandle, path: PathBuf) {
    if !path.is_dir() {
        // Folder is gone; drop from list silently.
        let mut list = recent::load(&app);
        list.retain(|p| p != &path);
        recent::clear(&app);
        for p in list {
            recent::push(&app, &p);
        }
        let _ = rebuild(&app);
        return;
    }
    recent::push(&app, &path);
    let _ = rebuild(&app);
    let _ = app.emit("open-folder", path.to_string_lossy().to_string());
}
