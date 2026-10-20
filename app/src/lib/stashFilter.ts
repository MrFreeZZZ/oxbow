// The Stashes list's filter and the short list of files on each card. No imports, so the tests
// run it as it is.

/** What the filter looks through on a stash card. */
export interface StashFacts {
  /** `stash@{n}`. */
  name: string;
  title: string;
  /** The branch it was made on; null on a detached `HEAD`. */
  branch: string | null;
  /** The commit it was made on, in full. */
  base: string;
  /** Its files, tracked and untracked; null until they are loaded. */
  paths: string[] | null;
}

/** The words of a filter, lowercased: a stash matches when it has every one of them. */
export function filterWords(filter: string): string[] {
  return filter.trim().toLowerCase().split(/\s+/).filter(Boolean);
}

/** Whether a stash has each word in its title, branch, name, base commit or file paths. A base
 *  commit matches from its start only, as a short SHA is typed. */
export function stashMatches(s: StashFacts, words: string[]): boolean {
  const text = [s.title, s.branch ?? "", s.name, ...(s.paths ?? [])].join("\n").toLowerCase();
  const base = s.base.toLowerCase();
  return words.every((w) => text.includes(w) || base.startsWith(w));
}

/** The file part of a path. */
const fileName = (path: string) => path.slice(path.lastIndexOf("/") + 1);

/** Up to `max` file names for a card, the ones the filter found first, and how many are left. */
export function filesBrief(paths: string[], words: string[], max = 3): { names: string[]; more: number } {
  const hit = (p: string) => words.some((w) => p.toLowerCase().includes(w));
  const ordered = words.length ? [...paths.filter(hit), ...paths.filter((p) => !hit(p))] : paths;
  const names = ordered.slice(0, max).map(fileName);
  return { names, more: Math.max(0, paths.length - names.length) };
}
