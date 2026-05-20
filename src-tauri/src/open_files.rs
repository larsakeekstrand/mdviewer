pub fn markdown_path(url: &tauri::Url) -> Option<std::path::PathBuf> {
    if url.scheme() != "file" {
        return None;
    }
    let path = url.to_file_path().ok()?;
    crate::markdown::is_markdown_path(&path).then_some(path)
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
