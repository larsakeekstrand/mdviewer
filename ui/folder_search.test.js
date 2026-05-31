import { test } from "node:test";
import assert from "node:assert/strict";
import {
  derivePanelState,
  groupResults,
  truncateLineText,
} from "./folder_search.js";

test("derivePanelState reports the empty-query hint when query is blank", () => {
  const s = derivePanelState({ query: "", results: null, error: null, busy: false });
  assert.equal(s.kind, "hint");
  assert.equal(s.message, "Type to search");
});

test("derivePanelState requires at least 2 characters", () => {
  const s = derivePanelState({ query: "a", results: null, error: null, busy: false });
  assert.equal(s.kind, "hint");
  assert.equal(s.message, "Type at least 2 characters");
});

test("derivePanelState surfaces backend errors", () => {
  const s = derivePanelState({
    query: "foo",
    results: null,
    error: "Folder not found",
    busy: false,
  });
  assert.equal(s.kind, "error");
  assert.equal(s.message, "Folder not found");
});

test("derivePanelState shows 'searching' while busy", () => {
  const s = derivePanelState({ query: "foo", results: null, error: null, busy: true });
  assert.equal(s.kind, "busy");
});

test("derivePanelState reports no-results when complete with empty matches", () => {
  const s = derivePanelState({
    query: "foo",
    results: {
      matches: [],
      truncated: false,
      files_scanned: 12,
      files_skipped_binary: 0,
      files_skipped_too_large: 0,
      files_unreadable: 0,
    },
    error: null,
    busy: false,
  });
  assert.equal(s.kind, "empty");
  assert.equal(s.footer, "12 files searched · 0 matches");
});

test("derivePanelState groups matches and reports footer counts", () => {
  const matches = [
    { path: "/a.md", line: 1, column: 1, line_text: "x foo y", match_start: 2, match_end: 5 },
    { path: "/a.md", line: 4, column: 1, line_text: "foo again", match_start: 0, match_end: 3 },
    { path: "/sub/b.md", line: 2, column: 1, line_text: "foo here", match_start: 0, match_end: 3 },
  ];
  const s = derivePanelState({
    query: "foo",
    results: {
      matches,
      truncated: false,
      files_scanned: 5,
      files_skipped_binary: 1,
      files_skipped_too_large: 0,
      files_unreadable: 0,
    },
    error: null,
    busy: false,
  });
  assert.equal(s.kind, "results");
  assert.equal(s.groups.length, 2);
  assert.equal(s.groups[0].path, "/a.md");
  assert.equal(s.groups[0].matches.length, 2);
  assert.equal(s.groups[1].path, "/sub/b.md");
  assert.equal(s.footer, "5 files searched · 3 matches · 1 binary skipped");
});

test("derivePanelState reports truncation in the footer", () => {
  const matches = Array.from({ length: 5000 }, (_, i) => ({
    path: "/a.md",
    line: i + 1,
    column: 1,
    line_text: "foo",
    match_start: 0,
    match_end: 3,
  }));
  const s = derivePanelState({
    query: "foo",
    results: {
      matches,
      truncated: true,
      files_scanned: 200,
      files_skipped_binary: 0,
      files_skipped_too_large: 0,
      files_unreadable: 0,
    },
    error: null,
    busy: false,
  });
  assert.equal(s.kind, "results");
  assert.match(s.footer, /Showing first 5000/);
});

test("groupResults preserves walker order across files", () => {
  const matches = [
    { path: "/z.md", line: 1, column: 1, line_text: "x", match_start: 0, match_end: 1 },
    { path: "/a.md", line: 1, column: 1, line_text: "y", match_start: 0, match_end: 1 },
    { path: "/z.md", line: 2, column: 1, line_text: "x", match_start: 0, match_end: 1 },
  ];
  const groups = groupResults(matches);
  assert.deepEqual(
    groups.map((g) => g.path),
    ["/z.md", "/a.md"],
  );
  assert.equal(groups[0].matches.length, 2);
});

test("truncateLineText returns the line unchanged when short", () => {
  const out = truncateLineText("hello foo world", 5, 8, 300);
  assert.equal(out.text, "hello foo world");
  assert.equal(out.matchStart, 5);
  assert.equal(out.matchEnd, 8);
});

test("truncateLineText centres on the match and adds ellipses", () => {
  const line = "x".repeat(400) + " needle " + "y".repeat(400);
  const start = 401;
  const end = 407;
  const out = truncateLineText(line, start, end, 60);
  assert.ok(out.text.length <= 62, `length ${out.text.length}`);
  assert.ok(out.text.startsWith("…"));
  assert.ok(out.text.endsWith("…"));
  assert.equal(out.text.slice(out.matchStart, out.matchEnd), "needle");
});
