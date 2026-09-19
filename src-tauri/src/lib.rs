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
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// Set on ExitRequested so the per-window teardown that follows ⌘Q can't
    /// overwrite the quit-time window snapshot.
    pub quitting: AtomicBool,
    /// True when main's root is the bare-cwd fallback (no argv root, no saved
    /// window, no usable last_folder), i.e. a placeholder a file open may repoint.
    pub main_placeholder: AtomicBool,
}

/// Marks the app as quitting and saves the window snapshot. An empty snapshot
/// (the last window already destroyed) must not overwrite the one
/// `on_destroyed` saved for it.
fn save_on_exit(handle: &tauri::AppHandle) {
    handle
        .state::<AppState>()
        .quitting
        .store(true, Ordering::SeqCst);
    let snap = windows::snapshot(handle);
    if !snap.is_empty() {
        recent::save_windows(handle, &snap);
    }
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
        quitting: AtomicBool::new(false),
        main_placeholder: AtomicBool::new(false),
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
            commands::helper_owner,
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
            let saved = recent::restore_windows(recent::load_windows(&handle), |p| p.is_dir());
            let from_saved = state.tree_root.is_none() && !saved.is_empty();
            let placeholder = state.tree_root.is_none()
                && saved.is_empty()
                && !recent::load_last(&handle).is_some_and(|p| p.is_dir());
            state.main_placeholder.store(placeholder, Ordering::SeqCst);
            let root = if from_saved {
                saved[0].root.clone()
            } else {
                commands::resolve_initial_root(&handle, state.tree_root.as_deref())
            };
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
                if let Some(b) = saved.first().filter(|_| from_saved).and_then(|w| w.bounds) {
                    let _ = window.set_position(tauri::LogicalPosition::new(b.x, b.y));
                    let _ = window.set_size(tauri::LogicalSize::new(b.w, b.h));
                }
                let _ = window.show();
            }
            windows::record_bounds(&handle, "main");
            let main_canonical = state.windows.lock().unwrap().root("main").ok();
            for w in saved.iter().skip(usize::from(from_saved)) {
                if Some(&w.root) != main_canonical.as_ref() {
                    if let Err(e) =
                        windows::create_project_window(&handle, w.root.clone(), w.bounds)
                    {
                        eprintln!("mdviewer: {e}");
                    }
                }
            }
            if saved.len() > usize::from(from_saved) {
                // Restored windows are built after main; keep the most recent in front.
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_focus();
                }
            }
            menu::install(&handle)?;
            mcp_server::start(handle.clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            let label = window.label().to_string();
            let app = window.app_handle();
            match event {
                tauri::WindowEvent::Focused(true) => {
                    let state = app.state::<AppState>();
                    let is_project = state.windows.lock().unwrap().get(&label).is_some();
                    if is_project {
                        state.focus.lock().unwrap().touch(&label);
                    }
                }
                tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                    let is_project = app
                        .state::<AppState>()
                        .windows
                        .lock()
                        .unwrap()
                        .get(&label)
                        .is_some();
                    if is_project {
                        windows::record_bounds(app, &label);
                    }
                }
                tauri::WindowEvent::Destroyed => windows::on_destroyed(app, &label),
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building mdviewer");

    app.run(move |handle, event| match event {
        // macOS ⌘Q (terminate:) reaches us only as Exit; closing the last
        // window and app.exit() go through ExitRequested. Both save here.
        tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit => save_on_exit(handle),
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Opened { urls } => open_files::handle_opened(handle, urls),
        _ => {}
    });
}
