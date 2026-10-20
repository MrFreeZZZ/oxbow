// What a screen had going when you left it, such as Edit Stack's plan or a pull request not yet
// sent, found again when Back or the sidebar brings you back. It lasts until the app quits or
// another repository opens.

const store = new Map<string, unknown>();

export const kept = {
  get<T>(key: string): T | undefined {
    return store.get(key) as T | undefined;
  },
  set(key: string, value: unknown) {
    store.set(key, value);
  },
  drop(key: string) {
    store.delete(key);
  },
  clear() {
    store.clear();
  },
};
