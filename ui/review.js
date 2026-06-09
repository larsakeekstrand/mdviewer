// Pure helpers for Review Mode. DOM-free + Tauri-free so they unit-test under
// `node --test`; the annotation UI and clipboard wiring live in app.js.

/** Collapse a block's text to one trimmed line, truncating long text with " …".
 *  Used both for the clipboard blockquote and (via the same normalization) as
 *  the stable key for re-anchoring across re-renders. */
export function quoteBlock(sourceText, max = 80) {
  const s = (sourceText || "").trim().replace(/\s+/g, " ");
  if (s.length <= max) return s;
  return s.slice(0, max).trimEnd() + " …";
}
