import {
  byLine,
  changedLines,
  commitsWithSplits,
  fileState,
  pickedCounts,
  pickedInHunk,
  picksOf,
  plannedSplits,
  splitFiles,
  splitOf,
  splitPart,
  splitProblem,
  toggleFile,
  toggleHunk,
  toggleLines,
  withSplit,
  withoutSplit,
  type DraftSplit,
  type Picks,
} from "../src/lib/split.ts";
import type { Draft, Item } from "../src/lib/stack.ts";
import type { DiffLine, FileDiff, Hunk, Stack, StackCommit } from "../src/lib/types.ts";
import assert from "node:assert/strict";

const line = (kind: DiffLine["kind"], oldLine: number | null, newLine: number | null, text = ""): DiffLine => ({ kind, oldLine, newLine, text, words: null });
const hunk = (lines: DiffLine[]): Hunk => ({ header: "@@", oldStart: 1, oldLines: 0, newStart: 1, newLines: 0, lines }) as Hunk;
const file = (path: string, hunks: Hunk[], extra: Partial<FileDiff["file"]> = {}): FileDiff =>
  ({ file: { path, status: "modified", additions: 0, deletions: 0, binary: false, ...extra }, hunks, tooLarge: false, limited: false, old: null, new: null }) as FileDiff;

// app.rs: line 2 replaced, and a line added at 8; readme is a binary file.
const first = hunk([line("context", 1, 1), line("removed", 2, null), line("added", null, 2), line("context", 3, 3)]);
const second = hunk([line("context", 7, 7), line("added", null, 8)]);
const app = file("app.rs", [first, second], { additions: 2, deletions: 1 });
const logo = file("logo.png", [], { binary: true, status: "added" });
const diffs = [app, logo];

// The ids of a split's new commit.
assert.equal(splitPart("abc"), "split:abc");
assert.equal(splitOf("split:abc"), "abc");
assert.equal(splitOf("abc"), null);

// Lines of text files can be picked; binary files go whole.
assert.deepEqual(changedLines(app), { removed: [2], added: [2, 8] });
assert.equal(byLine(app), true);
assert.equal(byLine(logo), false);
assert.equal(byLine({ ...app, tooLarge: true }), false);

// Picking lines: some, then all of them makes the file whole, none takes it out.
{
  let picks: Picks = new Map();
  assert.equal(fileState(picks, app), "none");
  picks = toggleLines(picks, app, [first.lines[1]]);
  assert.deepEqual(picks.get("app.rs"), { removed: [2], added: [] });
  assert.equal(fileState(picks, app), "some");
  assert.deepEqual(pickedInHunk(picks, app, first), [1]);
  // A range goes the way its first line goes; context lines in it don't count.
  picks = toggleLines(picks, app, [first.lines[2], first.lines[3], second.lines[1]]);
  assert.deepEqual(picks.get("app.rs"), { whole: true });
  assert.equal(fileState(picks, app), "all");
  picks = toggleLines(picks, app, [first.lines[1]]);
  assert.deepEqual(picks.get("app.rs"), { removed: [], added: [2, 8] });
  picks = toggleLines(picks, app, [first.lines[2], second.lines[1]]);
  assert.equal(picks.has("app.rs"), false);
  // Only context: nothing happens.
  assert.equal(toggleLines(picks, app, [first.lines[0]]), picks);
}

// A hunk is picked whole, then left whole; a file is picked and taken out.
{
  let picks: Picks = toggleHunk(new Map(), app, first);
  assert.deepEqual(picks.get("app.rs"), { removed: [2], added: [2] });
  assert.deepEqual(pickedInHunk(picks, app, second), []);
  picks = toggleHunk(picks, app, first);
  assert.equal(picks.has("app.rs"), false);
  picks = toggleFile(picks, logo);
  assert.equal(fileState(picks, logo), "all");
  picks = toggleFile(picks, logo);
  assert.equal(fileState(picks, logo), "none");
  // Some lines picked: the checkbox picks the whole file.
  picks = toggleFile(toggleLines(new Map(), app, [first.lines[1]]), app);
  assert.deepEqual(picks.get("app.rs"), { whole: true });
}

// What the core gets, in the commit's order, with what the new commit's row shows.
{
  const picks: Picks = new Map([
    ["logo.png", { whole: true }],
    ["app.rs", { removed: [], added: [8] }],
  ]);
  assert.deepEqual(splitFiles(picks, diffs), [
    { path: "app.rs", lines: { removed: [], added: [8] } },
    { path: "logo.png", lines: null },
  ]);
  assert.deepEqual(pickedCounts(picks, diffs), { files: 2, additions: 1, deletions: 0 });
  // Every line of a file picked one by one goes as the whole file.
  assert.deepEqual(splitFiles(new Map([["app.rs", { removed: [2], added: [2, 8] }]]), diffs), [{ path: "app.rs", lines: null }]);
  // A kept split opens with its picks.
  assert.deepEqual(picksOf({ id: "x", message: "m", files: splitFiles(picks, diffs) }), picks);
}

// Why it can't be made yet.
{
  assert.match(splitProblem(new Map(), diffs, "Message")!, /Pick the changes/);
  const all: Picks = new Map([
    ["app.rs", { whole: true }],
    ["logo.png", { whole: true }],
  ]);
  assert.match(splitProblem(all, diffs, "Message")!, /Leave some changes/);
  const some: Picks = new Map([["logo.png", { whole: true }]]);
  assert.match(splitProblem(some, diffs, "  ")!, /Type a message/);
  assert.equal(splitProblem(some, diffs, "Add logo"), null);
}

// In an Edit Stack plan: the new commit sits right above the commit, and goes again.
{
  const b = (name: string): Item => ({ kind: "branch", name });
  const c = (id: string): Item => ({ kind: "commit", id });
  const shape = (d: Draft) => d.items.map((i) => (i.kind === "branch" ? `[${i.name}]` : i.id)).join(" ");
  const draft: Draft = { items: [b("top"), c("y"), c("x"), b("low"), c("w")], acts: {}, messages: {}, onto: false };
  const split: DraftSplit = { id: "x", files: [{ path: "app.rs", lines: null }], message: "Set up\n\nFirst.", additions: 3, deletions: 1 };
  let next = withSplit(draft, split);
  assert.equal(shape(next), "[top] y split:x x [low] w");
  assert.deepEqual(plannedSplits(next), [{ id: "x", files: [{ path: "app.rs", lines: null }], message: "Set up\n\nFirst." }]);
  // Moved under another branch and made again with other picks: it stays where it was moved.
  next = { ...next, items: [b("top"), c("y"), c("x"), b("low"), c("split:x"), c("w")] };
  next = withSplit(next, { ...split, message: "Set up again" });
  assert.equal(shape(next), "[top] y x [low] split:x w");
  assert.equal(next.splits!.x.message, "Set up again");
  // An action on the new commit goes with it.
  next = { ...next, acts: { "split:x": "reword" }, messages: { "split:x": "Other" } };
  const back = withoutSplit(next, "x");
  assert.equal(shape(back), "[top] y x [low] w");
  assert.deepEqual([back.acts, back.messages, back.splits], [{}, {}, {}]);
  // A commit that is not in the plan is not split.
  assert.equal(withSplit(draft, { ...split, id: "gone" }), draft);
  assert.deepEqual(plannedSplits(draft), []);

  // The rows: the new commit above, the commit with what it keeps.
  const commit = (id: string, additions: number, deletions: number): StackCommit => ({ id, summary: id, message: id, authorName: "A", time: 0, additions, deletions, branch: "top", pushed: true });
  const stack = { commits: [commit("y", 1, 0), commit("x", 5, 2), commit("w", 1, 1)] } as Stack;
  const rows = commitsWithSplits(stack, withSplit(draft, split));
  assert.deepEqual(
    rows.map((r) => [r.id, r.summary, r.additions, r.deletions, r.pushed]),
    [
      ["y", "y", 1, 0, true],
      ["split:x", "Set up", 3, 1, false],
      ["x", "x", 2, 1, true],
      ["w", "w", 1, 1, true],
    ],
  );
}
