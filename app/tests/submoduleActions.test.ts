import { submoduleActions } from "../src/lib/submoduleActions.ts";
import type { Submodule } from "../src/lib/types.ts";
import assert from "node:assert/strict";

const sub = (change: Partial<Submodule> = {}): Submodule => ({
  name: "libs/lint",
  path: "libs/lint",
  title: "lint",
  url: "git@github.com:acme/lint.git",
  follows: null,
  recorded: "a1b2c3d4e5",
  committed: "a1b2c3d4e5",
  current: "a1b2c3d4e5",
  branch: "main",
  cloned: true,
  changed: 0,
  statusError: null,
  ahead: 0,
  behind: 0,
  missing: false,
  onNoBranch: 0,
  pending: false,
  ...change,
});
const shown = (s: Submodule, checking = false) => submoduleActions(s, checking).map((a) => [a.id, a.label, a.off ?? null, a.caution?.title ?? null]);

// At the recorded commit: Pull Latest is there, as on the card and in the menu alike.
assert.deepEqual(shown(sub()), [["latest", "Pull Latest", null, null]]);
assert.match(submoduleActions(sub({ follows: "release" }), false)[0].about, /^Newest commit of release/);

// Not cloned: Clone; Pull Latest is listed, with why it can't run yet.
assert.deepEqual(shown(sub({ cloned: false, current: null, branch: null })), [
  ["clone", "Clone", null, null],
  ["latest", "Pull Latest", "Clone lint first: Pull Latest works in its folder.", null],
]);

// Being checked: neither runs yet, both say why.
{
  const wait = "Oxbow is still checking what lint’s folder has.";
  assert.deepEqual(shown(sub({ pending: true, current: null, changed: null }), true), [
    ["update", "Update to a1b2c3d", wait, null],
    ["latest", "Pull Latest", wait, null],
  ]);
}

// Behind or ahead: going back to what is recorded, or on to the newest commit, told apart.
{
  const both = submoduleActions(sub({ current: "f0f0f0f0", behind: 2 }), false);
  assert.deepEqual(both.map((a) => [a.id, !!a.primary, a.off ?? null]), [
    ["update", true, null],
    ["latest", false, null],
  ]);
  assert.equal(both[0].about, "Back to the commit this repository records");
  assert.equal(submoduleActions(sub({ current: "f0f0f0f0", missing: true }), false)[0].about, "Fetch the commit this repository records");
}

// Commits made on no branch: both would leave them, and both say so, the same way.
{
  const actions = submoduleActions(sub({ current: "f0f0f0f0", branch: null, ahead: 2, onNoBranch: 2 }), false);
  assert.deepEqual(actions.map((a) => a.caution?.title), ["2 commits on no branch", "2 commits on no branch"]);
  assert.match(actions[0].caution!.text, /only its reflog has them: open it in a new window and make a branch first/);
}

// Changed files inside never stop either from being offered: git stops before overwriting them.
assert.deepEqual(shown(sub({ changed: 3, current: "f0f0f0f0" })), [
  ["update", "Update to a1b2c3d", null, null],
  ["latest", "Pull Latest", null, null],
]);
