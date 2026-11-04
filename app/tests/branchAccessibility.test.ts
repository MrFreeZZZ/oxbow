import assert from "node:assert/strict";
import { branchAccessibleName } from "../src/lib/branchAccessibility.ts";
import type { Tracking } from "../src/lib/types.ts";

const tracking: Tracking = {
  remote: "origin", branch: "main", ahead: 1, behind: 35, gone: false, rewritten: null, replay: null,
};

assert.equal(branchAccessibleName({ name: "main", current: true, tracking }),
  "main, current branch, 1 commit to push, 35 commits to pull");
assert.equal(branchAccessibleName({ name: "feature/auth", current: false, tracking: null,
  worktree: { name: "QA", missing: true }, mergeNumber: 42, operation: "REBASING" }),
  "feature/auth, checked out in QA, folder missing, merge request !42, rebasing in progress");
assert.equal(branchAccessibleName({ name: "old", current: false, tracking: { ...tracking, gone: true }, pullNumber: 8 }),
  "old, upstream origin/main deleted, pull request #8");
