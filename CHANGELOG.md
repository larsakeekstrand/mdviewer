# Changelog

User-facing notes for each release. The release workflow extracts the section
matching the tag into the GitHub release page and the in-app "What's new" modal.

## [1.17.0] - 2026-06-02

MDViewer is now an editor, not just a viewer.

- **Edit files in the app.** Click **Edit** on any text or Markdown tab to open a
  side-by-side source editor with a live preview that updates as you type. Save
  with ⌘S; tabs with unsaved changes show a dot. If the file changes on disk
  while you have unsaved edits, a banner lets you reload from disk or keep your
  version. The editor matches the light/dark theme.
- **Create, rename, duplicate, and delete from the file tree.** Right-click a
  file, a folder, or empty sidebar space for **New File…**, **New Folder…**,
  **Rename…**, **Duplicate**, and **Delete**. Renaming happens inline (Enter to
  confirm, Esc to cancel), open tabs follow a rename, and deletes go to the
  system Trash so they're recoverable.

## [1.16.0] - 2026-06-02

- Hardened HTML/PDF export so it never inlines files from outside the workspace.
- Pinned CI's third-party GitHub Actions to exact commits for supply-chain safety.
