// Pure helpers for the source editor. DOM-free so they unit-test under
// `node --test`; the CodeMirror wiring itself lives in app.js.

/** True when the editor buffer differs from the last loaded-or-saved content. */
export function isDirty(content, savedContent) {
  return content !== savedContent;
}

/** Decide what a `file-changed` event means for the active tab.
 *
 *  - "reload"   : adopt the disk content (not editing, or editing-but-clean).
 *  - "self"     : disk equals what we just saved — our own write; ignore.
 *  - "conflict" : disk diverged AND the editor has unsaved edits — warn, keep.
 */
export function classifyFileChange({ editing, dirty, diskContent, savedContent }) {
  if (!editing) return "reload";
  if (diskContent === savedContent) return "self";
  return dirty ? "conflict" : "reload";
}
