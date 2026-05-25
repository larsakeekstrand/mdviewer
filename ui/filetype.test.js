import { test } from "node:test";
import assert from "node:assert/strict";
import { isImagePath } from "./filetype.js";

test("isImagePath matches common image extensions, case-insensitively", () => {
  for (const p of [
    "a.png",
    "a.jpg",
    "a.jpeg",
    "a.gif",
    "a.webp",
    "a.avif",
    "a.bmp",
    "a.ico",
    "a.svg",
    "/some/dir/PHOTO.JPG",
    "C:\\pics\\Logo.SVG",
  ]) {
    assert.equal(isImagePath(p), true, `${p} should be an image`);
  }
});

test("isImagePath rejects non-image paths", () => {
  for (const p of [
    "a.md",
    "a.markdown",
    "a.txt",
    "README",
    "notes.png.md",
    "png",
    "a.pngx",
    "archive.tar.gz",
  ]) {
    assert.equal(isImagePath(p), false, `${p} should not be an image`);
  }
});

test("isImagePath handles empty/nullish input", () => {
  assert.equal(isImagePath(""), false);
  assert.equal(isImagePath(null), false);
  assert.equal(isImagePath(undefined), false);
});
