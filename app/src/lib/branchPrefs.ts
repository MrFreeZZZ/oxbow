// Settings › Git › Branches at work in the sheets that name a new branch: the prefixes offered,
// the one a name starts with, and the cleanup of what is typed.

import { cleanName, repoPrefixes, similarBranch, type BranchPrefix } from "./branchPrefixes";
import { prefs } from "./prefs.svelte";
import type { History } from "./types";

/** Branch names of the repository, remote ones without their remote. */
export function branchNames(history: History): string[] {
  return history.refs.flatMap((r) => (r.kind === "local" ? [r.name] : r.kind === "remote" && r.remote ? [r.name.slice(r.remote.length + 1)] : []));
}

/** The prefixes a Name box offers: those of Settings, then the repository's own. */
export function prefixesFor(history: History): BranchPrefix[] {
  const list = prefs.get("oxbow.branch.prefixes");
  return prefs.get("oxbow.branch.repoPrefixes") ? [...list, ...repoPrefixes(branchNames(history), list)] : list;
}

/** The name a new branch's Name box starts with: a prefix and nothing after it, or nothing. */
export function firstName(list: BranchPrefix[]): string {
  const first = prefs.get("oxbow.branch.firstPrefix");
  const known = (prefix: string) => (prefix === "" || list.some((p) => p.prefix === prefix) ? prefix : null);
  if (first !== "last") return known(first) ?? list[0]?.prefix ?? "";
  return known(prefs.get("oxbow.branch.lastPrefix")) ?? list[0]?.prefix ?? "";
}

/** The prefix picked last, for the next new branch. */
export function rememberPrefix(prefix: string | null) {
  if ((prefix ?? "") !== prefs.get("oxbow.branch.lastPrefix")) prefs.set("oxbow.branch.lastPrefix", prefix ?? "");
}

/** What was typed, cleaned up as Settings says. */
export function cleanTyped(text: string): string {
  return prefs.get("oxbow.branch.cleanNames") ? cleanName(text, prefs.get("oxbow.branch.lowercase")) : text;
}

/** Where a branch with this prefix starts: "production" when its prefix says so and Settings
 *  lets it choose. */
export function prefixStart(prefix: string | null, list: BranchPrefix[]): "production" | null {
  if (!prefix || !prefs.get("oxbow.branch.prefixStart")) return null;
  return list.find((p) => p.prefix === prefix)?.from ?? null;
}

/** A quiet note when a branch with almost this name exists: maybe it is the same work. */
export function similarNote(history: History, name: string): string | null {
  const locals = history.refs.filter((r) => r.kind === "local").map((r) => r.name);
  const mine = similarBranch(name, locals);
  if (mine) return `${mine} is here already. Is it the same work?`;
  const remotes = history.refs.filter((r) => r.kind === "remote" && r.remote);
  const theirs = similarBranch(name, remotes.map((r) => r.name.slice(r.remote!.length + 1)));
  const ref = theirs ? remotes.find((r) => r.name === `${r.remote}/${theirs}`) : undefined;
  return ref ? `${ref.remote} has ${theirs}. Is it the same work? Check it out from the sidebar instead.` : null;
}
