import { carryStates, checking, failChecking, withState } from "../src/lib/folders.ts";
import type { History, Submodule, Worktree } from "../src/lib/types.ts";
import assert from "node:assert/strict";

const tree = (name: string, changed: number | null, pending = false): Worktree => ({
  path: `/src/${name}`,
  name,
  branch: name,
  head: "a1",
  main: false,
  current: false,
  locked: null,
  missing: false,
  changed,
  statusError: null,
  pending,
});
const sub = (path: string, current: string | null, pending = false, cloned = true): Submodule => ({
  name: path,
  path,
  title: path,
  url: null,
  follows: null,
  recorded: "r1",
  committed: "r1",
  current,
  branch: null,
  cloned,
  changed: pending ? null : 0,
  statusError: null,
  ahead: 0,
  behind: 0,
  missing: false,
  pending,
});
const history = (worktrees: Worktree[], submodules: Submodule[]): History => ({ worktrees, submodules }) as History;

// Just listed: being checked, which is neither 0 nor "status unknown".
{
  const listed = tree("hotfix", null, true);
  assert.equal(checking(listed), true);
  assert.equal(checking(tree("hotfix", 2)), false);
  assert.equal(checking({ ...listed, statusError: "fatal: bad index" }), false);
}

// A reload lists the folders as pending again: they show the last reading meanwhile, still
// pending, so the new one replaces it.
{
  const shown = history([tree("main", 0), tree("hotfix", 3)], [sub("libs/a", "c1")]);
  const next = history([tree("main", 0), tree("hotfix", null, true), tree("spike", null, true)], [sub("libs/a", null, true)]);
  const carried = carryStates(next, shown);
  assert.deepEqual(
    carried.worktrees.map((t) => [t.name, t.changed, t.pending]),
    [
      ["main", 0, false],
      ["hotfix", 3, true],
      ["spike", null, true],
    ],
  );
  assert.equal(checking(carried.worktrees[1]), false);
  assert.equal(checking(carried.worktrees[2]), true);
  assert.equal(carried.submodules[0].current, "c1");
  assert.equal(carried.submodules[0].pending, true);
  // Nothing before: nothing carried.
  assert.equal(carryStates(next, null), next);
}

// Each state fills in its own folder only, and one of a folder no longer listed changes nothing.
{
  const shown = history([tree("hotfix", null, true)], [sub("libs/a", null, true)]);
  const one = withState(shown, { path: "/src/hotfix", submodule: false, changed: 2, statusError: null, checkout: null });
  assert.deepEqual([one.worktrees[0].changed, one.worktrees[0].pending], [2, false]);
  assert.equal(one.submodules[0].pending, true);
  const two = withState(one, {
    path: "libs/a",
    submodule: true,
    changed: 0,
    statusError: null,
    checkout: { current: "c0", branch: null, ahead: 0, behind: 2, missing: false },
  });
  assert.deepEqual([two.submodules[0].current, two.submodules[0].behind, two.submodules[0].pending], ["c0", 2, false]);
  // A worktree and a submodule may share a path string: the kind tells them apart.
  assert.equal(withState(two, { path: "libs/a", submodule: false, changed: 9, statusError: null, checkout: null }), two);
  assert.equal(withState(two, { path: "/src/gone", submodule: false, changed: 1, statusError: null, checkout: null }), two);
}

// When the folders can't be read at all, the ones being checked say why; earlier readings stay.
{
  const shown = history([tree("hotfix", 3, true), tree("spike", null, true)], []);
  const failed = failChecking(shown, "No repository is open");
  assert.deepEqual(
    failed.worktrees.map((t) => [t.changed, t.statusError, t.pending]),
    [
      [3, null, true],
      [null, "No repository is open", false],
    ],
  );
}
