// Pure helpers for the auto-update banner. No DOM or Tauri imports, so this
// runs under `node --test` as well as in the WebView (mirrors search.js /
// export.js).

/** GitHub release page URL for a version tag (tags are `v`-prefixed). */
export function releaseUrlFor(repo, version) {
  return `https://github.com/${repo}/releases/tag/v${version}`;
}

/** Banner headline for an available update. `currentVersion` may be undefined
 *  (the updater can omit it); fall back to a shorter sentence. */
export function bannerMessage(version, currentVersion) {
  return currentVersion
    ? `MDViewer ${version} is available — you have ${currentVersion}.`
    : `MDViewer ${version} is available.`;
}

/** Whole-percent download progress, or null when the total size is unknown
 *  (the updater reports contentLength 0/undefined for chunked responses). */
export function progressPercent(downloaded, contentLength) {
  if (!contentLength || contentLength <= 0) return null;
  const pct = Math.round((downloaded / contentLength) * 100);
  return Math.min(100, Math.max(0, pct));
}

/** Progress label for the banner; degrades gracefully without a total. */
export function progressText(downloaded, contentLength) {
  const pct = progressPercent(downloaded, contentLength);
  return pct === null ? "Downloading…" : `Downloading… ${pct}%`;
}
