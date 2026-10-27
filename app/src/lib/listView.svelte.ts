// How lists of changed files are shown, in Changes and in a commit alike: one list of paths, or
// files under their folders, and how wide their column beside the diff is. Every window and
// repository shares the choice.

import type { FileChange } from "./types";

const FOLDERS_KEY = "oxbow.changes.folders";
const WIDTH_KEY = "oxbow.changes.filesWidth";

/** The files column is never narrower than this, and leaves the diff at least `DIFF_MIN`. */
export const FILES_MIN = 200;
const FILES_WIDTH = 268;
const DIFF_MIN = 360;

export const listView = $state({ folders: readFolders(), width: readWidth() });

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

function readWidth(): number {
  try {
    const value = Number(localStorage.getItem(WIDTH_KEY));
    return localStorage.getItem(WIDTH_KEY) !== null && Number.isFinite(value) && value >= FILES_MIN ? value : FILES_WIDTH;
  } catch {
    return FILES_WIDTH;
  }
}

/** The width the files column takes in a body this wide: the one kept, cut down while the diff would get too little. */
export function filesWidth(body: number): number {
  if (body <= 0) return Math.round(listView.width);
  return Math.round(Math.max(Math.min(listView.width, body - DIFF_MIN), FILES_MIN));
}

/** Drags the border between the files and the diff; the width is kept once it is let go. */
export function resizeFiles(event: PointerEvent, body: number) {
  const startX = event.clientX;
  // From the width it shows, which a narrow panel may have cut down from the one kept.
  const from = filesWidth(body);
  const target = event.currentTarget as HTMLElement;
  target.setPointerCapture(event.pointerId);
  const move = (e: PointerEvent) => (listView.width = Math.max(Math.min(from + e.clientX - startX, body - DIFF_MIN), FILES_MIN));
  const up = () => {
    target.removeEventListener("pointermove", move);
    target.removeEventListener("pointerup", up);
    try {
      localStorage.setItem(WIDTH_KEY, String(Math.round(listView.width)));
    } catch {
      // Lasts until the app quits.
    }
  };
  target.addEventListener("pointermove", move);
  target.addEventListener("pointerup", up);
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
