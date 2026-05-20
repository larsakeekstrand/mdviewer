mod commands;
mod markdown;
mod menu;
pub mod open_files;
mod recent;
mod tree;
mod updates;
mod watcher;

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::Manager;

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

pub fn run(startup: Startup) {
    let state = AppState {
        tree_root: startup.tree_root,
        initial_file: startup.initial_file,
        watcher: Mutex::new(watcher::WatcherSlot::default()),
        opens: Mutex::new(PendingOpens::default()),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::get_initial_state,
            commands::list_dir,
            commands::render_file,
            commands::open_file,
            commands::read_source,
            commands::check_for_updates,
            commands::open_url,
            commands::open_path,
            commands::frontend_ready,
        ])
        .setup(|app| {
            // Pre-warm the markdown engine so the first render isn't laggy.
            std::thread::spawn(|| {
                markdown::prewarm();
            });
            let handle = app.handle().clone();
            let state = handle.state::<AppState>();
            recent::push(&handle, &state.tree_root);
            menu::install(&handle)?;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running mdviewer");
}
