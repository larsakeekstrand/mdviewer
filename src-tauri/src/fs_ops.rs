/// Validate a user-entered file or folder name (from the inline-rename input).
/// Rejects empties, path separators, and the `.`/`..` traversal names. The
/// frontend validates too (immediate feedback); this is the authoritative
/// backend guard.
pub fn validate_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if trimmed == "." || trimmed == ".." {
        return Err("invalid name".to_string());
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err("name cannot contain path separators".to_string());
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err("name cannot contain control characters".to_string());
    }
    Ok(())
}

/// Split a file name into (stem, extension). A leading-dot name with no other
/// dot (".gitignore") is all stem, no extension. "a.tar.gz" -> ("a.tar", "gz").
fn split_ext(name: &str) -> (&str, Option<&str>) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], Some(&name[i + 1..])),
        _ => (name, None),
    }
}

/// The nth duplicate candidate name: n=1 -> "note copy.md", n=2 -> "note copy 2.md".
pub fn duplicate_candidate(name: &str, n: usize) -> String {
    let (stem, ext) = split_ext(name);
    let suffix = if n <= 1 {
        " copy".to_string()
    } else {
        format!(" copy {n}")
    };
    match ext {
        Some(e) => format!("{stem}{suffix}.{e}"),
        None => format!("{stem}{suffix}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_name_accepts_ordinary_names() {
        assert!(validate_name("notes.md").is_ok());
        assert!(validate_name(".gitignore").is_ok());
        assert!(validate_name("My File 2.txt").is_ok());
    }

    #[test]
    fn validate_name_rejects_bad_names() {
        assert!(validate_name("").is_err());
        assert!(validate_name("   ").is_err());
        assert!(validate_name(".").is_err());
        assert!(validate_name("..").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("a\\b").is_err());
        assert!(validate_name("a\nb").is_err());
    }

    #[test]
    fn duplicate_candidate_handles_extensions_and_dotfiles() {
        assert_eq!(duplicate_candidate("note.md", 1), "note copy.md");
        assert_eq!(duplicate_candidate("note.md", 2), "note copy 2.md");
        assert_eq!(
            duplicate_candidate("archive.tar.gz", 1),
            "archive.tar copy.gz"
        );
        assert_eq!(duplicate_candidate("README", 1), "README copy");
        assert_eq!(duplicate_candidate(".gitignore", 1), ".gitignore copy");
    }
}
