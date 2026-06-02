import { test } from "node:test";
import assert from "node:assert/strict";
import {
  releaseUrlFor,
  bannerMessage,
  progressPercent,
  progressText,
  extractChangelog,
  changelogSection,
} from "./update.js";

test("releaseUrlFor builds a v-prefixed tag URL", () => {
  assert.equal(
    releaseUrlFor("larsakeekstrand/mdviewer", "1.5.0"),
    "https://github.com/larsakeekstrand/mdviewer/releases/tag/v1.5.0",
  );
});

test("bannerMessage includes both versions when current is known", () => {
  assert.equal(
    bannerMessage("1.5.0", "1.4.0"),
    "MDViewer 1.5.0 is available — you have 1.4.0.",
  );
});

test("bannerMessage omits the current version when undefined", () => {
  assert.equal(
    bannerMessage("1.5.0", undefined),
    "MDViewer 1.5.0 is available.",
  );
});

test("progressPercent rounds and clamps", () => {
  assert.equal(progressPercent(50, 200), 25);
  assert.equal(progressPercent(199, 200), 100); // 99.5 rounds up
  assert.equal(progressPercent(300, 200), 100); // clamp to 100
});

test("progressPercent returns null when total unknown", () => {
  assert.equal(progressPercent(100, 0), null);
  assert.equal(progressPercent(100, undefined), null);
});

test("progressText degrades without a total", () => {
  assert.equal(progressText(50, 200), "Downloading… 25%");
  assert.equal(progressText(50, 0), "Downloading…");
});

test("progressPercent returns null for negative totals", () => {
  assert.equal(progressPercent(10, -5), null);
});

test("bannerMessage omits the current version when null", () => {
  assert.equal(bannerMessage("1.5.0", null), "MDViewer 1.5.0 is available.");
});

test("progressText clamps to 100%", () => {
  assert.equal(progressText(300, 200), "Downloading… 100%");
});

test("extractChangelog returns the section after '## Changes'", () => {
  const body = "## Install\n\nblah\n\n## Changes\n\n- a (h1)\n- b (h2)\n";
  assert.equal(extractChangelog(body), "- a (h1)\n- b (h2)");
});

test("extractChangelog falls back to the full body when '## Changes' is absent", () => {
  const body = "# Notes\n\n- only this\n";
  assert.equal(extractChangelog(body), "# Notes\n\n- only this");
});

test("extractChangelog returns empty string for empty/null/undefined", () => {
  assert.equal(extractChangelog(""), "");
  assert.equal(extractChangelog(null), "");
  assert.equal(extractChangelog(undefined), "");
});

test("extractChangelog trims surrounding whitespace", () => {
  assert.equal(extractChangelog("## Changes\n\n\n- only\n\n\n"), "- only");
});

const SAMPLE = `# Changelog

## [1.16.0] - 2026-06-02

- Folder-wide search across the open tree
- Fixed a crash when exporting docs with broken images

## [1.15.0] - 2026-05-31

- Earlier feature
`;

test("changelogSection returns only the matching version's bullets", () => {
  assert.equal(
    changelogSection(SAMPLE, "1.16.0"),
    "- Folder-wide search across the open tree\n- Fixed a crash when exporting docs with broken images",
  );
});

test("changelogSection stops at the next version heading", () => {
  assert.equal(changelogSection(SAMPLE, "1.15.0"), "- Earlier feature");
});

test("changelogSection returns '' for a missing version", () => {
  assert.equal(changelogSection(SAMPLE, "9.9.9"), "");
});

test("changelogSection returns '' for a prerelease with no entry", () => {
  assert.equal(changelogSection(SAMPLE, "1.16.0-rc.1"), "");
});

test("changelogSection tolerates a heading with no date suffix", () => {
  const text = "## [2.0.0]\n\n- New thing\n";
  assert.equal(changelogSection(text, "2.0.0"), "- New thing");
});

test("changelogSection returns '' for empty input", () => {
  assert.equal(changelogSection("", "1.0.0"), "");
});

test("changelogSection selects the right section regardless of order", () => {
  const text =
    "# Changelog\n\n## [1.15.0] - 2026-05-31\n\n- Older\n\n## [1.16.0] - 2026-06-02\n\n- Newer\n";
  assert.equal(changelogSection(text, "1.16.0"), "- Newer");
  assert.equal(changelogSection(text, "1.15.0"), "- Older");
});
