// The Conflicts screen's model, apart from the page so it can be tested: what each conflict was
// decided as (a side, both, or lines written by hand), the whole result as text with git's
// markers around the conflicts still open, and what kind of conflict a file has.

import type { Chunk, ConflictFile, ConflictSide, Pick, Resolution } from "./types";

/** Left is the side brought in, right the user's own work, as on the screen. */
export type Choice = "left" | "right" | "both";
/** A conflict picked, written by hand, or still open. */
export type Decision = Choice | { lines: string[] } | null;

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

const marker = (line: string, m: string) => line === m || line.startsWith(`${m} `);

/** What each line of marked text is: a marker, a line of one side or of the base, or plain. */
export type Role = "marker" | "left" | "base" | "right" | null;

export function markerRoles(text: string): Role[] {
  return scan(text).roles;
}

function scan(text: string): { roles: Role[]; starts: number[] } {
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
  return { roles, starts };
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
  const roles = markerRoles(text);
  const count = (lines: Iterable<string>, into: Map<string, number>) => {
    for (const line of lines) {
      const m = markOf(line);
      if (m) into.set(m, (into.get(m) ?? 0) + 1);
    }
  };
  const own = new Map<string, number>();
  for (const chunk of chunks) {
    if (chunk.kind === "same") count(chunk.lines, own);
    else for (const lines of [chunk.ours, chunk.base, chunk.theirs]) count(lines, own);
  }
  const stray = new Map<string, number>();
  count(
    text.split("\n").filter((_, i) => roles[i] !== "marker"),
    stray,
  );
  let n = 0;
  for (const [m, found] of stray) n += Math.max(0, found - (own.get(m) ?? 0));
  return n;
}

/** The first line of each conflict still marked in the text. */
export const markerStarts = (text: string): number[] => scan(text).starts;

/** Conflicts still marked in the text. */
export const openMarkers = (text: string): number => scan(text).starts.length;

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

/** The Conflicts screen, while it is open, takes ⌥F5 and ⇧⌥F5 to step through its conflicts. */
export const conflictSteps: { step: ((by: 1 | -1) => boolean) | null } = { step: null };
