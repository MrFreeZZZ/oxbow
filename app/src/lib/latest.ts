// Replies that can come back in any order: one counts only while its request is the newest made,
// and for the repository it was made for. A reply to an older request, or one that comes after
// the window went to another repository (and back: A → B → A), is dropped.

/** The newest of a series of requests for one thing (a list, a check) of one repository. */
export class Latest {
  #at = 0;
  #key: () => string | null;

  /** `key`: the repository the requests are about now, by its path. */
  constructor(key: () => string | null = () => null) {
    this.#key = key;
  }

  /** A new request. The test says whether its reply is still the one wanted when it comes. */
  begin(): () => boolean {
    const mine = ++this.#at;
    const key = this.#key();
    return () => mine === this.#at && key === this.#key();
  }

  /** Every reply on its way is dropped: what they answer is gone. */
  drop() {
    this.#at++;
  }
}
