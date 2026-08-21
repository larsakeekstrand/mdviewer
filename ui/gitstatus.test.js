import { test } from "node:test";
import assert from "node:assert/strict";
import { aggregateDirStatus, buildDirStatuses } from "./gitstatus.js";

test("aggregateDirStatus returns null for no descendants", () => {
  assert.equal(aggregateDirStatus([]), null);
});

test("aggregateDirStatus prefers a conflict over everything else", () => {
  assert.equal(aggregateDirStatus(["??", " M", "UU"]), "UU");
});

test("aggregateDirStatus prefers modified over untracked", () => {
  assert.equal(aggregateDirStatus(["??", " M"]), " M");
});

test("aggregateDirStatus falls back to the first code when none match", () => {
  assert.equal(aggregateDirStatus(["!!", "xx"]), "!!");
});

test("buildDirStatuses rolls a file's code up to every ancestor directory", () => {
  const dirs = buildDirStatuses({ "/repo/docs/deep/a.md": " M" });
  assert.equal(dirs.get("/repo/docs/deep"), " M");
  assert.equal(dirs.get("/repo/docs"), " M");
  assert.equal(dirs.get("/repo"), " M");
});

test("buildDirStatuses does not give a file's own path a directory entry", () => {
  const dirs = buildDirStatuses({ "/repo/a.md": " M" });
  assert.equal(dirs.has("/repo/a.md"), false);
});

test("buildDirStatuses applies the aggregate priority per directory", () => {
  const dirs = buildDirStatuses({
    "/repo/src/new.md": "??",
    "/repo/src/edited.md": " M",
    "/repo/docs/other.md": "??",
  });
  assert.equal(dirs.get("/repo/src"), " M");
  assert.equal(dirs.get("/repo/docs"), "??");
  assert.equal(dirs.get("/repo"), " M");
});

test("buildDirStatuses agrees with aggregateDirStatus over the same entries", () => {
  const entries = {
    "/repo/a/one.md": "??",
    "/repo/a/two.md": "A ",
    "/repo/a/three.md": " D",
    "/repo/b/four.md": "MM",
  };
  const dirs = buildDirStatuses(entries);
  const codesUnder = (dir) =>
    Object.entries(entries)
      .filter(([p]) => p.startsWith(dir + "/"))
      .map(([, code]) => code);
  for (const dir of ["/repo", "/repo/a", "/repo/b"]) {
    assert.equal(dirs.get(dir), aggregateDirStatus(codesUnder(dir)), dir);
  }
});

test("buildDirStatuses returns an empty map for no entries", () => {
  assert.equal(buildDirStatuses({}).size, 0);
});

test("buildDirStatuses handles Windows path separators", () => {
  const dirs = buildDirStatuses({ "C:\\repo\\docs\\a.md": " M" });
  assert.equal(dirs.get("C:\\repo\\docs"), " M");
  assert.equal(dirs.get("C:\\repo"), " M");
});

test("buildDirStatuses terminates at a filesystem root", () => {
  const dirs = buildDirStatuses({ "/a.md": "??" });
  assert.equal(dirs.get("/"), "??");
  assert.equal(dirs.size, 1);
});
