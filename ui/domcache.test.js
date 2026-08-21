import { test } from "node:test";
import assert from "node:assert/strict";
import { DomCache, canRetain, entryUsable } from "./domcache.js";

test("a stored entry comes back", () => {
  const c = new DomCache(2);
  c.set("/a.md", { tag: "a" });
  assert.deepEqual(c.get("/a.md"), { tag: "a" });
});

test("an unknown path misses", () => {
  const c = new DomCache(2);
  assert.equal(c.get("/nope.md"), null);
});

test("the least recently used entry is evicted at capacity", () => {
  const c = new DomCache(2);
  c.set("/a.md", { tag: "a" });
  c.set("/b.md", { tag: "b" });
  c.set("/c.md", { tag: "c" });
  assert.equal(c.get("/a.md"), null);
  assert.deepEqual(c.get("/b.md"), { tag: "b" });
  assert.deepEqual(c.get("/c.md"), { tag: "c" });
});

test("reading an entry makes it the most recently used", () => {
  const c = new DomCache(2);
  c.set("/a.md", { tag: "a" });
  c.set("/b.md", { tag: "b" });
  c.get("/a.md");
  c.set("/c.md", { tag: "c" });
  assert.equal(c.get("/b.md"), null);
  assert.deepEqual(c.get("/a.md"), { tag: "a" });
});

test("re-storing a path replaces its entry without consuming capacity", () => {
  const c = new DomCache(2);
  c.set("/a.md", { tag: "old" });
  c.set("/a.md", { tag: "new" });
  c.set("/b.md", { tag: "b" });
  assert.deepEqual(c.get("/a.md"), { tag: "new" });
  assert.deepEqual(c.get("/b.md"), { tag: "b" });
});

test("delete and clear drop entries", () => {
  const c = new DomCache(4);
  c.set("/a.md", { tag: "a" });
  c.set("/b.md", { tag: "b" });
  c.delete("/a.md");
  assert.equal(c.get("/a.md"), null);
  c.clear();
  assert.equal(c.get("/b.md"), null);
});

const TAB = { path: "/a.md", raw: false, editing: false };
const LIVE = { path: "/a.md", raw: false, theme: "light", fromDisk: true };

test("a plain rendered tab can be retained", () => {
  assert.equal(canRetain({ tab: TAB, live: LIVE, theme: "light", exporting: false }), true);
});

test("a tab is not retained when nothing was painted from disk", () => {
  assert.equal(canRetain({ tab: TAB, live: null, theme: "light", exporting: false }), false);
  assert.equal(
    canRetain({ tab: TAB, live: { ...LIVE, fromDisk: false }, theme: "light", exporting: false }),
    false,
  );
});

test("a tab is not retained when the painted view does not match it", () => {
  assert.equal(
    canRetain({ tab: TAB, live: { ...LIVE, path: "/other.md" }, theme: "light", exporting: false }),
    false,
  );
  assert.equal(
    canRetain({ tab: TAB, live: { ...LIVE, raw: true }, theme: "light", exporting: false }),
    false,
  );
  assert.equal(
    canRetain({ tab: TAB, live: LIVE, theme: "dark", exporting: false }),
    false,
  );
});

test("editing tabs and in-flight exports are never retained", () => {
  assert.equal(
    canRetain({ tab: { ...TAB, editing: true }, live: LIVE, theme: "light", exporting: false }),
    false,
  );
  assert.equal(canRetain({ tab: TAB, live: LIVE, theme: "light", exporting: true }), false);
});

test("an entry is usable only for the same raw mode and theme", () => {
  const entry = { fragment: {}, raw: false, theme: "light" };
  assert.equal(entryUsable(entry, { raw: false, theme: "light" }), true);
  assert.equal(entryUsable(entry, { raw: true, theme: "light" }), false);
  assert.equal(entryUsable(entry, { raw: false, theme: "dark" }), false);
  assert.equal(entryUsable(null, { raw: false, theme: "light" }), false);
});
