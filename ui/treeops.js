// Pure helpers for tree file operations. Mirrors fs_ops.rs::validate_name so the
// inline-rename input gives immediate feedback; the backend re-validates.

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
