mod claude_hook;
mod code;
mod commands;
mod export;
mod fs_ops;
mod git;
pub mod launch;
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
    /// Paths forwarded by a second launch before setup finished; dispatched at
    /// the end of setup. None once drained.
    pub startup_queue: Mutex<Option<Vec<launch::LaunchTarget>>>,
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

/// A second launch's argv, forwarded by the single-instance plugin. `argv[1]`
/// is untrusted and resolves exactly like a cold launch's.
#[cfg(any(target_os = "macos", windows))]
fn single_instance_open(app: &tauri::AppHandle, argv: Vec<String>, cwd: String) {
    let Some(raw) = argv.get(1) else {
        // Bare relaunch: bring the most recent window forward.
        if let Some(w) = windows::front_label(app).and_then(|l| app.get_webview_window(&l)) {
            let _ = w.set_focus();
        }
        return;
    };
    let Ok(target) = launch::resolve_launch_path(raw, std::path::Path::new(&cwd)) else {
        return;
    };
    let state = app.state::<AppState>();
    if let Some(queue) = state.startup_queue.lock().unwrap().as_mut() {
        queue.push(target);
        return;
    }
    dispatch_launch(app, target);
}

#[cfg(any(target_os = "macos", windows))]
fn dispatch_launch(app: &tauri::AppHandle, target: launch::LaunchTarget) {
    match target {
        launch::LaunchTarget::Folder(root) => windows::open_folder_in_new_window(app, root),
        launch::LaunchTarget::File(f) => windows::deliver_path(app, f),
    }
}

/// The single-instance plugin's macOS socket. Must match the plugin's
/// `/tmp/<identifier with . and - → _>_si.sock` naming (2.4.x, no `semver`
/// feature) for bundle id `com.mdviewer.app`.
#[cfg(target_os = "macos")]
const SINGLE_INSTANCE_SOCKET: &str = "/tmp/com_mdviewer_app_si.sock";

/// A socket at the shared /tmp path that another user created would capture
/// every launch (and its argv/cwd); sticky /tmp stops us from removing it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn socket_owned_by_other(socket_uid: Option<u32>, my_uid: u32) -> bool {
    socket_uid.is_some_and(|u| u != my_uid)
}

#[cfg(target_os = "macos")]
fn single_instance_allowed() -> bool {
    use std::os::unix::fs::MetadataExt;
    // macOS's $TMPDIR is a per-user 0700 directory owned by the current user,
    // which gives us our uid without a libc dependency.
    let Ok(me) = std::fs::metadata(std::env::temp_dir()) else {
        return true;
    };
    let socket_uid = std::fs::symlink_metadata(SINGLE_INSTANCE_SOCKET)
        .ok()
        .map(|m| m.uid());
    !socket_owned_by_other(socket_uid, me.uid())
}

#[cfg(windows)]
fn single_instance_allowed() -> bool {
    true
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
        startup_queue: Mutex::new(Some(Vec::new())),
    };

    let builder = tauri::Builder::default();
    #[cfg(any(target_os = "macos", windows))]
    let builder = if single_instance_allowed() {
        builder.plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            single_instance_open(app, argv, cwd);
        }))
    } else {
        eprintln!("mdviewer: single-instance socket owned by another user; running without single-instance");
        builder
    };
    let app = builder
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
            // AppKit can deliver a cold launch-to-open before setup runs. Route
            // those like any other open, now that restored windows are
            // registered: a placeholder main is repointed at the first file's
            // root, and windows not yet ready buffer them for frontend_ready.
            let early = state.early_opens.lock().unwrap().take();
            for p in early.unwrap_or_default() {
                windows::deliver_path(&handle, p);
            }
            menu::install(&handle)?;
            mcp_server::start(handle.clone());
            #[cfg(any(target_os = "macos", windows))]
            {
                let queued = state.startup_queue.lock().unwrap().take();
                for t in queued.unwrap_or_default() {
                    dispatch_launch(&handle, t);
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_instance_socket_ownership() {
        assert!(!socket_owned_by_other(None, 501));
        assert!(!socket_owned_by_other(Some(501), 501));
        assert!(socket_owned_by_other(Some(502), 501));
    }
}
