mod commands;
mod export;
mod git;
mod markdown;
mod menu;
#[cfg(target_os = "macos")]
mod open_files;
mod recent;
mod search;
mod tasklist;
mod tree;
mod watcher;

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::Manager;

pub struct Startup {
    pub tree_root: Option<PathBuf>,
    pub initial_file: Option<PathBuf>,
}

#[derive(Default)]
pub struct PendingOpens {
    pub ready: bool,
    pub files: Vec<PathBuf>,
}

pub struct AppState {
    pub tree_root: Option<PathBuf>,
    pub initial_file: Option<PathBuf>,
    pub watcher: Mutex<watcher::WatcherSlot>,
    pub tree_watcher: Mutex<watcher::TreeWatcherSlot>,
    pub opens: Mutex<PendingOpens>,
    /// Serializes task-list write-backs. Held only for the read-verify-write
    /// critical section so two rapid clicks can't interleave reads.
    pub tasklist_lock: Mutex<()>,
}

pub fn run(startup: Startup) {
    let state = AppState {
        tree_root: startup.tree_root,
        initial_file: startup.initial_file,
        watcher: Mutex::new(watcher::WatcherSlot::default()),
        tree_watcher: Mutex::new(watcher::TreeWatcherSlot::default()),
        opens: Mutex::new(PendingOpens::default()),
        tasklist_lock: Mutex::new(()),
    };

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(state)
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
            commands::save_session,
            commands::install_cli,
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
            menu::install(&handle)?;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
            }
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
