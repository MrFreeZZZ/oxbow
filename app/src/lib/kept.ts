// Screen state kept when Back or the sidebar brings the screen back.

export class KeptScreens {
  #store = new Map<string, unknown>();

  get<T>(key: string): T | undefined {
    return this.#store.get(key) as T | undefined;
  }

  set(key: string, value: unknown) {
    this.#store.set(key, value);
  }

  drop(key: string) {
    this.#store.delete(key);
  }

  clear() {
    this.#store.clear();
  }
}
