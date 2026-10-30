// Browse Files at This Commit: the rows of a commit's folder tree as it is opened, finding a
// file by name, and the steps of the keyboard through the rows.
import type { TreeItem } from "./types";

/** A row of the tree: an entry and how deep it is. */
export type TreeRow = { item: TreeItem; depth: number };

/** The rows shown: the top folder's entries, and inside every open folder whose entries have
 *  been read, its own. */
export function treeRows(folders: Map<string, TreeItem[]>, open: Set<string>, dir = "", depth = 0): TreeRow[] {
  const out: TreeRow[] = [];
  for (const item of folders.get(dir) ?? []) {
    out.push({ item, depth });
    if (item.kind === "folder" && open.has(item.path)) out.push(...treeRows(folders, open, item.path, depth + 1));
  }
  return out;
}

/** The folders a file is in, outermost first: what opens to show it. */
export function foldersOf(path: string): string[] {
  const parts = path.split("/").slice(0, -1);
  return parts.map((_, i) => parts.slice(0, i + 1).join("/"));
}

/** The files whose path has every word of `query`, in that order, case aside: the ones whose
 *  name has the last word first, then by path; at most `limit`. */
export class FileSearch {
  #paths: { path: string; lower: string; name: string }[];

  constructor(paths: string[]) {
    this.#paths = paths.map((path) => {
      const lower = path.toLowerCase();
      return { path, lower, name: lower.slice(lower.lastIndexOf("/") + 1) };
    }).sort((a, b) => a.path.localeCompare(b.path));
  }

  find(query: string, limit = 200): string[] {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length || limit <= 0) return [];
    const hits: string[] = [];
    // Paths are sorted once. Two bounded passes keep the same name-first order without
    // allocating or sorting every match of a broad query.
    for (const named of [true, false]) {
      for (const entry of this.#paths) {
        if (entry.name.includes(words[words.length - 1]) !== named) continue;
        let at = 0;
        let ok = true;
        for (const word of words) {
          const found = entry.lower.indexOf(word, at);
          if (found < 0) { ok = false; break; }
          at = found + word.length;
        }
        if (ok) {
          hits.push(entry.path);
          if (hits.length >= limit) return hits;
        }
      }
    }
    return hits;
  }
}

export function findFiles(paths: string[], query: string, limit = 200): string[] {
  return new FileSearch(paths).find(query, limit);
}

/** Where ↑ and ↓ go from the row of `path` (`by` -1 or 1), staying in the list. */
export function stepRow(paths: string[], path: string | null, by: 1 | -1): string | null {
  if (!paths.length) return null;
  const at = path === null ? -1 : paths.indexOf(path);
  if (at < 0) return paths[by > 0 ? 0 : paths.length - 1];
  return paths[Math.max(0, Math.min(paths.length - 1, at + by))];
}

/** ← on a row: an open folder closes, anything else goes up to its folder; null at the top. */
export function leftOf(row: TreeRow, open: Set<string>): { close: string } | { go: string } | null {
  if (row.item.kind === "folder" && open.has(row.item.path)) return { close: row.item.path };
  const up = foldersOf(row.item.path).pop();
  return up ? { go: up } : null;
}

/** The lines of a file's text, without the empty one after its last line end. */
export function linesOf(text: string): string[] {
  const lines = text.split("\n");
  if (lines.length > 1 && lines[lines.length - 1] === "") lines.pop();
  return lines.map((l) => (l.endsWith("\r") ? l.slice(0, -1) : l));
}
