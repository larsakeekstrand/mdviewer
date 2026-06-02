import { findMatches } from "./search.js";
import {
  initSearchPanel,
  enterSearchMode,
  exitSearchMode,
  isSearchModeOpen,
} from "./folder_search.js";
import {
  exportFilename,
  baseName,
  documentNeedsKatex,
  inlineFontUrls,
  forceLightCss,
  buildHtmlDocument,
  isPathInsideDir,
} from "./export.js";
import {
  releaseUrlFor,
  bannerMessage,
  progressText,
  extractChangelog,
} from "./update.js";
import {
  THEME_KEY,
  isValidTheme,
  resolveTheme,
  nextTheme,
  themeButtonFace,
} from "./theme.js";
import { isImagePath } from "./filetype.js";
import { classifyFileChange, isDirty } from "./editor.js";
import { validateName } from "./treeops.js";

// mdviewer frontend
// Uses Tauri v2 IPC; window.__TAURI__ is injected because tauri.conf.json sets withGlobalTauri.

const { invoke, convertFileSrc } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const dialogApi = window.__TAURI__.dialog;

let IS_MAC = false;

async function detectPlatform() {
  try {
    const os = await invoke("platform");
    IS_MAC = os === "macos";
  } catch (e) {
    // Fallback to the navigator heuristic if the command is ever unavailable
    // (e.g., during HMR with a stale frontend). Don't fail init over this.
    IS_MAC = navigator.platform.toLowerCase().includes("mac");
  }
}

const MD_EXT = /\.(md|markdown|mdown|mkd|mkdn)$/i;
const DOUBLE_CLICK_MS = 280;

const tree = document.getElementById("tree");
const treeTitle = document.getElementById("tree-title");
const preview = document.getElementById("preview");
const previewEmpty = document.getElementById("preview-empty");
const previewScroll = document.getElementById("preview-scroll");
const tabBar = document.getElementById("tab-bar");
const tabsEl = document.getElementById("tabs");
const rawBtn = document.getElementById("toggle-raw");
const themeBtn = document.getElementById("toggle-theme");
const splitter = document.getElementById("splitter");
const editBtn = document.getElementById("toggle-edit");
const saveBtn = document.getElementById("save-file");
const editorPane = document.getElementById("editor-pane");
const editorSplitter = document.getElementById("editor-splitter");
const paneBody = document.getElementById("pane-body");

let cm = null; // the single CodeMirror instance (created lazily, reused)
let previewDebounce = null;
const EDITOR_PREVIEW_DEBOUNCE_MS = 150;

let treeRoot = null;
// path → reload counter; bumped on file-changed so the asset: URL cache-busts.
const imageVersions = new Map();
let currentTheme = resolveTheme(localStorage.getItem(THEME_KEY), colorScheme());
// Set as early as the CSP allows (no inline <head> script) to minimize the
// first-paint flash before the rest of the module runs.
document.documentElement.dataset.theme = currentTheme;
const childCache = new Map();

/* ---- Git decoration state ---- */

// Plain object map: absolute path → 2-char porcelain code. Empty when the
// current folder isn't inside a git working tree.
let gitEntries = Object.create(null);
let gitRepoRoot = null;
let gitRefreshTimer = null;
const GIT_REFRESH_DEBOUNCE_MS = 200;

// Tabs model
const tabs = []; // [{ path, sticky, raw, editing, dirty, savedContent }]
let activeIdx = -1;
let restoring = true; // suppress session persistence until init() finishes restoring

function activeTab() {
  return activeIdx >= 0 && activeIdx < tabs.length ? tabs[activeIdx] : null;
}

function findTab(path) {
  return tabs.findIndex((t) => t.path === path);
}

function colorScheme() {
  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

function mermaidTheme(theme) {
  return theme === "dark" ? "dark" : "default";
}

function initMermaid() {
  if (!window.mermaid) return;
  window.mermaid.initialize({
    startOnLoad: false,
    securityLevel: "strict",
    theme: mermaidTheme(currentTheme),
  });
}

function mermaidSource(el) {
  return (el.textContent || "").trim();
}

/** Insert via a parsed node rather than a raw HTML string so nothing in the
 *  SVG can execute even if it slipped past mermaid's strict sanitization. */
function setSvg(el, svg) {
  const node = new DOMParser().parseFromString(svg, "text/html").body
    .firstElementChild;
  if (!node) throw new Error("mermaid produced no SVG element");
  el.replaceChildren(node);
}

let mermaidIdSeq = 0;

async function renderMermaid({ force = false } = {}) {
  if (!window.mermaid) return;
  for (const el of preview.querySelectorAll("pre.mermaid")) {
    // Already-rendered/errored blocks that morphdom preserved keep their
    // state; only (re)render fresh, changed, or force-reset ones.
    if (!force && el.dataset.mvState) continue;
    const src = mermaidSource(el);
    const id = "mmd-" + mermaidIdSeq++;
    try {
      const { svg } = await window.mermaid.render(id, src);
      setSvg(el, svg);
      el.dataset.mvState = "ok";
      el.dataset.mermaidSrc = src;
    } catch (e) {
      const orphan =
        document.getElementById("d" + id) || document.getElementById(id);
      if (orphan) orphan.remove();
      el.replaceChildren(buildMermaidError(src, e));
      el.dataset.mvState = "err";
      el.dataset.mermaidSrc = src;
    }
  }
}

function buildMermaidError(src, err) {
  const wrap = document.createElement("div");
  wrap.className = "mermaid-error";
  const msg = document.createElement("div");
  msg.className = "mermaid-error-msg";
  msg.textContent =
    "Mermaid diagram error: " + (err && err.message ? err.message : String(err));
  const pre = document.createElement("pre");
  pre.className = "mermaid-error-src";
  const code = document.createElement("code");
  code.textContent = src;
  pre.appendChild(code);
  wrap.appendChild(msg);
  wrap.appendChild(pre);
  return wrap;
}

function basename(path) {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i === -1 ? path : path.slice(i + 1);
}

function parentDir(path) {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  if (i < 0) return path;
  if (i === 0) return path.slice(0, 1); // "/foo" → "/"
  return path.slice(0, i);
}

function cssEscape(s) {
  return s.replace(/(["\\])/g, "\\$1");
}

/** Returns a click handler that distinguishes single from double clicks. */
function singleOrDouble(onSingle, onDouble, delay = DOUBLE_CLICK_MS) {
  let timer = null;
  return () => {
    if (timer != null) {
      clearTimeout(timer);
      timer = null;
      onDouble();
      return;
    }
    timer = setTimeout(() => {
      timer = null;
      onSingle();
    }, delay);
  };
}

async function init() {
  await detectPlatform();
  initMermaid();
  initSearchPanel({
    invoke,
    openResult: (path, line) => openTabAtLine(path, line),
  });
  const initial = await invoke("get_initial_state");

  // Register listeners before the readiness handshake so a file opened the
  // instant the app becomes ready isn't missed.
  await listen("file-changed", async (ev) => {
    const tab = activeTab();
    if (tab && ev.payload === tab.path) {
      if (tab.editing) {
        await onEditingFileChanged(tab);
      } else {
        if (isImagePath(tab.path)) {
          imageVersions.set(tab.path, (imageVersions.get(tab.path) || 0) + 1);
        }
        await renderActive({ scrollLock: true });
      }
    }
    scheduleGitRefresh();
  });

  await listen("tree-changed", async () => {
    await refreshTree();
    scheduleGitRefresh();
  });

  await listen("open-file", async (ev) => {
    await openSticky(ev.payload);
  });

  await listen("open-folder", async (ev) => {
    await openExternalFolder(ev.payload);
  });

  await listen("edit-action", async (ev) => {
    await runEditAction(ev.payload);
  });

  await listen("export", async (ev) => {
    await onExport(ev.payload);
  });

  await listen("menu-check-updates", async () => {
    await checkForUpdates({ silent: false });
  });

  // The Settings window changed the update channel: clear any banner left from
  // the old channel and re-check on the new one immediately (no relaunch).
  await listen("channel-changed", async () => {
    if (updateInProgress) return;
    updateBanner.hidden = true;
    await checkForUpdates({ silent: true });
  });

  await listen("menu-install-cli", async () => {
    if (!IS_MAC) return;
    await installCli();
  });

  window
    .matchMedia("(prefers-color-scheme: dark)")
    .addEventListener("change", async () => {
      // Auto-follow the OS only until the user has made an explicit choice.
      if (hasThemePref()) return;
      await applyTheme(colorScheme());
    });

  rawBtn.addEventListener("click", onToggleRaw);
  editBtn.addEventListener("click", onToggleEdit);
  saveBtn.addEventListener("click", () => saveActive());
  themeBtn.addEventListener("click", onToggleTheme);
  updateThemeButton();

  // Drain files Finder buffered during a cold launch; afterwards, files opened
  // while running arrive live via the "open-file" listener above.
  let pending = [];
  try {
    pending = await invoke("frontend_ready");
  } catch (e) {
    console.error("frontend_ready failed", e);
  }

  // A cold Finder launch (no argv file) starts the sidebar at the file's folder.
  const coldFinder = !initial.initial_file && pending.length > 0;
  treeRoot = coldFinder ? parentDir(pending[0]) : initial.tree_root;
  treeTitle.textContent = basename(treeRoot) || treeRoot;
  treeTitle.title = treeRoot;
  // Explicit-arg and restored roots are already persisted by get_initial_state;
  // only the cold-Finder folder needs persisting here. The bare cwd default is
  // intentionally not persisted.
  if (coldFinder) rememberFolder(treeRoot);

  await renderRoot();
  refreshGitStatus();

  const plainLaunch = !initial.initial_file && pending.length === 0;
  if (plainLaunch) {
    await restoreSession(initial.restore_tabs, initial.active_tab);
  } else {
    if (initial.initial_file) await openSticky(initial.initial_file);
    for (const p of pending) await openSticky(p);
  }

  restoring = false;
  persistSession();
}

/* ---- Git decoration ---- */

/** Map a porcelain XY code to { label, cls } or null for "no badge".
 *  Worktree column (Y) wins for display when present — that's what the
 *  user is actively editing. Otherwise show the staged column (X). */
function gitDecoration(code) {
  if (!code || code.length < 2) return null;
  const x = code[0];
  const y = code[1];
  if (x === "?" && y === "?") return { label: "U", cls: "git-untracked" };
  if (x === "!" && y === "!") return { label: "!", cls: "git-ignored" };
  if (x === "U" || y === "U" || (x === "A" && y === "A") || (x === "D" && y === "D")) {
    return { label: "C", cls: "git-conflict" };
  }
  const c = y !== " " ? y : x;
  switch (c) {
    case "M":
      return { label: "M", cls: "git-modified" };
    case "A":
      return { label: "A", cls: "git-added" };
    case "D":
      return { label: "D", cls: "git-deleted" };
    case "R":
      return { label: "R", cls: "git-renamed" };
    case "C":
      return { label: "C", cls: "git-copied" };
    case "T":
      return { label: "T", cls: "git-modified" };
    default:
      return null;
  }
}

/** TODO(user): decide how a directory rolls up its descendants' statuses.
 *
 * `codes` is the list of porcelain codes for every changed descendant. Return
 * a single code string to show as the directory's badge, or null for none.
 *
 * Trade-offs to weigh:
 *   - VS Code shows "M" if anything inside is modified, dropping untracked-only
 *     dirs to a dimmer dot. Calmer, but hides new files.
 *   - You could surface "U" so an untracked subfolder still draws the eye —
 *     better for "what's new" but noisier in repos with many untracked.
 *   - You could return null entirely so only files get badges (least visual
 *     noise, but loses the "something inside changed" cue).
 *
 * Default below: prefer modified > added > deleted > conflict > untracked.
 * Swap the priority array (or the whole function body) to taste.
 */
function aggregateDirStatus(codes) {
  if (codes.length === 0) return null;
  const priority = ["UU", "DD", "AA", "M", "A", "D", "R", "C", "T", "?"];
  for (const want of priority) {
    for (const code of codes) {
      if (code.includes(want[0]) || code === want) return code;
    }
  }
  return codes[0];
}

/** Set or clear the `.badge` element on a tree row according to `code`. */
function applyBadge(row, code) {
  let badge = row.querySelector(":scope > .badge");
  const deco = gitDecoration(code);
  if (!deco) {
    if (badge) badge.remove();
    row.classList.remove("git-decorated");
    return;
  }
  if (!badge) {
    badge = document.createElement("span");
    badge.className = "badge";
    row.appendChild(badge);
  }
  badge.textContent = deco.label;
  badge.className = "badge " + deco.cls;
  row.classList.add("git-decorated");
}

/** For a directory's absolute path, collect codes of every entry inside it. */
function codesUnder(dirPath) {
  const prefix = dirPath.endsWith("/") ? dirPath : dirPath + "/";
  const out = [];
  for (const [p, code] of Object.entries(gitEntries)) {
    if (p.startsWith(prefix)) out.push(code);
  }
  return out;
}

/** Walk every rendered tree row and (re)apply its badge. Idempotent. */
function applyGitDecorations(scope = tree) {
  const lis = scope === tree ? tree.querySelectorAll("li[data-path]")
                              : scope.querySelectorAll(":scope li[data-path], :scope[data-path]");
  for (const li of lis) {
    const row = li.querySelector(":scope > .row");
    if (!row) continue;
    const path = li.dataset.path;
    const isDir = li.dataset.isDir === "1";
    let code = gitEntries[path] || null;
    if (!code && isDir) {
      code = aggregateDirStatus(codesUnder(path));
    }
    applyBadge(row, code);
  }
}

async function refreshGitStatus() {
  if (!treeRoot) return;
  try {
    const report = await invoke("git_status", { path: treeRoot });
    gitRepoRoot = report.repo_root;
    gitEntries = report.entries || Object.create(null);
  } catch (e) {
    // Not in a repo, git unavailable, or another transient error. Treat as
    // "no decorations" rather than surfacing — git status is a nice-to-have.
    console.debug("git_status skipped:", e);
    gitRepoRoot = null;
    gitEntries = Object.create(null);
  }
  applyGitDecorations();
}

function scheduleGitRefresh() {
  if (gitRefreshTimer != null) clearTimeout(gitRefreshTimer);
  gitRefreshTimer = setTimeout(() => {
    gitRefreshTimer = null;
    refreshGitStatus();
  }, GIT_REFRESH_DEBOUNCE_MS);
}

/* ---- Tree ---- */

async function renderRoot() {
  tree.replaceChildren();
  const children = await listDir(treeRoot);
  for (const entry of children) {
    tree.appendChild(makeNode(entry, 1));
  }
  applyGitDecorations();
  updateTreeWatch();
}

/** Tell the backend which directories to watch for live tree updates: the
 *  root plus every currently-expanded folder. Re-sent whenever the visible
 *  set changes. Best-effort — a failure just means the tree won't auto-refresh
 *  until the next change to the visible set. */
function updateTreeWatch() {
  invoke("watch_tree", { dirs: visibleDirs() }).catch((e) =>
    console.error("watch_tree failed", e),
  );
}

/** Root plus every expanded folder currently rendered in the tree. */
function visibleDirs() {
  const dirs = treeRoot ? [treeRoot] : [];
  for (const li of tree.querySelectorAll('li[data-is-dir="1"]')) {
    const row = li.querySelector(":scope > .row");
    if (row && row.classList.contains("open")) dirs.push(li.dataset.path);
  }
  return dirs;
}

function depthOf(li) {
  const d = parseInt(li.dataset.depth, 10);
  return Number.isFinite(d) ? d : 1;
}

/** Re-read the root and every expanded folder and reconcile each listing in
 *  place, so files added/removed/renamed by other apps appear without a
 *  restart. Reconciling reuses existing rows (preserving expanded subtrees,
 *  selection, and scroll) and only touches what actually changed. */
async function refreshTree() {
  if (!treeRoot) return;
  await refreshOneDir(treeRoot, tree, 1);
  // Snapshot after the root reconcile so vanished top-level folders are gone;
  // the static NodeList is safe to iterate while nested reconciles mutate it,
  // and tree.contains() skips any row a parent refresh has since removed.
  for (const li of tree.querySelectorAll('li[data-is-dir="1"]')) {
    if (!tree.contains(li)) continue;
    const row = li.querySelector(":scope > .row");
    const ul = li.querySelector(":scope > ul");
    if (!row || !row.classList.contains("open") || !ul) continue;
    await refreshOneDir(li.dataset.path, ul, depthOf(li) + 1);
  }
  applyGitDecorations();
  updateTreeWatch();
}

async function refreshOneDir(path, container, depth) {
  childCache.delete(path);
  let entries;
  try {
    entries = await listDir(path);
  } catch (e) {
    // Folder vanished or unreadable; its row (if any) is dropped when its
    // parent reconciles, so there's nothing to update here.
    console.debug("tree refresh skipped:", path, e);
    return;
  }
  reconcileChildren(container, entries, depth);
}

/** Update a rendered <ul> (root or a folder's) in place to match `entries`:
 *  reuse existing <li> by path, insert new entries in sorted order, drop
 *  vanished ones. A path whose type flipped (file <-> dir) is recreated. */
function reconcileChildren(container, entries, depth) {
  const existing = new Map();
  for (const li of [...container.children]) {
    if (li.dataset && li.dataset.path) existing.set(li.dataset.path, li);
  }
  const seen = new Set();
  let prev = null;
  for (const entry of entries) {
    let li = existing.get(entry.path);
    const wantDir = entry.is_dir ? "1" : "0";
    if (li && li.dataset.isDir !== wantDir) {
      li.remove();
      li = null;
    }
    if (!li) li = makeNode(entry, depth);
    seen.add(entry.path);
    if (prev) prev.after(li);
    else container.prepend(li);
    prev = li;
  }
  for (const [path, li] of existing) {
    if (!seen.has(path)) li.remove();
  }
}

function rememberFolder(path) {
  invoke("remember_folder", { path }).catch((e) =>
    console.error("remember_folder failed", e),
  );
}

async function setTreeRoot(path) {
  if (isSearchModeOpen()) exitSearchMode();
  treeRoot = path;
  treeTitle.textContent = basename(path) || path;
  treeTitle.title = path;
  childCache.clear();
  await renderRoot();
  refreshGitStatus();
  rememberFolder(path);
  const tab = activeTab();
  if (tab) highlightSelectedByPath(tab.path);
}

async function openExternalFolder(path) {
  await setTreeRoot(path);
}

async function listDir(path) {
  if (childCache.has(path)) return childCache.get(path);
  const entries = await invoke("list_dir", { path });
  childCache.set(path, entries);
  return entries;
}

function makeNode(entry, depth) {
  const li = document.createElement("li");
  li.dataset.path = entry.path;
  li.dataset.isDir = entry.is_dir ? "1" : "0";
  li.dataset.depth = String(depth);

  const row = document.createElement("div");
  row.className = "row " + (entry.is_dir ? "dir" : "file");
  if (!entry.is_dir && MD_EXT.test(entry.name)) row.classList.add("is-md");
  row.style.setProperty("--row-indent", `${depth * 12 + 4}px`);

  const chev = document.createElement("span");
  chev.className = "chev";
  chev.textContent = "▶";
  row.appendChild(chev);

  const icon = document.createElement("span");
  icon.className = "icon";
  icon.textContent = entry.is_dir
    ? "\u{1F4C1}"
    : MD_EXT.test(entry.name)
      ? "\u{1F4C4}"
      : "·";
  row.appendChild(icon);

  const name = document.createElement("span");
  name.className = "name";
  name.textContent = entry.name;
  row.appendChild(name);

  if (entry.is_dir) {
    row.addEventListener("click", () => onDirClick(entry, li, row, depth));
  } else {
    const handler = singleOrDouble(
      () => onTreeFileSingle(entry.path),
      () => onTreeFileDouble(entry.path),
    );
    row.addEventListener("click", handler);
  }

  li.appendChild(row);
  return li;
}

async function onDirClick(entry, li, row, depth) {
  const open = li.querySelector(":scope > ul");
  if (open) {
    open.remove();
    row.classList.remove("open");
    // Bust the cache so a folder that changes while collapsed (and thus
    // unwatched) reads fresh on the next expand.
    childCache.delete(entry.path);
    updateTreeWatch();
    return;
  }
  row.classList.add("open");
  let children;
  try {
    children = await listDir(entry.path);
  } catch (e) {
    console.error("list_dir failed", e);
    row.classList.remove("open");
    return;
  }
  const ul = document.createElement("ul");
  for (const child of children) {
    ul.appendChild(makeNode(child, depth + 1));
  }
  li.appendChild(ul);
  applyGitDecorations(ul);
  updateTreeWatch();
}

async function onTreeFileSingle(path) {
  const existing = findTab(path);
  if (existing !== -1) {
    await setActiveTab(existing);
    return;
  }
  await openPreview(path);
}

async function onTreeFileDouble(path) {
  await openSticky(path);
}

function highlightSelectedByPath(path) {
  for (const el of document.querySelectorAll(".tree .row.selected")) {
    el.classList.remove("selected");
  }
  if (!path) return;
  const li = tree.querySelector(`li[data-path="${cssEscape(path)}"]`);
  if (li) {
    const row = li.querySelector(":scope > .row");
    if (row) row.classList.add("selected");
  }
}

/* ---- Tree file operations ---- */

/** After a rename, rewrite any open tab whose path is the renamed entry or
 *  nested under it (folder rename), and rewire the active tab's watcher. */
function retargetTabsForRename(from, to) {
  let activeChanged = false;
  for (let i = 0; i < tabs.length; i++) {
    const p = tabs[i].path;
    if (p === from) {
      tabs[i].path = to;
      if (i === activeIdx) activeChanged = true;
    } else if (p.startsWith(from + "/")) {
      tabs[i].path = to + p.slice(from.length);
      if (i === activeIdx) activeChanged = true;
    }
  }
  renderTabBar();
  if (activeChanged && activeIdx >= 0) {
    invoke("open_file", { path: tabs[activeIdx].path }).catch((e) =>
      console.warn("rewire watcher after rename failed", e),
    );
  }
}

/** Close any tab pointing at `path` or nested under it (folder delete). */
function closeTabsUnder(path) {
  for (let i = tabs.length - 1; i >= 0; i--) {
    const p = tabs[i].path;
    if (p === path || p.startsWith(path + "/")) {
      tabs[i].dirty = false; // deleted on disk — don't prompt to save
      closeTab(i);
    }
  }
}

/** Replace a tree row's name span with an <input> for editing. Resolves to the
 *  committed (validated) name, or null on cancel. Does not touch disk. */
function promptInlineName(row, initial) {
  return new Promise((resolve) => {
    const nameEl = row.querySelector(":scope > .name");
    const input = document.createElement("input");
    input.className = "tree-rename-input";
    input.type = "text";
    input.value = initial;
    input.spellcheck = false;
    if (nameEl) nameEl.replaceWith(input);
    else row.appendChild(input);
    input.focus();
    const dot = initial.lastIndexOf(".");
    input.setSelectionRange(0, dot > 0 ? dot : initial.length);

    let settled = false;
    const restore = () => {
      const span = document.createElement("span");
      span.className = "name";
      span.textContent = initial;
      input.replaceWith(span);
    };
    const commit = () => {
      if (settled) return;
      const value = input.value;
      const err = validateName(value);
      if (err) {
        input.classList.add("invalid");
        input.title = err;
        return;
      }
      settled = true;
      resolve(value.trim());
    };
    const cancel = () => {
      if (settled) return;
      settled = true;
      restore();
      resolve(null);
    };
    input.addEventListener("keydown", (ev) => {
      ev.stopPropagation();
      if (ev.key === "Enter") {
        ev.preventDefault();
        commit();
      } else if (ev.key === "Escape") {
        ev.preventDefault();
        cancel();
      }
    });
    input.addEventListener("blur", cancel);
  });
}

async function renameTreeEntry(li) {
  const from = li.dataset.path;
  const row = li.querySelector(":scope > .row");
  if (!row) return;
  const newName = await promptInlineName(row, basename(from));
  if (newName == null || newName === basename(from)) {
    await refreshTree();
    return;
  }
  const to = parentDir(from) + "/" + newName;
  try {
    await invoke("rename_path", { from, to });
  } catch (e) {
    showTransientError("Rename failed: " + e);
    await refreshTree();
    return;
  }
  retargetTabsForRename(from, to);
  await refreshTree();
}

/** Insert a placeholder row into `container` and inline-edit its name to create
 *  a new file or folder via the backend. */
async function createTreeEntry(container, dir, depth, isDir) {
  const li = document.createElement("li");
  li.dataset.isDir = isDir ? "1" : "0";
  li.dataset.depth = String(depth);
  const row = document.createElement("div");
  row.className = "row " + (isDir ? "dir" : "file");
  row.style.setProperty("--row-indent", `${depth * 12 + 4}px`);
  const icon = document.createElement("span");
  icon.className = "icon";
  icon.textContent = isDir ? "\u{1F4C1}" : "·";
  row.appendChild(icon);
  const name = document.createElement("span");
  name.className = "name";
  row.appendChild(name);
  li.appendChild(row);
  container.prepend(li);

  const newName = await promptInlineName(row, isDir ? "untitled" : "untitled.md");
  if (newName == null) {
    li.remove();
    return;
  }
  try {
    const cmd = isDir ? "create_folder" : "create_file";
    const created = await invoke(cmd, { dir, name: newName });
    li.remove();
    await refreshTree();
    if (!isDir) {
      await openSticky(created);
      const t = activeTab();
      if (t && t.path === created) await enterEditMode(t);
    }
  } catch (e) {
    showTransientError("Create failed: " + e);
    li.remove();
    await refreshTree();
  }
}

/** A folder target's directory: the folder itself if `li` is a dir, else its
 *  parent. Used to decide where New File / New Folder land. */
function dirForNewEntry(li) {
  return li.dataset.isDir === "1" ? li.dataset.path : parentDir(li.dataset.path);
}

/** Ensure a directory row is expanded so a new child is visible after creation. */
async function ensureExpanded(li) {
  const row = li.querySelector(":scope > .row");
  if (row && !row.classList.contains("open")) {
    const entry = { path: li.dataset.path, is_dir: true, name: basename(li.dataset.path) };
    await onDirClick(entry, li, row, depthOf(li));
  }
}

/** The <ul> a new entry should be inserted into for `li`: the folder's own list
 *  (creating it if missing) when `li` is a dir, else `li`'s parent list. */
function newEntryContainer(li, isDir) {
  if (isDir) {
    let ul = li.querySelector(":scope > ul");
    if (!ul) {
      ul = document.createElement("ul");
      li.appendChild(ul);
    }
    return ul;
  }
  return li.parentElement || tree;
}

async function duplicateTreeEntry(li) {
  try {
    const created = await invoke("duplicate_file", { path: li.dataset.path });
    await refreshTree();
    await openPreview(created);
  } catch (e) {
    showTransientError("Duplicate failed: " + e);
  }
}

async function deleteTreeEntry(li) {
  const path = li.dataset.path;
  const ok = await dialogApi.ask(`Move "${basename(path)}" to Trash?`, {
    title: "MDViewer",
    kind: "warning",
  });
  if (!ok) return;
  try {
    await invoke("delete_to_trash", { path });
  } catch (e) {
    showTransientError("Delete failed: " + e);
    return;
  }
  closeTabsUnder(path);
  await refreshTree();
}

/* ---- Tabs ---- */

async function openPreview(path) {
  const existing = findTab(path);
  if (existing !== -1) {
    await setActiveTab(existing);
    return;
  }
  const previewIdx = tabs.findIndex((t) => !t.sticky);
  if (previewIdx !== -1) {
    tabs[previewIdx].path = path;
    tabs[previewIdx].raw = false;
    tabs[previewIdx].editing = false;
    tabs[previewIdx].dirty = false;
    tabs[previewIdx].savedContent = null;
    await setActiveTab(previewIdx, { forceRender: true });
    return;
  }
  tabs.push({ path, sticky: false, raw: false, editing: false, dirty: false, savedContent: null });
  await setActiveTab(tabs.length - 1);
}

async function openSticky(path) {
  const existing = findTab(path);
  if (existing !== -1) {
    tabs[existing].sticky = true;
    await setActiveTab(existing);
    return;
  }
  tabs.push({ path, sticky: true, raw: false, editing: false, dirty: false, savedContent: null });
  await setActiveTab(tabs.length - 1);
}

async function openTabAtLine(path, line) {
  const idx = findTab(path);
  if (idx !== -1) {
    tabs[idx].pendingJumpLine = line;
    await setActiveTab(idx, { forceRender: true });
    return;
  }
  const previewIdx = tabs.findIndex((t) => !t.sticky);
  await openPreview(path);
  const finalIdx = previewIdx !== -1 ? previewIdx : tabs.length - 1;
  if (finalIdx >= 0 && finalIdx < tabs.length) {
    tabs[finalIdx].pendingJumpLine = line;
    await renderActive({ scrollLock: false });
  }
}

function persistSession() {
  if (restoring) return;
  invoke("save_session", {
    tabs: tabs.map((t) => t.path),
    active: activeIdx >= 0 ? activeIdx : null,
  }).catch((e) => console.error("save_session failed", e));
}

async function restoreSession(paths, active) {
  for (const p of paths) {
    tabs.push({ path: p, sticky: true, raw: false, editing: false, dirty: false, savedContent: null });
  }
  if (tabs.length === 0) return;
  const idx =
    active != null && active >= 0 && active < tabs.length ? active : 0;
  await setActiveTab(idx);
}

async function setActiveTab(idx, { forceRender = false } = {}) {
  if (idx < 0 || idx >= tabs.length) {
    activeIdx = -1;
    renderTabBar();
    showEditorChrome(false);
    showEmptyState();
    return;
  }
  const same = idx === activeIdx;
  activeIdx = idx;
  if (typeof hideConflict === "function") hideConflict();
  renderTabBar();
  persistSession();
  highlightSelectedByPath(tabs[idx].path);
  try {
    await invoke("open_file", { path: tabs[idx].path });
  } catch (e) {
    console.warn("open_file failed", e);
  }
  const t = tabs[idx];
  if (t.editing) {
    ensureCm();
    showEditorChrome(true);
    cm.setValue(t.editBuffer != null ? t.editBuffer : t.savedContent);
    cm.clearHistory();
    cm.refresh();
    await renderFromEditor(t, { scrollLock: same && !forceRender });
  } else {
    showEditorChrome(false);
    await renderActive({ scrollLock: same && !forceRender });
  }
}

function makeStickyAt(idx) {
  if (idx < 0 || idx >= tabs.length) return;
  if (tabs[idx].sticky) return;
  tabs[idx].sticky = true;
  renderTabBar();
}

function closeTab(idx) {
  if (idx < 0 || idx >= tabs.length) return;
  const t = tabs[idx];
  if (t.editing && t.dirty) {
    dialogApi
      .ask(`Discard unsaved changes to ${basename(t.path)}?`, {
        title: "MDViewer",
        kind: "warning",
      })
      .then((discard) => {
        if (discard) {
          t.dirty = false;
          closeTab(idx);
        }
      });
    return;
  }
  tabs.splice(idx, 1);
  if (tabs.length === 0) {
    activeIdx = -1;
    renderTabBar();
    showEmptyState();
    persistSession();
    return;
  }
  let next;
  if (idx < activeIdx) next = activeIdx - 1;
  else if (idx === activeIdx) next = Math.min(idx, tabs.length - 1);
  else next = activeIdx;
  activeIdx = -1;
  setActiveTab(next);
}

function renderTabBar() {
  if (tabs.length === 0) {
    tabBar.hidden = true;
    tabsEl.replaceChildren();
    return;
  }
  tabBar.hidden = false;
  tabsEl.replaceChildren();
  for (let i = 0; i < tabs.length; i++) {
    tabsEl.appendChild(makeTabEl(tabs[i], i));
  }
  const t = activeTab();
  if (t) {
    const image = isImagePath(t.path);
    editBtn.hidden = image;
    if (!image) {
      editBtn.textContent = t.editing ? "Done" : "Edit";
      editBtn.setAttribute("aria-pressed", t.editing ? "true" : "false");
    }
    saveBtn.hidden = !t.editing;
    rawBtn.hidden = image || t.editing;
    if (!image) {
      rawBtn.textContent = t.raw ? "Rendered" : "Raw";
      rawBtn.setAttribute("aria-pressed", t.raw ? "true" : "false");
    }
  }
}

function makeTabEl(tab, idx) {
  const el = document.createElement("div");
  el.className = "tab";
  if (!tab.sticky) el.classList.add("preview");
  if (idx === activeIdx) el.classList.add("active");
  el.title = tab.path;
  el.setAttribute("role", "tab");

  const name = document.createElement("span");
  name.className = "tab-name";
  name.textContent = basename(tab.path);
  el.appendChild(name);

  if (tab.dirty) {
    const dot = document.createElement("span");
    dot.className = "tab-dirty";
    dot.textContent = "●";
    dot.title = "Unsaved changes";
    el.appendChild(dot);
  }

  const close = document.createElement("span");
  close.className = "tab-close";
  close.textContent = "×";
  close.title = "Close tab";
  close.addEventListener("click", (e) => {
    e.stopPropagation();
    closeTab(idx);
  });
  el.appendChild(close);

  const handler = singleOrDouble(
    () => setActiveTab(idx),
    () => makeStickyAt(idx),
  );
  el.addEventListener("click", handler);
  el.addEventListener("auxclick", (e) => {
    if (e.button === 1) {
      e.preventDefault();
      closeTab(idx);
    }
  });
  return el;
}

function onToggleRaw() {
  const t = activeTab();
  if (!t) return;
  t.raw = !t.raw;
  renderTabBar();
  renderActive({ scrollLock: false });
}

/* ---- Source editor ---- */

function ensureCm() {
  if (cm) return cm;
  cm = window.CodeMirror(editorPane, {
    value: "",
    mode: "markdown",
    lineNumbers: true,
    lineWrapping: true,
    theme: "default",
  });
  cm.on("change", onEditorChange);
  cm.setOption("extraKeys", {
    "Cmd-S": () => saveActive(),
    "Ctrl-S": () => saveActive(),
  });
  return cm;
}

async function onToggleEdit() {
  const t = activeTab();
  if (!t || isImagePath(t.path)) return;
  if (t.editing) {
    await exitEditMode(t);
  } else {
    await enterEditMode(t);
  }
}

async function enterEditMode(t) {
  let src;
  try {
    src = await invoke("read_source", { path: t.path });
  } catch (e) {
    showTransientError("Can't open this file for editing: " + e);
    return;
  }
  t.editing = true;
  t.raw = false;
  t.savedContent = src;
  t.dirty = false;
  t.editBuffer = src;
  ensureCm();
  cm.setValue(src);
  cm.clearHistory();
  showEditorChrome(true);
  renderTabBar();
  cm.refresh();
  cm.focus();
  await renderFromEditor(t, { scrollLock: false });
}

async function exitEditMode(t) {
  if (t.dirty) {
    const discard = await dialogApi.ask(
      `Discard unsaved changes to ${basename(t.path)}?`,
      { title: "MDViewer", kind: "warning" },
    );
    if (!discard) return;
  }
  t.editing = false;
  t.dirty = false;
  hideConflict();
  showEditorChrome(false);
  renderTabBar();
  await renderActive({ scrollLock: false });
}

function showEditorChrome(on) {
  editorPane.hidden = !on;
  editorSplitter.hidden = !on;
  paneBody.classList.toggle("editing", on);
}

function onEditorChange() {
  const t = activeTab();
  if (!t || !t.editing) return;
  t.editBuffer = cm.getValue();
  const dirty = isDirty(t.editBuffer, t.savedContent);
  if (dirty !== t.dirty) {
    t.dirty = dirty;
    renderTabBar();
  }
  if (previewDebounce) clearTimeout(previewDebounce);
  const path = t.path;
  previewDebounce = setTimeout(() => {
    previewDebounce = null;
    const current = activeTab();
    if (!current || !current.editing || current.path !== path) return;
    renderFromEditor(current, { scrollLock: true }).catch((e) =>
      console.error("live preview failed", e),
    );
  }, EDITOR_PREVIEW_DEBOUNCE_MS);
}

/** Render the editor buffer (not disk) into the preview via render_preview. */
async function renderFromEditor(t, { scrollLock = true, forceMermaid = false } = {}) {
  if (!cm || !t.editing) return;
  let html;
  try {
    html = await invoke("render_preview", {
      source: cm.getValue(),
      path: t.path,
      theme: currentTheme,
    });
  } catch (e) {
    console.error("render_preview failed", e);
    return;
  }
  await paintHtml(t, html, false, { scrollLock, forceMermaid });
}

async function saveActive() {
  const t = activeTab();
  if (!t || !t.editing || !cm) return;
  const content = cm.getValue();
  try {
    await invoke("save_file", {
      path: t.path,
      contents: content,
      expected: t.savedContent,
    });
    t.savedContent = content;
    t.editBuffer = content;
    t.dirty = false;
    hideConflict();
    renderTabBar();
  } catch (e) {
    if (String(e).includes("changed on disk")) {
      showConflict(t);
    } else {
      showTransientError("Save failed: " + e);
    }
  }
}

const conflictBanner = document.getElementById("editor-conflict");
const conflictReload = document.getElementById("editor-conflict-reload");
const conflictKeep = document.getElementById("editor-conflict-keep");

function showConflict(t) {
  conflictReload.onclick = () => reloadFromDisk(t);
  conflictKeep.onclick = () => forceSave(t);
  conflictBanner.hidden = false;
}

function hideConflict() {
  conflictBanner.hidden = true;
}

async function reloadFromDisk(t) {
  let disk;
  try {
    disk = await invoke("read_source", { path: t.path });
  } catch (e) {
    showTransientError("Reload failed: " + e);
    return;
  }
  if (cm) cm.setValue(disk);
  t.savedContent = disk;
  t.editBuffer = disk;
  t.dirty = false;
  hideConflict();
  renderTabBar();
  await renderFromEditor(t, { scrollLock: false });
}

async function forceSave(t) {
  if (!cm) return;
  const content = cm.getValue();
  try {
    await invoke("save_file", { path: t.path, contents: content, expected: null });
    t.savedContent = content;
    t.editBuffer = content;
    t.dirty = false;
    hideConflict();
    renderTabBar();
  } catch (e) {
    showTransientError("Save failed: " + e);
  }
}

async function onEditingFileChanged(t) {
  let disk;
  try {
    disk = await invoke("read_source", { path: t.path });
  } catch (e) {
    // File may have been removed/renamed externally; leave the buffer intact.
    console.debug("editing file-changed read skipped:", e);
    return;
  }
  const cls = classifyFileChange({
    editing: t.editing,
    dirty: t.dirty,
    diskContent: disk,
    savedContent: t.savedContent,
  });
  if (cls === "self") return; // our own write; nothing to do
  if (cls === "reload") {
    await reloadFromDisk(t);
  } else {
    showConflict(t);
  }
}

function hasThemePref() {
  return isValidTheme(localStorage.getItem(THEME_KEY));
}

function updateThemeButton() {
  const face = themeButtonFace(currentTheme);
  themeBtn.textContent = face.icon;
  themeBtn.title = face.label;
  themeBtn.setAttribute("aria-label", face.label);
}

async function applyTheme(theme) {
  currentTheme = theme;
  document.documentElement.dataset.theme = theme;
  initMermaid();
  updateThemeButton();
  const t = activeTab();
  if (t) {
    if (t.editing) {
      await renderFromEditor(t, { scrollLock: false, forceMermaid: true });
    } else {
      await renderActive({ scrollLock: false, forceMermaid: true });
    }
  }
}

let themeToggling = false;

async function onToggleTheme() {
  if (themeToggling) return;
  themeToggling = true;
  try {
    const next = nextTheme(currentTheme);
    localStorage.setItem(THEME_KEY, next);
    await applyTheme(next);
  } finally {
    themeToggling = false;
  }
}

/* ---- Rendering ---- */

function showEmptyState() {
  preview.hidden = true;
  previewEmpty.hidden = false;
  preview.replaceChildren();
  preview.classList.remove("raw-body");
  if (findOpen()) closeFind();
}

async function renderActive({ scrollLock = true, forceMermaid = false } = {}) {
  const t = activeTab();
  if (!t) {
    showEmptyState();
    return;
  }
  if (isImagePath(t.path)) {
    renderImage(t, { scrollLock });
    return;
  }
  let result;
  try {
    result = await invoke("render_file", {
      path: t.path,
      theme: currentTheme,
      raw: t.raw,
    });
  } catch (e) {
    console.error("render_file failed", e);
    showError(String(e));
    return;
  }
  await paintHtml(t, result.html, result.raw, { scrollLock, forceMermaid });
}

/** Diff `html` into #preview and run the post-render pipeline. Shared by the
 *  disk renderer (renderActive) and the editor's live preview. */
async function paintHtml(t, html, raw, { scrollLock = true, forceMermaid = false } = {}) {
  previewEmpty.hidden = true;
  preview.hidden = false;
  preview.classList.toggle("raw-body", raw);

  const anchor = scrollLock ? captureAnchor() : null;

  const incoming = document.createElement("article");
  incoming.className = "markdown-body" + (raw ? " raw-body" : "");
  incoming.id = "preview";
  incoming.innerHTML = html;

  window.morphdom(preview, incoming, {
    onBeforeElUpdated: (fromEl, toEl) => {
      // Keep an already-rendered diagram if its source is unchanged, so
      // editing nearby prose doesn't re-render (and flicker) the SVG.
      if (
        !forceMermaid &&
        fromEl.dataset.mvState &&
        fromEl.classList.contains("mermaid") &&
        fromEl.dataset.mermaidSrc === mermaidSource(toEl)
      ) {
        return false;
      }
      // Keep an already-rendered math span if its source is unchanged — same
      // reasoning as mermaid. Comrak re-emits the bare LaTeX on every render.
      if (
        fromEl.dataset.mathState === "ok" &&
        toEl.hasAttribute &&
        toEl.hasAttribute("data-math-style") &&
        (toEl.textContent || "").trim() === fromEl.dataset.mathSrc
      ) {
        return false;
      }
      // Keep an already-resolved image whose source file is unchanged, so the
      // asset:// rewrite below isn't undone (and the image re-fetched) on every
      // live reload.
      if (fromEl.tagName === "IMG" && toEl.tagName === "IMG") {
        const want = localImageUrl(parentDir(t.path), toEl.getAttribute("src"));
        if (want && fromEl.src === want) return false;
      }
      return !fromEl.isEqualNode(toEl);
    },
  });

  const hadPendingJump = t.pendingJumpLine != null;
  await postRender(t, { raw, forceMermaid });

  if (!hadPendingJump) {
    if (anchor) restoreAnchor(anchor);
    else previewScroll.scrollTop = 0;
  }

  if (findOpen()) runFind({ keepCurrent: true, scroll: false });
}

/** Render a standalone image file via the asset protocol, at natural size.
 *  Preserves scroll only when the same image is re-rendered (live reload),
 *  not when switching to a different image. */
function renderImage(t, { scrollLock = true } = {}) {
  previewEmpty.hidden = true;
  preview.hidden = false;
  if (findOpen()) closeFind();

  const base = convertFileSrc(t.path);
  const existing = preview.querySelector("img");
  const sameImage =
    preview.classList.contains("image-view") &&
    existing &&
    existing.src.split("?")[0] === base.split("?")[0];
  const top = sameImage && scrollLock ? previewScroll.scrollTop : 0;
  const left = sameImage && scrollLock ? previewScroll.scrollLeft : 0;

  preview.className = "image-view";

  const v = imageVersions.get(t.path) || 0;
  const img = document.createElement("img");
  img.alt = basename(t.path);
  img.onerror = () => {
    const err = document.createElement("div");
    err.className = "image-error";
    err.textContent = "Can't display this image.";
    preview.replaceChildren(err);
  };
  img.src = base + (v ? `?v=${v}` : "");

  preview.replaceChildren(img);
  previewScroll.scrollTop = top;
  previewScroll.scrollLeft = left;
}

/* ---- Post-render hooks ---- */

/** Runs after each morphdom patch. New hooks go here so the call site in
 *  renderActive stays one line and the ordering — link annotation, image
 *  resolution, copy buttons, then math/diagram rendering (both of which
 *  change element heights) — lives in one place. */
async function postRender(t, { raw = false, forceMermaid = false } = {}) {
  annotateLinks();
  resolveImages(parentDir(t.path));
  addCopyButtons();
  hookTaskListCheckboxes(t);
  if (!raw) {
    renderMath();
    await renderMermaid({ force: forceMermaid });
    addMermaidExportButtons();
  }
  if (t.pendingJumpLine != null) {
    const line = t.pendingJumpLine;
    t.pendingJumpLine = null;
    jumpToLine(line);
  }
}

function jumpToLine(line) {
  const target = findElementForLine(line);
  if (!target) return;
  target.scrollIntoView({ block: "center" });
  pulseJumpHighlight(target);
}

function findElementForLine(line) {
  // Comrak emits data-sourcepos="L1:C1-L2:C2"; pick the deepest element whose
  // [L1, L2] range contains `line`. Walk depth-first so nested blocks win
  // over their parents.
  const all = preview.querySelectorAll("[data-sourcepos]");
  let best = null;
  for (const el of all) {
    const m = el.dataset.sourcepos.match(/^(\d+):\d+-(\d+):\d+$/);
    if (!m) continue;
    const a = parseInt(m[1], 10);
    const b = parseInt(m[2], 10);
    if (a <= line && line <= b) {
      best = el;
    }
  }
  return best;
}

function pulseJumpHighlight(el) {
  if (
    typeof CSS === "undefined" ||
    !CSS.highlights ||
    typeof Highlight === "undefined"
  ) {
    return;
  }
  try {
    const range = document.createRange();
    range.selectNodeContents(el);
    const hl = new Highlight(range);
    CSS.highlights.set("search-jump", hl);
    setTimeout(() => CSS.highlights.delete("search-jump"), 1500);
  } catch {
    // selectNodeContents can throw on unusual targets; ignore.
  }
}

// Keys "path|line" for toggles currently in flight. Prevents wasted IPC
// from rapid double-clicks and avoids overlapping read-modify-write races
// on the same checkbox before the watcher delivers the final state.
const pendingToggles = new Set();

/** comrak's tasklist extension emits <input type="checkbox" disabled> inside
 *  an <li> with data-sourcepos. Strip disabled and attach a click handler.
 *  Morphdom may re-add the disabled attribute on the next live reload (the
 *  incoming HTML has it), but this function runs again from postRender and
 *  is idempotent via the dataset marker. */
function hookTaskListCheckboxes(t) {
  for (const input of preview.querySelectorAll(
    "li[data-sourcepos] > input[type=checkbox]",
  )) {
    input.removeAttribute("disabled");
    if (input.dataset.mvTaskHook === "1") continue;
    input.dataset.mvTaskHook = "1";
    input.addEventListener("click", (ev) => {
      onTaskCheckboxClick(ev, input, t);
    });
  }
}

async function onTaskCheckboxClick(ev, input, t) {
  const li = input.closest("li[data-sourcepos]");
  if (!li) return;
  const line = parseStartLine(li.getAttribute("data-sourcepos"));
  if (line == null) return;
  // input.checked has ALREADY been flipped by the browser to the new state.
  const newState = input.checked;
  const expectedCurrent = !newState;

  const key = `${t.path}|${line}`;
  if (pendingToggles.has(key)) {
    ev.preventDefault();
    input.checked = expectedCurrent;
    return;
  }
  pendingToggles.add(key);
  try {
    await invoke("toggle_task", {
      path: t.path,
      line,
      newState,
      expectedCurrent,
    });
    // Success: file watcher fires file-changed and re-renders. The browser-
    // flipped state already matches what's now on disk so there's no flicker.
  } catch (e) {
    console.error("toggle_task failed", e);
    input.checked = expectedCurrent;
    showTransientError(String(e));
  } finally {
    pendingToggles.delete(key);
  }
}

let transientErrorTimer = null;
/** Show a short-lived error banner without disturbing the rendered document
 *  (unlike showError, which clears the preview). Used for transient operation
 *  failures — task-list toggles and export. */
function showTransientError(msg) {
  let banner = document.getElementById("task-error-banner");
  if (!banner) {
    banner = document.createElement("div");
    banner.id = "task-error-banner";
    banner.className = "task-error-banner";
    document.body.appendChild(banner);
  }
  banner.textContent = msg;
  banner.hidden = false;
  if (transientErrorTimer) clearTimeout(transientErrorTimer);
  transientErrorTimer = setTimeout(() => {
    banner.hidden = true;
  }, 3000);
}

/* ---- Document export ---- */

let exportInProgress = false;

const EXPORT_PAGE_CSS = `
html { color-scheme: light; }
body { margin: 0; background: #ffffff; }
.markdown-body { box-sizing: border-box; min-width: 200px; max-width: 980px; margin: 0 auto; padding: 32px 24px; }
`;

/** Menu entry point: pick a destination, then export. */
async function onExport(format) {
  const t = activeTab();
  if (!t) {
    showTransientError("Open a document before exporting.");
    return;
  }
  if (isImagePath(t.path)) {
    showTransientError("Export is only available for text documents.");
    return;
  }
  const ext = format === "pdf" ? "pdf" : "html";
  const filters =
    ext === "pdf"
      ? [{ name: "PDF document", extensions: ["pdf"] }]
      : [{ name: "HTML document", extensions: ["html"] }];
  const path = await dialogApi.save({
    defaultPath: exportFilename(t.path, ext),
    filters,
  });
  if (!path) return;
  await exportDocument(format, path);
}

/** Snapshot view state, force a light rendered view, run the format-specific
 *  export, then restore. The light re-render reuses the real renderActive
 *  pipeline so math/Mermaid/code come out light and faithful. */
async function exportDocument(format, path) {
  if (exportInProgress) return;
  const t = activeTab();
  if (!t) return;
  exportInProgress = true;
  const prevTheme = currentTheme;
  const prevDataTheme = document.documentElement.dataset.theme;
  const prevRaw = t.raw;
  const prevScroll = previewScroll.scrollTop;
  try {
    currentTheme = "light";
    document.documentElement.dataset.theme = "light";
    t.raw = false;
    initMermaid();
    await renderActive({ scrollLock: false, forceMermaid: true });

    // Files outside the opened workspace must not be embedded (HTML) or
    // rendered (PDF). Use the tree root as the boundary, falling back to the
    // document's own directory when no folder is open.
    const boundary = treeRoot || parentDir(t.path);
    if (format === "html") {
      await exportHtml(t, path, boundary);
    } else if (format === "pdf") {
      // The native print uses WebKit's print pipeline, so the @media print
      // stylesheet (chrome hidden, preview reflowed) applies during capture.
      // Strip out-of-workspace images from the live preview first; the finally
      // block's re-render restores them.
      await neutralizeOutsideWorkspaceImages(preview, boundary);
      await invoke("export_pdf", { path });
    }
  } catch (e) {
    console.error("export failed", e);
    showTransientError("Export failed: " + e);
  } finally {
    // Clear the lock first so a throw while restoring the view can't leave
    // export permanently disabled for the session.
    exportInProgress = false;
    currentTheme = prevTheme;
    document.documentElement.dataset.theme = prevDataTheme;
    t.raw = prevRaw;
    initMermaid();
    if (t.editing) {
      await renderFromEditor(t, { scrollLock: false, forceMermaid: true });
    } else {
      await renderActive({ scrollLock: false, forceMermaid: true });
    }
    previewScroll.scrollTop = prevScroll;
  }
}

/** Serialize the (already light-rendered) preview into one standalone HTML file
 *  and write it via the save_export command. */
async function exportHtml(t, path, boundary) {
  const clone = preview.cloneNode(true);
  // Drop UI chrome injected after render (copy buttons, mermaid export buttons).
  clone
    .querySelectorAll(".export-btn-group, .copy-btn")
    .forEach((el) => el.remove());
  // The live checkboxes were made interactive by hookTaskListCheckboxes; the
  // exported file has no JS, so render them disabled like GitHub does.
  clone
    .querySelectorAll('input[type="checkbox"]')
    .forEach((cb) => cb.setAttribute("disabled", ""));
  await inlineImages(clone, boundary);
  const bodyHtml = clone.innerHTML;

  // github-markdown.css is attribute-driven (light base, dark only under
  // [data-theme="dark"]); the exported document sets no data-theme, so the
  // light base always wins. forceLightCss is now a defensive no-op on this
  // file — kept in case the vendored CSS ever reintroduces media-query themes.
  let css = forceLightCss(await fetchText("github-markdown.css"));
  if (documentNeedsKatex(bodyHtml)) {
    let katexCss = await fetchText("katex/katex.min.css");
    katexCss = inlineFontUrls(katexCss, await buildKatexFontMap(katexCss));
    css += "\n" + katexCss;
  }
  css += "\n" + EXPORT_PAGE_CSS;

  const html = buildHtmlDocument({
    title: baseName(t.path),
    css,
    bodyHtml,
  });
  await invoke("save_export", { path, data: html, base64Encoded: false });
}

async function fetchText(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`fetch ${url} failed: ${res.status}`);
  return await res.text();
}

/** Replace local (asset:// or relative) <img> sources with data: URLs so the
 *  exported file is standalone. Remote (http/https) and existing data: srcs are
 *  left as-is. Per-image failures are logged and skipped (the original src
 *  stays, still valid online). macOS asset URLs use the asset:// scheme, so the
 *  http(s) check below correctly leaves only true remote images alone. */
async function inlineImages(root, boundary) {
  // Drop out-of-workspace images first so their bytes are never fetched/embedded.
  await neutralizeOutsideWorkspaceImages(root, boundary);
  const imgs = [...root.querySelectorAll("img")];
  await Promise.all(
    imgs.map(async (img) => {
      const src = img.getAttribute("src") || "";
      if (!src || src.startsWith("data:") || /^https?:/i.test(src)) return;
      try {
        const blob = await (await fetch(src)).blob();
        const mime = blob.type || "image/png";
        img.setAttribute(
          "src",
          `data:${mime};base64,` + (await blobToBase64(blob)),
        );
      } catch (e) {
        console.warn("image inline failed:", src, e);
      }
    }),
  );
}

/** Build { "fonts/X.woff2": "data:font/woff2;base64,…" } for every woff2 the
 *  KaTeX CSS references. Only woff2 exists on disk; woff/ttf fallbacks are left
 *  untouched (browsers prefer the inlined woff2 via its format() hint). */
async function buildKatexFontMap(katexCss) {
  const refs = [
    ...new Set(
      [...katexCss.matchAll(/url\((fonts\/[^)]+\.woff2)\)/g)].map((m) => m[1]),
    ),
  ];
  const map = {};
  await Promise.all(
    refs.map(async (ref) => {
      const blob = await (await fetch("katex/" + ref)).blob();
      map[ref] = "data:font/woff2;base64," + (await blobToBase64(blob));
    }),
  );
  return map;
}

/** Add hover-revealed SVG / PNG export buttons to each rendered mermaid block.
 *  Only attaches once renderMermaid has produced an SVG; idempotent across
 *  morphdom updates via the :scope > .export-btn-group existence check. */
function addMermaidExportButtons() {
  for (const pre of preview.querySelectorAll("pre.mermaid")) {
    if (pre.dataset.mvState !== "ok") continue;
    if (pre.querySelector(":scope > .export-btn-group")) continue;
    const group = document.createElement("div");
    group.className = "export-btn-group";
    group.appendChild(makeExportButton("SVG", (btn) => exportMermaidSvg(pre, btn)));
    group.appendChild(makeExportButton("PNG", (btn) => exportMermaidPng(pre, btn)));
    pre.appendChild(group);
  }
}

function makeExportButton(label, onClick) {
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "export-btn";
  btn.textContent = label;
  btn.title = `Save diagram as ${label}`;
  btn.addEventListener("click", (ev) => {
    ev.preventDefault();
    ev.stopPropagation();
    onClick(btn);
  });
  return btn;
}

function flashButton(btn, msg, durationMs = 1200) {
  const original = btn.textContent;
  btn.textContent = msg;
  btn.classList.add("ok");
  setTimeout(() => {
    btn.textContent = original;
    btn.classList.remove("ok");
  }, durationMs);
}

/** Re-render the mermaid source with a guaranteed-portable config: light
 *  theme (so exports look right pasted into any document) and HTML labels
 *  disabled (mermaid's default uses <foreignObject>, which WebKit can't
 *  reliably rasterize to canvas — that was the cause of silent PNG failures).
 *  The viewing diagrams' config is restored on the way out. */
async function renderMermaidForExport(src) {
  const exportConfig = {
    startOnLoad: false,
    securityLevel: "strict",
    theme: "default",
    flowchart: { htmlLabels: false },
    sequence: { htmlLabels: false },
    class: { htmlLabels: false },
    state: { htmlLabels: false },
  };
  window.mermaid.initialize(exportConfig);
  try {
    const id = "mmd-export-" + mermaidIdSeq++;
    const { svg } = await window.mermaid.render(id, src);
    return svg;
  } finally {
    // Restore the on-screen config so subsequent live-reload renders keep
    // the user's current theme.
    initMermaid();
  }
}

async function exportMermaidSvg(pre, btn) {
  const src = pre.dataset.mermaidSrc;
  if (!src) return;
  try {
    const path = await dialogApi.save({
      defaultPath: "diagram.svg",
      filters: [{ name: "SVG image", extensions: ["svg"] }],
    });
    if (!path) return;
    const svg = await renderMermaidForExport(src);
    const xml = '<?xml version="1.0" encoding="UTF-8"?>\n' + svg;
    await invoke("save_export", { path, data: xml, base64Encoded: false });
    flashButton(btn, "Saved");
  } catch (e) {
    console.error("SVG export failed", e);
    flashButton(btn, "Failed");
  }
}

async function exportMermaidPng(pre, btn) {
  const src = pre.dataset.mermaidSrc;
  if (!src) return;
  try {
    const path = await dialogApi.save({
      defaultPath: "diagram.png",
      filters: [{ name: "PNG image", extensions: ["png"] }],
    });
    if (!path) return;
    const svgStr = await renderMermaidForExport(src);
    const base64 = await rasterizeSvgStringToPng(svgStr, 2);
    await invoke("save_export", { path, data: base64, base64Encoded: true });
    flashButton(btn, "Saved");
  } catch (e) {
    console.error("PNG export failed", e);
    flashButton(btn, "Failed");
  }
}

/** Rasterize an SVG (as a string) to a PNG, returning base64. The SVG's
 *  viewBox (or width/height attributes) sets the natural pixel size; `scale`
 *  multiplies for Retina crispness. A white fill is laid down first so the
 *  PNG isn't transparent (most viewers show transparent PNGs on black/checker
 *  backgrounds, which makes mermaid diagrams unreadable).
 *
 *  Uses a data: URL rather than blob: — blob: would require adding `blob:`
 *  to the CSP's img-src, which we'd rather not broaden for one feature. The
 *  size overhead for a typical mermaid SVG (a few KB) is negligible. */
async function rasterizeSvgStringToPng(svgStr, scale = 2) {
  const { width: naturalW, height: naturalH } = readSvgNaturalSize(svgStr);
  const dataUrl = svgStringToDataUrl(svgStr);
  const img = await loadImage(dataUrl);
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.ceil(naturalW * scale));
  canvas.height = Math.max(1, Math.ceil(naturalH * scale));
  const ctx = canvas.getContext("2d");
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
  const blob = await new Promise((resolve, reject) => {
    canvas.toBlob(
      (b) => (b ? resolve(b) : reject(new Error("canvas.toBlob returned null"))),
      "image/png",
    );
  });
  return await blobToBase64(blob);
}

function svgStringToDataUrl(svgStr) {
  // UTF-8 → byte array → binary string → base64. TextEncoder handles BMP
  // and non-BMP correctly; btoa alone would mishandle multi-byte chars.
  const bytes = new TextEncoder().encode(svgStr);
  let s = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    s += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return "data:image/svg+xml;base64," + btoa(s);
}

function readSvgNaturalSize(svgStr) {
  const doc = new DOMParser().parseFromString(svgStr, "image/svg+xml");
  const root = doc.documentElement;
  const vb = (root.getAttribute("viewBox") || "").trim();
  if (vb) {
    const parts = vb.split(/[\s,]+/).map(Number);
    if (parts.length === 4 && parts[2] > 0 && parts[3] > 0) {
      return { width: parts[2], height: parts[3] };
    }
  }
  const w = parseFloat(root.getAttribute("width"));
  const h = parseFloat(root.getAttribute("height"));
  return {
    width: Number.isFinite(w) && w > 0 ? w : 800,
    height: Number.isFinite(h) && h > 0 ? h : 600,
  };
}

function loadImage(src) {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error("image load failed"));
    img.src = src;
  });
}

async function blobToBase64(blob) {
  const buf = await blob.arrayBuffer();
  const bytes = new Uint8Array(buf);
  // String.fromCharCode(...big_array) blows the stack; chunk into 32K windows.
  let s = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    s += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(s);
}

/** Render every comrak math span via KaTeX. Each span starts as
 *  <span data-math-style="inline|display">SRC</span>; after render it has
 *  KaTeX HTML inside plus a dataset.mathState marker that the morphdom
 *  diff uses to skip unchanged spans on live reload (mirrors the mermaid
 *  preservation pattern). */
function renderMath() {
  if (!window.katex) return;
  for (const el of preview.querySelectorAll("span[data-math-style]")) {
    if (el.dataset.mathState) continue;
    const source = (el.textContent || "").trim();
    const displayMode = el.getAttribute("data-math-style") === "display";
    try {
      window.katex.render(source, el, {
        displayMode,
        // Render parse errors inline (red) instead of throwing — one bad
        // expression must not break the rest of the document.
        throwOnError: false,
        output: "html",
      });
    } catch (e) {
      console.error("KaTeX render failed", e);
      el.classList.add("math-error");
      el.textContent = source;
    }
    el.dataset.mathState = "ok";
    el.dataset.mathSrc = source;
  }
}

/** Attach a hover-revealed "Copy" button to every code block. Idempotent:
 *  morphdom strips the button on each render (it isn't in the incoming HTML),
 *  so we just re-attach. Mermaid blocks are skipped — their textContent is
 *  source code, but the user already sees a rendered diagram and would expect
 *  a different affordance there. */
function addCopyButtons() {
  for (const pre of preview.querySelectorAll("pre")) {
    if (pre.classList.contains("mermaid")) continue;
    if (pre.querySelector(":scope > .copy-btn")) continue;
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "copy-btn";
    btn.setAttribute("aria-label", "Copy code");
    btn.textContent = "Copy";
    btn.addEventListener("click", (ev) => {
      ev.preventDefault();
      ev.stopPropagation();
      onCopyButtonClick(btn, pre);
    });
    pre.appendChild(btn);
  }
}

async function onCopyButtonClick(btn, pre) {
  const code = pre.querySelector(":scope > code");
  // textContent gives the raw source: syntect's <span> tokens flatten back to
  // the original characters, and the newlines between them are real text nodes.
  const text = (code || pre).textContent || "";
  await copyText(text);
  btn.textContent = "Copied";
  btn.classList.add("ok");
  setTimeout(() => {
    btn.textContent = "Copy";
    btn.classList.remove("ok");
  }, 1200);
}

/* ---- Link handling ---- */

const URL_SCHEME = /^[a-z][a-z0-9+.\-]*:/i;

function isExternalUrl(href) {
  return URL_SCHEME.test(href);
}

function annotateLinks() {
  for (const a of preview.querySelectorAll("a[href]")) {
    const href = a.getAttribute("href");
    if (!href) continue;
    if (isExternalUrl(href)) {
      a.title = `${href}\n(⌘-click to open in browser)`;
    } else {
      a.title = href;
    }
  }
}

/** Asset-protocol URL for a local (relative or absolute) image path, resolved
 *  against the document's directory. Returns null for remote/data/already-asset
 *  srcs, which the WebView loads as-is. */
function localImageUrl(baseDir, src) {
  if (!src || isExternalUrl(src)) return null;
  return convertFileSrc(resolveRelative(baseDir, src));
}

/** Rewrite local <img> sources to asset:// URLs so the WebView can load files
 *  next to the document (a bare relative/absolute path is not fetchable from
 *  the tauri://localhost origin). */
function resolveImages(baseDir) {
  // Files outside the opened workspace must never be embedded into an export
  // (a malicious doc could reference ~/.ssh/id_rsa). Use the tree root as the
  // boundary so in-project images still inline; fall back to the document's own
  // directory when no folder is open.
  const boundary = treeRoot || baseDir;
  for (const img of preview.querySelectorAll("img[src]")) {
    const rawSrc = img.getAttribute("src");
    const url = localImageUrl(baseDir, rawSrc);
    if (!url) continue;
    img.src = url;
    const resolved = resolveRelative(baseDir, rawSrc);
    img.dataset.localPath = resolved;
    if (isPathInsideDir(resolved, boundary)) {
      delete img.dataset.exportBlocked;
    } else {
      img.dataset.exportBlocked = "1";
    }
  }
}

/** Strip the `src` from any local image that escapes `boundary`, so export
 *  never embeds (HTML) or renders (PDF) a file outside the opened workspace.
 *  `data-export-blocked` is a fast textual first pass; `path_within_dir` (Rust)
 *  is the authoritative check — it canonicalizes, so an in-workspace symlink
 *  pointing at e.g. ~/.ssh/id_rsa is caught too. */
async function neutralizeOutsideWorkspaceImages(root, boundary) {
  await Promise.all(
    [...root.querySelectorAll("img")].map(async (img) => {
      const local = img.dataset.localPath;
      if (!local) return; // remote / data: image — not a local file
      if (img.dataset.exportBlocked === "1") {
        img.removeAttribute("src");
        return;
      }
      const ok = await invoke("path_within_dir", {
        path: local,
        dir: boundary,
      }).catch(() => false);
      if (!ok) img.removeAttribute("src");
    }),
  );
}

/** Resolve a markdown link's href against the active tab's directory. */
function resolveRelative(baseDir, href) {
  let p = href.split("#")[0].split("?")[0];
  try {
    p = decodeURIComponent(p);
  } catch (_) {}
  if (p.startsWith("/")) return p;
  const segs = baseDir.split("/").filter(Boolean);
  for (const seg of p.split("/")) {
    if (seg === "" || seg === ".") continue;
    if (seg === "..") segs.pop();
    else segs.push(seg);
  }
  return "/" + segs.join("/");
}

preview.addEventListener("click", async (ev) => {
  const a = ev.target.closest("a[href]");
  if (!a || !preview.contains(a)) return;
  const href = a.getAttribute("href");
  if (!href) return;

  // Always intercept; default would navigate the WebView and destroy app state.
  ev.preventDefault();

  // Fragment-only: scroll within the current document.
  if (href.startsWith("#")) {
    let target = null;
    try {
      target = preview.querySelector(href);
    } catch (_) {}
    if (!target) {
      // Fragments emitted by comrak's header_id_prefix look like "md-h-foo".
      target = preview.querySelector(`[id="${CSS.escape(href.slice(1))}"]`);
    }
    if (target) target.scrollIntoView({ behavior: "smooth", block: "start" });
    return;
  }

  // Absolute URL (http, https, mailto, ftp, etc.): require ⌘ to open.
  if (isExternalUrl(href)) {
    if (ev.metaKey || ev.ctrlKey) {
      try {
        await invoke("open_url", { url: href });
      } catch (e) {
        console.error("open_url failed", e);
      }
    }
    return;
  }

  // Relative path: resolve against the active tab's directory.
  const tab = activeTab();
  if (!tab) return;
  const resolved = resolveRelative(parentDir(tab.path), href);

  if (MD_EXT.test(resolved)) {
    if (ev.metaKey || ev.ctrlKey) {
      await openSticky(resolved);
    } else {
      await openPreview(resolved);
    }
  } else if (ev.metaKey || ev.ctrlKey) {
    try {
      await invoke("open_path", { path: resolved });
    } catch (e) {
      console.error("open_path failed", e);
    }
  }
  // Plain click on a non-markdown relative path: no-op (tooltip shows path).
});

function showError(msg) {
  preview.hidden = false;
  previewEmpty.hidden = true;
  preview.replaceChildren();
  const div = document.createElement("div");
  div.style.color = "#cf222e";
  div.style.padding = "12px 16px";
  div.textContent = "Failed to render: " + msg;
  preview.appendChild(div);
}

/* ---- Scroll anchoring across live reloads ---- */

function parseStartLine(pos) {
  if (!pos) return null;
  const m = /^(\d+):/.exec(pos);
  return m ? parseInt(m[1], 10) : null;
}

function captureAnchor() {
  const paneRect = previewScroll.getBoundingClientRect();
  const nodes = preview.querySelectorAll("[data-sourcepos]");
  for (const node of nodes) {
    const rect = node.getBoundingClientRect();
    if (rect.bottom >= paneRect.top + 2) {
      const line = parseStartLine(node.getAttribute("data-sourcepos"));
      if (line != null) {
        return { line, offset: rect.top - paneRect.top };
      }
    }
  }
  return null;
}

function restoreAnchor(anchor) {
  const nodes = preview.querySelectorAll("[data-sourcepos]");
  let best = null;
  let bestDelta = Infinity;
  for (const node of nodes) {
    const line = parseStartLine(node.getAttribute("data-sourcepos"));
    if (line == null) continue;
    const delta = Math.abs(line - anchor.line);
    if (delta < bestDelta) {
      bestDelta = delta;
      best = node;
    }
  }
  if (!best) return;
  const paneRect = previewScroll.getBoundingClientRect();
  const targetRect = best.getBoundingClientRect();
  const correction = targetRect.top - paneRect.top - anchor.offset;
  previewScroll.scrollTop += correction;
}

/* ---- Edit actions (shared by Edit menu and right-click menu) ---- */

async function actionCopySelection() {
  await copyText(selectedText());
}

async function actionCopySource() {
  const t = activeTab();
  if (!t) return;
  try {
    const src = await invoke("read_source", { path: t.path });
    await copyText(src);
  } catch (e) {
    console.error("read_source failed", e);
  }
}

async function runEditAction(name) {
  const t = activeTab();
  if (
    t &&
    isImagePath(t.path) &&
    (name === "copy-source" || name === "toggle-raw" || name === "toggle-edit")
  ) {
    showTransientError("Not available for images.");
    return;
  }
  switch (name) {
    case "copy":
      await actionCopySelection();
      break;
    case "copy-source":
      await actionCopySource();
      break;
    case "toggle-raw":
      onToggleRaw();
      break;
    case "toggle-edit":
      await onToggleEdit();
      break;
    case "save":
      await saveActive();
      break;
    case "find":
      openFind();
      break;
    case "search-files":
      if (treeRoot) enterSearchMode(treeRoot, { treeRoot });
      break;
  }
}

/* ---- Custom context menu ---- */

const ctxMenu = document.createElement("div");
ctxMenu.className = "ctx-menu";
ctxMenu.hidden = true;
document.body.appendChild(ctxMenu);

function hideContextMenu() {
  ctxMenu.hidden = true;
  ctxMenu.replaceChildren();
}

function buildContextMenu(items, x, y) {
  ctxMenu.replaceChildren();
  for (const item of items) {
    if (item === "---") {
      const sep = document.createElement("div");
      sep.className = "ctx-separator";
      ctxMenu.appendChild(sep);
      continue;
    }
    const el = document.createElement("div");
    el.className = "ctx-item";
    if (item.disabled) el.classList.add("disabled");
    const label = document.createElement("span");
    label.textContent = item.label;
    el.appendChild(label);
    if (item.shortcut) {
      const sc = document.createElement("span");
      sc.className = "ctx-shortcut";
      sc.textContent = item.shortcut;
      el.appendChild(sc);
    }
    if (!item.disabled) {
      el.addEventListener("mousedown", (ev) => {
        ev.preventDefault();
        hideContextMenu();
        Promise.resolve().then(item.action).catch((e) => console.error(e));
      });
    }
    ctxMenu.appendChild(el);
  }
  ctxMenu.hidden = false;
  ctxMenu.style.left = "0px";
  ctxMenu.style.top = "0px";
  const w = ctxMenu.offsetWidth;
  const h = ctxMenu.offsetHeight;
  const clampedX = Math.min(x, window.innerWidth - w - 4);
  const clampedY = Math.min(y, window.innerHeight - h - 4);
  ctxMenu.style.left = Math.max(2, clampedX) + "px";
  ctxMenu.style.top = Math.max(2, clampedY) + "px";
}

function selectedText() {
  const sel = window.getSelection();
  return sel ? sel.toString() : "";
}

async function copyText(text) {
  if (!text) return;
  try {
    await navigator.clipboard.writeText(text);
  } catch (e) {
    console.error("clipboard write failed", e);
  }
}

function relativeToRoot(path, root) {
  if (!root) return path;
  if (path === root) return "";
  const sep = root.endsWith("/") || root.endsWith("\\") ? "" : "/";
  const prefix = root + sep;
  if (path.startsWith(prefix)) return path.slice(prefix.length);
  return path;
}

document.addEventListener("contextmenu", (ev) => {
  ev.preventDefault();
  const items = [];
  const text = selectedText();
  const tab = activeTab();

  const treeRow =
    ev.target instanceof Element
      ? ev.target.closest("li[data-path]")
      : null;
  if (treeRow && tree.contains(treeRow)) {
    const absolute = treeRow.dataset.path;
    const isDir = treeRow.dataset.isDir === "1";
    const relative = relativeToRoot(absolute, treeRoot);
    const dir = dirForNewEntry(treeRow);
    const depth = depthOf(treeRow) + (isDir ? 1 : 0);

    if (isDir) {
      items.push({
        label: "Search in Folder…",
        action: () => enterSearchMode(absolute, { treeRoot }),
      });
      items.push("---");
    }
    items.push({
      label: "New File…",
      action: async () => {
        if (isDir) await ensureExpanded(treeRow);
        await createTreeEntry(newEntryContainer(treeRow, isDir), dir, depth, false);
      },
    });
    items.push({
      label: "New Folder…",
      action: async () => {
        if (isDir) await ensureExpanded(treeRow);
        await createTreeEntry(newEntryContainer(treeRow, isDir), dir, depth, true);
      },
    });
    items.push("---");
    items.push({ label: "Rename…", action: () => renameTreeEntry(treeRow) });
    if (!isDir) {
      items.push({ label: "Duplicate", action: () => duplicateTreeEntry(treeRow) });
    }
    items.push({ label: "Delete", action: () => deleteTreeEntry(treeRow) });
    items.push("---");
    items.push({
      label: "Copy Relative Path",
      action: () => copyText(relative),
      disabled: !relative,
    });
    items.push({ label: "Copy Absolute Path", action: () => copyText(absolute) });
    buildContextMenu(items, ev.clientX, ev.clientY);
    return;
  }

  // Right-click on the sidebar background (header, padding, empty area below
  // the tree) → offer to create files/folders and search the entire open tree.
  const sidebar =
    ev.target instanceof Element ? ev.target.closest("#sidebar") : null;
  if (sidebar && treeRoot) {
    items.push({
      label: "New File…",
      action: () => createTreeEntry(tree, treeRoot, 1, false),
    });
    items.push({
      label: "New Folder…",
      action: () => createTreeEntry(tree, treeRoot, 1, true),
    });
    items.push("---");
    items.push({
      label: "Search in Folder…",
      action: () => enterSearchMode(treeRoot, { treeRoot }),
    });
    buildContextMenu(items, ev.clientX, ev.clientY);
    return;
  }

  if (text) {
    items.push({
      label: "Copy",
      shortcut: "⌘C",
      action: actionCopySelection,
    });
  }

  if (tab) {
    items.push({
      label: "Copy Source",
      action: actionCopySource,
    });
    if (items.length) items.push("---");
    items.push({
      label: tab.raw ? "Show Rendered" : "Show Raw",
      action: onToggleRaw,
    });
  }

  if (items.length === 0) return;
  buildContextMenu(items, ev.clientX, ev.clientY);
});

document.addEventListener("mousedown", (ev) => {
  if (!ctxMenu.hidden && !ctxMenu.contains(ev.target)) hideContextMenu();
});
document.addEventListener("keydown", (ev) => {
  if (ev.key === "Escape") hideContextMenu();
});
window.addEventListener("blur", hideContextMenu);
window.addEventListener("resize", hideContextMenu);
previewScroll.addEventListener("scroll", hideContextMenu);

/* ---- Resizable splitter ---- */
(() => {
  let dragging = false;
  splitter.addEventListener("mousedown", (e) => {
    dragging = true;
    splitter.classList.add("dragging");
    e.preventDefault();
  });
  window.addEventListener("mousemove", (e) => {
    if (!dragging) return;
    const min = 160;
    const max = Math.max(min + 100, window.innerWidth - 200);
    const w = Math.min(max, Math.max(min, e.clientX));
    document.documentElement.style.setProperty("--sidebar-width", `${w}px`);
  });
  window.addEventListener("mouseup", () => {
    if (dragging) {
      dragging = false;
      splitter.classList.remove("dragging");
    }
  });
})();

/* ---- Editor splitter ---- */
(() => {
  let dragging = false;
  editorSplitter.addEventListener("mousedown", (e) => {
    dragging = true;
    editorSplitter.classList.add("dragging");
    e.preventDefault();
  });
  window.addEventListener("mousemove", (e) => {
    if (!dragging) return;
    const rect = paneBody.getBoundingClientRect();
    const min = 200;
    const max = Math.max(min + 100, rect.width - 200);
    const w = Math.min(max, Math.max(min, e.clientX - rect.left));
    document.documentElement.style.setProperty("--editor-width", `${w}px`);
    if (cm) cm.refresh();
  });
  window.addEventListener("mouseup", () => {
    if (dragging) {
      dragging = false;
      editorSplitter.classList.remove("dragging");
    }
  });
})();

/* ---- In-document find ---- */

const findBar = document.getElementById("find-bar");
const findInput = document.getElementById("find-input");
const findCount = document.getElementById("find-count");
const findCaseBtn = document.getElementById("find-case");
const findWordBtn = document.getElementById("find-word");
const findPrevBtn = document.getElementById("find-prev");
const findNextBtn = document.getElementById("find-next");
const findCloseBtn = document.getElementById("find-close");

const HIGHLIGHT_SUPPORTED =
  typeof CSS !== "undefined" &&
  CSS.highlights &&
  typeof Highlight !== "undefined";

const findState = {
  caseSensitive: false,
  wholeWord: false,
  matches: [], // Range[]
  current: -1,
};

function findOpen() {
  return !findBar.hidden;
}

function openFind() {
  if (!activeTab()) return;
  const sel = selectedText();
  if (sel && sel.length <= 200 && !sel.includes("\n")) {
    findInput.value = sel;
  }
  findBar.hidden = false;
  findInput.focus();
  findInput.select();
  runFind({ keepCurrent: false });
}

function closeFind() {
  findBar.hidden = true;
  clearFindHighlights();
  findState.matches = [];
  findState.current = -1;
}

function clearFindHighlights() {
  if (!HIGHLIGHT_SUPPORTED) return;
  CSS.highlights.delete("search-match");
  CSS.highlights.delete("search-current");
}

/** Flatten the preview's text into one string plus an offset→node map,
 *  skipping rendered mermaid diagrams. */
function collectFindSegments() {
  const walker = document.createTreeWalker(preview, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      if (!node.nodeValue) return NodeFilter.FILTER_REJECT;
      const parent = node.parentElement;
      if (!parent) return NodeFilter.FILTER_ACCEPT;
      if (parent.closest("pre.mermaid")) return NodeFilter.FILTER_REJECT;
      if (parent.closest("[data-math-style]")) return NodeFilter.FILTER_REJECT;
      return NodeFilter.FILTER_ACCEPT;
    },
  });
  let text = "";
  const segs = []; // { node, start } — start is the offset of node within text
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    segs.push({ node: n, start: text.length });
    text += n.nodeValue;
  }
  return { text, segs };
}

/** Map a global text offset to its containing node and local offset. */
function locateFindOffset(segs, offset) {
  let lo = 0;
  let hi = segs.length - 1;
  let found = 0;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (segs[mid].start <= offset) {
      found = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  const seg = segs[found];
  return { node: seg.node, offset: offset - seg.start };
}

function findRangeFor(segs, start, end) {
  const a = locateFindOffset(segs, start);
  const b = locateFindOffset(segs, end);
  const range = document.createRange();
  range.setStart(a.node, a.offset);
  range.setEnd(b.node, b.offset);
  return range;
}

function runFind({ keepCurrent = true, scroll = true } = {}) {
  const query = findInput.value;
  const { text, segs } = collectFindSegments();
  const spans = segs.length
    ? findMatches(text, query, {
        caseSensitive: findState.caseSensitive,
        wholeWord: findState.wholeWord,
      })
    : [];
  const prev = keepCurrent ? findState.current : -1;
  findState.matches = spans.map(([s, e]) => findRangeFor(segs, s, e));
  if (findState.matches.length === 0) {
    findState.current = -1;
  } else {
    findState.current = Math.min(
      Math.max(prev, 0),
      findState.matches.length - 1,
    );
  }
  paintFindHighlights();
  updateFindCount(query);
  if (scroll && findState.current >= 0) scrollToFindCurrent();
}

function paintFindHighlights() {
  if (!HIGHLIGHT_SUPPORTED) return;
  CSS.highlights.delete("search-match");
  CSS.highlights.delete("search-current");
  if (findState.matches.length === 0) return;
  CSS.highlights.set("search-match", new Highlight(...findState.matches));
  if (findState.current >= 0) {
    const cur = new Highlight(findState.matches[findState.current]);
    cur.priority = 1;
    CSS.highlights.set("search-current", cur);
  }
}

function updateFindCount(query) {
  const n = findState.matches.length;
  if (!query) {
    findCount.textContent = "";
    findBar.classList.remove("no-match");
    return;
  }
  if (n === 0) {
    findCount.textContent = "No results";
    findBar.classList.add("no-match");
    return;
  }
  findBar.classList.remove("no-match");
  findCount.textContent = `${findState.current + 1} / ${n}`;
}

function scrollToFindCurrent() {
  const range = findState.matches[findState.current];
  if (!range) return;
  const rect = range.getBoundingClientRect();
  const paneRect = previewScroll.getBoundingClientRect();
  if (rect.top < paneRect.top || rect.bottom > paneRect.bottom) {
    const target =
      previewScroll.scrollTop +
      (rect.top - paneRect.top) -
      paneRect.height / 3;
    previewScroll.scrollTop = Math.max(0, target);
  }
}

function findStep(delta) {
  const n = findState.matches.length;
  if (n === 0) return;
  findState.current = (findState.current + delta + n) % n;
  paintFindHighlights();
  updateFindCount(findInput.value);
  scrollToFindCurrent();
}

function toggleFindOption(key, btn) {
  findState[key] = !findState[key];
  btn.setAttribute("aria-pressed", findState[key] ? "true" : "false");
  runFind({ keepCurrent: false });
  findInput.focus();
}

findInput.addEventListener("input", () => runFind({ keepCurrent: false }));
findCaseBtn.addEventListener("click", () =>
  toggleFindOption("caseSensitive", findCaseBtn),
);
findWordBtn.addEventListener("click", () =>
  toggleFindOption("wholeWord", findWordBtn),
);
findPrevBtn.addEventListener("click", () => findStep(-1));
findNextBtn.addEventListener("click", () => findStep(1));
findCloseBtn.addEventListener("click", () => closeFind());

findInput.addEventListener("keydown", (ev) => {
  if (ev.key === "Enter") {
    ev.preventDefault();
    findStep(ev.shiftKey ? -1 : 1);
  }
});

// ⌘G / ⇧⌘G navigate and Esc closes while the bar is open. (⌘F is delivered by
// the native Find… menu accelerator, not here.)
document.addEventListener("keydown", (ev) => {
  if (!findOpen()) return;
  const meta = ev.metaKey || ev.ctrlKey;
  if (meta && (ev.key === "g" || ev.key === "G")) {
    ev.preventDefault();
    findStep(ev.shiftKey ? -1 : 1);
  } else if (ev.key === "Escape") {
    ev.preventDefault();
    closeFind();
  }
});

/* ---- Update check & auto-update ---- */

const REPO = "larsakeekstrand/mdviewer";
const DISMISS_KEY = "mdviewer.update.dismissed_version";
const UPDATE_CHECK_INTERVAL_MS = 60 * 60 * 1000;
/** Wrap the metadata returned by the `check_update` command into the same
 *  surface the banner already consumes. `downloadAndInstall` reuses the updater
 *  plugin's own command via the resource id. */
function wrapUpdate(meta) {
  if (!meta) return null;
  return {
    version: meta.version,
    currentVersion: meta.currentVersion,
    body: meta.body,
    async downloadAndInstall(onEvent) {
      const channel = new window.__TAURI__.core.Channel();
      if (onEvent) channel.onmessage = onEvent;
      await invoke("plugin:updater|download_and_install", {
        rid: meta.rid,
        onEvent: channel,
      });
    },
  };
}
let updateInProgress = false;

const updateBanner = document.getElementById("update-banner");
const updateBannerText = document.getElementById("update-banner-text");
const updateBannerUpdate = document.getElementById("update-banner-update");
const updateBannerRestart = document.getElementById("update-banner-restart");
const updateBannerWhatsNew = document.getElementById("update-banner-whatsnew");
const updateBannerDismiss = document.getElementById("update-banner-dismiss");

const notesModal = document.getElementById("notes-modal");
const notesDialog = document.getElementById("notes-dialog");
const notesModalTitle = document.getElementById("notes-modal-title");
const notesModalBody = document.getElementById("notes-modal-body");
const notesModalLink = document.getElementById("notes-modal-link");
const notesModalUpdate = document.getElementById("notes-modal-update");
const notesModalClose = document.getElementById("notes-modal-close");
const notesModalX = document.getElementById("notes-modal-x");
let notesModalTrigger = null;

function setUpdateButtons({
  update = false,
  restart = false,
  whatsNew = false,
  dismiss = false,
} = {}) {
  updateBannerUpdate.hidden = !update;
  updateBannerRestart.hidden = !restart;
  updateBannerWhatsNew.hidden = !whatsNew;
  updateBannerDismiss.hidden = !dismiss;
}

function openNotesModal(update) {
  notesModalTrigger = document.activeElement;
  notesModalTitle.textContent = `What's new in ${update.version}`;
  notesModalLink.href = releaseUrlFor(REPO, update.version);

  notesModalUpdate.onclick = () => {
    closeNotesModal();
    runUpdate(update);
  };

  const md = extractChangelog(update.body);
  if (!md) {
    notesModalBody.textContent = "No release notes available.";
    revealNotesModal();
    return;
  }
  notesModalBody.textContent = "Loading…";
  revealNotesModal();
  invoke("render_notes", { source: md, theme: currentTheme })
    .then((html) => {
      const doc = new DOMParser().parseFromString(html, "text/html");
      notesModalBody.replaceChildren(...doc.body.childNodes);
    })
    .catch((e) => {
      console.error("render_notes failed", e);
      notesModalBody.textContent = md;
    });
}

function revealNotesModal() {
  notesModal.hidden = false;
  notesModalX.focus();
}

function closeNotesModal() {
  notesModal.hidden = true;
  if (notesModalTrigger && typeof notesModalTrigger.focus === "function") {
    notesModalTrigger.focus();
  }
  notesModalTrigger = null;
}

notesModalClose.addEventListener("click", closeNotesModal);
notesModalX.addEventListener("click", closeNotesModal);
notesModal.addEventListener("click", (ev) => {
  if (ev.target === notesModal) closeNotesModal();
});
document.addEventListener("keydown", (ev) => {
  if (ev.key === "Escape" && !notesModal.hidden) closeNotesModal();
});
notesDialog.addEventListener("click", (ev) => {
  const a = ev.target.closest("a[href]");
  if (!a || !notesDialog.contains(a)) return;
  ev.preventDefault();
  const href = a.getAttribute("href");
  if (href && isExternalUrl(href)) {
    invoke("open_url", { url: href }).catch((e) =>
      console.error("open_url failed", e),
    );
  }
});

async function checkForUpdates({ silent = true } = {}) {
  if (updateInProgress) return;
  let update;
  try {
    update = wrapUpdate(await invoke("check_update"));
  } catch (e) {
    if (silent) {
      // No published release yet, network error, etc.
      console.debug("update check skipped:", e);
      return;
    }
    await dialogApi.message("Couldn't check for updates.\n\n" + e, {
      title: "MDViewer",
      kind: "error",
    });
    return;
  }

  if (update) {
    if (silent) {
      let dismissed = null;
      try {
        dismissed = localStorage.getItem(DISMISS_KEY);
      } catch (_) {}
      if (dismissed === update.version) return;
    }
    showUpdateAvailable(update);
    return;
  }

  if (!silent) {
    await dialogApi.message("You're on the latest version.", {
      title: "MDViewer",
      kind: "info",
    });
  }
}

async function installCli() {
  let outcome;
  try {
    outcome = await invoke("install_cli");
  } catch (e) {
    await dialogApi.message("Couldn't install the command line tool.\n\n" + e, {
      title: "MDViewer",
      kind: "error",
    });
    return;
  }
  if (outcome === "cancelled") return;
  const msg =
    outcome === "already_installed"
      ? "The mdviewer command line tool is already installed."
      : "Installed. You can now run mdviewer from a terminal.";
  await dialogApi.message(msg, { title: "MDViewer", kind: "info" });
}

function showUpdateAvailable(update) {
  updateBannerText.textContent = bannerMessage(
    update.version,
    update.currentVersion,
  );
  setUpdateButtons({ update: true, whatsNew: true, dismiss: true });

  updateBannerUpdate.onclick = () => runUpdate(update);
  updateBannerWhatsNew.onclick = () => openNotesModal(update);
  updateBannerDismiss.onclick = () => {
    try {
      localStorage.setItem(DISMISS_KEY, update.version);
    } catch (_) {}
    updateBanner.hidden = true;
  };

  updateBanner.hidden = false;
}

async function runUpdate(update) {
  updateInProgress = true;
  setUpdateButtons();
  let downloaded = 0;
  let contentLength = 0;
  updateBannerText.textContent = "Downloading…";

  try {
    await update.downloadAndInstall((event) => {
      switch (event.event) {
        case "Started":
          contentLength = event.data.contentLength ?? 0;
          break;
        case "Progress":
          downloaded += event.data.chunkLength;
          updateBannerText.textContent = progressText(downloaded, contentLength);
          break;
        case "Finished":
          updateBannerText.textContent = "Installing…";
          break;
      }
    });
  } catch (e) {
    updateInProgress = false;
    console.error("update failed", e);
    updateBannerText.textContent = "Update failed: " + e;
    setUpdateButtons({ whatsNew: true, dismiss: true });
    updateBannerWhatsNew.onclick = () => openNotesModal(update);
    updateBannerDismiss.onclick = () => {
      updateBanner.hidden = true;
    };
    return;
  }

  updateBannerText.textContent = "Update installed.";
  setUpdateButtons({ restart: true });
  updateBannerRestart.onclick = async () => {
    try {
      await invoke("restart");
    } catch (e) {
      console.error("restart failed", e);
    }
  };
}

init()
  .then(() => {
    // Fire-and-forget — the check runs in the background and won't block
    // anything in init. Silent if no update or if the network call fails.
    checkForUpdates();
    setInterval(() => checkForUpdates(), UPDATE_CHECK_INTERVAL_MS);
  })
  .catch((e) => {
    console.error("init failed", e);
    document.body.innerText = "Failed to start: " + e;
  });
