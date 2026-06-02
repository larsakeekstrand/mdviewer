import { test } from "node:test";
import assert from "node:assert/strict";
import { classifyFileChange, isDirty } from "./editor.js";

test("not editing → always a plain reload", () => {
  assert.equal(
    classifyFileChange({ editing: false, dirty: false, diskContent: "x", savedContent: "y" }),
    "reload",
  );
});

test("editing, disk equals what we saved → our own write (ignore)", () => {
  assert.equal(
    classifyFileChange({ editing: true, dirty: true, diskContent: "v2", savedContent: "v2" }),
    "self",
  );
});

test("editing, external change, no unsaved edits → reload", () => {
  assert.equal(
    classifyFileChange({ editing: true, dirty: false, diskContent: "v2", savedContent: "v1" }),
    "reload",
  );
});

test("editing, external change, unsaved edits → conflict", () => {
  assert.equal(
    classifyFileChange({ editing: true, dirty: true, diskContent: "v2", savedContent: "v1" }),
    "conflict",
  );
});

test("isDirty compares buffer to saved", () => {
  assert.equal(isDirty("a", "a"), false);
  assert.equal(isDirty("a", "b"), true);
});
