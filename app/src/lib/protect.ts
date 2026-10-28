// Protected branches (Settings › This Repository): main, master and the production branch unless
// the repository lists others. Oxbow never force pushes them and doesn't rewrite their commits
// that are already pushed; Revert undoes such a commit instead. The core refuses those actions
// too (protect.rs); these tell the menus and sheets not to offer them.

import type { History, HistoryRow } from "./types";

/** What these need of the history. */
export type Protecting = Pick<History, "protected" | "head" | "rows" | "remotes">;

/** Whether the branch `name` is `pattern`, where `*` stands for any characters, `/` included. */
export function matches(pattern: string, name: string): boolean {
  const parts = pattern.split("*");
  if (parts.length === 1) return pattern === name;
  const first = parts[0];
  const last = parts[parts.length - 1];
  if (name.length < first.length + last.length || !name.startsWith(first) || !name.endsWith(last)) return false;
  let rest = name.slice(first.length, name.length - last.length);
  for (const part of parts.slice(1, -1)) {
    const at = rest.indexOf(part);
    if (at < 0) return false;
    rest = rest.slice(at + part.length);
  }
  return true;
}

/** Whether the branch `name` is one of `patterns`. */
export function isProtected(patterns: readonly string[] | null | undefined, name: string | null | undefined): boolean {
  return !!name && !!patterns?.some((p) => matches(p, name));
}

/** Names and patterns typed in, separated by spaces or commas, each once. */
export function parseProtected(value: string): string[] {
  return [...new Set(value.split(/[\s,]+/).filter(Boolean))];
}

/** The checked-out branch when it is protected. */
export function protectedHead(history: Protecting): string | null {
  const branch = history.head.branch;
  return isProtected(history.protected, branch) ? branch : null;
}

/** Whether a remote has the commit of `row`, as far as History knows: without remotes, none. */
const pushed = (history: Protecting, row: HistoryRow) => !row.unpushed && history.remotes.length > 0;

/** A commit of the checked-out branch that stays as it is: the branch is protected and the commit
 *  is already pushed. */
export function lockedCommit(history: Protecting, row: HistoryRow): boolean {
  return !!protectedHead(history) && pushed(history, row);
}

/** Whether moving the protected checked-out branch onto `keep` (Reset to it, a rebase onto it)
 *  would take a pushed commit off it. */
export function dropsPushed(history: Protecting, keep: string | null): boolean {
  if (!protectedHead(history) || !history.head.commit) return false;
  const byId = new Map(history.rows.map((r) => [r.id, r]));
  const reach = (start: string | null) => {
    const seen = new Set<string>();
    const stack = start ? [start] : [];
    while (stack.length) {
      const next = stack.pop()!;
      if (seen.has(next)) continue;
      seen.add(next);
      stack.push(...(byId.get(next)?.parents ?? []));
    }
    return seen;
  };
  const kept = reach(keep);
  for (const id of reach(history.head.commit)) {
    const row = byId.get(id);
    if (row && !kept.has(id) && pushed(history, row)) return true;
  }
  return false;
}

/** Why a menu leaves out what would rewrite `branch`'s pushed commits. */
export const lockedNote = (branch: string) => `${branch} is protected: its pushed commits can be reverted, not rewritten.`;

/** Where Settings › This Repository keeps the list. */
export const PROTECTED_KEY = "oxbow.protectedBranches";

/** Why `name` can't go into `list`, or null. */
export function protectedProblem(list: readonly string[], name: string): string | null {
  const text = name.trim();
  if (!text) return "Type a branch name or a pattern";
  if (/[\s,~^:?[\\]/.test(text) || text.includes("..") || text.startsWith("-") || text.startsWith("/") || text.endsWith("/") || text.endsWith(".lock")) {
    return "A branch name can’t have spaces, commas or ~ ^ : ? [ \\, or start with - or /";
  }
  if (list.includes(text)) return "It is in the list already";
  return null;
}

/** The value that keeps `list`; empty when none is protected. */
export const protectedValue = (list: readonly string[]) => list.join(" ");

/** The branches `pattern` matches among `branches`, full refs (`refs/heads/main`,
 *  `refs/remotes/origin/main`) or short names, each name once. */
export function matching(pattern: string, branches: readonly string[]): string[] {
  const names = branches.map((b) => b.replace(/^refs\/heads\//, "").replace(/^refs\/remotes\/[^/]+\//, "")).filter((n) => n !== "HEAD");
  return [...new Set(names.filter((n) => matches(pattern, n)))];
}
