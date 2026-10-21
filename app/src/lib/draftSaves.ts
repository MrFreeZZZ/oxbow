// Saves of the Conflicts screen's draft (drafts.ts), one after another in the order they were
// made, and outliving the screen: leaving it, opening another repository or closing the window
// starts the save still waiting for typing to pause, and closing waits until it is written.

export class DraftSaves {
  /** The save waiting for typing to pause. */
  #waiting: (() => Promise<void>) | null = null;
  #timer: ReturnType<typeof setTimeout> | undefined;
  /** Saves under way, so an older one never lands over a newer. */
  #queue: Promise<void> = Promise.resolve();

  /** Save with `save` once nothing new came for `ms`; it replaces the save waiting before it. */
  later(save: () => Promise<void>, ms: number) {
    this.#waiting = save;
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => void this.flush(), ms);
  }

  /** Start the waiting save now. Resolves once every save made so far is done, written or not. */
  flush(): Promise<void> {
    clearTimeout(this.#timer);
    const save = this.#waiting;
    this.#waiting = null;
    if (save) this.#queue = this.#queue.then(save).catch(() => {});
    return this.#queue;
  }
}

export const draftSaves = new DraftSaves();
