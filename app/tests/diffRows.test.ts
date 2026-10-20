import { changeStarts, changedRow, pairRows, sideKind } from "../src/lib/diffRows.ts";
import type { DiffLine } from "../src/lib/types.ts";
import assert from "node:assert/strict";

const line = (kind: DiffLine["kind"], text = ""): DiffLine => ({ kind, oldLine: null, newLine: null, text, words: null });
const c = line("context");
const r = line("removed");
const a = line("added");

// Removed lines face the added lines after them; the longer run leaves the other side empty.
{
  const lines = [c, r, r, a, c, a, c, r];
  assert.deepEqual(pairRows(lines), [
    { left: 0, right: 0 },
    { left: 1, right: 3 },
    { left: 2, right: null },
    { left: 4, right: 4 },
    { left: null, right: 5 },
    { left: 6, right: 6 },
    { left: 7, right: null },
  ]);
  // A hunk cut short pairs only the lines drawn.
  assert.deepEqual(pairRows(lines, 2), [
    { left: 0, right: 0 },
    { left: 1, right: null },
  ]);
  // More added lines than removed ones.
  assert.deepEqual(pairRows([r, a, a]), [
    { left: 0, right: 1 },
    { left: null, right: 2 },
  ]);
}

// An unchanged line is the same line on both sides; the sides say what they show.
{
  const lines = [c, r, a];
  const rows = pairRows([c, r, r, a]);
  assert.equal(changedRow(rows[0]), false);
  assert.equal(changedRow(rows[1]), true);
  assert.equal(changedRow(rows[2]), true);
  assert.equal(sideKind(lines, { left: 1, right: null }, "right"), "empty");
  assert.equal(sideKind(lines, { left: 1, right: 2 }, "left"), "removed");
  assert.equal(sideKind(lines, undefined, "left"), null);
}

// Changes start after an unchanged line or first in the hunk, counted from the block's start.
{
  const kinds = [true, true, false, true, false, false, true, true];
  const changed = (i: number) => kinds[i];
  assert.deepEqual(changeStarts(changed, 0, kinds.length), [0, 3, 6]);
  // A run that started in the block before is no new change.
  assert.deepEqual(changeStarts(changed, 1, 4), [2]);
  assert.deepEqual(changeStarts(changed, 7, 8), []);
}
