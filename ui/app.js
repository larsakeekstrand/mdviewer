// mdviewer frontend
// Uses Tauri v2 IPC; window.__TAURI__ is injected because tauri.conf.json sets withGlobalTauri.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const dialogApi = window.__TAURI__.dialog;

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
const splitter = document.getElementById("splitter");

let treeRoot = null;
let currentTheme = colorScheme();
const childCache = new Map();

// Tabs model
const tabs = []; // [{ path, sticky, raw }]
let activeIdx = -1;

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
  initMermaid();
  const initial = await invoke("get_initial_state");

  // Register listeners before the readiness handshake so a file opened the
  // instant the app becomes ready isn't missed.
  await listen("file-changed", async (ev) => {
    const tab = activeTab();
    if (tab && ev.payload === tab.path) {
      await renderActive({ scrollLock: true });
    }
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

  await listen("menu-check-updates", async () => {
    await checkForUpdates({ silent: false });
  });

  window
    .matchMedia("(prefers-color-scheme: dark)")
    .addEventListener("change", async () => {
      currentTheme = colorScheme();
      initMermaid();
      if (activeTab())
        await renderActive({ scrollLock: false, forceMermaid: true });
    });

  rawBtn.addEventListener("click", onToggleRaw);

  // Drain files Finder buffered during a cold launch; afterwards, files opened
  // while running arrive live via the "open-file" listener above.
  let pending = [];
  try {
    pending = await invoke("frontend_ready");
  } catch (e) {
    console.error("frontend_ready failed", e);
  }

  // A cold Finder launch (no argv file) starts the sidebar at the file's folder.
  treeRoot =
    !initial.initial_file && pending.length
      ? parentDir(pending[0])
      : initial.tree_root;
  treeTitle.textContent = basename(treeRoot) || treeRoot;
  treeTitle.title = treeRoot;

  await renderRoot();

  if (initial.initial_file) await openSticky(initial.initial_file);
  for (const p of pending) await openSticky(p);
}

/* ---- Tree ---- */

async function renderRoot() {
  tree.replaceChildren();
  const children = await listDir(treeRoot);
  for (const entry of children) {
    tree.appendChild(makeNode(entry, 1));
  }
}

async function setTreeRoot(path) {
  treeRoot = path;
  treeTitle.textContent = basename(path) || path;
  treeTitle.title = path;
  childCache.clear();
  await renderRoot();
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
    return;
  }
  row.classList.add("open");
  let children;
  try {
    children = await listDir(entry.path);
  } catch (e) {
    console.error("list_dir failed", e);
    return;
  }
  const ul = document.createElement("ul");
  for (const child of children) {
    ul.appendChild(makeNode(child, depth + 1));
  }
  li.appendChild(ul);
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
    await setActiveTab(previewIdx, { forceRender: true });
    return;
  }
  tabs.push({ path, sticky: false, raw: false });
  await setActiveTab(tabs.length - 1);
}

async function openSticky(path) {
  const existing = findTab(path);
  if (existing !== -1) {
    tabs[existing].sticky = true;
    await setActiveTab(existing);
    return;
  }
  tabs.push({ path, sticky: true, raw: false });
  await setActiveTab(tabs.length - 1);
}

async function setActiveTab(idx, { forceRender = false } = {}) {
  if (idx < 0 || idx >= tabs.length) {
    activeIdx = -1;
    renderTabBar();
    showEmptyState();
    return;
  }
  const same = idx === activeIdx;
  activeIdx = idx;
  renderTabBar();
  highlightSelectedByPath(tabs[idx].path);
  try {
    await invoke("open_file", { path: tabs[idx].path });
  } catch (e) {
    console.warn("open_file failed", e);
  }
  await renderActive({ scrollLock: same && !forceRender });
}

function makeStickyAt(idx) {
  if (idx < 0 || idx >= tabs.length) return;
  if (tabs[idx].sticky) return;
  tabs[idx].sticky = true;
  renderTabBar();
}

function closeTab(idx) {
  if (idx < 0 || idx >= tabs.length) return;
  tabs.splice(idx, 1);
  if (tabs.length === 0) {
    activeIdx = -1;
    renderTabBar();
    showEmptyState();
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
    rawBtn.textContent = t.raw ? "Rendered" : "Raw";
    rawBtn.setAttribute("aria-pressed", t.raw ? "true" : "false");
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

/* ---- Rendering ---- */

function showEmptyState() {
  preview.hidden = true;
  previewEmpty.hidden = false;
  preview.replaceChildren();
  preview.classList.remove("raw-body");
}

async function renderActive(
  { scrollLock = true, forceMermaid = false } = {},
) {
  const t = activeTab();
  if (!t) {
    showEmptyState();
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

  previewEmpty.hidden = true;
  preview.hidden = false;
  preview.classList.toggle("raw-body", result.raw);

  const anchor = scrollLock ? captureAnchor() : null;

  const incoming = document.createElement("article");
  incoming.className = "markdown-body" + (result.raw ? " raw-body" : "");
  incoming.id = "preview";
  incoming.innerHTML = result.html;

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
      return !fromEl.isEqualNode(toEl);
    },
  });

  annotateLinks();

  if (!result.raw) await renderMermaid({ force: forceMermaid });

  if (anchor) restoreAnchor(anchor);
  else previewScroll.scrollTop = 0;
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

document.addEventListener("contextmenu", (ev) => {
  ev.preventDefault();
  const items = [];
  const text = selectedText();
  const tab = activeTab();

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

/* ---- Update check ---- */

const DISMISS_KEY = "mdviewer.update.dismissed_version";
const updateBanner = document.getElementById("update-banner");
const updateBannerText = document.getElementById("update-banner-text");
const updateBannerView = document.getElementById("update-banner-view");
const updateBannerDismiss = document.getElementById("update-banner-dismiss");

async function checkForUpdates({ silent = true } = {}) {
  let info;
  try {
    info = await invoke("check_for_updates");
  } catch (e) {
    if (silent) {
      // 404 (no published releases yet), network error, etc.
      console.debug("update check skipped:", e);
      return;
    }
    await dialogApi.message("Couldn't check for updates.\n\n" + e, {
      title: "MDViewer",
      kind: "error",
    });
    return;
  }

  if (info && info.has_update) {
    if (silent) {
      let dismissed = null;
      try {
        dismissed = localStorage.getItem(DISMISS_KEY);
      } catch (_) {}
      if (dismissed === info.latest_version) return;
    }
    showUpdateBanner(info);
    return;
  }

  if (!silent) {
    const current = (info && info.current_version) || "this version";
    await dialogApi.message(
      `You're on version ${current}. This is the latest release.`,
      { title: "MDViewer", kind: "info" },
    );
  }
}

function showUpdateBanner(info) {
  updateBannerText.textContent =
    `MDViewer ${info.latest_version} is available — you have ${info.current_version}.`;

  updateBannerView.onclick = async () => {
    try {
      await invoke("open_url", { url: info.release_url });
    } catch (e) {
      console.error("open_url failed", e);
    }
  };
  updateBannerDismiss.onclick = () => {
    try {
      localStorage.setItem(DISMISS_KEY, info.latest_version);
    } catch (_) {}
    updateBanner.hidden = true;
  };

  updateBanner.hidden = false;
}

init()
  .then(() => {
    // Fire-and-forget — the check runs in the background and won't block
    // anything in init. Silent if no update or if the network call fails.
    checkForUpdates();
  })
  .catch((e) => {
    console.error("init failed", e);
    document.body.innerText = "Failed to start: " + e;
  });
