# MCP Server: let Claude Code drive mdviewer and collect reviews

**Date:** 2026-06-11
**Status:** Approved (brainstorming) — pending implementation plan

## Goal

Expose mdviewer to Claude Code as an MCP server so the agent can present
documents to the user and collect structured feedback without a clipboard hop.
The flagship interaction is the **full review loop**: Claude opens a plan in
mdviewer, calls a blocking `request_review` tool, the app auto-enters Review
Mode, and when the user hits **✓ Finish & Send** the structured review
(`formatReview` output) returns as the tool result. This is the fourth
Claude-companion feature, after Review Mode, the hook installer, and
reveal-in-tree.

```
Claude:  open_document("docs/plan.md")
Claude:  request_review("docs/plan.md")
         ⏳ blocks while the user annotates…
User:    [adds comments, hits ✓ Finish & Send]
Claude:  ← receives structured review
         "Addressing your 3 comments…"
```

## Decisions (locked during brainstorming)

| Question | Decision |
|---|---|
| Capability tier | **Full loop**: presentation + state + blocking `request_review`. |
| Review-request UX | **Auto-enter Review Mode** with a persistent banner ("Claude is waiting for your review" + optional instructions) and a **Decline** button. |
| Transport | **stdio proxy + local socket**: Claude Code spawns `mdviewer --mcp` (stdio MCP); the proxy relays to the running GUI over a Unix domain socket (macOS) / named pipe (Windows), launching the GUI if it isn't running — the `--claude-hook` precedent. |
| MCP implementation | **Hand-rolled** newline-delimited JSON-RPC 2.0 (`initialize`, `tools/list`, `tools/call`), not the `rmcp` SDK — three tools don't justify the dependency, and pure parse/dispatch is unit-testable like `claude_hook.rs`. |
| Long-call keepalive | Proxy emits MCP `notifications/progress` every ~10 s while `request_review` is pending; README documents `MCP_TOOL_TIMEOUT` as fallback. |
| Install story | **MDViewer ▸ Install MCP Server…** menu item merges an `mcpServers.mdviewer` entry into `<root>/.mcp.json`, with `merge_hook`-style idempotent Installed/Updated/refuse-to-clobber semantics. |
| Review result format | Identical to the clipboard format (`formatReview` in `ui/review.js`); the manual clipboard path is unchanged. |

**Out of scope (YAGNI):** separate `scroll_to_heading`/`highlight_block` tools
(the `line` param on `open_document` covers it); multi-document or queued review
sessions; any tool that mutates files; root-switching side effects from MCP
calls; HTTP transport.

---

## Tool surface (v1)

| Tool | Arguments | Behavior | Returns |
|---|---|---|---|
| `open_document` | `path`, optional `line` | Open as a warm tab (reveal-in-tree fires); with `line`, reuse the jump machinery (`openTabAtLine` → `pendingJumpLine` → highlight pulse) | success/error |
| `request_review` | `path`, optional `instructions` | Open the doc, auto-enter Review Mode, show the waiting banner (with `instructions`). **Blocks** until ✓ Finish or Decline | review markdown, or `{"declined": true}` |
| `get_viewer_state` | — | Report the active document and whether a review is in progress | `{ path, reviewing }` |

`request_review`'s result is the same structured markdown Review Mode puts on
the clipboard today (relative path, general note, quoted blocks with comments,
document order), so its prompt value carries over unchanged.

---

## Architecture

Four pieces; two are pure-function modules in the `claude_hook.rs` style.

### 1. `src-tauri/src/mcp.rs` — the stdio proxy

`main.rs` checks `args().nth(1) == "--mcp"` *before* GUI launch (next to the
`--claude-hook` check) and runs the proxy instead of the app. The proxy:

- Speaks MCP over stdin/stdout: newline-delimited JSON-RPC 2.0 handling
  `initialize`, `tools/list`, `tools/call`. Hand-rolled; parse/dispatch is
  pure and unit-tested.
- Forwards each `tools/call` over the socket as one-line internal JSON
  `{id, tool, args}` → `{id, result | error}`.
- If the socket doesn't answer, launches the GUI (the `open_in_mdviewer`
  logic: macOS `open -a <bundle>` or the dev binary; Windows re-spawn exe)
  and retries the connect with short backoff before failing with a JSON-RPC
  error.
- While `request_review` is pending, emits `notifications/progress` every
  ~10 s so the client's tool timeout doesn't fire mid-review.
- The proxy is a **stateless relay**: all decisions (containment, review
  state) live GUI-side, so a killed/restarted proxy can't desync the app.

### 2. GUI-side socket listener (`lib.rs` setup hook)

A background task binds a short per-user socket path
(`$TMPDIR/mdviewer-<uid>.sock` — macOS UDS paths cap at ~104 bytes, so NOT
`app_data_dir`; Windows: named pipe `\\.\pipe\mdviewer-<user>`). Per incoming
tool message it emits a Tauri event into the webview (`mcp-open-document`,
`mcp-request-review`, `mcp-get-state`) carrying a `request_id`, and parks the
reply half in a **pending-requests map**. Startup stale-socket handling: try
connecting first — if something answers, a live instance owns it (the second
instance reports "MCP busy", never steals); if not, unlink and bind.

### 3. Frontend wiring (`ui/app.js` + pure `ui/mcp.js`)

- `mcp-open-document` → `openTabAtLine(path, line)`.
- `mcp-request-review` → open the doc, set `reviewMode`, render the waiting
  banner (instructions + **Decline**). The Review toolbar button reads
  **✓ Finish & Send** for an MCP-initiated review (vs. **✓ Finish & Copy**).
- `finishReview` grows a branch: MCP-initiated reviews go to
  `invoke("mcp_review_result", { requestId, review })` instead of the
  clipboard.
- New commands `mcp_review_result` (review finish/decline) and `mcp_respond`
  (generic reply, used by `get_viewer_state` and any future synchronous tool)
  look up the parked `request_id` in the pending map and write the reply down
  the socket.

### 4. `.mcp.json` installer

**MDViewer ▸ Install MCP Server…** merges
`{"mcpServers": {"mdviewer": {"command": "<current_exe>", "args": ["--mcp"]}}}`
into `<root>/.mcp.json` against the open tree root — idempotent
Installed/Updated, refuses to clobber wrong-typed or unparseable config,
written with `write_atomically`.

### Data flow (full loop)

```
Claude Code ──stdio──▶ mdviewer --mcp (proxy)
                          │  launch GUI if socket dead
                          ▼  {id, tool:"request_review", args}
                    UDS / named pipe
                          ▼
                GUI socket listener ──Tauri event──▶ webview
                          ▲                            │ open doc, enter
                  pending-request map                  │ Review Mode, banner
                          │                            ▼
                          ◀──invoke mcp_review_result── ✓ Finish / Decline
                          ▼
           {id, result: review markdown} ──▶ proxy ──stdio──▶ Claude
```

---

## UX details & edge cases

- **Waiting banner**: persistent (not auto-dismissing) transient-banner reuse:
  "💬 Claude is waiting for your review of `plan.md`" + optional instructions
  + **Decline**.
- **Concurrent `request_review` while one is pending** → immediate error
  result: "a review is already in progress". No queueing in v1.
- **User closes the reviewed tab** → treated as Decline; the parked request
  resolves `{"declined": true}`.
- **User quits the app mid-review** → socket EOF; proxy returns a JSON-RPC
  error ("mdviewer closed before the review finished") — a failed tool call,
  not a hang.
- **Proxy killed mid-review** (Claude session ends) → the GUI detects the
  socket write failing at Finish and falls back to the clipboard with a toast
  ("Claude is gone — review copied to clipboard instead"). Annotations are
  never lost.
- **User-initiated review while Claude requests one** → MCP request errors
  out; the manual review wins. The user outranks the agent.
- **Decline is a successful result** (`{"declined": true}`), not an error, so
  Claude proceeds gracefully.
- **`open_document` on a non-viewable path** → error result. Allowlist:
  `md`/`markdown` (the hook's gate) plus `isImagePath` extensions.
- **Different folder open than Claude's project** → `open_document` still
  works (tabs can hold out-of-root paths today — Finder opens do); the tree
  simply doesn't reveal it. MCP calls never switch the tree root.

---

## Security containment

- **Socket trust boundary**: per-user `$TMPDIR` path, mode 0600 (macOS
  `$TMPDIR` is per-user 0700 already); Windows named pipe default same-user
  ACL. Any connecting process runs as the user — the hook's trust level.
- **Paths from Claude are untrusted**: canonicalized before open;
  extension-**allowlisted** (stricter than `UNSAFE_OPEN_EXTS`'s denylist). No
  MCP tool writes, renames, or deletes; the five file-op commands stay
  UI-only.
- **No shell interpolation**: GUI launch reuses `open_in_mdviewer`'s argv
  spawn; the installer reuses `hook_command`-style escaping of
  `current_exe()`.
- **Reply integrity**: `mcp_review_result` only resolves a `request_id`
  present in the pending map — the webview cannot fabricate responses to
  requests that were never made.
- **`.mcp.json` merge refuses to clobber** unparseable or wrong-typed
  existing config, exactly like `merge_hook`.

---

## Testing

- **Rust unit (pure, no I/O)** — JSON-RPC parse/dispatch (initialize /
  tools-list / tools-call, malformed input, unknown tool), socket message
  codec, `.mcp.json` merge (install / update / refuse), tool-argument
  validation (extension gate, path shapes).
- **Rust integration** — proxy ↔ fake GUI over a real socket pair: round-trip
  a `tools/call`, progress-notification cadence, EOF-mid-request.
- **JS unit (`node --test`)** — pure helpers in `ui/mcp.js`: banner-state
  derivation, finish-routing (clipboard vs. send vs. fallback), decline
  classification.
- **End-to-end smoke (manual, pre-merge)** — register the real `.mcp.json` in
  a scratch project, run `claude` against it, drive `request_review` through
  a real annotate-and-finish, check the banner in both themes.
