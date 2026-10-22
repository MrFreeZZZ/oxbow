// Saves of the Conflicts screen's draft (drafts.ts), one after another in the order they were
// made, and outliving the screen: leaving it, opening another repository or closing the window
// starts the save still waiting for typing to pause, and closing waits until it is written. Each
// save names its working folder when it is made, so one that runs after the window opened
// another repository still goes to its own. Only a folder's newest draft is written: an older
// save still queued is skipped. The newest, when it failed, is kept for Try Again.

import type { Draft } from "./drafts";

/** The commands that keep a working folder's draft (api.ts). */
export interface DraftApi {
  saveConflictDraft(workdir: string, draft: Draft): Promise<void>;
  dropConflictDraft(workdir: string): Promise<void>;
}

/** What became of a save, for the screen that made it: written, removed (nothing left to keep),
 *  or failed and why. */
export type DraftNote = { kind: "saved"; at: number } | { kind: "dropped"; at: number } | { kind: "failed"; why: string };

interface Save {
  folder: string;
  /** Which of its folder's drafts this is: each save made gets the next number, so a newer one
   *  is always known, wherever it is (waiting, queued or being written). */
  rev: number;
  make: () => Draft | null;
  told: (note: DraftNote) => void;
}

export class DraftSaves {
  #api: DraftApi;
  /** The save waiting for typing to pause. */
  #waiting: Save | null = null;
  #timer: ReturnType<typeof setTimeout> | undefined;
  /** Saves under way, so an older one never lands over a newer. */
  #queue: Promise<void> = Promise.resolve();
  /** The newest save made of each folder: only it is written, the ones before it are skipped. */
  #newest = new Map<string, number>();
  /** Saves queued or being written, so Try Again doesn't add one twice. */
  #queued = new Set<Save>();
  /** The newest save of each folder, when it failed: what closing waits for and Try Again makes
   *  again. A newer save of the folder takes its place. */
  #failed = new Map<string, Save & { why: string }>();

  constructor(api: DraftApi) {
    this.#api = api;
  }

  /** Save the draft `make` gives for `folder` once nothing new came for `ms`; it replaces the
   *  save waiting before it. `told` hears how it went. */
  later(folder: string, make: () => Draft | null, ms: number, told: (note: DraftNote) => void = () => {}) {
    const rev = (this.#newest.get(folder) ?? 0) + 1;
    this.#newest.set(folder, rev);
    this.#waiting = { folder, rev, make, told };
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => void this.flush(), ms);
  }

  /** Start the waiting save now. Resolves once every save made so far is done, written or not. */
  flush(): Promise<void> {
    clearTimeout(this.#timer);
    const save = this.#waiting;
    this.#waiting = null;
    if (save) this.#run(save);
    return this.#queue;
  }

  /** Flush and wait up to `ms`: null once every draft is written, else why one isn't. */
  async settle(ms: number): Promise<string | null> {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const late = new Promise<"late">((resolve) => (timer = setTimeout(() => resolve("late"), ms)));
    const done = await Promise.race([this.flush().then(() => "done" as const), late]);
    clearTimeout(timer);
    if (done === "late") return `it was still being written after ${Math.round(ms / 1000)} seconds`;
    return this.#failed.values().next().value?.why ?? null;
  }

  /** Make the saves that failed again: only a folder's newest, and not while it is queued or
   *  being written. */
  retry() {
    for (const failed of [...this.#failed.values()]) if (!this.#stale(failed) && !this.#queued.has(failed)) this.#run(failed);
  }

  /** A newer save of its folder was made since: that one is written instead. */
  #stale(save: Save): boolean {
    return save.rev < (this.#newest.get(save.folder) ?? 0);
  }

  #run(save: Save) {
    this.#queued.add(save);
    this.#queue = this.#queue.then(async () => {
      if (this.#stale(save)) return void this.#queued.delete(save);
      const at = Date.now();
      try {
        const draft = save.make();
        if (draft) await this.#api.saveConflictDraft(save.folder, draft);
        else await this.#api.dropConflictDraft(save.folder);
        if ((this.#failed.get(save.folder)?.rev ?? 0) <= save.rev) this.#failed.delete(save.folder);
        save.told(draft ? { kind: "saved", at } : { kind: "dropped", at });
      } catch (err) {
        const why = String(err);
        // One a newer save was made of while it ran: that one is written next and says how it went.
        if (!this.#stale(save)) this.#failed.set(save.folder, { ...save, why });
        save.told({ kind: "failed", why });
      }
      this.#queued.delete(save);
    });
  }
}
