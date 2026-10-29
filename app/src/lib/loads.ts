// The loads of one window's repository (repoSession.svelte.ts and historyLoad.svelte.ts): which
// answers still count, and reloads of the history one at a time.

/** Where the window's repository is: none open, opening, shown, or the last open failed. */
export type Phase = "idle" | "loading" | "ready" | "failed";

/** A request: the repository open it was made for (`repoId`, one per open or close) and its own
 *  number (`requestId`, one per request of any kind). */
export interface Ticket {
  readonly repoId: number;
  readonly requestId: number;
}

/** The tickets of one window. An answer counts only while its repository is the one open: one
 *  to a reload started before another repository opened, or before an earlier open finished,
 *  is dropped, so the window never shows one repository's commits while the backend has another. */
export class Tickets {
  #repo = 0;
  #request = 0;

  /** Another repository opens, or none: every request made before is for the old one. */
  switch(): Ticket {
    this.#repo++;
    return this.take();
  }

  /** A request for the repository open now. */
  take(): Ticket {
    return { repoId: this.#repo, requestId: ++this.#request };
  }

  /** Whether the answer to `ticket` is still for the repository open now. */
  current(ticket: Ticket): boolean {
    return ticket.repoId === this.#repo;
  }
}

/** Whether a reload can run now: not without a repository or while one opens ("off"), and not
 *  while older commits are on their way ("paging"): it follows them, with the new limit. */
export type ReloadGate = "on" | "off" | "paging";

/** Reloads of the history. One runs at a time: asked for meanwhile (focus, a fetch, an action
 *  finishing), one more follows it, for everyone who asked, so that it sees what changed. */
export class Reloads {
  #running: Promise<void> | null = null;
  #queued: Promise<void> | null = null;
  #afterPage = false;
  #run: () => Promise<void>;
  #gate: () => ReloadGate;

  constructor(run: () => Promise<void>, gate: () => ReloadGate) {
    this.#run = run;
    this.#gate = gate;
  }

  /** The reload under way, if one is. */
  get running(): Promise<void> | null {
    return this.#running;
  }

  /** Reload now, or after the reload under way. */
  ask(): Promise<void> {
    const gate = this.#gate();
    if (gate === "off") return Promise.resolve();
    if (gate === "paging") {
      this.#afterPage = true;
      return Promise.resolve();
    }
    if (this.#queued) return this.#queued;
    if (this.#running) {
      const queued = this.#running.then(() => {
        if (this.#queued === queued) this.#queued = null;
        return this.ask();
      });
      this.#queued = queued;
      return queued;
    }
    const done = this.#run().finally(() => {
      if (this.#running === done) this.#running = null;
    });
    this.#running = done;
    return done;
  }

  /** The page of older commits is in: a reload asked for meanwhile runs now. */
  paged() {
    if (!this.#afterPage) return;
    this.#afterPage = false;
    this.ask();
  }

  /** Paging stopped for another repository: a reload asked for meanwhile was for the old one. */
  forgetAfterPage() {
    this.#afterPage = false;
  }
}
