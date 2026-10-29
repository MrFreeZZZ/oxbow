import { countedFile, firstShown, keepShown, lineCounts, nextAfter, pickRange, stepShown, type Lists, type Shown } from "../src/lib/changesView.ts";
import type { FileChange } from "../src/lib/types.ts";
import assert from "node:assert/strict";

const file = (path: string, status: FileChange["status"] = "modified") => ({ path, status, additions: 1, deletions: 0 }) as FileChange;
const lists = (unstaged: FileChange[], staged: FileChange[] = [], conflicted: FileChange[] = []): Lists => ({ unstaged, staged, conflicted });

// Changes opens on the edits, else the next commit, else the first new file.
assert.deepEqual(firstShown(lists([file("a")], [file("b")])), { kind: "all", side: "unstaged" });
assert.deepEqual(firstShown(lists([], [], [file("c", "conflicted")])), { kind: "all", side: "unstaged" });
assert.deepEqual(firstShown(lists([file("new", "untracked")], [file("b")])), { kind: "all", side: "staged" });
assert.deepEqual(firstShown(lists([file("new", "untracked")])), { kind: "file", path: "new", side: "unstaged" });
assert.equal(firstShown(lists([])), null);

// A file staged whole is shown in Staged; staged in part, it stays where it was.
const open: Shown = { kind: "file", path: "a", side: "unstaged" };
assert.deepEqual(keepShown(open, lists([], [file("a")])), { kind: "file", path: "a", side: "staged" });
assert.deepEqual(keepShown(open, lists([file("a")], [file("a")])), open);
// Committed or discarded, it gives way to what Changes opens on.
assert.deepEqual(keepShown(open, lists([file("b")])), { kind: "all", side: "unstaged" });
// Selected files stay while on their side; one left is shown alone, none gives its list.
const two: Shown = { kind: "files", paths: ["a", "b"], side: "unstaged" };
assert.equal(keepShown(two, lists([file("a"), file("b")])), two);
assert.deepEqual(keepShown(two, lists([file("b")], [file("a")])), { kind: "file", path: "b", side: "unstaged" });
assert.deepEqual(keepShown({ kind: "files", paths: ["a", "b"], side: "unstaged", untracked: true }, lists([], [file("a"), file("b")])), { kind: "all", side: "unstaged", untracked: true });
// Every file of a side stays, even with none left.
assert.deepEqual(keepShown({ kind: "all", side: "unstaged" }, lists([], [file("a")])), { kind: "all", side: "unstaged" });

// Stepping goes by the list and round, or stops at the ends.
const seq: Shown[] = [{ kind: "all", side: "unstaged" }, { kind: "file", path: "a", side: "unstaged" }, { kind: "all", side: "staged" }];
assert.deepEqual(stepShown(seq, seq[0], 1, false), seq[1]);
assert.deepEqual(stepShown(seq, seq[2], 1, true), seq[0]);
assert.equal(stepShown(seq, seq[2], 1, false), undefined);
assert.equal(stepShown(seq, seq[0], -1, false), undefined);
assert.deepEqual(stepShown(seq, { kind: "file", path: "gone", side: "staged" }, -1, false), seq[2]);
assert.equal(stepShown([], null, 1, true), undefined);
// All the new files are a step of their own, apart from the unstaged edits.
const fresh: Shown[] = [{ kind: "all", side: "unstaged" }, { kind: "all", side: "unstaged", untracked: true }, { kind: "all", side: "staged" }];
assert.deepEqual(stepShown(fresh, { kind: "all", side: "unstaged", untracked: true }, 1, false), fresh[2]);
assert.deepEqual(stepShown(fresh, { kind: "all", side: "unstaged" }, 1, false), fresh[1]);

// Shift-click adds the files between, in either direction, and keeps those picked before.
const order = ["a", "b", "c", "d"];
assert.deepEqual(pickRange(order, [], "b", "d"), ["b", "c", "d"]);
assert.deepEqual(pickRange(order, ["d"], "c", "a"), ["d", "a", "b", "c"]);
assert.deepEqual(pickRange(order, ["a"], null, "c"), ["a", "c"]);
assert.deepEqual(pickRange(order, [], "hidden", "c"), ["c"]);

// Stage & Next File opens the file below, else the one above.
assert.equal(nextAfter(order, "b"), "c");
assert.equal(nextAfter(order, "d"), "c");
assert.equal(nextAfter(["a"], "a"), null);
assert.equal(nextAfter(order, "gone"), "a");

// Listed before their lines are counted, files never look empty: "…" until the counts or their
// diff come, whose counts the row takes.
{
  const edited = { path: "a", status: "modified", additions: 0, deletions: 0 } as FileChange;
  const none = new Map<string, FileChange>();
  assert.equal(countedFile(edited, false, none, "unstaged"), null);
  assert.equal(lineCounts(countedFile(edited, false, none, "unstaged")), "…");
  const shown = new Map([["unstaged:a", { ...edited, additions: 3, deletions: 1 }]]);
  assert.equal(lineCounts(countedFile(edited, false, shown, "unstaged")), "+3 −1");
  // The diff of the other side is not this row's.
  assert.equal(lineCounts(countedFile(edited, false, shown, "staged")), "…");
  // Counted, a file with no changed lines says so; a conflicted file has nothing to wait for.
  assert.equal(lineCounts(countedFile(edited, true, none, "unstaged")), "no lines");
  assert.equal(countedFile({ ...edited, status: "conflicted" }, false, none, "unstaged")?.path, "a");
}
