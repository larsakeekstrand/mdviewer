pub fn markdown_path(url: &tauri::Url) -> Option<std::path::PathBuf> {
    if url.scheme() != "file" {
        return None;
    }
    let path = url.to_file_path().ok()?;
    crate::markdown::is_markdown_path(&path).then_some(path)
}

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
    fn accepts_uppercase_extension() {
        let u = Url::parse("file:///tmp/notes.MD").unwrap();
        assert_eq!(markdown_path(&u), Some(PathBuf::from("/tmp/notes.MD")));
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
