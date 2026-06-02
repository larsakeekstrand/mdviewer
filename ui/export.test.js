import { test } from "node:test";
import assert from "node:assert/strict";
import {
  baseName,
  exportFilename,
  documentNeedsKatex,
  inlineFontUrls,
  forceLightCss,
  buildHtmlDocument,
  isPathInsideDir,
} from "./export.js";

test("isPathInsideDir accepts the dir itself and nested paths", () => {
  assert.equal(isPathInsideDir("/work", "/work"), true);
  assert.equal(isPathInsideDir("/work/doc/img.png", "/work"), true);
  assert.equal(isPathInsideDir("/work/assets/logo.svg", "/work"), true);
});

test("isPathInsideDir rejects paths outside the dir", () => {
  assert.equal(isPathInsideDir("/etc/passwd", "/work"), false);
  assert.equal(isPathInsideDir("/Users/me/.ssh/id_rsa", "/work"), false);
  // Prefix collision must not count as inside.
  assert.equal(isPathInsideDir("/work-secret/x", "/work"), false);
});

test("isPathInsideDir collapses .. so escapes can't slip through", () => {
  assert.equal(isPathInsideDir("/work/../etc/passwd", "/work"), false);
  assert.equal(isPathInsideDir("/work/sub/../img.png", "/work"), true);
});

test("isPathInsideDir is false on empty inputs", () => {
  assert.equal(isPathInsideDir("", "/work"), false);
  assert.equal(isPathInsideDir("/work/x", ""), false);
});

test("baseName returns the final path segment", () => {
  assert.equal(baseName("/a/b/README.md"), "README.md");
  assert.equal(baseName("README.md"), "README.md");
  assert.equal(baseName("/a/b/"), "/a/b/"); // trailing slash → fall back to input
});

test("exportFilename replaces the last extension", () => {
  assert.equal(exportFilename("/a/b/README.md", "html"), "README.html");
  assert.equal(exportFilename("/a/notes.tar.gz", "pdf"), "notes.tar.pdf");
});

test("exportFilename appends when there is no extension", () => {
  assert.equal(exportFilename("/a/Makefile", "html"), "Makefile.html");
});

test("exportFilename keeps a leading-dot name intact", () => {
  assert.equal(exportFilename("/a/.env", "html"), ".env.html");
});

test("documentNeedsKatex detects rendered KaTeX markup", () => {
  assert.equal(documentNeedsKatex('<span class="katex">x</span>'), true);
  assert.equal(
    documentNeedsKatex('<span class="katex-display"><span class="katex">y</span></span>'),
    true,
  );
});

test("documentNeedsKatex is false without math", () => {
  assert.equal(documentNeedsKatex("<p>no math here</p>"), false);
  assert.equal(documentNeedsKatex(""), false);
});

test("inlineFontUrls replaces mapped url() references", () => {
  const css =
    "@font-face{src:url(fonts/A.woff2) format('woff2'),url(fonts/A.woff) format('woff')}";
  const out = inlineFontUrls(css, { "fonts/A.woff2": "data:font/woff2;base64,XX" });
  assert.ok(out.includes("url(data:font/woff2;base64,XX)"));
  assert.ok(out.includes("url(fonts/A.woff)")); // unmapped refs untouched
});

test("inlineFontUrls handles multiple distinct fonts", () => {
  const css = "url(fonts/A.woff2) url(fonts/B.woff2)";
  const out = inlineFontUrls(css, {
    "fonts/A.woff2": "data:font/woff2;base64,AA",
    "fonts/B.woff2": "data:font/woff2;base64,BB",
  });
  assert.equal(out, "url(data:font/woff2;base64,AA) url(data:font/woff2;base64,BB)");
});

test("forceLightCss removes dark blocks and unwraps light blocks", () => {
  const css = [
    ".a{color:red}",
    "@media (prefers-color-scheme: dark){ .a{color:white} }",
    "@media (prefers-color-scheme: light){ .a{--x: black} }",
    ".b{margin:0}",
  ].join("\n");
  const out = forceLightCss(css);
  assert.ok(!out.includes("color:white"), "dark rules removed");
  assert.ok(out.includes("--x: black"), "light rules kept");
  assert.ok(!out.includes("prefers-color-scheme"), "no media wrappers remain");
  assert.ok(out.includes(".a{color:red}"));
  assert.ok(out.includes(".b{margin:0}"));
});

test("forceLightCss ignores braces inside strings and comments", () => {
  const css =
    "@media (prefers-color-scheme: dark){ .a::before{content:'{'} /* } */ .b{x:1} }\n.c{y:2}";
  const out = forceLightCss(css);
  assert.ok(!out.includes("x:1"), "whole dark block removed despite stray braces");
  assert.ok(out.includes(".c{y:2}"), "content after the block survives");
});

test("forceLightCss tolerates whitespace variations", () => {
  const css = "@media(prefers-color-scheme:light){.a{color:green}}";
  const out = forceLightCss(css);
  assert.ok(out.includes(".a{color:green}"));
  assert.ok(!out.includes("prefers-color-scheme"));
});

test("forceLightCss is a no-op on CSS with no color-scheme blocks", () => {
  const css = ".a{color:red} .b{margin:0}";
  assert.equal(forceLightCss(css), css);
});

test("buildHtmlDocument wraps body and inlines css", () => {
  const doc = buildHtmlDocument({ title: "T", css: ".a{}", bodyHtml: "<p>x</p>" });
  assert.ok(doc.startsWith("<!doctype html>"));
  assert.ok(doc.includes("<title>T</title>"));
  assert.ok(doc.includes('name="color-scheme" content="light"'));
  assert.ok(doc.includes("<style>.a{}</style>"));
  assert.ok(doc.includes('<article class="markdown-body">'));
  assert.ok(doc.includes("<p>x</p>"));
});

test("buildHtmlDocument escapes the title", () => {
  const doc = buildHtmlDocument({ title: "a<b>&c", css: "", bodyHtml: "" });
  assert.ok(doc.includes("<title>a&lt;b&gt;&amp;c</title>"));
});
