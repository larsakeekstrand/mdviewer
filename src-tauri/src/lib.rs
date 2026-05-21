mod commands;
mod markdown;
mod menu;
mod open_files;
mod recent;
mod tree;
mod updates;
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
    pub opens: Mutex<PendingOpens>,
}

pub fn run(startup: Startup) {
    let state = AppState {
        tree_root: startup.tree_root,
        initial_file: startup.initial_file,
        watcher: Mutex::new(watcher::WatcherSlot::default()),
        opens: Mutex::new(PendingOpens::default()),
    };

    let app = tauri::Builder::default()
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
            commands::remember_folder,
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
