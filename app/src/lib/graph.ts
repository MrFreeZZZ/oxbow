// The graph's straight lines come as runs over many rows (see `Run`), apart from each row's own
// segments. This puts a row's lines back together for drawing.

import type { History, Run, Segment } from "./types";

/** Rows per bucket of the run index. */
const BLOCK = 64;

/** The runs over each block of rows, to find the ones over a row without going through them all. */
export class RunIndex {
  #blocks: Run[][] = [];

  constructor(runs: Run[]) {
    for (const run of runs) {
      for (let b = Math.floor(run.first / BLOCK); b <= Math.floor(run.last / BLOCK); b++) (this.#blocks[b] ??= []).push(run);
    }
  }

  /** The runs over `row`, in their order. */
  at(row: number): Run[] {
    return (this.#blocks[Math.floor(row / BLOCK)] ?? []).filter((run) => run.first <= row && row <= run.last);
  }
}

const indexes = new WeakMap<History, RunIndex>();

/** Every line of row `row` of `history`: the runs over it, then its own segments. */
export function segmentsAt(history: History, row: number): Segment[] {
  let index = indexes.get(history);
  if (!index) indexes.set(history, (index = new RunIndex(history.runs)));
  const own = history.rows[row]?.graph.segments ?? [];
  const runs = index.at(row);
  if (!runs.length) return own;
  return [...runs.map((r) => ({ from: r.column, to: r.column, color: r.color, dashed: r.dashed, thick: r.thick })), ...own];
}
