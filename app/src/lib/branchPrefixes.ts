// The prefixes a new branch's name can start with, each with what it is for. A Name field shows
// the chosen one as a pop-up at its start (PrefixField.svelte); the name itself stays one string.

export interface BranchPrefix {
  prefix: string;
  words: string;
  /** "production": a branch with it starts from the production branch, e.g. a hot fix. */
  from?: "production";
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

/** Why a list of prefixes from settings.json can't be used, or null. */
export function prefixesProblem(list: unknown[]): string | null {
  for (const item of list) {
    const p = item as Partial<BranchPrefix> | null;
    if (!p || typeof p !== "object" || typeof p.prefix !== "string" || typeof p.words !== "string" || (p.from !== undefined && p.from !== "production"))
      return 'Expected items like { "prefix": "feature/", "words": "New functionality" }, with "from": "production" for a prefix that starts from the production branch';
    if (!/^[^\s~^:?*[\\]+\/$/.test(p.prefix)) return `“${p.prefix}” is not a prefix: a word and a /, such as feature/`;
  }
  return null;
}

/** What a new branch's name starts with until another prefix is picked. */
export const FIRST_PREFIX = "feature/";

/** The known prefix a name starts with, and the rest of it. */
export function splitPrefix(name: string): { prefix: string | null; rest: string } {
  const known = BRANCH_PREFIXES.find((p) => name.startsWith(p.prefix));
  return known ? { prefix: known.prefix, rest: name.slice(known.prefix.length) } : { prefix: null, rest: name };
}

/** The name with `prefix` in place of the one it has; null takes it off. */
export function withPrefix(name: string, prefix: string | null): string {
  return (prefix ?? "") + splitPrefix(name).rest;
}

/** The whole name once the box after the prefix says `typed`: a known prefix typed or pasted
 *  there takes the place of the chosen one. */
export function typedAfter(prefix: string | null, typed: string): string {
  return splitPrefix(typed).prefix ? typed : (prefix ?? "") + typed;
}

/** What the prefix is for, e.g. "Fixing a bug"; null for a name without a known one. */
export function prefixWords(name: string): string | null {
  const { prefix } = splitPrefix(name);
  return BRANCH_PREFIXES.find((p) => p.prefix === prefix)?.words ?? null;
}
