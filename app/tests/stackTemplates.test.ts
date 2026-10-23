import { messageNote, moveToBranch, neighborBranch, squashWip, wipOf } from "../src/lib/stackTemplates.ts";
import type { Draft, Item } from "../src/lib/stack.ts";
import assert from "node:assert/strict";

const b = (name: string): Item => ({ kind: "branch", name });
const c = (id: string): Item => ({ kind: "commit", id });
const shape = (items: Item[]) => items.map((i) => (i.kind === "branch" ? `[${i.name}]` : i.id)).join(" ");
const draft = (items: Item[]): Draft => ({ items, acts: {}, messages: {}, onto: false });

// What subjects count as work in progress.
{
  assert.equal(wipOf("WIP"), "wip");
  assert.equal(wipOf("wip: banner"), "wip");
  assert.equal(wipOf("..."), "wip");
  assert.equal(wipOf("Wipe the cache"), null);
  assert.deepEqual(wipOf("fixup! fixup! Add banner"), { fold: "fixup", of: "Add banner" });
  assert.deepEqual(wipOf("squash! Add banner"), { fold: "squash", of: "Add banner" });
  assert.equal(wipOf("Add banner"), null);
}

// Squash WIP: a fixup! goes next to the commit it names, across a branch line; a plain WIP folds
// into the commit below it, unless a branch line is in between.
{
  const commits = [
    { id: "f", summary: "fixup! Add models" },
    { id: "w", summary: "wip" },
    { id: "a", summary: "Add banner" },
    { id: "x", summary: "WIP more" },
    { id: "m", summary: "Add models" },
  ];
  const start = draft([b("top"), c("f"), c("w"), c("a"), b("low"), c("x"), b("base"), c("m")]);
  const { draft: next, folded, left } = squashWip(start, commits);
  assert.equal(shape(next.items), "[top] w a [low] x [base] f m");
  assert.deepEqual(next.acts, { f: "fixup", w: "fixup" });
  assert.equal(folded, 2);
  assert.deepEqual(left, ["x"]);
  // The draft it started from is left alone.
  assert.equal(shape(start.items), "[top] f w a [low] x [base] m");
}

// A dropped commit is skipped: the WIP folds into the next kept one.
{
  const commits = [
    { id: "w", summary: "WIP" },
    { id: "d", summary: "Old idea" },
    { id: "a", summary: "Add banner" },
  ];
  const start: Draft = { ...draft([b("top"), c("w"), c("d"), c("a")]), acts: { d: "drop" } };
  const { draft: next, left } = squashWip(start, commits);
  assert.equal(next.acts.w, "fixup");
  assert.deepEqual(left, []);
}

// Fix Messages' notes.
{
  assert.equal(messageNote("Add session banner"), null);
  assert.match(messageNote("wip")!, /Work in progress/);
  assert.match(messageNote("fix")!, /little/);
  assert.match(messageNote("Add the banner.")!, /period/);
  assert.equal(messageNote("Bump to 1.2..."), null);
  assert.match(messageNote("x".repeat(80))!, /80 characters/);
}

// Move to Branch: down it becomes the newest commit of the branch below, up the oldest of the one above.
{
  const items = [b("top"), c("1"), c("2"), b("mid"), c("3"), c("4"), b("low"), c("5")];
  assert.equal(neighborBranch(items, "4", 1), "low");
  assert.equal(neighborBranch(items, "4", -1), "top");
  assert.equal(neighborBranch(items, "1", -1), null);
  assert.equal(neighborBranch(items, "5", 1), null);
  assert.equal(shape(moveToBranch(items, "3", 1)!), "[top] 1 2 [mid] 4 [low] 3 5");
  assert.equal(shape(moveToBranch(items, "4", -1)!), "[top] 1 2 4 [mid] 3 [low] 5");
  assert.equal(shape(moveToBranch(items, "2", 1)!), "[top] 1 [mid] 2 3 4 [low] 5");
  assert.equal(moveToBranch(items, "1", -1), null);
}
