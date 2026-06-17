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
  tableStyleCss,
  tableFitCss,
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

test("clampBaseSize exact boundaries", () => {
  assert.equal(clampBaseSize(9), 9);
  assert.equal(clampBaseSize(16), 16);
});

test("clampBaseSize NaN/non-finite falls back to defaultSettings().baseSize", () => {
  const fallback = defaultSettings().baseSize;
  assert.equal(clampBaseSize(NaN), fallback);
  assert.equal(clampBaseSize("abc"), fallback);
  assert.equal(clampBaseSize(Infinity), fallback);
  assert.equal(clampBaseSize(-Infinity), fallback);
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

test("presetDefaults includes the new table + orientation keys", () => {
  const s = presetDefaults("clean");
  for (const k of ["tableStyle", "tableFit", "orientation"]) {
    assert.ok(k in s, `missing ${k}`);
  }
});

test("defaults are editorial / wrap / portrait", () => {
  const s = defaultSettings();
  assert.equal(s.tableStyle, "editorial");
  assert.equal(s.tableFit, "wrap");
  assert.equal(s.orientation, "portrait");
});

test("editorial style: bounding rules, no zebra, no full grid", () => {
  const css = settingsToCss(presetDefaults("clean")); // clean => editorial
  assert.match(css, /\.markdown-body table\s*\{[^}]*border-top:\s*2px solid/);
  assert.match(css, /tr:nth-child\(2n\)\s*\{\s*background-color:\s*transparent/);
});

test("grid style keeps the accent header tint", () => {
  const css = settingsToCss(mergeSettings(defaultSettings(), { tableStyle: "grid" }));
  assert.match(css, /table th\s*\{\s*background:\s*color-mix\(in srgb, var\(--pdf-accent\)/);
});

test("minimal style underlines the header but draws no table top rule", () => {
  const css = settingsToCss(mergeSettings(defaultSettings(), { tableStyle: "minimal" }));
  assert.match(css, /table th\s*\{[^}]*border-bottom:\s*2px solid/);
  assert.doesNotMatch(css, /\.markdown-body table\s*\{[^}]*border-top:\s*2px solid/);
});

test("minimal keeps github zebra (no nth-child suppression) but editorial removes it", () => {
  const minimal = settingsToCss(mergeSettings(defaultSettings(), { tableStyle: "minimal" }));
  const editorial = settingsToCss(mergeSettings(defaultSettings(), { tableStyle: "editorial" }));
  assert.doesNotMatch(minimal, /nth-child\(2n\)[^}]*transparent/);
  assert.match(editorial, /nth-child\(2n\)\s*\{\s*background-color:\s*transparent/);
});

test("tableFitCss emits wrap layout only in wrap mode", () => {
  const wrap = tableFitCss(mergeSettings(defaultSettings(), { tableFit: "wrap" }));
  assert.match(wrap, /display:\s*table/);
  assert.match(wrap, /table-layout:\s*fixed/);
  assert.match(wrap, /overflow-wrap:\s*anywhere/);
  assert.equal(tableFitCss(mergeSettings(defaultSettings(), { tableFit: "fit" })), "");
});
