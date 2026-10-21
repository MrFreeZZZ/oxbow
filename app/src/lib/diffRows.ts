// Side by Side: the lines of a hunk as rows of old and new, the unchanged lines between hunks
// opened a few at a time, and stepping from change to change in whatever diff is on screen,
// drawn or not.

import type { DiffLine, Hunk, SourceLine } from "./types";

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

/** Where a hunk sits on each side: how many lines come before it, and the number of its last. */
export type Span = { oldLo: number; oldHi: number; newLo: number; newHi: number };

/** Where each hunk sits, from its lines' numbers. A side a hunk has no lines on (a pure
 *  addition with no context) sits where the lines between hunks put it. */
export function spans(hunks: Hunk[]): Span[] {
  const out: Span[] = [];
  // New line numbers minus old ones, the same all through the lines between two hunks.
  let shift = 0;
  for (const hunk of hunks) {
    let firstOld: number | null = null;
    let lastOld: number | null = null;
    let firstNew: number | null = null;
    let lastNew: number | null = null;
    for (const line of hunk.lines) {
      if (line.oldLine !== null) {
        firstOld ??= line.oldLine;
        lastOld = line.oldLine;
      }
      if (line.newLine !== null) {
        firstNew ??= line.newLine;
        lastNew = line.newLine;
      }
    }
    let oldLo = firstOld !== null ? firstOld - 1 : null;
    let newLo = firstNew !== null ? firstNew - 1 : null;
    oldLo ??= (newLo ?? 0) - shift;
    newLo ??= oldLo + shift;
    const span = { oldLo, oldHi: lastOld ?? oldLo, newLo, newHi: lastNew ?? newLo };
    shift = span.newHi - span.oldHi;
    out.push(span);
  }
  return out;
}

/** Unchanged lines read so far, from the old side: runs in order, none touching another, each
 *  from its first line's number. */
export type Known = { start: number; lines: SourceLine[] }[];

/** `known` with `lines` from line `start` added. */
export function addKnown(known: Known, start: number, lines: SourceLine[]): Known {
  if (!lines.length) return known;
  const runs = [...known, { start, lines }].sort((a, b) => a.start - b.start);
  const out: Known = [];
  for (const run of runs) {
    const last = out[out.length - 1];
    const end = last ? last.start + last.lines.length : -1;
    if (last && run.start <= end) {
      // The same file, so where runs overlap their lines are the same.
      const tail = run.lines.slice(end - run.start);
      if (tail.length) out[out.length - 1] = { start: last.start, lines: [...last.lines, ...tail] };
    } else out.push(run);
  }
  return out;
}

/** What a file's diff draws in order: its hunks, the unchanged lines opened between them, and
 *  the unchanged lines still folded. `from` and `to` are old line numbers; `shift` turns them
 *  into new ones. A folded run opens from its top when something is drawn above it, and from its
 *  bottom when something is below. */
export type Piece =
  | { kind: "hunk"; h: number }
  | { kind: "shown"; from: number; hunk: Hunk }
  | { kind: "folded"; from: number; to: number; shift: number; above: boolean; below: boolean };

const shownHunks = new WeakMap<SourceLine[], Map<string, Hunk>>();

/** Lines `from` to `to` of a run read, as a hunk of unchanged lines, made once for each. */
function shownHunk(run: Known[number], from: number, to: number, shift: number): Hunk {
  let byRange = shownHunks.get(run.lines);
  if (!byRange) shownHunks.set(run.lines, (byRange = new Map()));
  const key = `${from}:${to}:${shift}`;
  let hunk = byRange.get(key);
  if (!hunk) {
    const lines: DiffLine[] = [];
    for (let n = from; n <= to; n++) {
      const line = run.lines[n - run.start];
      lines.push({ kind: "context", oldLine: n, newLine: n + shift, text: line.text, cr: line.cr, words: null });
    }
    hunk = { header: "", oldStart: from, oldLines: to - from + 1, newStart: from + shift, newLines: to - from + 1, lines };
    byRange.set(key, hunk);
  }
  return hunk;
}

/** The pieces of a diff with these hunks, as much of the old side as is `known`, and its
 *  `total` lines: unknown leaves out the lines after the last hunk. */
export function pieces(hunks: Hunk[], known: Known, total: number | null): Piece[] {
  if (!hunks.length) return [];
  const at = spans(hunks);
  const out: Piece[] = [];
  for (let g = 0; g <= hunks.length; g++) {
    const last = g === hunks.length;
    const from = g === 0 ? 1 : at[g - 1].oldHi + 1;
    const to = last ? total : at[g].oldLo;
    if (to !== null && to >= from) {
      const shift = last ? at[g - 1].newHi - at[g - 1].oldHi : at[g].newLo - at[g].oldLo;
      let n = from;
      const fold = (end: number) => {
        if (end >= n) out.push({ kind: "folded", from: n, to: end, shift, above: n > 1, below: !last || end < to });
      };
      for (const run of known) {
        const runEnd = run.start + run.lines.length - 1;
        if (runEnd < n || run.start > to) continue;
        fold(Math.min(to, run.start - 1));
        const end = Math.min(to, runEnd);
        const start = Math.max(n, run.start);
        out.push({ kind: "shown", from: start, hunk: shownHunk(run, start, end, shift) });
        n = end + 1;
      }
      fold(to);
    }
    if (!last) out.push({ kind: "hunk", h: g });
  }
  return out;
}

/** How far below the top of its list the change stepped to sits: clear of a file's sticky bar. */
const BELOW_TOP = 64;

/** The change each list was last stepped to, and where it was scrolled then. */
const lastStep = new WeakMap<HTMLElement, { block: HTMLElement; at: number; scrollTop: number }>();

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
  // Near the end of the list a change can't scroll up to the line, nor near the start down to
  // it: by position it would be picked again and again. So while the list stays where the last
  // step left it, the next step goes by order from that change.
  const last = lastStep.get(scroller);
  const from =
    last && Math.abs(scroller.scrollTop - last.scrollTop) <= 1
      ? marks.findIndex((m) => m.block === last.block && m.at === last.at)
      : -1;
  const target =
    from >= 0
      ? marks[(from + by + marks.length) % marks.length]
      : by > 0
        ? (marks.find((m) => top(m.block, m.at) > line + 4) ?? marks[0])
        : ([...marks].reverse().find((m) => top(m.block, m.at) < line - 4) ?? marks[marks.length - 1]);
  scroller.scrollTop += top(target.block, target.at) - line;
  const remember = () => lastStep.set(scroller, { block: target.block, at: target.at, scrollTop: scroller.scrollTop });
  remember();
  // A block far away is drawn once it is near, and those above it may change height as they
  // are: once they have, the change is put in its place again.
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      if (lastStep.get(scroller)?.block !== target.block || lastStep.get(scroller)?.at !== target.at) return;
      const off = top(target.block, target.at) - line;
      if (Math.abs(off) > 1) scroller.scrollTop += off;
      remember();
    }),
  );
  return true;
}
