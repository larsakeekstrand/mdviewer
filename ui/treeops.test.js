import { test } from "node:test";
import assert from "node:assert/strict";
import { validateName } from "./treeops.js";

test("validateName accepts ordinary names", () => {
  assert.equal(validateName("notes.md"), null);
  assert.equal(validateName(".gitignore"), null);
  assert.equal(validateName("My File 2.txt"), null);
});

test("validateName returns an error string for bad names", () => {
  assert.match(validateName(""), /empty/);
  assert.match(validateName("   "), /empty/);
  assert.match(validateName("."), /invalid/i);
  assert.match(validateName(".."), /invalid/i);
  assert.match(validateName("a/b"), /separator/);
  assert.match(validateName("a\\b"), /separator/);
});
