// What the diff column of Changes shows, and how it follows the files as they are staged,
// unstaged and committed.

import type { FileChange, Side } from "./types";

/** One file on one side of the staging area, or every file of a side: the unstaged edits, the
 *  new files git does not track yet (`untracked`), or the next commit. */
export type Shown = { kind: "file"; path: string; side: Side } | { kind: "all"; side: Side; untracked?: boolean };

export interface Lists {
  unstaged: FileChange[];
  staged: FileChange[];
  conflicted: FileChange[];
}

export const sameShown = (a: Shown | null, b: Shown | null): boolean =>
  a === b || (!!a && !!b && a.kind === b.kind && a.side === b.side && (a.kind === "all" ? !!a.untracked === !!(b as typeof a).untracked : a.path === (b as typeof a).path));

/** What Changes opens on: the edits not staged yet, else the next commit, else the first new file. */
export function firstShown(lists: Lists): Shown | null {
  if (lists.conflicted.length || lists.unstaged.some((f) => f.status !== "untracked")) return { kind: "all", side: "unstaged" };
  if (lists.staged.length) return { kind: "all", side: "staged" };
  const fresh = lists.unstaged[0];
  return fresh ? { kind: "file", path: fresh.path, side: "unstaged" } : null;
}

/** What stays shown after the files were read again. A file stays on its side while it has
 *  changes there; staged or unstaged whole, it is shown where it went. A file with no changes
 *  left gives way to what Changes opens on. Every file of a side stays, even with none left:
 *  the column then says so. */
export function keepShown(shown: Shown | null, lists: Lists): Shown | null {
  if (!shown) return firstShown(lists);
  if (shown.kind === "all") return shown;
  const has = (side: Side) => (side === "staged" ? lists.staged : [...lists.conflicted, ...lists.unstaged]).some((f) => f.path === shown.path);
  if (has(shown.side)) return shown;
  const other: Side = shown.side === "staged" ? "unstaged" : "staged";
  if (has(other)) return { kind: "file", path: shown.path, side: other };
  return firstShown(lists);
}

/** The one `by` away from `shown` in `seq`, round from the last to the first when `wrap` is on;
 *  undefined past either end, or when `seq` is empty. */
export function stepShown(seq: Shown[], shown: Shown | null, by: 1 | -1, wrap: boolean): Shown | undefined {
  if (!seq.length) return undefined;
  const at = seq.findIndex((s) => sameShown(s, shown));
  const to = at < 0 ? (by > 0 ? 0 : seq.length - 1) : at + by;
  if (to < 0 || to >= seq.length) return wrap ? seq[(to + seq.length) % seq.length] : undefined;
  return seq[to];
}

/** The files picked after a Shift-click on `path`: every file from `anchor` to it is added. When
 *  either is not in `order` (folded away, filtered out), only `path` is. */
export function pickRange(order: string[], picked: string[], anchor: string | null, path: string): string[] {
  const from = anchor === null ? -1 : order.indexOf(anchor);
  const to = order.indexOf(path);
  const span = from >= 0 && to >= 0 ? order.slice(Math.min(from, to), Math.max(from, to) + 1) : [path];
  const have = new Set(picked);
  return [...picked, ...span.filter((p) => !have.has(p))];
}

/** The file Stage & Next File opens after staging `path`: the one below it in `order`, else the
 *  one above; null when `path` was the only one. */
export function nextAfter(order: string[], path: string): string | null {
  const at = order.indexOf(path);
  if (at < 0) return order[0] ?? null;
  return order[at + 1] ?? order[at - 1] ?? null;
}
