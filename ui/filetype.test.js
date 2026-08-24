import { test } from "node:test";
import assert from "node:assert/strict";
import {
  viewKind,
  isEditable,
  hasSplitPreview,
  hasRawToggle,
  isAnnotatable,
  isRetainable,
  isExportable,
  rendersFromDisk,
  bustsCacheOnChange,
} from "./filetype.js";

test("viewKind recognizes every image extension, case-insensitively", () => {
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
    assert.equal(viewKind(p), "image", `${p} should be an image`);
  }
});

test("viewKind recognizes every markdown extension, case-insensitively", () => {
  for (const p of ["a.md", "A.MARKDOWN", "x.mdown", "y.mkd", "z.mkdn"]) {
    assert.equal(viewKind(p), "markdown", p);
  }
});

test("viewKind only matches a family at the end of the name", () => {
  // A near-miss extension must degrade to code, and an image-looking stem with
  // a markdown extension is markdown — the suffix decides, not the substring.
  assert.equal(viewKind("notes.png.md"), "markdown");
  assert.equal(viewKind("a.pngx"), "code");
  assert.equal(viewKind("a.pdfx"), "code");
  assert.equal(viewKind("a.xlsxx"), "code");
  assert.equal(viewKind("png"), "code");
  assert.equal(viewKind("README"), "code");
});

test("viewKind resolves each family, case-insensitively", () => {
  assert.equal(viewKind("notes.md"), "markdown");
  assert.equal(viewKind("A.MARKDOWN"), "markdown");
  assert.equal(viewKind("pic.PNG"), "image");
  assert.equal(viewKind("/docs/report.pdf"), "pdf");
  assert.equal(viewKind("C:\\books\\Manual.PDF"), "pdf");
  assert.equal(viewKind("budget.xlsx"), "sheet");
  assert.equal(viewKind("legacy.xls"), "sheet");
  assert.equal(viewKind("macro.xlsm"), "sheet");
  assert.equal(viewKind("binary.xlsb"), "sheet");
  assert.equal(viewKind("open.ods"), "sheet");
  assert.equal(viewKind("main.rs"), "code");
});

test("viewKind falls back to code for anything unrecognized", () => {
  for (const p of ["Makefile", "a.pngx", "notes.pdf.txt", "archive.tar.gz", ""]) {
    assert.equal(viewKind(p), "code", p);
  }
});

test("viewKind handles nullish input without throwing", () => {
  assert.equal(viewKind(null), "code");
  assert.equal(viewKind(undefined), "code");
});

test("capability table: exact rows for all five kinds", () => {
  const table = {
    //          edit  split  raw   annot retain export disk  bust
    markdown: [true, true, true, true, true, true, false, false],
    code: [true, false, false, false, true, false, false, false],
    image: [false, false, false, false, false, false, true, true],
    pdf: [false, false, false, false, false, false, true, true],
    sheet: [false, false, false, false, true, true, false, false],
  };
  const fns = [
    isEditable,
    hasSplitPreview,
    hasRawToggle,
    isAnnotatable,
    isRetainable,
    isExportable,
    rendersFromDisk,
    bustsCacheOnChange,
  ];
  for (const [kind, expected] of Object.entries(table)) {
    expected.forEach((want, i) => {
      assert.equal(fns[i](kind), want, `${fns[i].name}("${kind}")`);
    });
  }
});

test("predicates treat an unknown kind as code, not as a crash", () => {
  assert.equal(isEditable("nonsense"), true);
  assert.equal(isExportable("nonsense"), false);
  assert.equal(rendersFromDisk(undefined), false);
});
