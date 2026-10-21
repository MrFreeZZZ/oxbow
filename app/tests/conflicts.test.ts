import { fileText, joinLines, kindOf, markedText, markerRoles, markerStarts, openMarkers, splitLines, stepIndex, strayMarkers, toResolution, type Sides } from "../src/lib/conflicts.ts";
import type { Chunk, ConflictFile } from "../src/lib/types.ts";
import assert from "node:assert/strict";

// A merge: main brings in feature; on the screen feature is left, main (yours) right.
const sides: Sides = { left: "theirs", right: "ours", leftLabel: "feature", rightLabel: "main" };
const chunks: Chunk[] = [
  { kind: "same", lines: ["one"] },
  { kind: "conflict", ours: ["2 on main"], base: ["two"], theirs: ["TWO on feature"] },
  { kind: "same", lines: ["three"] },
  { kind: "conflict", ours: ["4"], base: ["four"], theirs: [] },
];

// An empty box is no lines; a box with one empty line can't be told from it, and goes too.
{
  assert.deepEqual(splitLines(""), []);
  assert.deepEqual(splitLines("a\n"), ["a", ""]);
  assert.equal(joinLines(splitLines("a\n\nb")), "a\n\nb");
}

// Open conflicts get git's markers, named by branch; decided ones their lines.
{
  const text = markedText(chunks, [null, { lines: ["4, by hand"] }], sides, false);
  assert.equal(text, ["one", "<<<<<<< feature", "TWO on feature", "=======", "2 on main", ">>>>>>> main", "three", "4, by hand"].join("\n"));
  assert.equal(openMarkers(text), 1);
  assert.deepEqual(markerStarts(text), [1]);
  assert.deepEqual(markerRoles(text), [null, "marker", "left", "marker", "right", "marker", null, null]);

  const withBase = markedText(chunks, ["both", null], sides, true);
  assert.equal(withBase.split("\n").slice(0, 4).join("|"), "one|TWO on feature|2 on main|three");
  // feature removed line four, so its part of the block is empty.
  assert.deepEqual(markerRoles(withBase).slice(4), ["marker", "marker", "base", "marker", "right", "marker"]);
  assert.equal(openMarkers(withBase), 1);
}

// A stray `=======` or an opening marker that never closes is just a line.
{
  assert.equal(openMarkers("a\n=======\nb"), 0);
  assert.equal(openMarkers("<<<<<<< x\na\n=======\nb"), 0);
  assert.equal(openMarkers("<<<<<<< x\na\n=======\nb\n>>>>>>> y\n<<<<<<< x\n=======\n>>>>>>> y"), 2);
}

// A block missing any one of its markers is no conflict, but what is left of it is stray, so
// the file isn't ready; a `=======` the file itself has is not.
{
  const whole = markedText(chunks, [null, "right"], sides, false).split("\n");
  assert.equal(strayMarkers(whole.join("\n"), chunks), 0);
  for (const gone of ["<<<<<<< feature", "=======", ">>>>>>> main"]) {
    const text = whole.filter((l) => l !== gone).join("\n");
    assert.equal(openMarkers(text), 0, gone);
    assert.equal(strayMarkers(text, chunks), 2, gone);
  }
  const own: Chunk[] = [{ kind: "same", lines: ["Title", "======="] }, ...chunks];
  assert.equal(strayMarkers("Title\n=======\none\n2 on main\nthree\n4", own), 0);
  assert.equal(strayMarkers("Title\n=======\none\n=======\nthree\n4", own), 1);
}

// Text to edit ends with the file's own last line break, so it is written as it is.
{
  const decided = markedText(chunks, ["left", "right"], sides, false);
  assert.equal(fileText(chunks, ["left", "right"], sides, false, false), `${decided}\n`);
  assert.equal(fileText(chunks, ["left", "right"], sides, false, true), decided);
  assert.equal(fileText([{ kind: "same", lines: [""] }], [], sides, false, false), "\n");
  assert.equal(fileText([], [], sides, false, false), "");
}

// Both, as it reads on screen: the left side first.
{
  assert.equal(toResolution("both", sides), "theirsThenOurs");
  assert.equal(toResolution("left", sides), "theirs");
  assert.deepEqual(toResolution({ lines: [] }, sides), { lines: [] });
  const rebase: Sides = { left: "ours", right: "theirs", leftLabel: "main", rightLabel: "auth" };
  assert.equal(toResolution("both", rebase), "oursThenTheirs");
}

// Stepping goes round the ends.
{
  assert.equal(stepIndex(2, 1, 3), 0);
  assert.equal(stepIndex(0, -1, 3), 2);
  assert.equal(stepIndex(0, 1, 0), 0);
}

// The kind of conflict, by branch.
{
  const file = (over: Partial<ConflictFile>): ConflictFile => ({ path: "a", base: true, ours: true, theirs: true, binary: false, link: false, text: true, renamed: null, chunks: [], ...over });
  const label = (side: "ours" | "theirs") => (side === "ours" ? "main" : "feature");
  assert.equal(kindOf(file({}), label).short, "Text");
  assert.equal(kindOf(file({ base: false }), label).short, "Added on both sides");
  assert.equal(kindOf(file({ theirs: false }), label).long, "feature deleted this file, while main changed it.");
  assert.equal(kindOf(file({ renamed: { from: "old.rs", side: "theirs" } }), label).short, "Renamed on feature");
  assert.equal(kindOf(file({ ours: false, renamed: { from: "old.rs", side: "theirs" } }), label).short, "Renamed on feature, deleted on main");
  assert.equal(kindOf(file({ binary: true }), label).short, "Binary file");
}
