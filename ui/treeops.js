// Pure helpers for tree file operations. Mirrors fs_ops.rs::validate_name so the
// inline-rename input gives immediate feedback; the backend re-validates.

/** Ancestor directory paths to expand to reveal `filePath`, top-down and
 *  strictly between `root` and the file. Returns `[]` when the file sits
 *  directly in `root`, or `null` when `filePath` is not under `root` (or equals
 *  it). DOM-free; handles `/` and `\` separators and a trailing root separator. */
export function treeAncestors(root, filePath) {
  if (!root || !filePath) return null;
  const sep = filePath.includes("\\") ? "\\" : "/";
  const r = root.endsWith(sep) ? root.slice(0, -sep.length) : root;
  if (filePath === r) return null;
  const prefix = r + sep;
  if (!filePath.startsWith(prefix)) return null;
  const segs = filePath.slice(prefix.length).split(sep).filter((s) => s.length > 0);
  const ancestors = [];
  let cur = r;
  for (let i = 0; i < segs.length - 1; i++) {
    cur = cur + sep + segs[i];
    ancestors.push(cur);
  }
  return ancestors;
}

/** Returns null when valid, or an error message string when not. */
export function validateName(name) {
  const trimmed = (name || "").trim();
  if (trimmed === "") return "Name cannot be empty";
  if (trimmed === "." || trimmed === "..") return "Invalid name";
  if (trimmed.includes("/") || trimmed.includes("\\")) {
    return "Name cannot contain path separators";
  }
  // Reject ASCII control characters (mirrors Rust char::is_control for the
  // common cases relevant to filenames).
  for (let i = 0; i < trimmed.length; i++) {
    const c = trimmed.charCodeAt(i);
    if (c < 0x20 || c === 0x7f) return "Name cannot contain control characters";
  }
  return null;
}
