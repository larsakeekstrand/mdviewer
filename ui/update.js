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

/** The release-notes body is the full GitHub release text (install steps,
 *  quarantine note, an Updating section, then a "## Changes" changelog). For
 *  the in-app "What's new" modal we want only the changelog. Returns the text
 *  after the "## Changes" heading (heading dropped, trimmed); falls back to the
 *  whole body when that heading is absent; "" for empty input. */
export function extractChangelog(body) {
  if (!body) return "";
  const lines = body.split("\n");
  const idx = lines.findIndex((l) => /^##\s+Changes\s*$/.test(l));
  if (idx === -1) return body.trim();
  return lines.slice(idx + 1).join("\n").trim();
}

/** Extract one version's notes from a Keep-a-Changelog `CHANGELOG.md` body.
 *  `version` is the semver without a leading `v` (e.g. "1.16.0"). Matches a
 *  heading `## [<version>]` (a trailing ` - DATE` and surrounding whitespace
 *  are tolerated) and returns the lines up to the next `## ` heading, trimmed.
 *  Returns "" when no matching section exists (caller decides the fallback). */
export function changelogSection(text, version) {
  if (!text || !version) return "";
  const escaped = version.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const heading = new RegExp(`^##\\s+\\[${escaped}\\]`);
  const lines = text.split("\n");
  const start = lines.findIndex((l) => heading.test(l));
  if (start === -1) return "";
  const rest = lines.slice(start + 1);
  const end = rest.findIndex((l) => /^##\s/.test(l));
  return (end === -1 ? rest : rest.slice(0, end)).join("\n").trim();
}
