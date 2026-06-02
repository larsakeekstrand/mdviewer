# User-friendly "What's new" release notes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the raw `git log` "What's new" content with curated prose from a hand-maintained `CHANGELOG.md`, falling back to the commit log when a release has no curated section.

**Architecture:** A `CHANGELOG.md` at the repo root is the source of truth. A pure helper `changelogSection(text, version)` (in `ui/update.js`, unit-tested under `node --test`) extracts the section matching a release tag. A tiny Node wrapper (`scripts/changelog-section.mjs`) lets `release.yml`'s `changelog` step call it; if it yields nothing, the step falls back to the existing `git log` block. All three downstream consumers (GitHub release page, macOS `latest.json` `.notes`, Windows `.notes` via `polish-release`) already read this step's output unchanged.

**Tech Stack:** Vanilla ES modules + `node:test`, GitHub Actions (bash), comrak (unchanged).

**Spec:** `docs/superpowers/specs/2026-06-02-user-friendly-release-notes-design.md`

---

### Task 1: `changelogSection` pure helper

**Files:**
- Modify: `ui/update.js` (append a new exported function)
- Test: `ui/update.test.js` (extend the import + add tests)

- [ ] **Step 1: Write the failing tests**

In `ui/update.test.js`, add `changelogSection` to the import block at the top:

```js
import {
  releaseUrlFor,
  bannerMessage,
  progressPercent,
  progressText,
  extractChangelog,
  changelogSection,
} from "./update.js";
```

Then append these tests to the end of the file:

```js
const SAMPLE = `# Changelog

## [1.16.0] - 2026-06-02

- Folder-wide search across the open tree
- Fixed a crash when exporting docs with broken images

## [1.15.0] - 2026-05-31

- Earlier feature
`;

test("changelogSection returns only the matching version's bullets", () => {
  assert.equal(
    changelogSection(SAMPLE, "1.16.0"),
    "- Folder-wide search across the open tree\n- Fixed a crash when exporting docs with broken images",
  );
});

test("changelogSection stops at the next version heading", () => {
  assert.equal(changelogSection(SAMPLE, "1.15.0"), "- Earlier feature");
});

test("changelogSection returns '' for a missing version", () => {
  assert.equal(changelogSection(SAMPLE, "9.9.9"), "");
});

test("changelogSection returns '' for a prerelease with no entry", () => {
  assert.equal(changelogSection(SAMPLE, "1.16.0-rc.1"), "");
});

test("changelogSection tolerates a heading with no date suffix", () => {
  const text = "## [2.0.0]\n\n- New thing\n";
  assert.equal(changelogSection(text, "2.0.0"), "- New thing");
});

test("changelogSection returns '' for empty input", () => {
  assert.equal(changelogSection("", "1.0.0"), "");
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `node --test ui/update.test.js`
Expected: FAIL — `changelogSection is not a function` (or import error) on the new tests.

- [ ] **Step 3: Implement the helper**

Append to `ui/update.js` (after `extractChangelog`):

```js
/** Extract one version's notes from a Keep-a-Changelog `CHANGELOG.md` body.
 *  `version` is the semver without a leading `v` (e.g. "1.16.0"). Matches a
 *  heading `## [<version>]` (a trailing ` - DATE` and surrounding whitespace
 *  are tolerated) and returns the lines up to the next `## ` heading, trimmed.
 *  Returns "" when no matching section exists (caller decides the fallback). */
export function changelogSection(text, version) {
  if (!text || !version) return "";
  const escaped = version.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const heading = new RegExp(`^##\\s+\\[${escaped}\\]`);
  const lines = text.split("\n");
  const start = lines.findIndex((l) => heading.test(l));
  if (start === -1) return "";
  const rest = lines.slice(start + 1);
  const end = rest.findIndex((l) => /^##\s/.test(l));
  return (end === -1 ? rest : rest.slice(0, end)).join("\n").trim();
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `node --test ui/update.test.js`
Expected: PASS — all tests, including the six new ones.

- [ ] **Step 5: Commit**

```bash
git add ui/update.js ui/update.test.js
git commit -m "Add changelogSection helper to parse CHANGELOG.md sections"
```

---

### Task 2: Seed `CHANGELOG.md`

**Files:**
- Create: `CHANGELOG.md` (repo root)

- [ ] **Step 1: Create the file**

Create `CHANGELOG.md` with a real 1.16.0 section (the current released version). Use plain user-facing language — no commit hashes, no internal plumbing:

```markdown
# Changelog

User-facing notes for each release. The release workflow extracts the section
matching the tag into the GitHub release page and the in-app "What's new" modal.

## [1.16.0] - 2026-06-02

- Hardened HTML/PDF export so it never inlines files from outside the workspace.
- Pinned CI's third-party GitHub Actions to exact commits for supply-chain safety.
```

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "Add CHANGELOG.md as the source for release notes"
```

---

### Task 3: Node wrapper for the workflow

**Files:**
- Create: `scripts/changelog-section.mjs`

- [ ] **Step 1: Create the wrapper script**

Create `scripts/changelog-section.mjs`. It imports the helper (path is relative to this file, so it resolves to `<repo>/ui/update.js` regardless of cwd), reads the changelog, and prints the matching section. A missing file or missing section prints nothing (exit 0) so the caller can fall back.

```js
import { readFileSync } from "node:fs";
import { changelogSection } from "../ui/update.js";

const version = (process.argv[2] || "").replace(/^v/, "");
const path = process.argv[3] || "CHANGELOG.md";

let text = "";
try {
  text = readFileSync(path, "utf8");
} catch {
  process.exit(0);
}

const out = changelogSection(text, version);
if (out) process.stdout.write(out);
```

- [ ] **Step 2: Verify it prints the seeded section**

Run: `node scripts/changelog-section.mjs v1.16.0 CHANGELOG.md`
Expected: prints the two 1.16.0 bullets (no heading, no trailing newline noise).

- [ ] **Step 3: Verify the fallback path prints nothing**

Run: `node scripts/changelog-section.mjs v0.0.1 CHANGELOG.md; echo "exit=$?"`
Expected: no section text, `exit=0`.

- [ ] **Step 4: Commit**

```bash
git add scripts/changelog-section.mjs
git commit -m "Add Node wrapper to extract a CHANGELOG.md section for releases"
```

---

### Task 4: Wire the wrapper into `release.yml`

**Files:**
- Modify: `.github/workflows/release.yml:104-128` (the `Build changelog` step)

- [ ] **Step 1: Replace the step body**

Replace the existing `Build changelog` step (lines 104-128) with the version below. It tries the curated section first (deriving the bare version from the `v`-prefixed `CURRENT` tag) and falls back to the unchanged `git log` logic when the section is empty:

```yaml
      - name: Build changelog
        id: changelog
        env:
          CURRENT: ${{ needs.meta.outputs.tag }}
        run: |
          set -euo pipefail
          VERSION="${CURRENT#v}"
          CURATED=$(node scripts/changelog-section.mjs "$VERSION" CHANGELOG.md || true)

          if [ -n "$CURATED" ]; then
            echo "source=CHANGELOG.md (curated)" >&2
            BODY="$CURATED"
          else
            echo "source=git log (fallback)" >&2
            PREV=$(git describe --tags --abbrev=0 "${CURRENT}^" 2>/dev/null || true)
            echo "current=$CURRENT" >&2
            echo "previous=$PREV" >&2
            if [ -z "$PREV" ]; then
              HEADER="Initial release."
              LOG=$(git log HEAD --pretty=format:'- %s (%h)' --no-merges --reverse)
            else
              HEADER="Commits since [${PREV}](https://github.com/${GITHUB_REPOSITORY}/releases/tag/${PREV}):"
              LOG=$(git log "${PREV}..HEAD" --pretty=format:'- %s (%h)' --no-merges --reverse)
            fi
            BODY=$(printf '%s\n\n%s' "$HEADER" "$LOG")
          fi

          {
            echo "value<<CHANGELOG_EOF"
            echo "$BODY"
            echo "CHANGELOG_EOF"
          } >> "$GITHUB_OUTPUT"
```

- [ ] **Step 2: Sanity-check the YAML parses**

Run: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/release.yml')); print('ok')"`
Expected: `ok`

- [ ] **Step 3: Reproduce the step locally (curated path)**

Run from the repo root:
```bash
CURRENT=v1.16.0; VERSION="${CURRENT#v}"; node scripts/changelog-section.mjs "$VERSION" CHANGELOG.md
```
Expected: the two curated 1.16.0 bullets — confirming the step's primary branch produces curated text for a real tag.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "Use CHANGELOG.md for release notes, fall back to git log"
```

---

### Task 5: Document the new release step

**Files:**
- Modify: `CLAUDE.md` ("Cutting a release" section)

- [ ] **Step 1: Add the CHANGELOG step to the checklist**

In `CLAUDE.md`, find the numbered "Cutting a release" list. After step 1 (the README-update step) and before the version-bump step, insert a new step:

```markdown
2. Add a `## [X.Y.Z] - <date>` section to `CHANGELOG.md` with short,
   user-facing bullets (no commit hashes, no internal/test/bump commits) —
   this is what the GitHub release page and the in-app "What's new" modal
   show. If omitted, the workflow falls back to the raw commit log.
```

Renumber the subsequent steps (the old 2→3, 3→4, etc.).

- [ ] **Step 2: Update the "Beta channel" / cut-a-beta note**

In the "Cutting a beta" paragraph, add a sentence noting that a prerelease tag
(e.g. `v1.16.0-rc.1`) only gets curated notes if `CHANGELOG.md` has a matching
`## [1.16.0-rc.1]` section; otherwise it falls back to the commit log (which is
fine for testers).

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "Document CHANGELOG.md step in the release checklist"
```

---

## Self-Review

**Spec coverage:**
- `CHANGELOG.md` source of truth → Task 2. ✓
- `changelogSection(text, version)` tested pure helper, `""` on miss, exact prerelease match → Task 1. ✓
- Workflow extracts section, falls back to `git log` → Tasks 3 + 4. ✓
- Nothing downstream changes (`extractChangelog`, `polish-release`) → no task needed; verified the step's `value` output contract is preserved in Task 4. ✓
- Docs / "Cutting a release" step → Task 5 (CLAUDE.md only; README has no release checklist). ✓
- Tests enumerated in the spec → all six covered in Task 1 Step 1. ✓

**Placeholder scan:** none — every code/step block is concrete.

**Type/name consistency:** `changelogSection(text, version)` is defined in Task 1 and called identically in Task 3's wrapper; the wrapper's CLI shape (`<version> <path>`) matches its invocation in Task 4. ✓
