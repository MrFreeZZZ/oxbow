import { titled, windowTitle, type Shown } from "../src/lib/windowTitle.ts";
import assert from "node:assert/strict";

const shown = (over: Partial<Shown> = {}): Shown => ({
  screen: "history",
  repo: "oxbow",
  steps: false,
  conflicts: null,
  file: null,
  github: { place: "MrFreeZZZ/oxbow", open: 2 },
  pull: null,
  series: null,
  compare: { base: "main", target: "feature" },
  head: "main",
  running: null,
  failed: null,
  branches: 3,
  bisect: null,
  detached: false,
  ...over,
});

// History: the repository's name as a title, its branches, and what runs or failed.
{
  assert.equal(titled("oxbow"), "Oxbow");
  assert.equal(titled("myRepo"), "myRepo");
  assert.deepEqual(windowTitle(shown()), { name: "Oxbow history", sub: "3 branches", steps: false, amber: false });
  assert.equal(windowTitle(shown({ branches: 1, detached: true })).sub, "1 branch · detached HEAD");
  assert.equal(windowTitle(shown({ bisect: { found: null }, detached: true })).sub, "3 branches · Bisecting");
  assert.equal(windowTitle(shown({ bisect: { found: "a1b2c3" } })).sub, "3 branches · Bisect done");
  assert.equal(windowTitle(shown({ running: { title: "Fetching origin", percent: null } })).sub, "Fetching origin…");
  assert.equal(windowTitle(shown({ running: { title: "Fetching origin", percent: 40 } })).sub, "Fetching origin · 40%");
  const failed = windowTitle(shown({ failed: "Can't reach origin" }));
  assert.equal(failed.sub, "Fetch failed · Can't reach origin");
  assert.equal(failed.amber, true);
}

// Resolve Conflicts comes before the screen under it.
{
  const title = windowTitle(shown({ screen: "stashes", conflicts: { kind: "am", noun: "Applying patches", conflicted: 1 } }));
  assert.deepEqual(title, { name: "Resolve Conflicts", sub: "Applying patches · 1 file left", steps: false, amber: false });
  const merge = windowTitle(shown({ conflicts: { kind: "merge", noun: "Merge", conflicted: 3 } }));
  assert.equal(merge.sub, "Merge in progress · 3 files left");
}

// A file: its name, then where it is; after steps that led here only the line.
{
  const file = { path: "src/lib/a.ts", line: 12 };
  assert.deepEqual(windowTitle(shown({ screen: "file", file })), { name: "a.ts", sub: "oxbow · src/lib/a.ts · line 12", steps: true, amber: false });
  assert.equal(windowTitle(shown({ screen: "file", file, steps: true })).sub, "Line 12");
  assert.equal(windowTitle(shown({ screen: "file", file: { path: "a.ts", line: null }, steps: true })).sub, "a.ts");
  // No file yet: the History title.
  assert.equal(windowTitle(shown({ screen: "file" })).name, "Oxbow history");
}

// A pull request: by number when opened on it, the branch otherwise.
{
  const pr = (pull: Shown["pull"], steps = false) => windowTitle(shown({ screen: "pull", pull, steps }));
  assert.deepEqual(pr({ branch: "fix", pr: 7, byNumber: true }), { name: "PR #7", sub: "Pull request #7 · MrFreeZZZ/oxbow", steps: true, amber: false });
  assert.equal(pr({ branch: "fix", pr: 7, byNumber: false }).name, "fix");
  assert.equal(pr({ branch: "fix", pr: null, byNumber: false }, true).sub, "No pull request yet");
}

// The other screens.
{
  assert.equal(windowTitle(shown({ screen: "reviews" })).sub, "MrFreeZZZ/oxbow · 2 open");
  assert.equal(windowTitle(shown({ screen: "reviews", github: { place: null, open: 0 } })).sub, "oxbow · 0 open");
  assert.equal(windowTitle(shown({ screen: "stack" })).sub, "Interactive rebase · oxbow");
  assert.equal(windowTitle(shown({ screen: "compare" })).sub, "feature with main");
  assert.equal(windowTitle(shown({ screen: "series", series: { branch: "b", from: "Edit stack b · now" } })).sub, "b · Edit stack b · now");
  assert.equal(windowTitle(shown({ screen: "stashes", head: null })).sub, "oxbow · on detached HEAD");
  assert.equal(windowTitle(shown({ screen: "oplog", steps: true })).sub, "every step, to undo or restore");
}
