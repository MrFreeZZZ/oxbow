import { firstShown, keepShown, nextAfter, pickRange, stepShown, type Lists, type Shown } from "../src/lib/changesView.ts";
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
