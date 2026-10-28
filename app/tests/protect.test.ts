import { dropsPushed, isProtected, lockedCommit, matches, matching, parseProtected, protectedHead, protectedProblem, protectedValue, type Protecting } from "../src/lib/protect.ts";
import type { HistoryRow } from "../src/lib/types.ts";
import assert from "node:assert/strict";
import { test } from "node:test";

const row = (id: string, parents: string[], unpushed: boolean) => ({ id, parents, unpushed }) as HistoryRow;

/** main: a ← b ← c, where a and b are on origin and c isn't; `side` forks from a. */
function history(branch: string | null = "main", protectedList = ["main", "master"], remotes = ["origin"]): Protecting {
  return {
    protected: protectedList,
    head: { branch, commit: "c" },
    remotes,
    rows: [row("c", ["b"], true), row("side", ["a"], true), row("b", ["a"], false), row("a", [], false)],
  };
}

test("a star stands for any characters, slashes too", () => {
  assert.ok(matches("main", "main"));
  assert.ok(!matches("main", "main2"));
  assert.ok(matches("release/*", "release/2.1"));
  assert.ok(matches("release/*", "release/2/hotfix"));
  assert.ok(!matches("release/*", "releases/2"));
  assert.ok(matches("*-stable", "6.1-stable"));
  assert.ok(matches("v*.*", "v2.0"));
  assert.ok(!matches("v*.*", "v2"));
  assert.ok(!matches("a*a", "a"));
  assert.ok(isProtected(["main", "release/*"], "release/3"));
  assert.ok(!isProtected(["main"], null));
  assert.ok(!isProtected(undefined, "main"));
});

test("the setting is names separated by spaces or commas, each once", () => {
  assert.deepEqual(parseProtected(" main, release/*  main\tprod "), ["main", "release/*", "prod"]);
  assert.deepEqual(parseProtected(""), []);
  assert.equal(protectedValue(["main", "release/*"]), "main release/*");
  assert.equal(protectedValue([]), "");
});

test("only pushed commits of a protected checked-out branch are locked", () => {
  const h = history();
  assert.equal(protectedHead(h), "main");
  const [c, , b] = h.rows;
  assert.ok(lockedCommit(h, b));
  assert.ok(!lockedCommit(h, c), "not pushed yet: still editable");
  assert.ok(!lockedCommit(history("feature"), b), "another branch");
  assert.ok(!lockedCommit(history("main", []), b), "nothing protected");
  assert.ok(!lockedCommit(history("main", ["main"], []), b), "no remote, nothing pushed");
  assert.ok(!lockedCommit(history(null), b), "detached HEAD");
});

test("a reset or rebase is refused only when it takes a pushed commit off", () => {
  const h = history();
  assert.ok(!dropsPushed(h, "b"), "back to what origin has");
  assert.ok(!dropsPushed(h, "c"));
  assert.ok(dropsPushed(h, "a"), "b would go");
  assert.ok(dropsPushed(h, "side"), "onto a branch without b");
  assert.ok(dropsPushed(h, null), "a commit not loaded keeps nothing");
  assert.ok(!dropsPushed(history("feature"), "a"));
});

test("a protected name must be a branch name, once", () => {
  assert.equal(protectedProblem(["main"], "  "), "Type a branch name or a pattern");
  assert.equal(protectedProblem(["main"], "main"), "It is in the list already");
  assert.match(protectedProblem([], "my branch") ?? "", /can’t have spaces/);
  assert.match(protectedProblem([], "a..b") ?? "", /can’t/);
  assert.match(protectedProblem([], "-x") ?? "", /can’t/);
  assert.equal(protectedProblem(["main"], "release/*"), null);
});

test("the branches a pattern matches, local and remote, each name once", () => {
  const refs = ["refs/heads/main", "refs/remotes/origin/main", "refs/remotes/origin/HEAD", "refs/heads/release/1", "refs/remotes/upstream/release/2"];
  assert.deepEqual(matching("main", refs), ["main"]);
  assert.deepEqual(matching("release/*", refs), ["release/1", "release/2"]);
  assert.deepEqual(matching("prod", refs), []);
});
