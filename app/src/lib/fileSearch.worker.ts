// The index and every query stay off the window's thread.
import { FileSearch } from "./browseTree";

let index: FileSearch | null = null;
let latest = 0;
self.onmessage = (event: MessageEvent<{ paths?: string[]; query: string; id: number }>) => {
  const { paths, query, id } = event.data;
  if (paths) index = new FileSearch(paths);
  latest = id;
  // Let pending input replace an older query before spending time on it.
  setTimeout(() => {
    if (id === latest) self.postMessage({ id, hits: index?.find(query) ?? [] });
  }, 0);
};
