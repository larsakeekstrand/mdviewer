import { test } from "node:test";
import assert from "node:assert/strict";
import { modeForPath } from "./editor-modes.js";

test("modeForPath maps code extensions to CodeMirror modes", () => {
  assert.equal(modeForPath("main.rs"), "rust");
  assert.equal(modeForPath("app.tsx"), "javascript");
  assert.equal(modeForPath("data.json"), "javascript");
  assert.equal(modeForPath("script.py"), "python");
  assert.equal(modeForPath("style.css"), "css");
  assert.equal(modeForPath("page.html"), "htmlmixed");
  assert.equal(modeForPath("a.b.c.rs"), "rust");
  assert.equal(modeForPath("readme.md"), "markdown");
});

test("modeForPath returns null for unknown or extensionless paths", () => {
  assert.equal(modeForPath("notes.xyz"), null);
  assert.equal(modeForPath("Makefile"), null);
  assert.equal(modeForPath(""), null);
  assert.equal(modeForPath(null), null);
});
