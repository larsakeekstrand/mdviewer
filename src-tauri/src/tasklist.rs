//! Pure transform for toggling a GFM task-list checkbox on a specific line.

pub fn toggle_checkbox_at_line(
    content: &str,
    line: usize,
    new_state: bool,
) -> Result<String, ToggleError> {
    if line == 0 {
        return Err(ToggleError::LineOutOfRange);
    }
    // split_inclusive keeps the trailing `\n` (and any `\r` before it) on
    // each segment, so rejoining preserves line endings byte-for-byte.
    let segments: Vec<&str> = content.split_inclusive('\n').collect();
    if line > segments.len() {
        return Err(ToggleError::LineOutOfRange);
    }
    let target = segments[line - 1];

    let (body, eol) = split_eol(target);
    let (prefix_len, current) = parse_task_marker(body).ok_or(ToggleError::NotATaskListLine)?;
    if current == new_state {
        return Err(ToggleError::AlreadyInRequestedState);
    }

    let mut new_line = String::with_capacity(body.len() + eol.len());
    new_line.push_str(&body[..prefix_len]);
    new_line.push('[');
    new_line.push(if new_state { 'x' } else { ' ' });
    new_line.push(']');
    new_line.push_str(&body[prefix_len + 3..]);
    new_line.push_str(eol);

    let mut out = String::with_capacity(content.len());
    for (i, seg) in segments.iter().enumerate() {
        if i + 1 == line {
            out.push_str(&new_line);
        } else {
            out.push_str(seg);
        }
    }
    Ok(out)
}

/// Split a line segment into (body, trailing-newline-bytes). Handles both LF
/// and CRLF; returns an empty `eol` for the final line if there's no trailing
/// newline.
fn split_eol(s: &str) -> (&str, &str) {
    if let Some(stripped) = s.strip_suffix("\r\n") {
        (stripped, "\r\n")
    } else if let Some(stripped) = s.strip_suffix('\n') {
        (stripped, "\n")
    } else {
        (s, "")
    }
}

/// If `body` is a task-list line, return (byte offset of `[`, current state).
/// Recognizes `- `, `* `, `+ ` markers (with optional leading whitespace) and
/// `[ ]` / `[x]` / `[X]` checkboxes. Tabs in the leading indentation count.
fn parse_task_marker(body: &str) -> Option<(usize, bool)> {
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    if !matches!(bytes[i], b'-' | b'*' | b'+') {
        return None;
    }
    i += 1;
    let space_start = i;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i == space_start {
        return None;
    }
    if i + 2 >= bytes.len() {
        return None;
    }
    if bytes[i] != b'[' || bytes[i + 2] != b']' {
        return None;
    }
    let state = match bytes[i + 1] {
        b' ' => false,
        b'x' | b'X' => true,
        _ => return None,
    };
    Some((i, state))
}

#[derive(Debug, PartialEq, Eq)]
pub enum ToggleError {
    /// `line` is 0 or past the end of the file.
    LineOutOfRange,
    /// The target line doesn't contain a recognizable `[ ]` / `[x]` marker.
    NotATaskListLine,
    /// The current marker state already matches the requested state.
    /// Treated as a soft error so the frontend can ignore it silently
    /// (covers double-click and stale watcher-driven renders).
    AlreadyInRequestedState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles_unchecked_to_checked() {
        let src = "- [ ] task\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, true).unwrap(),
            "- [x] task\n"
        );
    }

    #[test]
    fn toggles_checked_to_unchecked() {
        let src = "- [x] task\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, false).unwrap(),
            "- [ ] task\n"
        );
    }

    #[test]
    fn toggles_uppercase_x_to_unchecked() {
        let src = "- [X] task\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, false).unwrap(),
            "- [ ] task\n"
        );
    }

    #[test]
    fn accepts_asterisk_marker() {
        let src = "* [ ] task\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, true).unwrap(),
            "* [x] task\n"
        );
    }

    #[test]
    fn accepts_plus_marker() {
        let src = "+ [ ] task\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, true).unwrap(),
            "+ [x] task\n"
        );
    }

    #[test]
    fn preserves_leading_indentation() {
        let src = "  - [ ] nested\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, true).unwrap(),
            "  - [x] nested\n"
        );
    }

    #[test]
    fn preserves_tabs_in_indentation() {
        let src = "\t- [ ] tabbed\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, true).unwrap(),
            "\t- [x] tabbed\n"
        );
    }

    #[test]
    fn preserves_crlf_line_endings() {
        let src = "line one\r\n- [ ] task\r\nline three\r\n";
        let out = toggle_checkbox_at_line(src, 2, true).unwrap();
        assert_eq!(out, "line one\r\n- [x] task\r\nline three\r\n");
    }

    #[test]
    fn preserves_surrounding_lines_byte_for_byte() {
        let src = "# heading\n\n- [ ] one\n- [x] two\n\nparagraph\n";
        let out = toggle_checkbox_at_line(src, 3, true).unwrap();
        assert_eq!(out, "# heading\n\n- [x] one\n- [x] two\n\nparagraph\n");
    }

    #[test]
    fn toggles_only_the_target_line_when_multiple_match() {
        let src = "- [ ] a\n- [ ] b\n- [ ] c\n";
        let out = toggle_checkbox_at_line(src, 2, true).unwrap();
        assert_eq!(out, "- [ ] a\n- [x] b\n- [ ] c\n");
    }

    #[test]
    fn already_in_state_is_a_soft_error() {
        let src = "- [x] already done\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, true),
            Err(ToggleError::AlreadyInRequestedState)
        );
    }

    #[test]
    fn line_zero_is_out_of_range() {
        let src = "- [ ] task\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 0, true),
            Err(ToggleError::LineOutOfRange)
        );
    }

    #[test]
    fn line_past_end_is_out_of_range() {
        let src = "- [ ] task\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 99, true),
            Err(ToggleError::LineOutOfRange)
        );
    }

    #[test]
    fn non_task_line_is_rejected() {
        let src = "just a paragraph\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, true),
            Err(ToggleError::NotATaskListLine)
        );
    }

    #[test]
    fn line_with_marker_but_no_brackets_is_rejected() {
        let src = "- regular bullet\n";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, true),
            Err(ToggleError::NotATaskListLine)
        );
    }

    #[test]
    fn file_without_trailing_newline_works() {
        let src = "- [ ] only line";
        assert_eq!(
            toggle_checkbox_at_line(src, 1, true).unwrap(),
            "- [x] only line"
        );
    }
}
