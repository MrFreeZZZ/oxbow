// How lists of changed files are shown, in Changes and in a commit alike: one list of paths, or
// files under their folders. Every window and repository shares the choice.

import type { FileChange } from "./types";

const FOLDERS_KEY = "oxbow.changes.folders";

export const listView = $state({ folders: readFolders() });

function readFolders(): boolean {
  try {
    return localStorage.getItem(FOLDERS_KEY) === "true";
  } catch {
    return false;
  }
}

export function setFolders(on: boolean) {
  listView.folders = on;
  try {
    localStorage.setItem(FOLDERS_KEY, String(on));
  } catch {
    // Lasts until the app quits.
  }
}

/** Files grouped by the folder they are in: the top folder first, then by name. */
export function byFolder(files: FileChange[]): { dir: string; files: FileChange[] }[] {
  const map = new Map<string, FileChange[]>();
  for (const file of files) {
    const dir = file.path.slice(0, file.path.lastIndexOf("/") + 1);
    const list = map.get(dir);
    if (list) list.push(file);
    else map.set(dir, [file]);
  }
  return [...map]
    .sort(([a], [b]) => (a === "" ? -1 : b === "" ? 1 : a.localeCompare(b)))
    .map(([dir, files]) => ({ dir, files }));
}
