// Compare counts the lines of its files once per key (the two sides and the diff settings), in
// one read lane. Which read of the diffs starts that count is decided here: the last one started
// for the key, once its first page comes. An answer that comes late for an older read starts
// nothing, so it can't stop the count of a newer comparison.

export class CountTurns {
  private owner: { key: string; run: number; started: boolean } | null = null;
  private runs = 0;

  /** A read of the diffs for `key` starts; it counts the lines, unless they are `counted`
   *  already or a count for the key runs. Returns the read's number. */
  read(key: string, counted: boolean): number {
    const run = ++this.runs;
    if (!counted && !(this.owner?.key === key && this.owner.started)) this.owner = { key, run, started: false };
    return run;
  }

  /** Whether read `run` starts the count for `key` now, while `current` is the key shown. */
  start(key: string, run: number, current: string): boolean {
    if (this.owner?.run !== run || this.owner.started || key !== current) return false;
    this.owner = { key, run, started: true };
    return true;
  }

  /** The count of read `run` ended, answered or not: a failed one is asked for again by the
   *  next read. */
  end(run: number) {
    if (this.owner?.run === run) this.owner = null;
  }
}
