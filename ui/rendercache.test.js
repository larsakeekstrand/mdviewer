import { test } from "node:test";
import assert from "node:assert/strict";
import { RenderCache } from "./rendercache.js";

test("a stored render is returned with its stamp", () => {
  const c = new RenderCache(1000);
  c.set("/a.md", "light", false, "<h1>Hi</h1>", "1:4");
  assert.deepEqual(c.get("/a.md", "light", false), {
    html: "<h1>Hi</h1>",
    stamp: "1:4",
  });
});

test("an unknown file misses", () => {
  const c = new RenderCache(1000);
  assert.equal(c.get("/nope.md", "light", false), null);
});

test("theme and raw mode are part of the key", () => {
  const c = new RenderCache(1000);
  c.set("/a.md", "light", false, "light html", "1:4");
  assert.equal(c.get("/a.md", "dark", false), null);
  assert.equal(c.get("/a.md", "light", true), null);
  assert.equal(c.get("/a.md", "light", false).html, "light html");
});

test("deleteByPath drops every variant of that file only", () => {
  const c = new RenderCache(1000);
  c.set("/a.md", "light", false, "a light", "1:4");
  c.set("/a.md", "dark", true, "a dark raw", "1:4");
  c.set("/b.md", "light", false, "b", "1:4");

  c.deleteByPath("/a.md");

  assert.equal(c.get("/a.md", "light", false), null);
  assert.equal(c.get("/a.md", "dark", true), null);
  assert.equal(c.get("/b.md", "light", false).html, "b");
});

test("the least recently used entry is evicted when over budget", () => {
  const c = new RenderCache(20);
  c.set("/a.md", "light", false, "0123456789", "s");
  c.set("/b.md", "light", false, "0123456789", "s");
  c.set("/c.md", "light", false, "0123456789", "s");

  assert.equal(c.get("/a.md", "light", false), null);
  assert.equal(c.get("/b.md", "light", false).html, "0123456789");
  assert.equal(c.get("/c.md", "light", false).html, "0123456789");
});

test("reading an entry makes it the most recently used", () => {
  const c = new RenderCache(20);
  c.set("/a.md", "light", false, "0123456789", "s");
  c.set("/b.md", "light", false, "0123456789", "s");
  c.get("/a.md", "light", false); // a is now newer than b
  c.set("/c.md", "light", false, "0123456789", "s");

  assert.equal(c.get("/b.md", "light", false), null);
  assert.equal(c.get("/a.md", "light", false).html, "0123456789");
});

test("a render bigger than the whole budget is not cached and evicts nothing", () => {
  const c = new RenderCache(20);
  c.set("/small.md", "light", false, "0123456789", "s");
  c.set("/huge.md", "light", false, "x".repeat(50), "s");

  assert.equal(c.get("/huge.md", "light", false), null);
  assert.equal(c.get("/small.md", "light", false).html, "0123456789");
});

test("re-storing a key replaces it without double-counting its size", () => {
  const c = new RenderCache(20);
  c.set("/a.md", "light", false, "0123456789", "s1");
  c.set("/a.md", "light", false, "abcdefghij", "s2");
  c.set("/b.md", "light", false, "0123456789", "s");

  assert.equal(c.get("/a.md", "light", false).stamp, "s2");
  assert.equal(c.get("/b.md", "light", false).html, "0123456789");
});

test("clear empties the cache", () => {
  const c = new RenderCache(1000);
  c.set("/a.md", "light", false, "a", "s");
  c.clear();
  assert.equal(c.get("/a.md", "light", false), null);
});
