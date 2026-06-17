import {
  PRESETS, presetIds, presetDefaults, defaultSettings,
  mergeSettings, clampBaseSize,
} from "./pdf-presets.js";

const { invoke } = window.__TAURI__.core;
const { emit } = window.__TAURI__.event;

const el = (id) => document.getElementById(id);
let settings = defaultSettings();
let previewTimer = null;

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

async function init() {
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
