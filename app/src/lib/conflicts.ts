// The Conflicts screen's model, apart from the page so it can be tested: what each conflict was
// decided as (a side, both, lines written by hand, or both changes merged when the sides changed
// different lines), the whole result as text with git's markers around the conflicts still open,
// and what kind of conflict a file has.

import type { Chunk, ConflictFile, ConflictSide, Pick, Resolution } from "./types";

/** Left is the side brought in, right the user's own work, as on the screen. */
export type Choice = "left" | "right" | "both";
/** A conflict picked, written by hand, merged by Oxbow (`merged`: both sides changed different
 *  lines, so both changes are in), or still open. */
export type Decision = Choice | { lines: string[]; merged?: true } | null;

export interface Sides {
  left: ConflictSide;
  right: ConflictSide;
  leftLabel: string;
  rightLabel: string;
}

type Conflict = Extract<Chunk, { kind: "conflict" }>;

/** A text box's value as lines: an empty box holds no lines at all, so the conflict goes. */
export const splitLines = (value: string): string[] => (value === "" ? [] : value.split("\n"));
export const joinLines = (lines: string[]): string => lines.join("\n");

/** The lines a decision puts in place of the conflict; null while it is open. */
export function linesOf(chunk: Conflict, decision: Decision, sides: Sides): string[] | null {
  if (decision === null) return null;
  if (typeof decision === "object") return decision.lines;
  const left = chunk[sides.left];
  const right = chunk[sides.right];
  return decision === "left" ? left : decision === "right" ? right : [...left, ...right];
}

/** What the backend writes for a decision. */
export function toResolution(decision: Exclude<Decision, null>, sides: Sides): Resolution {
  if (typeof decision === "object") return { lines: decision.lines };
  if (decision === "left") return sides.left;
  if (decision === "right") return sides.right;
  const both: Pick = sides.left === "theirs" ? "theirsThenOurs" : "oursThenTheirs";
  return both;
}

/** Git's markers: seven of each, named by branch, as `git merge` writes them. */
const OPEN = "<<<<<<<";
const BASE = "|||||||";
const MIDDLE = "=======";
const CLOSE = ">>>>>>>";

/** The whole result as text: decided conflicts as their lines, open ones between git's markers,
 *  with the common version too when `withBase`. */
export function markedText(chunks: Chunk[], decisions: Decision[], sides: Sides, withBase: boolean): string {
  return joinLines(markedLines(chunks, decisions, sides, withBase));
}

/** `markedText` as lines: no lines at all tells apart from one empty line. */
function markedLines(chunks: Chunk[], decisions: Decision[], sides: Sides, withBase: boolean): string[] {
  const out: string[] = [];
  let k = 0;
  for (const chunk of chunks) {
    if (chunk.kind === "same") {
      out.push(...chunk.lines);
      continue;
    }
    const lines = linesOf(chunk, decisions[k++] ?? null, sides);
    if (lines) {
      out.push(...lines);
      continue;
    }
    out.push(`${OPEN} ${sides.leftLabel}`, ...chunk[sides.left]);
    if (withBase) out.push(`${BASE} common ancestor`, ...chunk.base);
    out.push(MIDDLE, ...chunk[sides.right], `${CLOSE} ${sides.rightLabel}`);
  }
  return out;
}

/** Where each chunk starts in the result, by line number: open conflicts take no lines yet. */
export function resultStarts(chunks: Chunk[], decisions: Decision[], sides: Sides): number[] {
  const starts: number[] = [];
  let no = 1;
  let k = 0;
  for (const chunk of chunks) {
    starts.push(no);
    no += chunk.kind === "same" ? chunk.lines.length : (linesOf(chunk, decisions[k++] ?? null, sides)?.length ?? 0);
  }
  return starts;
}

/** What a side shows around the conflicts: one line of context, the rest folded. A line of a
 *  conflict knows when it is the block's last, to round it off. */
export type SideItem =
  | { kind: "line"; no: number; text: string; k: number | null; end: boolean }
  | { kind: "fold"; count: number }
  | { kind: "head"; k: number }
  | { kind: "empty"; k: number };

export function sideItems(chunks: Chunk[], side: ConflictSide | "base"): SideItem[] {
  const items: SideItem[] = [];
  let no = 1;
  let k = 0;
  chunks.forEach((chunk, ci) => {
    if (chunk.kind === "same") {
      const n = chunk.lines.length;
      // The line after a conflict and the one before the next stay; the rest fold away.
      const first = ci > 0 ? 0 : n;
      const last = ci < chunks.length - 1 ? n - 1 : -1;
      const kept = [...new Set([first, last])].filter((i) => i >= 0 && i < n).sort((a, b) => a - b);
      let at = 0;
      for (const i of kept) {
        if (i > at) items.push({ kind: "fold", count: i - at });
        items.push({ kind: "line", no: no + i, text: chunk.lines[i], k: null, end: false });
        at = i + 1;
      }
      if (n > at) items.push({ kind: "fold", count: n - at });
      no += n;
    } else {
      items.push({ kind: "head", k });
      const lines = chunk[side];
      if (!lines.length) items.push({ kind: "empty", k });
      lines.forEach((text, i) => items.push({ kind: "line", no: no++, text, k, end: i === lines.length - 1 }));
      k++;
    }
  });
  return dedent(items);
}

/** Narrow panes drop the indentation every shown line shares, like the design. */
function dedent(items: SideItem[]): SideItem[] {
  let cut = Infinity;
  for (const item of items) {
    if (item.kind !== "line" || !item.text.trim()) continue;
    cut = Math.min(cut, /^[ \t]*/.exec(item.text)![0].length);
    if (!cut) return items;
  }
  return cut === Infinity ? items : items.map((i) => (i.kind === "line" ? { ...i, text: i.text.slice(cut) } : i));
}

/** A run of a long list drawn as one block: from `start` up to `end`, about `rows` lines high,
 *  holding the heads of conflicts `first` to `last` (none when `first` is greater). */
export interface Block {
  start: number;
  end: number;
  rows: number;
  first: number;
  last: number;
}

/** A side's items in blocks of `size`: heads and folds are taller than a line. */
export function sideBlocks(items: SideItem[], size: number): Block[] {
  const blocks: Block[] = [];
  for (let start = 0; start < items.length; start += size) {
    const end = Math.min(items.length, start + size);
    let rows = 0;
    let first = Infinity;
    let last = -1;
    for (let i = start; i < end; i++) {
      const item = items[i];
      rows += item.kind === "line" || item.kind === "empty" ? 1 : 1.6;
      if (item.kind === "head") {
        first = Math.min(first, item.k);
        last = item.k;
      }
    }
    blocks.push({ start, end, rows, first, last });
  }
  return blocks;
}

/** A piece of the result drawn in a block: lines `from` up to `to` of chunk `ci`, its agreed
 *  lines or, for conflict `k`, the lines picked for it; a conflict's first piece holds its head. */
export interface Piece {
  ci: number;
  k: number;
  from: number;
  to: number;
  head: boolean;
}

/** The result in blocks of about `size` lines, the same whatever is decided: a conflict is given
 *  room for both its sides. `first` to `last` are the conflicts whose heads a block holds. */
export interface ResultBlock {
  pieces: Piece[];
  first: number;
  last: number;
}

export function resultBlocks(chunks: Chunk[], size: number): ResultBlock[] {
  const blocks: ResultBlock[] = [];
  let block: ResultBlock = { pieces: [], first: Infinity, last: -1 };
  let room = size;
  const put = (piece: Piece, lines: number) => {
    block.pieces.push(piece);
    if (piece.head) {
      block.first = Math.min(block.first, piece.k);
      block.last = piece.k;
    }
    room -= lines;
    if (room <= 0) {
      blocks.push(block);
      block = { pieces: [], first: Infinity, last: -1 };
      room = size;
    }
  };
  let k = 0;
  chunks.forEach((chunk, ci) => {
    const n = chunk.kind === "same" ? chunk.lines.length : Math.max(1, chunk.ours.length + chunk.theirs.length);
    const conflict = chunk.kind === "same" ? -1 : k++;
    // A conflict's head takes a line or two of the block, so it always gets one.
    for (let from = 0; from < n || (conflict >= 0 && from === 0); ) {
      const to = Math.min(n, from + room);
      put({ ci, k: conflict, from, to, head: conflict >= 0 && from === 0 }, to - from + (conflict >= 0 && from === 0 ? 2 : 0));
      from = to;
    }
  });
  if (block.pieces.length) blocks.push(block);
  return blocks;
}

const marker = (line: string, m: string) => line === m || line.startsWith(`${m} `);

/** What each line of marked text is: a marker, a line of one side or of the base, or plain. */
export type Role = "marker" | "left" | "base" | "right" | null;

export function markerRoles(text: string): Role[] {
  return scanned(text).roles;
}

type Scan = { lines: string[]; roles: Role[]; starts: number[] };

/** The last texts scanned: a long file's text is asked about several times for each key typed,
 *  and by the file list, so each is read once. */
const scans: { text: string; found: Scan }[] = [];

function scanned(text: string): Scan {
  const hit = scans.find((s) => s.text === text);
  if (hit) return hit.found;
  const found = scan(text);
  scans.unshift({ text, found });
  if (scans.length > 4) scans.pop();
  return found;
}

function scan(text: string): Scan {
  const lines = text.split("\n");
  const roles: Role[] = [];
  const starts: number[] = [];
  let state: Role = null;
  // Only a whole block counts: a stray `=======` in a file is just a line.
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (state === null && marker(line, OPEN) && closes(lines, i)) {
      roles.push("marker");
      starts.push(i);
      state = "left";
    } else if (state === "left" && marker(line, BASE)) {
      roles.push("marker");
      state = "base";
    } else if ((state === "left" || state === "base") && line === MIDDLE) {
      roles.push("marker");
      state = "right";
    } else if (state === "right" && marker(line, CLOSE)) {
      roles.push("marker");
      state = null;
    } else roles.push(state);
  }
  return { lines, roles, starts };
}

/** Whether the block opened at line `at` has its middle and closing markers, before another opens. */
function closes(lines: string[], at: number): boolean {
  let middle = false;
  for (let i = at + 1; i < lines.length; i++) {
    if (marker(lines[i], OPEN)) return false;
    if (lines[i] === MIDDLE) middle = true;
    else if (middle && marker(lines[i], CLOSE)) return true;
  }
  return false;
}

/** The whole file to edit as text: `markedText` with the file's last line break, so what is
 *  typed is written as it is. */
export function fileText(chunks: Chunk[], decisions: Decision[], sides: Sides, withBase: boolean, openEnd: boolean): string {
  // Counted after the decisions: conflicts whose lines were all deleted leave none, and an
  // empty result stays an empty file.
  const lines = markedLines(chunks, decisions, sides, withBase);
  const text = joinLines(lines);
  return openEnd || !lines.length ? text : `${text}\n`;
}

/** The file as the decisions or the text typed would write it, for its draft. */
export function resultOf(file: ConflictFile, decisions: Decision[], text: { text: string } | null, sides: Sides): string {
  return text ? text.text : fileText(file.chunks, decisions, sides, false, file.openEnd);
}

const MARKS = [OPEN, BASE, MIDDLE, CLOSE] as const;
/** Which of git's markers a line looks like, if any. */
export const markOf = (line: string) => MARKS.find((m) => (m === MIDDLE ? line === m : marker(line, m))) ?? null;

/** Lines that look like git's markers but are not part of a whole conflict, such as what is
 *  left of a block when one of its markers was deleted: more of them than the file's own
 *  versions hold, so a `=======` that is just part of the file never counts. */
export function strayMarkers(text: string, chunks: Chunk[]): number {
  const { lines, roles } = scanned(text);
  const own = ownMarkers(chunks);
  const stray = new Map<string, number>();
  count(
    lines.filter((_, i) => roles[i] !== "marker"),
    stray,
  );
  let n = 0;
  for (const [m, found] of stray) n += Math.max(0, found - (own.get(m) ?? 0));
  return n;
}

function count(lines: Iterable<string>, into: Map<string, number>) {
  for (const line of lines) {
    const m = markOf(line);
    if (m) into.set(m, (into.get(m) ?? 0) + 1);
  }
}

/** The marker-like lines the file's own versions hold, counted once for each file read. */
const owns = new WeakMap<Chunk[], Map<string, number>>();

function ownMarkers(chunks: Chunk[]): Map<string, number> {
  let own = owns.get(chunks);
  if (own) return own;
  own = new Map();
  for (const chunk of chunks) {
    if (chunk.kind === "same") count(chunk.lines, own);
    else for (const lines of [chunk.ours, chunk.base, chunk.theirs]) count(lines, own);
  }
  owns.set(chunks, own);
  return own;
}

/** The first line of each conflict still marked in the text. */
export const markerStarts = (text: string): number[] => scanned(text).starts;

/** Conflicts still marked in the text. */
export const openMarkers = (text: string): number => scanned(text).starts.length;

/** Going `by` from `at` among `count`, round the ends. */
export const stepIndex = (at: number, by: 1 | -1, count: number): number => (count ? (((at + by) % count) + count) % count : 0);

/** What kind of conflict a file has: a few words for the list, a sentence for the file bar. */
export function kindOf(file: ConflictFile, labelOf: (side: ConflictSide) => string): { short: string; long: string } {
  const renamed = file.renamed ? labelOf(file.renamed.side) : null;
  const from = file.renamed?.from ?? "";
  if (file.binary) return { short: "Binary file", long: "A binary file: one side’s version is taken whole." };
  if (file.link) return { short: "Link or submodule", long: "A symbolic link or submodule: one side’s version is taken whole." };
  if (!file.ours || !file.theirs) {
    const gone = labelOf(!file.ours ? "ours" : "theirs");
    const kept = labelOf(!file.ours ? "theirs" : "ours");
    return renamed
      ? { short: `Renamed on ${renamed}, deleted on ${gone}`, long: `${renamed} renamed it from ${from}, while ${gone} deleted it.` }
      : { short: `Deleted on ${gone}`, long: `${gone} deleted this file, while ${kept} changed it.` };
  }
  if (renamed) return { short: `Renamed on ${renamed}`, long: `${renamed} renamed it from ${from}, and both sides changed the same lines.` };
  if (!file.base) return { short: "Added on both sides", long: "Both sides added this file, with different text." };
  return { short: "Text", long: "Both sides changed the same lines." };
}

/** Where the Conflicts screen keeps its decisions while another screen is open; let go once no
 *  operation is in progress, so a later one never starts from them. */
export const KEPT_CONFLICTS = "conflicts";

/** The Conflicts screen, while it is open, takes ⌥F5 and ⇧⌥F5 to step through its conflicts,
 *  and Apply Non-Conflicting Changes from the palette and the Git menu. */
export const conflictSteps: { step: ((by: 1 | -1) => boolean) | null; applyAll: (() => void) | null } = { step: null, applyAll: null };

// --- Simple conflicts -------------------------------------------------------------------------
// git marks a conflict wherever both sides changed lines next to each other, even when they
// changed different lines. Those can be merged line by line, the way PyCharm's Resolve Simple
// Conflicts does: each side's own changes against the version both started from, applied
// together when no two of them touch the same lines.

/** A run of lines of the common version, `from` up to `to`, that one side replaced with `lines`. */
interface Edit {
  from: number;
  to: number;
  lines: string[];
}

/** A line of a merged conflict and where it comes from. */
export interface MergedLine {
  text: string;
  from: ConflictSide | "base";
}

/** The largest conflict, in lines of the common version times lines of a side, that is merged
 *  line by line; a longer one stays for the user. */
const MAX_CELLS = 4_000_000;

/** What `side` changed against `base`, as runs of replaced lines, in order; null when too long
 *  to work out. */
function editsOf(base: string[], side: string[]): Edit[] | null {
  // Lines both keep at the start and the end are no change; the rest is lined up by the longest
  // run of lines in common.
  let head = 0;
  while (head < base.length && head < side.length && base[head] === side[head]) head++;
  let tail = 0;
  while (tail < base.length - head && tail < side.length - head && base[base.length - 1 - tail] === side[side.length - 1 - tail]) tail++;
  const b = base.slice(head, base.length - tail);
  const s = side.slice(head, side.length - tail);
  if (!b.length && !s.length) return [];
  if ((b.length + 1) * (s.length + 1) > MAX_CELLS) return null;
  // common[i][j]: lines in common of b[i..] and s[j..].
  const width = s.length + 1;
  const common = new Uint32Array((b.length + 1) * width);
  for (let i = b.length - 1; i >= 0; i--) {
    for (let j = s.length - 1; j >= 0; j--) {
      common[i * width + j] = b[i] === s[j] ? common[(i + 1) * width + j + 1] + 1 : Math.max(common[(i + 1) * width + j], common[i * width + j + 1]);
    }
  }
  const edits: Edit[] = [];
  let open: Edit | null = null;
  let i = 0;
  let j = 0;
  const grow = () => (open ??= { from: head + i, to: head + i, lines: [] });
  while (i < b.length || j < s.length) {
    if (i < b.length && j < s.length && b[i] === s[j] && common[i * width + j] === common[(i + 1) * width + j + 1] + 1) {
      if (open) edits.push(open);
      open = null;
      i++;
      j++;
    } else if (j < s.length && (i === b.length || common[i * width + j + 1] >= common[(i + 1) * width + j])) {
      grow().lines.push(s[j++]);
    } else {
      grow();
      i++;
      open!.to = head + i;
    }
  }
  if (open) edits.push(open);
  return edits;
}

/** Whether two edits of the common version touch the same lines. One that only adds lines
 *  touches what is right before and after it: where both sides add, or one adds next to what
 *  the other changed, the order of the lines is not clear. */
function overlap(a: Edit, b: Edit): boolean {
  if (a.from === a.to || b.from === b.to) return a.from <= b.to && b.from <= a.to;
  return a.from < b.to && b.from < a.to;
}

const same = (a: Edit, b: Edit) => a.from === b.from && a.to === b.to && a.lines.length === b.lines.length && a.lines.every((l, i) => l === b.lines[i]);

/** Both sides' changes in one, when the sides of the conflict changed different lines of the
 *  version they started from; null when they touch the same lines, or a side has no version. */
export function mergeConflict(chunk: Conflict): MergedLine[] | null {
  const ours = editsOf(chunk.base, chunk.ours);
  const theirs = editsOf(chunk.base, chunk.theirs);
  if (!ours || !theirs || !ours.length || !theirs.length) return null;
  const all: { edit: Edit; from: ConflictSide }[] = [];
  for (const edit of ours) all.push({ edit, from: "ours" });
  for (const edit of theirs) {
    // The same change on both sides goes in once.
    if (ours.some((o) => same(o, edit))) continue;
    if (ours.some((o) => overlap(o, edit))) return null;
    all.push({ edit, from: "theirs" });
  }
  all.sort((a, b) => a.edit.from - b.edit.from || a.edit.to - b.edit.to);
  const out: MergedLine[] = [];
  let at = 0;
  for (const { edit, from } of all) {
    for (; at < edit.from; at++) out.push({ text: chunk.base[at], from: "base" });
    for (const text of edit.lines) out.push({ text, from });
    at = edit.to;
  }
  for (; at < chunk.base.length; at++) out.push({ text: chunk.base[at], from: "base" });
  return out;
}

/** The decision that merges a simple conflict, or null for one that isn't. */
export function mergedDecision(chunk: Chunk): Exclude<Decision, null> | null {
  if (chunk.kind !== "conflict") return null;
  const merged = mergeConflict(chunk);
  return merged ? { lines: merged.map((l) => l.text), merged: true } : null;
}

/** Whether a decision is a merge Oxbow made. */
export const isMerged = (d: Decision): d is { lines: string[]; merged: true } => typeof d === "object" && d !== null && d.merged === true;

/** Apply Non-Conflicting Changes: every open conflict of a file whose sides changed different
 *  lines gets both changes; the others stay as they are. How many it merged comes with it. */
export function mergeSimple(chunks: Chunk[], decisions: Decision[]): { decisions: Decision[]; merged: number } {
  const conflicts = chunks.filter((c): c is Conflict => c.kind === "conflict");
  let merged = 0;
  const next = conflicts.map((chunk, k) => {
    const was = decisions[k] ?? null;
    if (was !== null) return was;
    const made = mergedDecision(chunk);
    if (made) merged++;
    return made;
  });
  return { decisions: next, merged };
}

/** How many open conflicts of a file Apply Non-Conflicting Changes would merge. */
export const simpleCount = (chunks: Chunk[], decisions: Decision[]): number => mergeSimple(chunks, decisions).merged;
