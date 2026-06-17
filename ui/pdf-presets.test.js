import test from "node:test";
import assert from "node:assert/strict";
import {
  PRESETS,
  presetIds,
  presetDefaults,
  defaultSettings,
  mergeSettings,
  clampBaseSize,
  marginMm,
  paperMm,
  settingsToCss,
} from "./pdf-presets.js";

test("presetIds are the three documented presets", () => {
  assert.deepEqual(presetIds().sort(), ["clean", "compact", "report"]);
});

test("presetDefaults returns a complete settings object tagged with its id", () => {
  const s = presetDefaults("compact");
  assert.equal(s.preset, "compact");
  for (const k of ["preset", "baseSize", "paper", "margins", "pageNumbers"]) {
    assert.ok(k in s, `missing ${k}`);
  }
});

test("presetDefaults falls back to clean for unknown ids", () => {
  assert.deepEqual(presetDefaults("nope"), presetDefaults("clean"));
});

test("defaultSettings is the clean preset", () => {
  assert.deepEqual(defaultSettings(), presetDefaults("clean"));
});

test("compact uses a smaller base size than clean", () => {
  assert.ok(presetDefaults("compact").baseSize < presetDefaults("clean").baseSize);
});

test("mergeSettings overrides win, nullish ignored", () => {
  const base = presetDefaults("clean");
  const out = mergeSettings(base, { baseSize: 13, paper: undefined, margins: null });
  assert.equal(out.baseSize, 13);
  assert.equal(out.paper, base.paper);
  assert.equal(out.margins, base.margins);
});

test("clampBaseSize clamps to 9..16", () => {
  assert.equal(clampBaseSize(2), 9);
  assert.equal(clampBaseSize(99), 16);
  assert.equal(clampBaseSize(12), 12);
});

test("marginMm: wide > normal > narrow uniformly", () => {
  assert.ok(marginMm("wide").top > marginMm("normal").top);
  assert.ok(marginMm("normal").top > marginMm("narrow").top);
  const n = marginMm("normal");
  assert.equal(n.top, n.bottom);
  assert.equal(n.left, n.right);
});

test("marginMm unknown falls back to normal", () => {
  assert.deepEqual(marginMm("???"), marginMm("normal"));
});

test("paperMm portrait dimensions", () => {
  assert.deepEqual(paperMm("a4"), { w: 210, h: 297 });
  assert.deepEqual(paperMm("letter"), { w: 215.9, h: 279.4 });
});

test("settingsToCss is scoped to .markdown-body and reflects base size", () => {
  const css = settingsToCss(mergeSettings(defaultSettings(), { baseSize: 13 }));
  assert.match(css, /\.markdown-body\s*\{/);
  assert.match(css, /font-size:\s*13pt/);
});

test("settingsToCss justifies only the report preset", () => {
  assert.match(settingsToCss(presetDefaults("report")), /text-align:\s*justify/);
  assert.doesNotMatch(settingsToCss(presetDefaults("clean")), /text-align:\s*justify/);
});
