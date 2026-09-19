import { test } from "node:test";
import assert from "node:assert/strict";
import { anyDirty, themeFromStorageEvent } from "./windowscope.js";

const valid = (v) => v === "light" || v === "dark";

test("anyDirty is true only when some tab is dirty", () => {
  assert.equal(anyDirty([]), false);
  assert.equal(anyDirty([{ dirty: false }, { dirty: true }]), true);
  assert.equal(anyDirty([{ dirty: false }]), false);
});

test("themeFromStorageEvent returns the new theme for our key only", () => {
  assert.equal(themeFromStorageEvent({ key: "mdviewer.theme", newValue: "dark" }, "mdviewer.theme", valid), "dark");
  assert.equal(themeFromStorageEvent({ key: "other", newValue: "dark" }, "mdviewer.theme", valid), null);
  assert.equal(themeFromStorageEvent({ key: "mdviewer.theme", newValue: "purple" }, "mdviewer.theme", valid), null);
  assert.equal(themeFromStorageEvent({ key: "mdviewer.theme", newValue: null }, "mdviewer.theme", valid), null);
});
