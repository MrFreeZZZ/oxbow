// Changelists in Changes: which list each unstaged file is in, what a list holds of a file it
// shares with another, the names a list can have, and the requests that stage, discard and
// commit what a list holds.

import type { Part, Request } from "./confirm.svelte";
import type { Changelist, Changelists, FileChange, Hunk } from "./types";

/** One of the lists of Changes: Unstaged, Untracked, Staged, or a changelist (`cl:` and its id). */
export type ListKey = "unstaged" | "untracked" | "staged" | `cl:${string}`;

export const keyOf = (id: string): ListKey => `cl:${id}`;
/** The changelist's id of a list key; null for the panel's own lists. */
export const idOf = (key: string): string | null => (key.startsWith("cl:") ? key.slice(3) : null);

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

/** Names that would read as the lists of Changes themselves. */
const TAKEN = ["unstaged", "staged", "untracked", "conflicted"];

/** Why `name` can't name a list (`except`: the list being renamed); null when it can. As the
 *  core checks it. */
export function nameProblem(name: string, lists: Changelist[], except: string | null = null): string | null {
  const trimmed = name.trim();
  if (!trimmed) return "A changelist needs a name.";
  if (TAKEN.includes(trimmed.toLowerCase())) return `“${trimmed}” is the name of one of the lists of Changes.`;
  if (lists.some((l) => l.id !== except && l.name.toLowerCase() === trimmed.toLowerCase())) return `There is a changelist called “${trimmed}” already.`;
  return null;
}

/** The list a file is in apart from its runs kept elsewhere: the first that names it. */
export function ownerOf(lists: Changelists | null, path: string): string | null {
  if (!lists || lists.split.some((s) => s.path === path)) return null;
  return lists.lists.find((l) => l.paths.includes(path))?.id ?? null;
}

/** The list a run of `path`'s unstaged changes is in; null: Unstaged. */
export function runList(lists: Changelists | null, path: string, key: string): string | null {
  const split = lists?.split.find((s) => s.path === path);
  if (split) return split.parts.find((p) => p.runs.includes(key))?.list ?? null;
  return lists?.lists.find((l) => l.paths.includes(path))?.id ?? null;
}

/** The lists the runs of a hunk are in, each once, in order. */
export function hunkLists(lists: Changelists | null, path: string, hunk: Hunk): (string | null)[] {
  const out: (string | null)[] = [];
  for (const key of hunk.runs ?? []) {
    const list = runList(lists, path, key);
    if (!out.includes(list)) out.push(list);
  }
  return out.length ? out : [ownerOf(lists, path)];
}

/** The keys of the runs that hold lines `picked` (indexes into the hunk's lines): a run is the
 *  changed lines between two unchanged ones, in the order of `hunk.runs`. */
export function runsOfLines(hunk: Hunk, picked: number[]): string[] {
  const keys = hunk.runs ?? [];
  const out: string[] = [];
  let run = -1;
  hunk.lines.forEach((line, i) => {
    if (line.kind === "context") return;
    if (i === 0 || hunk.lines[i - 1].kind === "context") run++;
    const key = keys[run];
    if (key !== undefined && picked.includes(i) && !out.includes(key)) out.push(key);
  });
  return out;
}

/** What list `list` (null: Unstaged) holds of a file shared between lists: how many of its
 *  changes, of how many, and the other lists that hold the rest. Null for a file not shared. */
export function shareOf(lists: Changelists | null, path: string, list: string | null): { runs: number; total: number; others: (string | null)[] } | null {
  const split = lists?.split.find((s) => s.path === path);
  const part = split?.parts.find((p) => p.list === list);
  if (!split || !part) return null;
  return {
    runs: part.runs.length,
    total: split.parts.reduce((n, p) => n + p.runs.length, 0),
    others: split.parts.filter((p) => p !== part).map((p) => p.list),
  };
}

/** A list's name, for words: Unstaged for null. */
export function listName(lists: Changelists | null, id: string | null): string {
  if (id === null) return "Unstaged";
  return lists?.lists.find((l) => l.id === id)?.name ?? "a changelist";
}

/** The file with the lines of what list `list` holds of it, when it is shared. */
function partOf(lists: Changelists | null, file: FileChange, list: string | null): FileChange {
  const part = lists?.split.find((s) => s.path === file.path)?.parts.find((p) => p.list === list);
  return part ? { ...file, additions: part.additions, deletions: part.deletions } : file;
}

export interface Grouped {
  /** Edits to tracked files that are in no changelist, with Unstaged's part of a shared one. */
  tracked: FileChange[];
  /** New files git does not track yet that are in no changelist. */
  untracked: FileChange[];
  /** Each changelist with its files, in the order of the lists. */
  lists: { list: Changelist; files: FileChange[] }[];
}

/** The unstaged files sorted into Unstaged, Untracked and the changelists. A file shared
 *  between lists is in each, with the lines of its own part there. */
export function groupFiles(unstaged: FileChange[], lists: Changelists | null): Grouped {
  const byPath = new Map(unstaged.map((f) => [f.path, f]));
  const listed = new Set<string>();
  const groups = (lists?.lists ?? []).map((list) => {
    const files: FileChange[] = [];
    for (const path of list.paths) {
      const file = byPath.get(path);
      if (!file) continue;
      listed.add(path);
      files.push(partOf(lists, file, list.id));
    }
    return { list, files };
  });
  // A shared file with a part left in Unstaged is there too.
  const left = (f: FileChange) => !listed.has(f.path) || !!lists?.split.some((s) => s.path === f.path && s.parts.some((p) => p.list === null));
  const rest = unstaged.filter(left).map((f) => partOf(lists, f, null));
  return {
    tracked: rest.filter((f) => f.status !== "untracked"),
    untracked: rest.filter((f) => f.status === "untracked"),
    lists: groups,
  };
}

const count = (files: FileChange[]) => (files.length === 1 ? (files[0].path.split("/").pop() ?? files[0].path) : plural(files.length, "file"));
const named = (files: FileChange[]): Part[] => (files.length === 1 ? [{ code: files[0].path }] : [plural(files.length, "file")]);

/** Stage what list `list` (null: Unstaged) holds of `files`. */
export function stagePartRequest(files: FileChange[], list: string | null, lists: Changelists | null, branch: Part): Request {
  const name = listName(lists, list);
  return {
    title: `Stage ${count(files)}?`,
    body: ["Adds the changes of ", { quote: name }, " in ", ...named(files), " to the next commit on ", branch, ". Changes of other lists in the same files stay unstaged."],
    icon: "stage",
    button: "Stage",
    past: `Staged ${count(files)} of ${name}`,
    action: { kind: "stageChangelist", list, paths: files.map((f) => f.path) },
  };
}

/** Throw away what list `list` (null: Unstaged) holds of `files`. */
export function discardPartRequest(files: FileChange[], list: string | null, lists: Changelists | null): Request {
  const name = listName(lists, list);
  return {
    title: `Discard the changes of ${name} in ${count(files)}?`,
    body: ["Throws away the changes of ", { quote: name }, " in ", ...named(files), ". Changes of other lists in the same files stay as they are; new files git does not track yet are deleted."],
    icon: "discard",
    button: "Discard",
    danger: true,
    note: "Undo brings it back, from the toast or the Operation Log.",
    done: `Discarded the changes of ${name} in ${count(files)}.`,
    action: { kind: "discardChangelist", list, paths: files.map((f) => f.path) },
  };
}

/** Ask for a list's name: a new one (`list` null) or a new name for `list`. `save` keeps it and
 *  says what it did. */
export function nameRequest(lists: Changelist[], list: Changelist | null, name: string, save: (name: string) => Promise<string>, moving = ""): Request {
  const problem = nameProblem(name, lists, list?.id ?? null);
  return {
    title: list ? `Rename “${list.name}”?` : "New changelist",
    body: list
      ? ["Its changes stay as they are; only the list gets the new name."]
      : [`A named group of unstaged changes${moving ? `, starting with ${moving}` : ""}. Commit takes one list at a time, and the staged files stay staged.`],
    icon: "edit",
    button: list ? "Rename" : "Create",
    fields: [
      {
        label: "Name",
        text: { value: name, placeholder: "What these changes are for", focus: true, edit: (next) => nameRequest(lists, list, next, save, moving) },
        error: name.trim() && problem ? problem : undefined,
      },
    ],
    invalid: problem,
    status: list ? "Renaming…" : "Creating…",
    local: { commands: [], run: () => save(name.trim()) },
  };
}
