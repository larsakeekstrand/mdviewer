import { test } from "node:test";
import assert from "node:assert/strict";
import { isImagePath, isMarkdownPath, isCodeView } from "./filetype.js";

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

test("isMarkdownPath true for markdown extensions", () => {
  for (const p of ["a.md", "A.MARKDOWN", "x.mdown", "y.mkd", "z.mkdn"]) {
    assert.equal(isMarkdownPath(p), true, p);
  }
});

test("isMarkdownPath false for non-markdown", () => {
  for (const p of ["main.rs", "pic.png", "Makefile", ""]) {
    assert.equal(isMarkdownPath(p), false, p);
  }
});

test("isCodeView is true only for non-markdown, non-image", () => {
  assert.equal(isCodeView("main.rs"), true);
  assert.equal(isCodeView("Makefile"), true);
  assert.equal(isCodeView("readme.md"), false);
  assert.equal(isCodeView("pic.png"), false);
});
