#!/bin/sh
# PostToolUse(Edit|Write): keep Rust formatted.
#
# CI gates on `cargo fmt --check` with -D warnings, so an unformatted .rs file
# is a guaranteed merge failure. Formatting on edit removes the most common
# reason a commit bounces. Reads the tool-call JSON on stdin; runs only when a
# .rs path is involved. Always exits 0 — formatting must never block an edit.
input=$(cat)
case "$input" in
  *.rs\"*)
    dir="${CLAUDE_PROJECT_DIR:-.}/src-tauri"
    [ -f "$dir/Cargo.toml" ] && (cd "$dir" && cargo fmt >/dev/null 2>&1)
    ;;
esac
exit 0
