import { test } from "node:test";
import assert from "node:assert/strict";
import { quoteBlock, formatReview, reanchorReviews } from "./review.js";

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

test("formatReview emits header, general note, divider, and ordered comments", () => {
  const reviews = [
    { sourcepos: "58:1-58:40", quotedText: "Wire the toolbar button before the command.", comment: "do this after the command wiring" },
    { sourcepos: "42:1-42:45", quotedText: "store bookmarks in localStorage keyed by path", comment: "use recent.json, not localStorage" },
  ];
  const out = formatReview(reviews, "This plan never says where bookmarks persist.", "docs/x.md", []);
  assert.equal(
    out,
    "Review of docs/x.md\n" +
    "\n" +
    "General note: This plan never says where bookmarks persist.\n" +
    "\n" +
    "---\n" +
    "\n" +
    "> store bookmarks in localStorage keyed by path\n" +
    "↳ use recent.json, not localStorage\n" +
    "\n" +
    "> Wire the toolbar button before the command.\n" +
    "↳ do this after the command wiring\n",
  );
});

test("formatReview omits the general-note line and divider when note is blank", () => {
  const out = formatReview(
    [{ sourcepos: "3:1-3:5", quotedText: "hello", comment: "fix" }],
    "   ",
    "a.md",
    [],
  );
  assert.equal(out, "Review of a.md\n\n> hello\n↳ fix\n");
});

test("formatReview omits the divider when a general note has no block comments", () => {
  const out = formatReview([], "Just a thought.", "a.md", []);
  assert.equal(out, "Review of a.md\n\nGeneral note: Just a thought.\n");
});

test("formatReview lists orphaned comments first with a changed tag", () => {
  const out = formatReview(
    [{ sourcepos: "10:1-10:5", quotedText: "still here", comment: "keep" }],
    "",
    "a.md",
    [{ quotedText: "was here", comment: "this is gone now" }],
  );
  assert.equal(
    out,
    "Review of a.md\n\n" +
    "> was here  ⚠ this block changed\n↳ this is gone now\n\n" +
    "> still here\n↳ keep\n",
  );
});

test("reanchorReviews refreshes sourcepos for matched blocks", () => {
  const reviews = [{ sourcepos: "42:1-42:9", quotedText: "hello world", comment: "c" }];
  const newBlocks = [
    { sourcepos: "1:1-1:5", text: "intro" },
    { sourcepos: "52:1-52:9", text: "hello world" },
  ];
  const { anchored, orphaned } = reanchorReviews(reviews, newBlocks);
  assert.equal(orphaned.length, 0);
  assert.equal(anchored.length, 1);
  assert.equal(anchored[0].sourcepos, "52:1-52:9");
  assert.equal(anchored[0].comment, "c");
});

test("reanchorReviews orphans a comment whose block text is gone", () => {
  const reviews = [{ sourcepos: "42:1-42:9", quotedText: "was here", comment: "c" }];
  const { anchored, orphaned } = reanchorReviews(reviews, [{ sourcepos: "1:1-1:3", text: "new" }]);
  assert.equal(anchored.length, 0);
  assert.deepEqual(orphaned, [{ quotedText: "was here", comment: "c" }]);
});

test("reanchorReviews matches the first block when text repeats", () => {
  const reviews = [{ sourcepos: "9:1-9:3", quotedText: "dup", comment: "c" }];
  const newBlocks = [
    { sourcepos: "2:1-2:3", text: "dup" },
    { sourcepos: "8:1-8:3", text: "dup" },
  ];
  const { anchored } = reanchorReviews(reviews, newBlocks);
  assert.equal(anchored[0].sourcepos, "2:1-2:3");
});
