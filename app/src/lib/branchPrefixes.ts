// The prefixes a new branch's name can start with, each with what it is for. A Name field shows
// the chosen one as a pop-up at its start (PrefixField.svelte); the name itself stays one string.
// Settings › Git › Branches keeps the list (branchPrefs.ts reads it); everything here is pure.

export interface BranchPrefix {
  prefix: string;
  words: string;
  /** "production": a branch with it starts from the production branch, e.g. a hot fix. */
  from?: "production";
  /** Not in Settings: the repository's branches have it. */
  repo?: true;
}

export const BRANCH_PREFIXES: BranchPrefix[] = [
  { prefix: "feature/", words: "New functionality" },
  { prefix: "bugfix/", words: "Fixing a bug" },
  { prefix: "hotfix/", words: "An urgent fix for production", from: "production" },
  { prefix: "release/", words: "Getting a release ready", from: "production" },
  { prefix: "refactor/", words: "Changing code, not what it does" },
  { prefix: "chore/", words: "Upkeep: builds, dependencies, settings" },
  { prefix: "docs/", words: "Documentation" },
  { prefix: "test/", words: "Tests" },
];

/** Why git can't start a branch name with `prefix`, or null: one or more words, each followed by a /. */
export function prefixProblem(prefix: string): string | null {
  if (!/^([^/\s~^:?*[\\.][^/\s~^:?*[\\]*\/)+$/.test(prefix) || prefix.includes("..") || prefix.includes("@{") || /\.lock\//.test(prefix))
    return `“${prefix}” is not a prefix git can take: words and a /, such as feature/`;
  return null;
}

/** Why a list of prefixes from settings.json can't be used, or null. */
export function prefixesProblem(list: unknown[]): string | null {
  const seen = new Set<string>();
  for (const item of list) {
    const p = item as Partial<BranchPrefix> | null;
    if (!p || typeof p !== "object" || typeof p.prefix !== "string" || typeof p.words !== "string" || (p.from !== undefined && p.from !== "production"))
      return 'Expected items like { "prefix": "feature/", "words": "New functionality" }, with "from": "production" for a prefix that starts from the production branch';
    const problem = prefixProblem(p.prefix);
    if (problem) return problem;
    if (seen.has(p.prefix)) return `${p.prefix} is in the list twice`;
    seen.add(p.prefix);
  }
  return null;
}

/** The prefix of `list` a name starts with, and the rest of it. */
export function splitPrefix(name: string, list: BranchPrefix[] = BRANCH_PREFIXES): { prefix: string | null; rest: string } {
  const known = list.find((p) => name.startsWith(p.prefix));
  return known ? { prefix: known.prefix, rest: name.slice(known.prefix.length) } : { prefix: null, rest: name };
}

/** The name with `prefix` in place of the one it has; null takes it off. */
export function withPrefix(name: string, prefix: string | null, list: BranchPrefix[] = BRANCH_PREFIXES): string {
  return (prefix ?? "") + splitPrefix(name, list).rest;
}

/** The whole name once the box after the prefix says `typed`: a known prefix typed or pasted
 *  there takes the place of the chosen one. */
export function typedAfter(prefix: string | null, typed: string, list: BranchPrefix[] = BRANCH_PREFIXES): string {
  return splitPrefix(typed, list).prefix ? typed : (prefix ?? "") + typed;
}

/** A name that is nothing yet but a prefix, such as feature/ or team/feature/, or nothing at all. */
export const onlyPrefix = (name: string) => name === "" || name.endsWith("/");

/** A name as git can take it: spaces become dashes, characters git refuses go, and with
 *  `lowercase` capitals become small letters. What is still being typed, such as a dash at the
 *  end, stays. */
export function cleanName(text: string, lowercase: boolean): string {
  const clean = text
    .replace(/\s+/g, "-")
    .replace(/[~^:?*[\\\x00-\x1f\x7f]/g, "")
    .replace(/\.{2,}/g, ".")
    .replace(/\/{2,}/g, "/")
    .replace(/@\{/g, "@")
    .replace(/^[-./]+/, "");
  return lowercase ? clean.toLowerCase() : clean;
}

/** Prefixes the repository's branches have that `list` doesn't, the most used first, each
 *  saying how many branches have it. `names` are local branch names and remote ones without
 *  their remote. */
export function repoPrefixes(names: string[], list: BranchPrefix[], most = 5): BranchPrefix[] {
  const counts = new Map<string, Set<string>>();
  for (const name of names) {
    const cut = name.indexOf("/");
    if (cut <= 0 || cut === name.length - 1) continue;
    const prefix = name.slice(0, cut + 1);
    if (list.some((p) => p.prefix === prefix)) continue;
    counts.set(prefix, (counts.get(prefix) ?? new Set()).add(name));
  }
  return [...counts]
    .sort((a, b) => b[1].size - a[1].size || a[0].localeCompare(b[0]))
    .slice(0, most)
    .map(([prefix, branches]) => ({ prefix, words: branches.size === 1 ? "1 branch here" : `${branches.size} branches here`, repo: true }));
}

/** The letters and digits of a name's last part, for comparing two names. */
const core = (name: string) => (name.split("/").pop() ?? name).toLowerCase().replace(/[^a-z0-9]/g, "");

/** Edits between two short words, or more than `max` of them. */
function distance(a: string, b: string, max: number): number {
  if (Math.abs(a.length - b.length) > max) return max + 1;
  let row = Array.from({ length: b.length + 1 }, (_, i) => i);
  for (let i = 1; i <= a.length; i++) {
    const next = [i];
    for (let j = 1; j <= b.length; j++) next[j] = Math.min(row[j] + 1, next[j - 1] + 1, row[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
    row = next;
  }
  return row[b.length];
}

/** A branch whose name is almost `name`: the same words with other capitals or dashes, one
 *  starting with the other (payout-rounding and payout-rounding-v2), or a letter or two apart.
 *  The same name exactly is left to the sheet's own notes. */
export function similarBranch(name: string, names: string[]): string | null {
  const mine = core(name);
  if (mine.length < 4) return null;
  for (const other of names) {
    if (other === name) continue;
    const theirs = core(other);
    if (theirs.length < 4) continue;
    if (theirs === mine) return other;
    if ((theirs.startsWith(mine) || mine.startsWith(theirs)) && Math.abs(theirs.length - mine.length) <= 3) return other;
    if (mine.length >= 6 && distance(mine, theirs, 2) <= 2) return other;
  }
  return null;
}
