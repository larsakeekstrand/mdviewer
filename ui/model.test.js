import { test } from "node:test";
import assert from "node:assert/strict";
import { modelFormat, formatDimensions, frameCamera, keepCamera } from "./model.js";

test("modelFormat maps extensions case-insensitively", () => {
  assert.equal(modelFormat("a.STL"), "stl");
  assert.equal(modelFormat("b.3mf"), "3mf");
  assert.equal(modelFormat("c.obj"), "obj");
  assert.equal(modelFormat("d.md"), null);
  assert.equal(modelFormat(""), null);
});

test("formatDimensions uses one decimal and a multiplication sign", () => {
  assert.equal(
    formatDimensions({ x: 0, y: 0, z: 0 }, { x: 120, y: 45.54, z: 30 }),
    "120.0 × 45.5 × 30.0 mm",
  );
  assert.equal(
    formatDimensions({ x: -10, y: -5, z: 0 }, { x: 10, y: 5, z: 0 }),
    "20.0 × 10.0 × 0.0 mm",
  );
  assert.equal(
    formatDimensions({ x: 1, y: 1, z: 1 }, { x: 1, y: 1, z: 1 }),
    "0.0 × 0.0 × 0.0 mm",
  );
});

const dist = (a, b) => Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z);

test("frameCamera survives a zero radius", () => {
  const c = { x: 0, y: 0, z: 0 };
  const f = frameCamera(c, 0, 45, 1);
  for (const v of [...Object.values(f.position), ...Object.values(f.target), f.near, f.far]) {
    assert.ok(Number.isFinite(v));
  }
  assert.ok(f.near > 0);
  assert.ok(f.far > f.near);
});

test("frameCamera fits the sphere and widens for narrow aspects", () => {
  const c = { x: 1, y: 2, z: 3 };
  const wide = frameCamera(c, 50, 45, 1);
  assert.deepEqual(wide.target, c);
  const d1 = dist(wide.position, wide.target);
  assert.ok(d1 >= 50 / Math.sin((22.5 * Math.PI) / 180));
  const narrow = frameCamera(c, 50, 45, 0.5);
  assert.ok(dist(narrow.position, narrow.target) > d1);
  assert.equal(wide.near, d1 / 100);
  assert.equal(wide.far, d1 * 100);
});

test("keepCamera is true only for a reload of the same path", () => {
  assert.equal(keepCamera("/a.stl", "/a.stl", "reload"), true);
  assert.equal(keepCamera("/a.stl", "/b.stl", "reload"), false);
  assert.equal(keepCamera("/a.stl", "/a.stl", "switch"), false);
  assert.equal(keepCamera(null, "/a.stl", "open"), false);
});
