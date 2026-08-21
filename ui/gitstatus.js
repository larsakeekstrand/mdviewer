// Pure helpers for git status decoration in the file tree. DOM-free.

/** The roll-up policy: how a directory summarises its descendants' statuses.
 *
 * Prefer modified > added > deleted > conflict > untracked. Both roll-up
 * functions below read this array, so changing the priority (or the tie-break)
 * here changes the badges everywhere.
 *
 * TODO(user): this is a taste call and can be changed. Trade-offs to weigh:
 *   - VS Code shows "M" if anything inside is modified, dropping untracked-only
 *     dirs to a dimmer dot. Calmer, but hides new files.
 *   - You could surface "U" so an untracked subfolder still draws the eye —
 *     better for "what's new" but noisier in repos with many untracked.
 *   - You could drop directory badges entirely (least visual noise, but loses
 *     the "something inside changed" cue).
 *
 * Production goes through `buildDirStatuses`, NOT `aggregateDirStatus` — edit
 * the policy where it runs. `aggregateDirStatus` is kept as the readable
 * reference the equivalence test in gitstatus.test.js pins the fast path
 * against, so a policy change has to be made in both to stay green.
 */
const PRIORITY = ["UU", "DD", "AA", "M", "A", "D", "R", "C", "T", "?"];

/** Reference implementation of the roll-up (see PRIORITY): the single badge
 *  code for a directory whose changed descendants carry `codes`, or null for
 *  none. Not on the decoration path — see `buildDirStatuses`. */
export function aggregateDirStatus(codes) {
  if (codes.length === 0) return null;
  for (const want of PRIORITY) {
    for (const code of codes) {
      if (code.includes(want[0]) || code === want) return code;
    }
  }
  return codes[0];
}

/** Rank of a porcelain code in PRIORITY; PRIORITY.length when nothing matches
 *  (the aggregate's "first code wins" fallback). */
function rank(code) {
  for (let i = 0; i < PRIORITY.length; i++) {
    const want = PRIORITY[i];
    if (code.includes(want[0]) || code === want) return i;
  }
  return PRIORITY.length;
}

function parentOf(path) {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  if (i < 0) return null;
  if (i === 0) return path.slice(0, 1); // "/foo" → "/"
  return path.slice(0, i);
}

/** Aggregated badge code for every ancestor directory of every changed path.
 *  THIS is what applyGitDecorations calls — the roll-up policy documented on
 *  PRIORITY takes effect here, via `rank`.
 *
 * One pass over `entries` (absolute path → porcelain code) instead of scanning
 * the whole entry set per directory row, which made decoration O(rows ×
 * entries). Equivalent to calling `aggregateDirStatus` on each directory's
 * descendants: a directory keeps the best-ranked code it sees, ties going to
 * whichever came first in iteration order — the same rule the per-directory
 * scan applied. */
export function buildDirStatuses(entries) {
  const best = new Map();
  const bestRank = new Map();
  for (const [path, code] of Object.entries(entries)) {
    const r = rank(code);
    let dir = parentOf(path);
    while (dir) {
      const prev = bestRank.get(dir);
      if (prev === undefined || r < prev) {
        bestRank.set(dir, r);
        best.set(dir, code);
      }
      const up = parentOf(dir);
      if (up === dir) break; // filesystem root
      dir = up;
    }
  }
  return best;
}
