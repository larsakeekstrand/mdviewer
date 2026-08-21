import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DomCache,
  canRetain,
  entryUsable,
  revalidationApplies,
} from "./domcache.js";

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

/* ---- revalidationApplies ---- */

const REVAL = {
  token: 3,
  seq: 3,
  tab: { path: "/a.md", raw: false, editing: false },
  exporting: false,
  theme: "light",
  currentTheme: "light",
  raw: false,
};
// The tab the request was dispatched for is still the displayed one.
REVAL.active = REVAL.tab;

test("a revalidation whose view state is unchanged applies", () => {
  assert.equal(revalidationApplies(REVAL), true);
});

test("a superseded revalidation does not apply", () => {
  assert.equal(revalidationApplies({ ...REVAL, seq: 4 }), false);
});

test("a revalidation for a tab that is no longer displayed does not apply", () => {
  assert.equal(
    revalidationApplies({ ...REVAL, active: { path: "/b.md", raw: false, editing: false } }),
    false,
  );
  assert.equal(revalidationApplies({ ...REVAL, active: null }), false);
});

test("a revalidation does not apply once the tab entered edit mode", () => {
  const tab = { ...REVAL.tab, editing: true };
  assert.equal(revalidationApplies({ ...REVAL, tab, active: tab }), false);
});

test("a revalidation does not apply while an export is in flight", () => {
  assert.equal(revalidationApplies({ ...REVAL, exporting: true }), false);
});

test("a revalidation rendered for the previous theme does not apply", () => {
  assert.equal(revalidationApplies({ ...REVAL, currentTheme: "dark" }), false);
});

test("a revalidation rendered for the previous raw mode does not apply", () => {
  const tab = { ...REVAL.tab, raw: true };
  assert.equal(revalidationApplies({ ...REVAL, tab, active: tab }), false);
});
