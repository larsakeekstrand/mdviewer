import {
  PRESETS, presetIds, presetDefaults, defaultSettings,
  mergeSettings, clampBaseSize,
} from "./pdf-presets.js";
import { THEME_KEY, resolveTheme } from "./theme.js";

const { invoke } = window.__TAURI__.core;
const { emit, listen } = window.__TAURI__.event;
const { save } = window.__TAURI__.dialog;

const el = (id) => document.getElementById(id);

// Match the window chrome to the app's theme (the PDF preview itself stays a
// light document — it's what the exported PDF looks like). localStorage is
// shared across same-origin windows, so a theme toggle in the main window
// fires a `storage` event here.
function applyWindowTheme() {
  const os = window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
  document.documentElement.dataset.theme = resolveTheme(
    localStorage.getItem(THEME_KEY),
    os,
  );
}
applyWindowTheme();
window.addEventListener("storage", (e) => {
  if (e.key === THEME_KEY) applyWindowTheme();
});
window
  .matchMedia("(prefers-color-scheme: dark)")
  .addEventListener("change", applyWindowTheme);

let settings = defaultSettings();
let previewTimer = null;
let pending = null; // "save" | "exact"
let activeName = "export.pdf";

function fillPresetOptions() {
  for (const id of presetIds()) {
    const opt = document.createElement("option");
    opt.value = id;
    opt.textContent = PRESETS[id].label;
    el("preset").appendChild(opt);
  }
}

function reflect() {
  el("preset").value = settings.preset;
  el("base-size").value = settings.baseSize;
  el("size-val").textContent = `${settings.baseSize}pt`;
  el("paper").value = settings.paper;
  el("margins").value = settings.margins;
  el("page-numbers").value = settings.pageNumbers;
}

function update(overrides) {
  settings = mergeSettings(settings, overrides);
  reflect();
  schedulePreview();
}

function schedulePreview() {
  clearTimeout(previewTimer);
  previewTimer = setTimeout(() => {
    emit("pdf-export-request-preview", { settings }).catch(() => {});
  }, 120);
}

el("preset").addEventListener("change", (e) => {
  settings = presetDefaults(e.target.value);
  reflect();
  schedulePreview();
});
el("base-size").addEventListener("input", (e) => {
  update({ baseSize: clampBaseSize(parseFloat(e.target.value)) });
});
el("paper").addEventListener("change", (e) => update({ paper: e.target.value }));
el("margins").addEventListener("change", (e) => update({ margins: e.target.value }));
el("page-numbers").addEventListener("change", (e) => update({ pageNumbers: e.target.value }));
el("reset").addEventListener("click", () => {
  settings = presetDefaults(settings.preset);
  reflect();
  schedulePreview();
});

function showExactTab() {
  el("tab-exact").classList.add("active");
  el("tab-live").classList.remove("active");
  el("exact-preview").hidden = false;
  el("live-preview").hidden = true;
}
function showLiveTab() {
  el("tab-live").classList.add("active");
  el("tab-exact").classList.remove("active");
  el("live-preview").hidden = false;
  el("exact-preview").hidden = true;
}
el("tab-live").addEventListener("click", showLiveTab);
el("tab-exact").addEventListener("click", () => {
  pending = "exact";
  el("status").textContent = "Rendering exact PDF…";
  emit("pdf-export-run", { settings, mode: "exact" }).catch(() => {});
});
el("export").addEventListener("click", async () => {
  const path = await save({
    defaultPath: activeName,
    filters: [{ name: "PDF document", extensions: ["pdf"] }],
  });
  if (!path) return;
  pending = "save";
  el("status").textContent = "Exporting…";
  await emit("pdf-export-run", { settings, mode: "save", path });
});

async function init() {
  await listen("pdf-export-preview-html", (ev) => {
    const { html, error } = ev.payload;
    if (error) {
      el("status").textContent = "Preview error: " + error;
      return;
    }
    el("live-preview").srcdoc = html;
    el("status").textContent = "";
  });
  await listen("pdf-export-active-name", (ev) => {
    if (ev.payload && ev.payload.name) activeName = ev.payload.name;
  });
  await listen("pdf-export-done", (ev) => {
    const { ok, url, error } = ev.payload;
    if (!ok) {
      el("status").textContent = "Export failed: " + (error || "");
      pending = null;
      return;
    }
    if (pending === "exact" && url) {
      el("exact-preview").src = url;
      showExactTab();
    } else if (pending === "save") {
      el("status").textContent = "Saved.";
    }
    pending = null;
  });
  fillPresetOptions();
  try {
    settings = mergeSettings(defaultSettings(), await invoke("get_pdf_settings"));
  } catch (e) {
    console.error("get_pdf_settings failed", e);
  }
  reflect();
  schedulePreview();
}
init().catch((e) => console.error("pdf-export init failed", e));
