// What Changes reads from git: the files and the changelists with each reload, their changed
// lines once counted, and the diffs of the diff column, a page at a time.

import { api, stopped } from "./api";
import type { Changelists, FileDiff, Side, WorkingTree } from "./types";
import type { Grouped } from "./changelists";
import { sameShown, type Shown } from "./changesView";

/** Lines of diff a page of every file of a side brings, about what the diff shows at first. */
const PAGE_LINES = 4000;
/** Lanes of the diff reads of each side: a newer read in a lane stops the older one. */
const LANES: Record<Side | "counts", string> = { unstaged: "changes-unstaged", staged: "changes-staged", counts: "changes-counts" };

/** What the diffs shown were asked for: later pages must be of the same. */
interface Ask {
  side: Side;
  paths: string[];
  whole: boolean;
  reading: number | null;
  settings: string;
  round: number;
  target: Shown;
}

function sameAsk(a: Ask, b: Ask) {
  return sameShown(a.target, b.target) && a.side === b.side && a.whole === b.whole && a.reading === b.reading && a.settings === b.settings && a.round === b.round && a.paths.length === b.paths.length && a.paths.every((p, i) => p === b.paths[i]);
}

export class ChangesReads {
  tree = $state<WorkingTree | null>(null);
  /** The changelists, read with the files; null until read, or when they can't be. */
  lists = $state<Changelists | null>(null);
  error = $state<string | null>(null);
  /** Goes up with each reading of the files, so the diffs are read again. */
  loads = $state(0);
  /** The diffs in the diff column, and what they were read for: until a new read is in, the
   *  column keeps the old diffs, with their own side's actions. */
  diffs = $state<FileDiff[]>([]);
  diffsOf = $state<Shown | null>(null);
  /** Where the next page of diffs starts (null once all are loaded), and how many files are still to come. */
  more = $state<number | null>(null);
  pending = $state(0);

  /** Which reading of the files `tree` is, for diffs of the same one; null for one of its own. */
  #reading: number | null = null;
  #asked: Ask | null = null;
  #loadingMore = false;
  #listed: (next: WorkingTree) => void;
  #diffsIn: () => void;

  /** `listed`: a new reading of the files is in, for what is shown and ticked to follow it.
   *  `diffsIn`: new diffs replaced the shown ones. */
  constructor(listed: (next: WorkingTree) => void, diffsIn: () => void) {
    this.#listed = listed;
    this.#diffsIn = diffsIn;
  }

  /** Show the files of the reload, `given` as reading `snapshot`, or read them when it has none. */
  async load(given: WorkingTree | null, snapshot: number) {
    try {
      const read = given ? snapshot : null;
      // The same reading again, counted here already: its counts stay.
      const kept = given && !given.counted && read === this.#reading && this.tree?.counted ? this.tree : null;
      const next = kept ?? given ?? (await api.workingTree());
      let listsError: string | null = null;
      const found = await api.changelists(read).catch((err) => ((listsError = String(err)), null));
      this.#reading = read;
      this.tree = next;
      this.lists = found;
      this.error = listsError;
      this.#listed(next);
      this.loads++;
      if (!next.counted && read !== null) this.#countLines(read);
    } catch (err) {
      this.error = String(err);
    }
  }

  /** The changed lines of the files of reading `read`, which the history lists before they are
   *  counted: the list shows at once, and the counts come after. */
  async #countLines(read: number) {
    const counted = await api.workingCounts(read, LANES.counts).catch(() => null);
    // A newer reading has counts of its own coming.
    if (counted && this.#reading === read) this.tree = counted;
  }

  /** Read the diffs of `target` for the diff column; `needle` filters every file of a side. Runs
   *  in an effect: it reads again when what it reads changes. */
  show(target: Shown | null, wholeFile: boolean, settings: string, grouped: Grouped, needle: () => string) {
    const round = this.loads;
    const t = this.tree;
    const g = grouped;
    if (!t || !target) {
      this.#asked = null;
      this.diffs = [];
      this.diffsOf = target;
      return;
    }
    const n = target.kind === "all" ? needle() : "";
    const pool =
      target.side === "staged"
        ? t.staged
        : target.kind !== "all"
          ? [...t.conflicted, ...t.unstaged]
          : target.list
            ? (g.lists.find((l) => l.list.id === target.list)?.files ?? [])
            : target.untracked
              ? g.untracked
              : [...t.conflicted, ...g.tracked];
    const chosen = target.kind === "files" ? new Set(target.paths) : null;
    const paths = pool
      .filter((f) => (target.kind === "file" ? f.path === target.path : chosen ? chosen.has(f.path) : !n || f.path.toLowerCase().includes(n) || !!f.oldPath?.toLowerCase().includes(n)))
      .map((f) => f.path);
    const ask: Ask = { side: target.side, paths, whole: wholeFile, reading: this.#reading, settings, round, target };
    // The counts of the same reading came in: the diffs shown or on their way are still the ones asked for.
    if (this.#asked && sameAsk(this.#asked, ask)) return;
    this.#asked = ask;
    const first = paths.length ? api.workingDiffPage(paths, target.side, wholeFile, this.#reading, 0, PAGE_LINES, LANES[target.side]) : Promise.resolve({ diffs: [], next: null, files: 0 });
    first.then(
      (page) => {
        if (this.#asked !== ask) return;
        this.diffs = page.diffs;
        this.diffsOf = target;
        this.more = page.next;
        this.pending = page.files - page.diffs.length;
        this.#diffsIn();
      },
      (err) => {
        if (this.#asked === ask && !stopped(err)) this.error = String(err);
      },
    );
  }

  /** The next page of the diffs of every file of a side. */
  loadMore() {
    const ask = this.#asked;
    const from = this.more;
    if (!ask || from === null || this.#loadingMore) return;
    this.#loadingMore = true;
    api.workingDiffPage(ask.paths, ask.side, ask.whole, ask.reading, from, PAGE_LINES, LANES[ask.side]).then(
      (page) => {
        this.#loadingMore = false;
        if (this.#asked !== ask || this.more !== from) return;
        this.diffs = [...this.diffs, ...page.diffs];
        this.more = page.next;
        this.pending = Math.max(0, this.pending - page.diffs.length);
      },
      (err) => {
        this.#loadingMore = false;
        if (this.#asked === ask && !stopped(err)) this.error = String(err);
      },
    );
  }

  /** The panel closes: its reads stop, their answers would go nowhere. */
  stop() {
    api.stopReads(Object.values(LANES)).catch(() => {});
  }
}
