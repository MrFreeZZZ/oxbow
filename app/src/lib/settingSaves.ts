// How a window keeps its settings in step with settings.json while it saves them too. The backend
// numbers every change it writes, so a window takes a change only when it is newer than what it
// has for that key, whatever order the words arrive in. Saves of one key run one after another,
// in the order they were made, so the last one made is the one that stays.

/** Whether a change numbered `revision` is newer than what the window has for `key`: its own
 *  number in `revisions`, else the number settings.json was loaded at. */
export function isNewer(revisions: Record<string, number>, loaded: number, key: string, revision: number): boolean {
  return revision > (revisions[key] ?? loaded);
}

/** The values after loading `values`, read at change `revision`: a key changed since keeps the
 *  value it has now. */
export function loadedOver(now: Record<string, unknown>, revisions: Record<string, number>, revision: number, values: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const key of new Set([...Object.keys(now), ...Object.keys(values)])) {
    const value = (revisions[key] ?? -1) > revision ? now[key] : values[key];
    if (value !== undefined) out[key] = value;
  }
  return out;
}

/** Runs the saves of each key one after another, in the order they were made. A save that a
 *  newer one of the same key replaced before its turn is skipped and resolves to `skipped`. */
export class SaveQueue {
  #tails = new Map<string, Promise<unknown>>();
  #newest = new Map<string, number>();
  #count = 0;

  /** `work` gets whether its save is still the newest of its key, asked as it finishes. */
  run<T>(key: string, work: (newest: () => boolean) => Promise<T>, skipped: T): Promise<T> {
    const id = ++this.#count;
    this.#newest.set(key, id);
    const newest = () => this.#newest.get(key) === id;
    const next = (this.#tails.get(key) ?? Promise.resolve()).then(() => (newest() ? work(newest) : skipped));
    // A save that fails mustn't hold up the next one.
    this.#tails.set(key, next.catch(() => {}));
    return next;
  }
}
