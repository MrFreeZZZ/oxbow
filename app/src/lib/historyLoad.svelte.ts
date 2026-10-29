// The history of the window's repository: its first page, reloads (asked for, after an action,
// after a change outside), older pages as the list scrolls on, and the other folders' states.
// The repository itself opens and closes in repoSession.svelte.ts.

import { api, stopped } from "./api";
import type { History, HistoryRow, OutsideChange, ReadPart } from "./types";
import { reloadNews, retried, settle, stillFailing, withRows, type ReadStates } from "./reads";
import { carryStates, failChecking, withState } from "./folders";
import { Reloads } from "./loads";
import type { RepoSession } from "./repoSession.svelte";
import { activity } from "./activity.svelte";
import { confirm } from "./confirm.svelte";
import { oplog } from "./oplog.svelte";
import { lfs } from "./lfs.svelte";

/** Commits of the first load. Each load as the list scrolls on doubles what is loaded, up to
 *  `MAX_STEP` more at a time, and brings only the rows that are new. */
const PAGE = 2_000;
const MAX_STEP = 50_000;

export class HistoryLoader {
  history = $state<History | null>(null);
  /** How well each part of `history` besides the commits was read: see reads.ts. */
  reads = $state<ReadStates | null>(null);
  /** Older commits on their way: "more" for the next page, or the commit loaded down to. */
  paging = $state<string | null>(null);
  /** Current Branch Only: History shows the checked-out branch, its upstream and the trunk alone. */
  focusOnly = $state(false);
  /** Goes up on every reload of the history, so the views showing it reload too. */
  version = $state(0);
  readonly rowsById = $derived(new Map<string, HistoryRow>(this.history?.rows.map((r) => [r.id, r]) ?? []));

  #session: RepoSession;
  /** Commits to load: a page, and more once the list has scrolled on or a commit further down was asked for. */
  #limit = PAGE;
  /** Commits loaded down to that no branch reaches, so History never gets them. */
  #unreachable = new Set<string>();
  /** Past the unpushed commits a load walks, the rest are told in the background, and a reload
   *  marks them. */
  #settling = false;
  /** The next reload asks git for everything: something was done here since the last one began. */
  #all = true;
  readonly #reloads = new Reloads(
    () => this.#reloadNow(),
    () => (!this.#session.repo || this.#session.loading ? "off" : this.paging ? "paging" : "on"),
  );
  /** The page load under way: one that finishes after another started, or after the repository
   *  changed, leaves `paging` to the newer one. */
  #page: number | null = null;
  /** The page of older commits on its way, if one is. */
  #pageRun: Promise<boolean> | null = null;
  /** Since when an earlier reading of a folder no longer counts (ms): the window came back to
   *  the front or Reload was asked for, and files there may have changed with nothing in git
   *  showing it. */
  #foldersSince: number | null = null;
  /** When the reload under way, or the last one, began reading (ms). */
  #readSince = 0;
  /** Changes outside that came while the window was out of sight: read once it shows again. */
  #unseen: OutsideChange | null = null;
  /** After a reload that read something new: what the window keeps selected. */
  #reloaded: (next: History) => void;

  constructor(session: RepoSession, reloaded: (next: History) => void) {
    this.#session = session;
    this.#reloaded = reloaded;
  }

  /** A reload is under way. */
  get refreshing(): boolean {
    return !!this.#reloads.running;
  }

  /** Commits loaded down to that no branch reaches, so History never gets them. */
  get unreachable(): ReadonlySet<string> {
    return this.#unreachable;
  }

  /** Changes outside wait for the window to show again. */
  get unseen(): boolean {
    return !!this.#unseen;
  }

  /** Another repository opens: it starts on a page, with every branch. */
  reset() {
    this.#limit = PAGE;
    this.#unreachable = new Set();
    this.focusOnly = false;
  }

  /** The first page of the repository being opened, read before it shows. */
  first(): Promise<History> {
    return api.history(this.#limit);
  }

  /** The repository opened with `next`: shown, and Undo and LFS read for it. */
  opened(next: History) {
    this.#take(next, null);
    this.version++;
    oplog.load();
    lfs.load();
  }

  /** No repository: nothing to show. */
  clear() {
    this.history = null;
    this.reads = null;
  }

  /** Reload after the repository changed, here or outside the app, keeping the selection when it
   *  still exists. */
  refresh(): Promise<void> {
    this.#all = true;
    return this.#reloads.ask();
  }

  /** Reload for nothing done here that the watch wouldn't see: the window came back to the
   *  front, something changed outside, a fetch, which leaves the files alone. What the watch
   *  noticed no change in isn't asked of git again; when nothing changed, nothing is read again. */
  refreshQuietly(): Promise<void> {
    return this.#reloads.ask();
  }

  /** Reload, asked for: Activity gets what changed and the git commands it ran, and unless
   *  Activity is open to show it, so does a toast. */
  async reload() {
    this.#foldersSince = Date.now();
    const before = this.history;
    await this.refresh();
    const after = this.history;
    if (!before || !after || after === before) return;
    const news = reloadNews(before, after);
    activity.reloaded(news.text, after.reads);
    if (!activity.open) confirm.say(`Reloaded. ${news.text}.`, news.partial ? "warn" : "done");
  }

  async #reloadNow() {
    const session = this.#session;
    const mine = session.tickets.take();
    const shown = this.history;
    const quiet = !this.#all && !!shown;
    this.#all = false;
    this.#readSince = Date.now();
    try {
      const next = withRows(await api.history(this.#limit, null, shown?.key ?? null, this.focusOnly, quiet), shown);
      // Another repository was opened meanwhile: this answer may come from the old one.
      if (!session.tickets.current(mine) || session.loading) return;
      this.#take(next, shown);
      // Nothing changed: the views, Undo and LFS have it all already.
      if (!next.unchanged) {
        this.version++;
        oplog.load();
        lfs.load();
      }
      this.#reloaded(next);
    } catch (err) {
      // What was done here is still to be read.
      if (!quiet) this.#all = true;
      if (session.tickets.current(mine) && session.repo) await session.failed(session.repo.path, err, mine);
      else if (session.tickets.current(mine)) session.error = String(err);
    }
  }

  /** Show `next`, read after `shown`: a part it couldn't read keeps `shown`'s reading, marked
   *  stale, or is not known (see reads.ts). What the other folders have comes after it. */
  #take(next: History, shown: History | null) {
    const settled = settle(carryStates(next, shown), shown, shown ? this.reads : null, Date.now());
    this.history = settled.history;
    this.reads = settled.states;
    this.#loadFolders();
  }

  /** Read what the other worktrees and the submodules' folders have, after the history: each
   *  row fills in as its folder is read. A newer call stops this one. */
  async #loadFolders() {
    const history = this.history;
    if (!history || (!history.worktrees.some((t) => t.pending) && !history.submodules.some((s) => s.pending))) return;
    const tickets = this.#session.tickets;
    const mine = tickets.take();
    try {
      await api.folderStates(this.#foldersSince, (state) => {
        if (tickets.current(mine) && this.history) this.history = withState(this.history, state);
      });
    } catch (err) {
      if (tickets.current(mine) && this.history && !stopped(err)) this.history = failChecking(this.history, String(err));
    }
  }

  /** Past the unpushed commits a load walks, the rest are told in the background; a reload
   *  marks them. */
  settleUnpushed() {
    const h = this.history;
    if (!h?.unpushedPending || this.#settling) return;
    const tickets = this.#session.tickets;
    const mine = tickets.take();
    this.#settling = true;
    const ids = h.rows.filter((r) => !r.unpushed && !r.worktree).map((r) => r.id);
    api.settleUnpushed(ids, "history-unpushed").then(
      (found) => {
        this.#settling = false;
        if (found && tickets.current(mine)) this.refreshQuietly();
      },
      () => (this.#settling = false),
    );
  }

  /** The window came back to the front: what changed meanwhile, also in the other folders. */
  cameBack() {
    this.#foldersSince = Date.now();
    this.showsAgain();
  }

  /** Something changed in the repository outside this window: a terminal, an editor, another
   *  window. Read it now, also while the window is behind others; branches, HEAD, the stashes
   *  or what is staged moving goes to Activity. The selection and the scrolled-to commit stay. */
  async outsideChanged(change: OutsideChange) {
    if (!this.#session.repo || !this.history) return;
    // This window's own action and the reload after it go first: what they changed is no news.
    while (confirm.running || this.#reloads.running) await (this.#reloads.running ?? new Promise((done) => setTimeout(done, 200)));
    // A reload that began after it sees it already, e.g. the one after an action here.
    if (this.#readSince > change.at) return;
    if (document.visibilityState === "hidden") {
      const unseen = this.#unseen;
      this.#unseen = unseen ? { git: unseen.git || change.git, files: unseen.files || change.files, at: Math.max(unseen.at, change.at), window: change.window ?? unseen.window } : change;
      return;
    }
    const before = this.history;
    const tickets = this.#session.tickets;
    const mine = tickets.take();
    await this.refreshQuietly();
    const after = this.history;
    if (!change.git || !after || after === before || !tickets.current(mine)) return;
    const news = reloadNews(before, after);
    if (news.text !== "Nothing new") activity.outside(news.text, after.reads, change.window);
  }

  /** The window shows again: minimized, hidden or covered by other windows before, which needn't
   *  bring it to the front. What changed meanwhile is read as it would have been. */
  showsAgain() {
    const change = this.#unseen;
    this.#unseen = null;
    if (change) this.outsideChanged(change);
    else this.refreshQuietly();
  }

  /** Read one part again after it failed. The worktrees and submodules are read alone; the
   *  rest come with the history. */
  async retryRead(part: ReadPart) {
    if (!this.reads || !this.history) return;
    const tickets = this.#session.tickets;
    const mine = tickets.take();
    this.reads = { ...this.reads, [part]: { ...this.reads[part], phase: "loading" } };
    if (part !== "worktrees" && part !== "submodules") {
      const error = this.reads[part].error ?? "";
      await this.refresh();
      // No new reading came (the reload failed, or waits for older commits): as it was.
      if (tickets.current(mine) && this.reads?.[part].phase === "loading") this.reads = stillFailing(this.reads, part, error);
      return;
    }
    try {
      const patch = part === "worktrees" ? { worktrees: await api.worktrees() } : { submodules: await api.submodules() };
      if (!tickets.current(mine) || !this.history || !this.reads) return;
      const done = retried(this.history, this.reads, part, patch, Date.now());
      this.history = done.history;
      this.reads = done.states;
    } catch (err) {
      if (tickets.current(mine) && this.reads) this.reads = stillFailing(this.reads, part, String(err));
    }
  }

  /** Load the next page of older commits, or with `until` the history down to that commit.
   *  Whether `until` is in History now. */
  loadOlder(until: string | null = null): Promise<boolean> {
    const run = this.#loadPage(until);
    if (this.paging) {
      this.#pageRun = run;
      run.finally(() => this.#pageRun === run && (this.#pageRun = null));
    }
    return run;
  }

  async #loadPage(until: string | null): Promise<boolean> {
    const session = this.#session;
    if (!session.repo || session.loading || this.paging || !this.history?.truncated) return false;
    const mine = session.tickets.take();
    this.#page = mine.requestId;
    this.paging = until ?? "more";
    try {
      // A reload already on its way would land after this one, with fewer commits.
      while (this.#reloads.running) await this.#reloads.running;
      const shown = this.history;
      const limit = this.#limit;
      const next = withRows(await api.history(until ? limit : limit + Math.min(limit, MAX_STEP), until, shown?.key ?? null, this.focusOnly), shown);
      if (!session.tickets.current(mine) || session.loading) return false;
      this.#limit = Math.max(this.#limit, next.loaded);
      this.#take(next, shown);
      const found = !!until && next.rows.some((r) => r.id === until);
      if (until && !found) this.#unreachable.add(until);
      return found;
    } catch (err) {
      if (session.tickets.current(mine)) session.error = String(err);
      return false;
    } finally {
      if (this.#page === mine.requestId) {
        this.#page = null;
        this.paging = null;
        this.#reloads.paged();
      }
    }
  }

  /** Make sure History has the commit, loading older commits down to it when it is further down
   *  than what is loaded. Whether History has it. */
  async ensureLoaded(id: string): Promise<boolean> {
    if (this.rowsById.has(id)) return true;
    // A page on its way may bring it, and the next one can start only after it.
    if (this.#pageRun) await this.#pageRun;
    if (this.rowsById.has(id)) return true;
    if (!this.history?.truncated || this.#unreachable.has(id)) return false;
    return this.loadOlder(id);
  }

  /** Another repository, or none: older commits on their way belong to the old one. */
  stopPaging() {
    this.#page = null;
    this.paging = null;
    this.#reloads.forgetAfterPage();
    this.#settling = false;
    api.stopReads(["history-unpushed"]).catch(() => {});
  }
}
