// Pure helpers for the Claude Code integration window + first-run nudge.
// DOM-free + Tauri-free so they unit-test under `node --test`.

/** Button label for an install row: Update when present, else Install. */
export function statusButtonLabel(installed) {
  return installed ? "Update" : "Install";
}

/** Status text for an install row. */
export function statusLabel(installed) {
  return installed ? "Installed" : "Not installed";
}

/** Whether to show the first-run nudge: only in a git project where neither
 *  piece is installed and the user hasn't permanently dismissed it. */
export function shouldNudge(isGitRepo, hookInstalled, mcpInstalled, dismissed) {
  return isGitRepo && !hookInstalled && !mcpInstalled && !dismissed;
}
