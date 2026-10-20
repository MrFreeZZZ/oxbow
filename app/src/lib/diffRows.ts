// Side by Side: the lines of a hunk as rows of old and new, and stepping from change to change
// in whatever diff is on screen, drawn or not.

import type { DiffLine, Hunk } from "./types";

/** A row of Side by Side: the line on the old side and the line on the new side, by index in
 *  the hunk. An unchanged line is on both sides; null leaves that side empty. */
export type Row = { left: number | null; right: number | null };

/** Rows of the first `count` lines: removed lines face the added lines that follow them, one to
 *  one, and whichever run is longer leaves the other side empty. */
export function pairRows(lines: DiffLine[], count = lines.length): Row[] {
  const rows: Row[] = [];
  let i = 0;
  while (i < count) {
    if (lines[i].kind === "context") {
      rows.push({ left: i, right: i });
      i++;
      continue;
    }
    const removed: number[] = [];
    const added: number[] = [];
    while (i < count && lines[i].kind === "removed") removed.push(i++);
    while (i < count && lines[i].kind === "added") added.push(i++);
    for (let k = 0; k < Math.max(removed.length, added.length); k++) rows.push({ left: removed[k] ?? null, right: added[k] ?? null });
  }
  return rows;
}

const paired = new WeakMap<DiffLine[], Map<number, Row[]>>();

/** `pairRows` of a hunk, made once for each hunk and count. */
export function rowsOf(hunk: Hunk, count = hunk.lines.length): Row[] {
  let byCount = paired.get(hunk.lines);
  if (!byCount) paired.set(hunk.lines, (byCount = new Map()));
  let rows = byCount.get(count);
  if (!rows) byCount.set(count, (rows = pairRows(hunk.lines, count)));
  return rows;
}

/** An unchanged line is the same line on both sides. */
export const changedRow = (row: Row) => row.left !== row.right;

/** Where changes start among items `start` to `end`, counted from `start`: an item that changes
 *  right after one that doesn't, or first in its hunk. */
export function changeStarts(changed: (i: number) => boolean, start: number, end: number): number[] {
  const out: number[] = [];
  for (let i = start; i < end; i++) if (changed(i) && (i === 0 || !changed(i - 1))) out.push(i - start);
  return out;
}

/** What the old side of a row shows, and the new side: a kind of line, or nothing. */
export function sideKind(lines: DiffLine[], row: Row | undefined, side: "left" | "right"): DiffLine["kind"] | "empty" | null {
  if (!row) return null;
  const at = side === "left" ? row.left : row.right;
  return at === null ? "empty" : lines[at].kind;
}

/** How far below the top of its list the change stepped to sits: clear of a file's sticky bar. */
const BELOW_TOP = 64;

function scrollerOf(el: Element): HTMLElement | null {
  for (let up = el.parentElement; up; up = up.parentElement) {
    const overflow = getComputedStyle(up).overflowY;
    if (overflow === "auto" || overflow === "scroll") return up;
  }
  return null;
}

/** Scroll the diff on screen to its next change, or previous one, round from the last to the
 *  first as in VS Code. The diff is the one the focus is in, or else the first one shown. False
 *  when no diff with changes is shown. */
export function stepChange(by: 1 | -1): boolean {
  const all = [...document.querySelectorAll<HTMLElement>("[data-marks]")].filter((el) => el.getClientRects().length > 0);
  if (!all.length) return false;
  const focused = document.activeElement;
  const scrollers = all.map(scrollerOf);
  const scroller = scrollers.find((s) => s && focused && s.contains(focused)) ?? scrollers.find((s) => s) ?? null;
  if (!scroller) return false;
  const blocks = all.filter((_, i) => scrollers[i] === scroller);

  /** Where a mark is on screen: its row if the block is drawn, else about where it will be. */
  const top = (block: HTMLElement, at: number): number => {
    const row = block.children[at];
    if (row) return row.getBoundingClientRect().top;
    const line = parseFloat(getComputedStyle(block).lineHeight);
    return block.getBoundingClientRect().top + at * (Number.isFinite(line) && line > 0 ? line : 18);
  };
  const marks = blocks.flatMap((block) => block.dataset.marks!.split(",").map((at) => ({ block, at: Number(at) })));
  const line = scroller.getBoundingClientRect().top + BELOW_TOP;
  const target =
    by > 0
      ? (marks.find((m) => top(m.block, m.at) > line + 4) ?? marks[0])
      : ([...marks].reverse().find((m) => top(m.block, m.at) < line - 4) ?? marks[marks.length - 1]);
  scroller.scrollTop += top(target.block, target.at) - line;
  // A block far away is drawn once it is near, and those above it may change height as they
  // are: once they have, the change is put in its place again.
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      const off = top(target.block, target.at) - line;
      if (Math.abs(off) > 1) scroller.scrollTop += off;
    }),
  );
  return true;
}
