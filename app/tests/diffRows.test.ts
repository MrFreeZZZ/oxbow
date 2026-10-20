import { changeStarts, changedRow, pairRows, sideKind, stepChange } from "../src/lib/diffRows.ts";
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

// Next Change goes round from the last change to the first even when the last one can't scroll
// up to where steps put it, and Previous from the first to the last.
{
  // A list 400 high over 1000 of diff, changes at 100, 500 and 990: it scrolls at most to 600.
  const frames: (() => void)[] = [];
  const g = globalThis as Record<string, unknown>;
  const scroller = {
    top: 0,
    get scrollTop() {
      return this.top;
    },
    set scrollTop(to: number) {
      this.top = Math.max(0, Math.min(600, to));
    },
    parentElement: null,
    getBoundingClientRect: () => ({ top: 0 }),
    contains: () => false,
  };
  const at = [100, 500, 990];
  const block = {
    dataset: { marks: "0,1,2" },
    parentElement: scroller,
    children: at.map((y) => ({ getBoundingClientRect: () => ({ top: y - scroller.top }) })),
    getClientRects: () => [{}],
    getBoundingClientRect: () => ({ top: -scroller.top }),
  };
  g.document = { querySelectorAll: () => [block], activeElement: null };
  g.getComputedStyle = (el: unknown) => ({ overflowY: el === scroller ? "auto" : "visible", lineHeight: "18px" });
  g.requestAnimationFrame = (f: () => void) => frames.push(f);
  const step = (by: 1 | -1) => {
    assert.equal(stepChange(by), true);
    while (frames.length) frames.shift()!();
    return scroller.scrollTop;
  };
  assert.deepEqual([step(1), step(1), step(1), step(1), step(1)], [36, 436, 600, 36, 436]);
  assert.deepEqual([step(-1), step(-1), step(-1)], [36, 600, 436]);
  // Scrolled by hand, the next step goes by what is on screen again.
  scroller.scrollTop = 0;
  assert.equal(step(1), 36);
  scroller.scrollTop = 450;
  assert.equal(step(1), 600);
  delete g.document;
  delete g.getComputedStyle;
  delete g.requestAnimationFrame;
}
