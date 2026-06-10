import { test } from "node:test";
import assert from "node:assert/strict";
import { validateName, treeAncestors } from "./treeops.js";

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

test("treeAncestors lists ancestor dirs top-down between root and file", () => {
  assert.deepEqual(treeAncestors("/r", "/r/a/b/c.md"), ["/r/a", "/r/a/b"]);
});

test("treeAncestors returns [] for a file directly in root", () => {
  assert.deepEqual(treeAncestors("/r", "/r/x.md"), []);
});

test("treeAncestors returns null for a file outside root", () => {
  assert.equal(treeAncestors("/r", "/other/x.md"), null);
  assert.equal(treeAncestors("/r", "/r2/x.md"), null); // not fooled by a prefix
});

test("treeAncestors returns null when path equals root", () => {
  assert.equal(treeAncestors("/r", "/r"), null);
});

test("treeAncestors tolerates a trailing separator on root", () => {
  assert.deepEqual(treeAncestors("/r/", "/r/a/x.md"), ["/r/a"]);
});

test("treeAncestors handles Windows separators", () => {
  assert.deepEqual(treeAncestors("C:\\r", "C:\\r\\a\\x.md"), ["C:\\r\\a"]);
});
