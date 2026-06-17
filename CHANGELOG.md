# Changelog

User-facing notes for each release. The release workflow extracts the section
matching the tag into the GitHub release page and the in-app "What's new" modal.

## [1.19.1] - 2026-06-17

Copy and paste now work everywhere.

- **Fixed copy and paste.** Cut, Copy, Paste, and Select All now work via the
  keyboard (⌘X/⌘C/⌘V/⌘A) and the menu across the rendered preview, raw view,
  and the source editor — including pasting into the editor and copying out to
  other apps. Right-clicking in the editor or any text field shows a compact
  Cut / Copy / Paste / Select All menu instead of macOS's full text menu.

## [1.19.0] - 2026-06-14

Claude Code integration, and much better PDFs.

- **MCP server for Claude Code.** Install with **MDViewer ▸ Install MCP
  Server…**. Claude Code can then open documents in the viewer, see what you're
  currently reading, and ask you for an in-app review — you comment on blocks
  and click **✓ Finish & Send** to deliver the review straight back to the
  waiting Claude session, no clipboard step. **Decline** (or closing the tab)
  tells Claude you're skipping it.
- **Generate PDFs from Claude Code.** The MCP server can also render a Markdown
  file in your project to a PDF (with all the fidelity below), written inside
  the open folder.
- **Claude Code Integration panel.** **MDViewer ▸ Claude Code Integration…**
  shows, for the current project, whether the auto-open hook and the MCP server
  are installed, each with a one-click Install/Update button. A first-run banner
  points you to it when neither is set up yet.
- **PDF export now matches what you see.** Backgrounds and colors (code blocks,
  table striping, blockquotes, Mermaid diagram fills) print as they appear on
  screen instead of dropping out, and Mermaid diagram labels render correctly.
- **Smarter page breaks.** Headings stay attached to the content that follows
  them, code blocks / tables / images / diagrams / math are never split across a
  page, paragraphs don't strand a single line, and a table too wide for the page
  is scaled down to fit whole rather than clipping off the right edge.
- HTML export now embeds Mermaid diagrams in a print-safe form too.

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
