# User-friendly "What's new" release notes — design

**Date:** 2026-06-02
**Status:** Approved, pending implementation plan

## Problem

The in-app **What's new** modal (and the GitHub release page, and `latest.json`'s
`notes`) currently show a raw `git log` dump: one bullet per non-merge commit,
formatted `- <subject> (<hash>)`. Normal users see internal plumbing commits
(`Bump to 1.16.0`, `Format path_within_dir test`, `Pin … to commit SHAs`) and
dev-speak subjects with short hashes — noise that means nothing to them.

The notes are never *authored* anywhere; they are a side effect of
`release.yml`'s `changelog` step:

```
git log "${PREV}..HEAD" --pretty=format:'- %s (%h)' --no-merges --reverse
```

## Goal

Make the "What's new" content human-readable, curated prose, while:

- never shipping a release with empty notes (safety net),
- changing nothing in the per-commit / per-PR workflow,
- following the repo's existing convention of pure helpers unit-tested under
  `node --test`.

## Decision summary

- **Source of truth:** a hand-curated `CHANGELOG.md` at the repo root
  (Keep-a-Changelog flavored).
- **Maintenance cadence:** author a new `## [X.Y.Z] - DATE` section at release
  cut time (same moment versions are bumped and the README is updated). No
  change to the per-PR workflow.
- **Pipeline:** the workflow *extracts* the section matching the tag instead of
  generating one from `git log`. Falls back to the existing `git log` output
  when no matching section exists.

Explicitly out of scope (YAGNI): Conventional Commits, automatic grouping, an
LLM summarization step, any frontend code change.

## Architecture / data flow

```
CHANGELOG.md ──(extract section for v1.16.0)──> release.yml `changelog` step
                                                       │
                          ┌────────────────────────────┼────────────────────────────┐
                          ▼                             ▼                             ▼
                ## Changes in release body    latest.json .notes (mac)    polish-release restores
                (GitHub release page)         via tauri-action             .notes for Windows latest.json
                                                       │
                                                       ▼
                                     in-app "What's new" modal
                                (extractChangelog → render_notes → comrak)
```

The key property: **nothing downstream changes.** `extractChangelog`
(`ui/update.js`) already returns everything after the `## Changes` heading, and
`polish-release` already reuses the `changelog` step's output to restore the
Windows `latest.json` `.notes`. Swapping what the `changelog` step *emits* fixes
all three consumers (release page, macOS modal, Windows modal) at once.

## Components

### 1. `CHANGELOG.md` (new, repo root)

Keep-a-Changelog flavored. Flat bullets by default; optional `### Added` /
`### Fixed` subheadings are allowed because the modal renders arbitrary
markdown.

```markdown
# Changelog

## [1.16.0] - 2026-06-02

- Folder-wide search: ⌘⇧F to search every file in the open tree
- Fixed a crash when exporting docs with broken images
```

Seed the file with a real `## [1.16.0]` section so the next release has curated
content. (Earlier versions need not be back-filled; the fallback covers any tag
without a section.)

### 2. `changelogSection(text, version)` — tested pure helper

Lives in `ui/update.js`, co-located with the existing `extractChangelog`, and is
unit-tested in `ui/update.test.js` (CI runs `node --test ui/*.test.js`).

Signature and behavior:

- `changelogSection(changelogText, version) -> string`
- `version` is the tag with any leading `v` stripped (`v1.16.0` → `1.16.0`).
- Matches a heading of the form `## [<version>]` (the ` - DATE` suffix and
  surrounding whitespace are tolerated), and returns the body **up to the next
  `## ` heading**, trimmed.
- Returns `""` when no matching section is found (caller decides the fallback).
- Prerelease handling: try the exact version first
  (`1.16.0-rc.1` → `## [1.16.0-rc.1]`); if absent, the caller's fallback to
  `git log` applies. (No automatic base-version stripping — keep it simple;
  betas are for testers and the commit-log fallback is acceptable for them.)

### 3. Workflow wire-up (`.github/workflows/release.yml`)

The `changelog` step:

1. Derives `VERSION` from the tag (strip leading `v`).
2. Runs a tiny Node wrapper (e.g. `scripts/changelog-section.mjs`) that imports
   `changelogSection` from `ui/update.js`, reads `CHANGELOG.md`, and prints the
   matching section. GitHub `macos-14` runners have Node preinstalled.
3. If the wrapper prints a non-empty section, use it as the `value` output.
4. Otherwise fall back to the **existing** `git log` block unchanged.

The `value` output continues to flow into the release body `## Changes` section
and into `polish-release`'s `.notes` restore exactly as today.

### 4. Documentation

- `CLAUDE.md` and `README.md` "Cutting a release" checklist: add a step
  *"Add a `## [X.Y.Z] - DATE` section to `CHANGELOG.md` with user-facing
  bullets"*, adjacent to the existing README-update and version-bump steps.

## Testing

`node --test` over `changelogSection`:

- exact version match returns only that section's bullets;
- section body stops at the next `## ` heading;
- missing version returns `""`;
- prerelease version (`1.16.0-rc.1`) with no entry returns `""`;
- multiple sections — the correct one is selected regardless of order;
- leading/trailing whitespace and a ` - DATE` suffix on the heading are
  tolerated.

The workflow fallback is exercised implicitly by any tag that lacks a curated
section (e.g. existing/older tags, betas).

## Risks / notes

- **Forgetting to add a section** degrades gracefully to the old `git log`
  behavior rather than failing the release.
- **Node availability on runners:** confirmed for `macos-14`; the wrapper is
  only invoked in the macOS build job (which owns the `changelog` output).
- No CSP / rendering changes: the modal already renders curated markdown via
  `render_notes` (comrak) — bullets, headings, and inline formatting all work.
