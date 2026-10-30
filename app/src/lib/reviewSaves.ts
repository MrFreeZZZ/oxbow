// A review editor's captured address, initial read and ordered writes. The queue outlives the
// screen; every write replaces the revision read or written just before it, including deletion.
import type { ReviewDraft, ReviewDraftState, ReviewTarget } from "./types.ts";

export interface ReviewDraftApi {
  reviewDraft(target: ReviewTarget, number: number): Promise<ReviewDraftState>;
  saveReviewDraft(target: ReviewTarget, number: number, draft: ReviewDraft | null, expected: string): Promise<string>;
}

type Edit = (draft: ReviewDraft | null) => ReviewDraft | null;
const pending = new Set<ReviewSaves>();

export class ReviewSaves {
  readonly target: ReviewTarget;
  readonly number: number;
  state: ReviewDraftState | null = null;
  #api: ReviewDraftApi;
  #apply: (draft: ReviewDraft | null) => void;
  #fail: (err: unknown) => void;
  #value: ReviewDraft | null = null;
  #edits: Edit[] = [];
  #version = 0;
  #saved = 0;
  #loaded: Promise<void> | null = null;
  #queue = Promise.resolve();
  #timer: ReturnType<typeof setTimeout> | undefined;

  constructor(api: ReviewDraftApi, target: ReviewTarget, number: number, apply: (draft: ReviewDraft | null) => void, fail: (err: unknown) => void) {
    this.#api = api;
    this.target = Object.freeze({ workdir: target.workdir, place: Object.freeze({ ...target.place }) });
    this.number = number;
    this.#apply = apply;
    this.#fail = fail;
    void this.ready().catch(fail);
  }

  ready(): Promise<void> {
    if (this.state) return Promise.resolve();
    if (this.#loaded) return this.#loaded;
    const loaded = this.#api.reviewDraft(this.target, this.number).then(state => {
      // Replay local intentions over the disk draft, preserving comments not yet read.
      const value = this.#edits.reduce((draft, edit) => edit(draft), state.draft);
      this.state = state;
      this.#value = value;
      this.#edits = [];
      this.#apply(value);
    });
    this.#loaded = loaded;
    void loaded.then(() => { this.#loaded = null; }, () => { this.#loaded = null; });
    return loaded;
  }

  edit(change: Edit, pause = 0) {
    this.#version++;
    if (!this.state) this.#edits.push(change);
    this.#value = change(this.#value);
    this.#apply(this.#value);
    pending.add(this);
    clearTimeout(this.#timer);
    if (pause) this.#timer = setTimeout(() => void this.flush().catch(this.#fail), pause);
    else void this.flush().catch(this.#fail);
  }

  flush(): Promise<void> {
    clearTimeout(this.#timer);
    const run = this.#queue.then(async () => {
      await this.ready();
      if (this.#saved === this.#version) return;
      const version = this.#version;
      const revision = await this.#api.saveReviewDraft(this.target, this.number, this.#value, this.state!.revision);
      this.state = { ...this.state!, revision };
      this.#saved = version;
      if (this.#saved === this.#version) pending.delete(this);
    });
    this.#queue = run.catch(() => {});
    return run;
  }

  async clear() {
    await this.flush();
    const run = this.#queue.then(async () => {
      const version = this.#version;
      const revision = await this.#api.saveReviewDraft(this.target, this.number, null, this.state!.revision);
      this.state = { ...this.state!, revision };
      // A refused deletion leaves the local draft intact too.
      if (version === this.#version) {
        this.#value = null;
        this.#apply(null);
        this.#saved = version;
        pending.delete(this);
      }
    });
    this.#queue = run.catch(() => {});
    return run;
  }

  dispose() { void this.flush().catch(this.#fail); }
}

/** Closing waits for edits even after their screen was replaced. Failed writes stay retryable. */
export async function settleReviewSaves(ms: number): Promise<string | null> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      Promise.all([...pending].map(save => save.flush())).then(() => null, err => String(err)),
      new Promise<string>(resolve => { timer = setTimeout(() => resolve("the review draft is still being written"), ms); }),
    ]);
  } finally { clearTimeout(timer); }
}
