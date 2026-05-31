// Folder content search panel. Pure helpers (no DOM/Tauri imports) live at
// the top so they run under `node --test`; the DOM-bound entry points live
// at the bottom.

const MIN_QUERY = 2;
const TOTAL_CAP = 5000;
const LINE_TEXT_MAX = 300;

/** Pure reducer: given the panel's inputs, return the renderable shape. */
export function derivePanelState({ query, results, error, busy }) {
  const trimmed = (query ?? "").trim();
  if (!trimmed) {
    return { kind: "hint", message: "Type to search" };
  }
  if (trimmed.length < MIN_QUERY) {
    return { kind: "hint", message: `Type at least ${MIN_QUERY} characters` };
  }
  if (error) {
    return { kind: "error", message: error };
  }
  if (busy && !results) {
    return { kind: "busy" };
  }
  if (!results) {
    return { kind: "busy" };
  }
  const footer = formatFooter(results);
  if (results.matches.length === 0) {
    return { kind: "empty", footer };
  }
  return { kind: "results", groups: groupResults(results.matches), footer };
}

/** Group flat matches by `path`, preserving the walker order. */
export function groupResults(matches) {
  const order = [];
  const map = new Map();
  for (const m of matches) {
    if (!map.has(m.path)) {
      map.set(m.path, { path: m.path, matches: [] });
      order.push(m.path);
    }
    map.get(m.path).matches.push(m);
  }
  return order.map((p) => map.get(p));
}

/** Centre an excerpt of `line` on `[start, end)` so the result is ≤ `max`
 *  chars. Match offsets are byte-indexed (matching the Rust side); we treat
 *  them as char-indexed here, which is identical for ASCII and acceptably
 *  close for the lengths we render. */
export function truncateLineText(line, start, end, max = LINE_TEXT_MAX) {
  if (line.length <= max) {
    return { text: line, matchStart: start, matchEnd: end };
  }
  const matchLen = Math.max(0, end - start);
  const context = Math.max(0, Math.floor((max - matchLen) / 2));
  const winStart = Math.max(0, start - context);
  const winEnd = Math.min(line.length, end + context);
  const prefix = winStart > 0 ? "…" : "";
  const suffix = winEnd < line.length ? "…" : "";
  const text = prefix + line.slice(winStart, winEnd) + suffix;
  const matchStart = prefix.length + (start - winStart);
  const matchEnd = prefix.length + (end - winStart);
  return { text, matchStart, matchEnd };
}

function formatFooter(r) {
  const parts = [`${r.files_scanned} files searched`];
  parts.push(`${r.matches.length} matches`);
  if (r.files_skipped_binary > 0) {
    parts.push(`${r.files_skipped_binary} binary skipped`);
  }
  if (r.files_skipped_too_large > 0) {
    parts.push(`${r.files_skipped_too_large} too large`);
  }
  if (r.files_unreadable > 0) {
    parts.push(`${r.files_unreadable} unreadable`);
  }
  let base = parts.join(" · ");
  if (r.truncated) {
    base += ` · Showing first ${TOTAL_CAP} — refine your query`;
  }
  return base;
}

/* -------------------- DOM glue (browser only) -------------------- */

let _state = {
  query: "",
  results: null,
  error: null,
  busy: false,
  caseSensitive: false,
  wholeWord: false,
};
let _root = null;
let _rootRelativeTo = null;
let _seq = 0;
let _debounceTimer = null;
let _invoke = null;
let _ui = null;
let _onOpenResult = null;

const DEBOUNCE_MS = 150;

/** Wire the panel once on app start. `opts.invoke` is `window.__TAURI__.core.invoke`;
 *  `opts.openResult` is the callback that opens a tab at a given line. */
export function initSearchPanel({ invoke, openResult }) {
  _invoke = invoke;
  _onOpenResult = openResult;
  _ui = {
    sidebar: document.getElementById("sidebar"),
    panel: document.getElementById("search-panel"),
    title: document.getElementById("search-panel-title"),
    back: document.getElementById("search-back"),
    input: document.getElementById("search-input"),
    caseBtn: document.getElementById("search-case"),
    wordBtn: document.getElementById("search-word"),
    results: document.getElementById("search-results"),
    footer: document.getElementById("search-footer"),
  };
  _ui.back.addEventListener("click", exitSearchMode);
  _ui.input.addEventListener("input", () => {
    _state.query = _ui.input.value;
    scheduleSearch();
    render();
  });
  _ui.input.addEventListener("keydown", (ev) => {
    if (ev.key === "Escape") {
      ev.preventDefault();
      exitSearchMode();
    }
  });
  _ui.caseBtn.addEventListener("click", () => {
    _state.caseSensitive = !_state.caseSensitive;
    _ui.caseBtn.setAttribute("aria-pressed", String(_state.caseSensitive));
    scheduleSearch();
  });
  _ui.wordBtn.addEventListener("click", () => {
    _state.wholeWord = !_state.wholeWord;
    _ui.wordBtn.setAttribute("aria-pressed", String(_state.wholeWord));
    scheduleSearch();
  });
}

export function enterSearchMode(folderPath, { treeRoot } = {}) {
  _root = folderPath;
  _rootRelativeTo = treeRoot || folderPath;
  _state = {
    query: "",
    results: null,
    error: null,
    busy: false,
    caseSensitive: false,
    wholeWord: false,
  };
  _ui.input.value = "";
  _ui.caseBtn.setAttribute("aria-pressed", "false");
  _ui.wordBtn.setAttribute("aria-pressed", "false");
  _ui.title.textContent = relPath(folderPath, _rootRelativeTo) || folderPath;
  _ui.panel.hidden = false;
  _ui.sidebar.classList.add("searching");
  _ui.input.focus();
  render();
}

export function exitSearchMode() {
  _ui.sidebar.classList.remove("searching");
  _ui.panel.hidden = true;
  _root = null;
  _state.results = null;
  _state.error = null;
  _state.busy = false;
  if (_debounceTimer) {
    clearTimeout(_debounceTimer);
    _debounceTimer = null;
  }
  _seq++;
}

export function isSearchModeOpen() {
  return _ui && !_ui.panel.hidden;
}

function scheduleSearch() {
  if (_debounceTimer) clearTimeout(_debounceTimer);
  _debounceTimer = setTimeout(runSearch, DEBOUNCE_MS);
}

async function runSearch() {
  _debounceTimer = null;
  const query = _state.query.trim();
  if (query.length < MIN_QUERY) {
    _state.results = null;
    _state.error = null;
    _state.busy = false;
    render();
    return;
  }
  const seq = ++_seq;
  _state.busy = true;
  _state.error = null;
  render();
  try {
    const results = await _invoke("search_in_folder", {
      root: _root,
      query,
      caseSensitive: _state.caseSensitive,
      wholeWord: _state.wholeWord,
    });
    if (seq !== _seq) return;
    _state.results = results;
    _state.busy = false;
    render();
  } catch (e) {
    if (seq !== _seq) return;
    _state.error = String(e);
    _state.busy = false;
    _state.results = null;
    render();
  }
}

function render() {
  const view = derivePanelState({
    query: _state.query,
    results: _state.results,
    error: _state.error,
    busy: _state.busy,
  });
  _ui.results.replaceChildren();
  _ui.footer.textContent = view.footer || "";
  if (view.kind === "hint") {
    const el = document.createElement("div");
    el.className = "search-hint";
    el.textContent = view.message;
    _ui.results.appendChild(el);
    return;
  }
  if (view.kind === "error") {
    const el = document.createElement("div");
    el.className = "search-error";
    el.textContent = view.message;
    _ui.results.appendChild(el);
    return;
  }
  if (view.kind === "busy") {
    const el = document.createElement("div");
    el.className = "search-busy";
    el.textContent = "Searching…";
    _ui.results.appendChild(el);
    return;
  }
  if (view.kind === "empty") {
    const el = document.createElement("div");
    el.className = "search-empty";
    el.textContent = "No matches";
    _ui.results.appendChild(el);
    return;
  }
  for (const group of view.groups) {
    _ui.results.appendChild(renderGroup(group));
  }
}

function renderGroup(group) {
  const det = document.createElement("details");
  det.open = true;
  const sum = document.createElement("summary");
  sum.textContent = relPath(group.path, _rootRelativeTo);
  const cnt = document.createElement("span");
  cnt.className = "search-file-count";
  cnt.textContent = `(${group.matches.length})`;
  sum.appendChild(cnt);
  det.appendChild(sum);
  for (const m of group.matches) {
    det.appendChild(renderMatch(m));
  }
  return det;
}

function renderMatch(m) {
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "search-match";
  const { text, matchStart, matchEnd } = truncateLineText(
    m.line_text,
    m.match_start,
    m.match_end,
  );
  const lineLabel = document.createElement("span");
  lineLabel.className = "search-match-line";
  lineLabel.textContent = `${m.line}:`;
  btn.appendChild(lineLabel);
  btn.appendChild(document.createTextNode(text.slice(0, matchStart)));
  const mark = document.createElement("mark");
  mark.textContent = text.slice(matchStart, matchEnd);
  btn.appendChild(mark);
  btn.appendChild(document.createTextNode(text.slice(matchEnd)));
  btn.addEventListener("click", () => {
    if (_onOpenResult) _onOpenResult(m.path, m.line);
  });
  return btn;
}

function relPath(absolute, root) {
  if (!root) return absolute;
  if (absolute === root) return "";
  const sep = root.endsWith("/") || root.endsWith("\\") ? "" : "/";
  const prefix = root + sep;
  if (absolute.startsWith(prefix)) return absolute.slice(prefix.length);
  return absolute;
}
