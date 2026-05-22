import { test } from "node:test";
import assert from "node:assert/strict";
import { findMatches, isWordChar } from "./search.js";

test("returns no matches for an empty query", () => {
  assert.deepEqual(findMatches("hello", ""), []);
});

test("finds multiple occurrences in document order", () => {
  assert.deepEqual(findMatches("hello world hello", "hello"), [
    [0, 5],
    [12, 17],
  ]);
});

test("is case-insensitive by default", () => {
  assert.deepEqual(findMatches("Hello HELLO", "hello"), [
    [0, 5],
    [6, 11],
  ]);
});

test("respects the case-sensitive option", () => {
  assert.deepEqual(
    findMatches("Hello hello", "hello", { caseSensitive: true }),
    [[6, 11]],
  );
});

test("whole-word skips substrings inside larger words", () => {
  assert.deepEqual(findMatches("cat category cat", "cat", { wholeWord: true }), [
    [0, 3],
    [13, 16],
  ]);
});

test("whole-word treats punctuation as a boundary", () => {
  assert.deepEqual(findMatches("(cat)", "cat", { wholeWord: true }), [[1, 4]]);
});

test("whole-word treats underscore as part of the word", () => {
  assert.deepEqual(findMatches("cat_x cat", "cat", { wholeWord: true }), [
    [6, 9],
  ]);
});

test("returns non-overlapping matches", () => {
  assert.deepEqual(findMatches("aaaa", "aa"), [
    [0, 2],
    [2, 4],
  ]);
});

test("isWordChar recognizes letters, digits, and underscore", () => {
  assert.equal(isWordChar("a"), true);
  assert.equal(isWordChar("7"), true);
  assert.equal(isWordChar("_"), true);
  assert.equal(isWordChar(" "), false);
  assert.equal(isWordChar(null), false);
});
