import { test } from "node:test";
import assert from "node:assert/strict";
import { statusButtonLabel, statusLabel, shouldNudge } from "./integration.js";

test("statusButtonLabel: install vs update", () => {
  assert.equal(statusButtonLabel(false), "Install");
  assert.equal(statusButtonLabel(true), "Update");
});

test("statusLabel: not installed vs installed", () => {
  assert.equal(statusLabel(false), "Not installed");
  assert.equal(statusLabel(true), "Installed");
});

test("shouldNudge: only in a git repo with nothing installed and not dismissed", () => {
  assert.equal(shouldNudge(true, false, false, false), true);
  assert.equal(shouldNudge(false, false, false, false), false);
  assert.equal(shouldNudge(true, true, false, false), false);
  assert.equal(shouldNudge(true, false, true, false), false);
  assert.equal(shouldNudge(true, false, false, true), false);
});
