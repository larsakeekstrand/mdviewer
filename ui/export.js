// Pure helpers for document export. No DOM or Tauri imports, so this runs under
// `node --test` as well as in the WebView (mirrors search.js).

/** Whether absolute path `path` is `dir` itself or nested inside it. Both are
 *  normalized (`.`/`..`/empty segments collapsed) so a `..` escape can't slip
 *  through, and the boundary is matched on a path separator so `/work` does not
 *  count `/work-secret` as inside. Used to gate which local files export may
 *  inline — embedding a file outside the opened workspace would leak it. */
export function isPathInsideDir(path, dir) {
  if (!path || !dir) return false;
  const norm = (s) => {
    const out = [];
    for (const seg of String(s).split("/")) {
      if (seg === "" || seg === ".") continue;
      if (seg === "..") out.pop();
      else out.push(seg);
    }
    return "/" + out.join("/");
  };
  const p = norm(path);
  const d = norm(dir);
  return p === d || p.startsWith(d === "/" ? "/" : d + "/");
}

/** Final path segment of a Unix or Windows path. Falls back to the whole
 *  input when the path ends in a separator (no usable basename). */
export function baseName(path) {
  const parts = String(path).split(/[\\/]/);
  const last = parts[parts.length - 1];
  return last || String(path);
}

/** `srcPath`'s basename with its extension replaced by `ext` (or `ext`
 *  appended when there is no extension). A leading-dot name (".env") is treated
 *  as having no extension. */
export function exportFilename(srcPath, ext) {
  const name = baseName(srcPath);
  const dot = name.lastIndexOf(".");
  const stem = dot > 0 ? name.slice(0, dot) : name;
  return `${stem}.${ext}`;
}

/** True if the rendered HTML contains KaTeX output, meaning the export must
 *  embed the KaTeX stylesheet + fonts. KaTeX wraps every formula in an element
 *  with class "katex" (display math adds a "katex-display" wrapper). */
export function documentNeedsKatex(html) {
  return String(html).includes('class="katex');
}

/** Replace `url(<ref>)` occurrences with `url(<dataUrl>)` for each entry in
 *  `fontMap` (keyed by the exact ref text as it appears in the CSS). Plain
 *  string replacement avoids regex-escaping the path. */
export function inlineFontUrls(cssText, fontMap) {
  let out = cssText;
  for (const [ref, dataUrl] of Object.entries(fontMap)) {
    out = out.split(`url(${ref})`).join(`url(${dataUrl})`);
  }
  return out;
}

/** Advance past a quoted string starting at `i` (the opening quote). Returns
 *  the index of the closing quote (or end of input). Handles backslash escapes. */
function skipString(s, i) {
  const quote = s[i];
  i++;
  while (i < s.length) {
    if (s[i] === "\\") {
      i += 2;
      continue;
    }
    if (s[i] === quote) return i;
    i++;
  }
  return i;
}

/** Advance past a CSS comment starting at `i`. Returns the index of the closing
 *  slash, or one past the end of input if the comment is unterminated. */
function skipComment(s, i) {
  i += 2;
  while (i < s.length && !(s[i] === "*" && s[i + 1] === "/")) i++;
  return i + 1;
}

/** Index of the `}` matching the `{` at `openBraceIdx`, ignoring braces that
 *  appear inside CSS strings or comments. -1 if unbalanced. */
function matchingBraceEnd(s, openBraceIdx) {
  let depth = 0;
  for (let i = openBraceIdx; i < s.length; i++) {
    const c = s[i];
    if (c === '"' || c === "'") {
      i = skipString(s, i);
      continue;
    }
    if (c === "/" && s[i + 1] === "*") {
      i = skipComment(s, i);
      continue;
    }
    if (c === "{") depth++;
    else if (c === "}") {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
}

/** For each `@media (...) {` matched by `headerRe` (which must be global and end
 *  at the `{`), either drop the whole block (keepInner=false) or splice in its
 *  inner rules without the wrapper (keepInner=true). */
function transformMediaBlocks(css, headerRe, keepInner) {
  let result = "";
  let pos = 0;
  for (const m of css.matchAll(headerRe)) {
    const headerStart = m.index;
    if (headerStart < pos) continue; // already inside a consumed block
    const braceIdx = headerStart + m[0].length - 1;
    const end = matchingBraceEnd(css, braceIdx);
    if (end === -1) break;
    result += css.slice(pos, headerStart);
    if (keepInner) result += css.slice(braceIdx + 1, end);
    pos = end + 1;
  }
  result += css.slice(pos);
  return result;
}

/** Force a prefers-color-scheme stylesheet to its light variant: remove dark
 *  media blocks entirely, unwrap light media blocks so their rules always
 *  apply. Only the simple `@media (prefers-color-scheme: …)` form is matched;
 *  compound queries (`@media screen and (…)`) are left unchanged. */
export function forceLightCss(cssText) {
  const dark = /@media\s*\(\s*prefers-color-scheme\s*:\s*dark\s*\)\s*\{/gi;
  const light = /@media\s*\(\s*prefers-color-scheme\s*:\s*light\s*\)\s*\{/gi;
  let out = transformMediaBlocks(cssText, dark, false);
  out = transformMediaBlocks(out, light, true);
  return out;
}

function escapeHtml(s) {
  return String(s).replace(
    /[&<>]/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c],
  );
}

/** Assemble a complete, standalone HTML document. `css` is the already-prepared
 *  stylesheet text (light-forced, fonts inlined); `bodyHtml` is the serialized
 *  rendered content. The content is wrapped in an `article.markdown-body` so the
 *  GitHub stylesheet applies and the page CSS can center it. */
export function buildHtmlDocument({ title, css, bodyHtml }) {
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="color-scheme" content="light">
<title>${escapeHtml(title)}</title>
<style>${css}</style>
</head>
<body>
<article class="markdown-body">
${bodyHtml}
</article>
</body>
</html>
`;
}
