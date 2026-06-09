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

/** Build the clipboard review string: header + optional general note + divider,
 *  then orphaned comments (tagged), then anchored comments in document order. */
export function formatReview(reviews, generalNote, relativePath, orphaned = []) {
  const note = (generalNote || "").trim();
  const ordered = [...reviews].sort(
    (a, b) => startLine(a.sourcepos) - startLine(b.sourcepos),
  );
  const items = [
    ...orphaned.map((o) => ({ ...o, changed: true })),
    ...ordered,
  ];

  const out = [`Review of ${relativePath}`, ""];
  if (note) {
    out.push(`General note: ${note}`, "");
    if (items.length) out.push("---", "");
  }
  for (const it of items) {
    const tag = it.changed ? "  ⚠ this block changed" : "";
    out.push(`> ${quoteBlock(it.quotedText)}${tag}`, `↳ ${it.comment}`, "");
  }
  return out.join("\n").trimEnd() + "\n";
}

function startLine(sourcepos) {
  const m = /^(\d+):/.exec(sourcepos || "");
  return m ? parseInt(m[1], 10) : 0;
}

/** Re-locate each review against freshly-rendered blocks by matching its
 *  quotedText. Matched reviews get the new sourcepos; unmatched become orphaned.
 *  newBlocks: [{ sourcepos, text }] where `text` MUST equal quoteBlock(rawBlockText)
 *  — already normalized — since quotedText is stored post-normalization; a
 *  mismatch silently orphans the review. The first matching block wins, so pass
 *  newBlocks in document order. */
export function reanchorReviews(reviews, newBlocks) {
  const anchored = [];
  const orphaned = [];
  for (const r of reviews) {
    const match = newBlocks.find((b) => b.text === r.quotedText);
    if (match) {
      anchored.push({ ...r, sourcepos: match.sourcepos });
    } else {
      orphaned.push({ quotedText: r.quotedText, comment: r.comment });
    }
  }
  return { anchored, orphaned };
}
