import { test } from "node:test";
import assert from "node:assert/strict";
import {
  THEME_KEY,
  isValidTheme,
  resolveTheme,
  nextTheme,
  themeButtonFace,
} from "./theme.js";

test("isValidTheme accepts only light and dark", () => {
  assert.equal(isValidTheme("light"), true);
  assert.equal(isValidTheme("dark"), true);
  assert.equal(isValidTheme("system"), false);
  assert.equal(isValidTheme(null), false);
  assert.equal(isValidTheme(""), false);
  assert.equal(isValidTheme(undefined), false);
});

test("resolveTheme prefers a valid stored value", () => {
  assert.equal(resolveTheme("dark", "light"), "dark");
  assert.equal(resolveTheme("light", "dark"), "light");
});

test("resolveTheme falls back to the OS theme when stored is missing/invalid", () => {
  assert.equal(resolveTheme(null, "dark"), "dark");
  assert.equal(resolveTheme("bogus", "light"), "light");
  assert.equal(resolveTheme(undefined, "dark"), "dark");
});

test("nextTheme flips between light and dark", () => {
  assert.equal(nextTheme("light"), "dark");
  assert.equal(nextTheme("dark"), "light");
});

test("themeButtonFace shows the target theme (action convention)", () => {
  assert.equal(themeButtonFace("light").icon, "☾");
  assert.equal(themeButtonFace("light").label, "Switch to dark theme");
  assert.equal(themeButtonFace("dark").icon, "☀");
  assert.equal(themeButtonFace("dark").label, "Switch to light theme");
});

test("THEME_KEY is namespaced", () => {
  assert.equal(THEME_KEY, "mdviewer.theme");
});
