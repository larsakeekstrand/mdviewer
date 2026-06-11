import { test } from "node:test";
import assert from "node:assert/strict";
import {
  reviewButtonLabel,
  mcpHintText,
  reviewBusy,
  viewerState,
} from "./mcp.js";

test("reviewButtonLabel: off, manual, and MCP states", () => {
  assert.equal(reviewButtonLabel(false, null), "💬 Review");
  assert.equal(reviewButtonLabel(true, null), "✓ Finish & Copy");
  assert.equal(reviewButtonLabel(true, 7), "✓ Finish & Send");
});

test("mcpHintText: with and without instructions", () => {
  assert.equal(
    mcpHintText("plan.md", ""),
    "Claude is waiting for your review of plan.md.",
  );
  const expected = "Claude is waiting for your review of plan.md — “focus on Phase 2”";
  assert.equal(
    mcpHintText("plan.md", "  focus on Phase 2  "),
    expected,
  );
});

test("reviewBusy: any reviewing or MCP-pending tab blocks", () => {
  assert.equal(reviewBusy([]), false);
  assert.equal(reviewBusy([{ reviewMode: false }]), false);
  assert.equal(reviewBusy([{ reviewMode: true }]), true);
  assert.equal(reviewBusy([{ reviewMode: false, mcpRequestId: 3 }]), true);
});

test("viewerState: reports active path and any review in progress", () => {
  assert.deepEqual(viewerState([], -1), { path: null, reviewing: false });
  const tabs = [{ path: "/a.md", reviewMode: false }, { path: "/b.md", reviewMode: true }];
  assert.deepEqual(viewerState(tabs, 0), { path: "/a.md", reviewing: true });
});
