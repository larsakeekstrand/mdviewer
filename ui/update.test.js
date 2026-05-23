import { test } from "node:test";
import assert from "node:assert/strict";
import {
  releaseUrlFor,
  bannerMessage,
  progressPercent,
  progressText,
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
