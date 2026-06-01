// Settings window logic. External module only (CSP script-src 'self').
const { invoke } = window.__TAURI__.core;

const toggle = document.getElementById("beta-toggle");
const versionEl = document.getElementById("version");

async function load() {
  const prefs = await invoke("get_preferences");
  toggle.checked = prefs.channel === "beta";
  versionEl.textContent = `Current version: ${prefs.version}`;
}

toggle.addEventListener("change", async () => {
  const channel = toggle.checked ? "beta" : "stable";
  try {
    await invoke("set_update_channel", { channel });
  } catch (e) {
    console.error("set_update_channel failed", e);
    // Revert the visual state so it reflects what was actually persisted.
    toggle.checked = !toggle.checked;
  }
});

load().catch((e) => console.error("loading preferences failed", e));
