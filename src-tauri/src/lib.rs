mod claude_hook;
mod code;
mod commands;
mod export;
mod fs_ops;
mod git;
mod markdown;
pub mod mcp;
mod mcp_server;
mod menu;
#[cfg(target_os = "macos")]
mod open_files;
#[cfg(target_os = "macos")]
mod pdf_postprocess;
mod recent;
mod routing;
mod search;
mod tasklist;
mod tree;
mod watcher;
mod windows;
pub mod xlsx;

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::Manager;

pub struct Startup {
    pub tree_root: Option<PathBuf>,
    pub initial_file: Option<PathBuf>,
}

pub struct AppState {
    pub tree_root: Option<PathBuf>,
    pub initial_file: Option<PathBuf>,
    pub windows: Mutex<windows::Registry>,
    pub focus: Mutex<routing::FocusOrder>,
    /// Files opened (Finder) before setup registered any window; drained into
    /// "main" at the end of setup. None once drained.
    pub early_opens: Mutex<Option<Vec<PathBuf>>>,
    /// Serializes task-list write-backs. Held only for the read-verify-write
    /// critical section so two rapid clicks can't interleave reads.
    pub tasklist_lock: Mutex<()>,
}

/// Run the `--claude-hook` PostToolUse handler and return (never launches the GUI).
pub fn run_claude_hook() {
    claude_hook::run_hook();
}

/// Run the `--mcp` stdio MCP proxy and return (never starts the Tauri runtime
/// in this process; the proxy may spawn the GUI as a separate process).
pub fn run_mcp_proxy() {
    mcp::run_proxy();
}

pub fn run(startup: Startup) {
    let state = AppState {
        tree_root: startup.tree_root,
        initial_file: startup.initial_file,
        windows: Mutex::new(windows::Registry::default()),
        focus: Mutex::new(routing::FocusOrder::default()),
        early_opens: Mutex::new(Some(Vec::new())),
        tasklist_lock: Mutex::new(()),
    };

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(state)
        .manage(mcp_server::McpPending::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_initial_state,
            commands::list_dir,
            commands::git_status,
            commands::watch_tree,
            commands::render_file,
            commands::render_preview,
            commands::render_notes,
            commands::open_file,
            commands::read_source,
            commands::restart,
            commands::get_preferences,
            commands::set_update_channel,
            commands::get_pdf_settings,
            commands::save_pdf_settings,
            commands::check_update,
            commands::open_url,
            commands::open_path,
            commands::path_within_dir,
            commands::save_export,
            export::export_pdf,
            commands::toggle_task,
            commands::save_file,
            commands::frontend_ready,
            commands::remember_folder,
            commands::create_file,
            commands::create_folder,
            commands::rename_path,
            commands::duplicate_file,
            commands::delete_to_trash,
            commands::save_session,
            commands::install_cli,
            commands::install_claude_hook,
            commands::install_mcp_server,
            commands::show_integration_window,
            commands::integration_status,
            commands::mcp_respond,
            commands::mcp_review_result,
            commands::platform,
            commands::search_in_folder,
        ])
        .setup(|app| {
            // Pre-warm the markdown engine so the first render isn't laggy.
            std::thread::spawn(|| {
                markdown::prewarm();
            });
            let handle = app.handle().clone();
            let state = handle.state::<AppState>();
            if let Some(root) = &state.tree_root {
                recent::push(&handle, root);
            }
            let root = commands::resolve_initial_root(&handle, state.tree_root.as_deref());
            state.windows.lock().unwrap().insert("main", root.clone());
            // AppKit can deliver a cold launch-to-open before setup runs. Those
            // files wait for main's frontend_ready drain like any other early open.
            let early = state.early_opens.lock().unwrap().take();
            if let Some(files) = early.filter(|f| !f.is_empty()) {
                if let Some(w) = state.windows.lock().unwrap().get_mut("main") {
                    w.pending_files.extend(files);
                }
            }
            state.focus.lock().unwrap().touch("main");
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title(&windows::window_title(&root));
                let _ = window.show();
            }
            menu::install(&handle)?;
            mcp_server::start(handle.clone());
            Ok(())
        })
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
