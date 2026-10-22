// Saves of the Conflicts screen's draft (drafts.ts), one after another in the order they were
// made, and outliving the screen: leaving it, opening another repository or closing the window
// starts the save still waiting for typing to pause, and closing waits until it is written. Each
// save names its working folder when it is made, so one that runs after the window opened
// another repository still goes to its own. A save that failed is kept for Try Again.

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
  /** The last save of each folder that failed, until one of that folder goes through. */
  #failed = new Map<string, Save & { why: string }>();

  constructor(api: DraftApi) {
    this.#api = api;
  }

  /** Save the draft `make` gives for `folder` once nothing new came for `ms`; it replaces the
   *  save waiting before it. `told` hears how it went. */
  later(folder: string, make: () => Draft | null, ms: number, told: (note: DraftNote) => void = () => {}) {
    this.#waiting = { folder, make, told };
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

  /** Make the saves that failed again, each unless a newer one of its folder is waiting. */
  retry() {
    for (const failed of [...this.#failed.values()]) if (this.#waiting?.folder !== failed.folder) this.#run(failed);
  }

  #run(save: Save) {
    this.#queue = this.#queue.then(async () => {
      const at = Date.now();
      try {
        const draft = save.make();
        if (draft) await this.#api.saveConflictDraft(save.folder, draft);
        else await this.#api.dropConflictDraft(save.folder);
        this.#failed.delete(save.folder);
        save.told(draft ? { kind: "saved", at } : { kind: "dropped", at });
      } catch (err) {
        const why = String(err);
        this.#failed.set(save.folder, { ...save, why });
        save.told({ kind: "failed", why });
      }
    });
  }
}
