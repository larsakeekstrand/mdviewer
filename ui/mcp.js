// Pure helpers for the MCP review loop. DOM-free + Tauri-free so they
// unit-test under `node --test`; event wiring lives in app.js.

/** Toolbar label: Send for an MCP-initiated review, Copy for a manual one. */
export function reviewButtonLabel(reviewMode, mcpRequestId) {
  if (!reviewMode) return "💬 Review";
  return mcpRequestId != null ? "✓ Finish & Send" : "✓ Finish & Copy";
}

/** Review-bar hint while Claude waits, with its optional instructions. */
export function mcpHintText(fileName, instructions) {
  const base = `Claude is waiting for your review of ${fileName}`;
  const extra = (instructions || "").trim();
  if (!extra) return `${base}.`;
  // em-dash and curly quotes
  return `${base} \u2014 \u201c${extra}\u201d`;
}

/** True when an incoming request_review must be rejected: some tab is already
 *  reviewing (manual outranks the agent) or another MCP review is pending. */
export function reviewBusy(tabs) {
  return tabs.some((t) => !!t.reviewMode || t.mcpRequestId != null);
}

/** What get_viewer_state reports. */
export function viewerState(tabs, activeIdx) {
  const t = activeIdx >= 0 && activeIdx < tabs.length ? tabs[activeIdx] : null;
  return {
    path: t ? t.path : null,
    reviewing: tabs.some((x) => !!x.reviewMode),
  };
}
