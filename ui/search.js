// Pure text-matching for the in-document find bar. No DOM or Tauri imports, so
// it runs under `node --test` as well as in the WebView.

/** True if `ch` is a word character: Unicode letter, number, or underscore. */
export function isWordChar(ch) {
  return ch != null && /[\p{L}\p{N}_]/u.test(ch);
}

function isWholeWord(text, start, end) {
  const before = start > 0 ? text[start - 1] : null;
  const after = end < text.length ? text[end] : null;
  return !isWordChar(before) && !isWordChar(after);
}

/**
 * Find every occurrence of `query` in `text`.
 *
 * @param {string} text
 * @param {string} query
 * @param {{caseSensitive?: boolean, wholeWord?: boolean}} [opts]
 * @returns {Array<[number, number]>} [start, end) offset pairs, in order,
 *   non-overlapping.
 */
export function findMatches(text, query, opts = {}) {
  const { caseSensitive = false, wholeWord = false } = opts;
  if (!query) return [];
  const hay = caseSensitive ? text : text.toLowerCase();
  const needle = caseSensitive ? query : query.toLowerCase();
  const out = [];
  let from = 0;
  let lastEnd = -1;
  for (;;) {
    const i = hay.indexOf(needle, from);
    if (i === -1) break;
    const end = i + needle.length;
    if (i >= lastEnd && (!wholeWord || isWholeWord(text, i, end))) {
      out.push([i, end]);
      lastEnd = end;
    }
    from = i + 1;
  }
  return out;
}
