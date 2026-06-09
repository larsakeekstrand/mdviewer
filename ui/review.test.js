import { test } from "node:test";
import assert from "node:assert/strict";
import { quoteBlock } from "./review.js";

test("quoteBlock returns short text unchanged, trimmed", () => {
  assert.equal(quoteBlock("  store bookmarks in localStorage  "), "store bookmarks in localStorage");
});

test("quoteBlock collapses internal whitespace and newlines to single spaces", () => {
  assert.equal(quoteBlock("line one\n   line two"), "line one line two");
});

test("quoteBlock truncates long text with a trailing ellipsis", () => {
  const long = "a".repeat(100);
  const out = quoteBlock(long, 80);
  assert.equal(out.length, 82); // 80 chars + " …"
  assert.ok(out.endsWith(" …"));
});

test("quoteBlock handles empty/undefined input", () => {
  assert.equal(quoteBlock(""), "");
  assert.equal(quoteBlock(undefined), "");
});
