// Pure extension -> CodeMirror 5 mode-name mapping (no DOM), unit-testable.

const EXT_MODE = {
  md: "markdown", markdown: "markdown", mdown: "markdown", mkd: "markdown", mkdn: "markdown",
  js: "javascript", jsx: "javascript", mjs: "javascript", cjs: "javascript",
  ts: "javascript", tsx: "javascript", json: "javascript",
  py: "python",
  rs: "rust",
  c: "clike", h: "clike", cpp: "clike", hpp: "clike", cc: "clike", cxx: "clike",
  hxx: "clike", java: "clike", cs: "clike",
  css: "css", scss: "css", less: "css",
  html: "htmlmixed", htm: "htmlmixed", xhtml: "htmlmixed",
  xml: "xml", svg: "xml",
  sh: "shell", bash: "shell", zsh: "shell",
  yml: "yaml", yaml: "yaml",
  go: "go",
  sql: "sql",
};

// Returns the CodeMirror mode name for a file path, or null (plain text).
export function modeForPath(path) {
  const m = /\.([^.\/\\]+)$/.exec(path || "");
  if (!m) return null;
  return EXT_MODE[m[1].toLowerCase()] || null;
}
