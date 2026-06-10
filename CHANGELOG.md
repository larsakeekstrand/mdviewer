# Changelog

User-facing notes for each release. The release workflow extracts the section
matching the tag into the GitHub release page and the in-app "What's new" modal.

## [1.18.0] - 2026-06-10

Reviewing AI-generated documents — and always knowing which file you're on.

- **Review Mode.** Click **💬 Review** on any Markdown document to comment on it:
  hover a block for a **+**, attach a comment, and add a document-wide note. When
  you're done, click **✓ Finish & Copy** to put a structured review (the file
  path, your note, and each quoted block with its comment) on the clipboard —
  ready to paste back to an AI coding assistant like Claude Code. Comments follow
  their blocks as the document changes; ones whose text moved on are flagged.
- **Install Claude Code Hook.** **MDViewer ▸ Install Claude Code Hook…** sets up
  the open project so that plan, spec, and design Markdown files Claude Code
  writes there open automatically in MDViewer.
- **The active tab's file is revealed in the tree.** Switching tabs now expands
  the file's folders, scrolls its row into view, and highlights it with an accent
  bar — so you always know which file you're looking at.

## [1.17.1] - 2026-06-09

Security hardening.

- Hardened the "open in default app" check so a launchable file can't slip past
  it with a crafted name (a trailing dot or space, which the OS ignores when
  launching).
- Restricted document export to its actual output formats (HTML, PDF, SVG, PNG).

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
