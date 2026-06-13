// Claude Code Integration window. External module only (CSP script-src 'self').
import { statusButtonLabel, statusLabel } from "./integration.js";

const { invoke } = window.__TAURI__.core;

const projectEl = document.getElementById("project");
const noFolderEl = document.getElementById("no-folder");
const hookStatusEl = document.getElementById("hook-status");
const hookBtn = document.getElementById("hook-btn");
const mcpStatusEl = document.getElementById("mcp-status");
const mcpBtn = document.getElementById("mcp-btn");

function setRow(statusEl, btn, installed, disabled) {
  statusEl.textContent = statusLabel(installed);
  btn.textContent = statusButtonLabel(installed);
  btn.disabled = disabled;
}

async function load() {
  const s = await invoke("integration_status");
  if (!s.root) {
    projectEl.hidden = true;
    noFolderEl.hidden = false;
    setRow(hookStatusEl, hookBtn, false, true);
    setRow(mcpStatusEl, mcpBtn, false, true);
    return;
  }
  noFolderEl.hidden = true;
  projectEl.hidden = false;
  projectEl.textContent = `Project: ${s.root}`;
  setRow(hookStatusEl, hookBtn, s.hook, false);
  setRow(mcpStatusEl, mcpBtn, s.mcp, false);
}

async function runInstall(command, btn) {
  btn.disabled = true;
  try {
    await invoke(command);
  } catch (e) {
    console.error(command, "failed", e);
    btn.disabled = false;
    return;
  }
  // The Rust command emits integration-changed (the main window listens);
  // here we just refresh this window's own status + labels.
  await load();
}

hookBtn.addEventListener("click", () => runInstall("install_claude_hook", hookBtn));
mcpBtn.addEventListener("click", () => runInstall("install_mcp_server", mcpBtn));

load().catch((e) => console.error("integration status load failed", e));
