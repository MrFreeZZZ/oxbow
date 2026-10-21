import { addKnown, changeStarts, changedRow, pairRows, pieces, sideKind, spans, stepChange } from "../src/lib/diffRows.ts";
import type { DiffLine, Hunk } from "../src/lib/types.ts";
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

// Unchanged lines between hunks: where each hunk sits, and what is folded or opened around it.
{
  const at = (kind: DiffLine["kind"], oldLine: number | null, newLine: number | null): DiffLine => ({ kind, oldLine, newLine, text: "", words: null });
  const hunk = (...lines: DiffLine[]): Hunk => ({ header: "", oldStart: 0, oldLines: 0, newStart: 0, newLines: 0, lines });
  // Lines 4-6 with line 5 replaced by two, then lines 20-22 with 21 removed.
  const hunks = [
    hunk(at("context", 4, 4), at("removed", 5, null), at("added", null, 5), at("added", null, 6), at("context", 6, 7)),
    hunk(at("context", 20, 21), at("removed", 21, null), at("context", 22, 22)),
  ];
  assert.deepEqual(spans(hunks), [
    { oldLo: 3, oldHi: 6, newLo: 3, newHi: 7 },
    { oldLo: 19, oldHi: 22, newLo: 20, newHi: 22 },
  ]);
  // A pure addition without context sits where the lines before it put it, on the old side.
  assert.deepEqual(spans([hunk(at("added", null, 11), at("added", null, 12))]), [{ oldLo: 10, oldHi: 10, newLo: 10, newHi: 12 }]);

  const kinds = (ps: ReturnType<typeof pieces>) =>
    ps.map((p) => (p.kind === "hunk" ? `h${p.h}` : p.kind === "shown" ? `shown ${p.from}-${p.from + p.hunk.lines.length - 1}` : `folded ${p.from}-${p.to}${p.above ? " above" : ""}${p.below ? " below" : ""}`));
  // Nothing read yet, the file's length unknown: no run after the last hunk.
  assert.deepEqual(kinds(pieces(hunks, [], null)), ["folded 1-3 below", "h0", "folded 7-19 above below", "h1"]);
  assert.deepEqual(kinds(pieces(hunks, [], 30)), ["folded 1-3 below", "h0", "folded 7-19 above below", "h1", "folded 23-30 above"]);

  const lines = (n: number) => Array.from({ length: n }, (_, i) => ({ text: `l${i}` }));
  // Opened under the first hunk and over the second; the rest of the run stays folded.
  let known = addKnown([], 7, lines(3));
  known = addKnown(known, 17, lines(3));
  const ps = pieces(hunks, known, 30);
  assert.deepEqual(kinds(ps), ["folded 1-3 below", "h0", "shown 7-9", "folded 10-16 above below", "shown 17-19", "h1", "folded 23-30 above"]);
  // Opened lines carry both numbers: one more line on the new side after the first hunk.
  const shown = ps[2].kind === "shown" ? ps[2].hunk.lines[0] : null;
  assert.deepEqual([shown?.oldLine, shown?.newLine, shown?.kind], [7, 8, "context"]);
  // The same lines give the same hunk, so its rows are paired once.
  const again = pieces(hunks, known, 30)[2];
  assert.equal(again.kind === "shown" && ps[2].kind === "shown" && again.hunk === ps[2].hunk, true);
  // Runs that meet become one; overlapping ones keep each line once.
  known = addKnown(known, 9, lines(10));
  assert.deepEqual(known.map((r) => [r.start, r.lines.length]), [[7, 13]]);
  assert.deepEqual(kinds(pieces(hunks, known, 30)), ["folded 1-3 below", "h0", "shown 7-19", "h1", "folded 23-30 above"]);
  // A run read under the last hunk, with the file ending after it.
  assert.deepEqual(kinds(pieces(hunks, addKnown(known, 23, lines(8)), 30)).slice(-2), ["h1", "shown 23-30"]);
  assert.deepEqual(pieces([], [], 10), []);
}
