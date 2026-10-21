// What the other worktrees and the submodules' folders have comes after the history, one folder
// at a time as each `git status` there ends (folders.rs). Until then a folder is pending: it
// shows what the last reading said, or that it is being checked, never "0 changed" or "clean".

import type { FolderState, History, Submodule, Worktree } from "./types";

/** Not read yet, and nothing read before is known: its row says it is being checked. */
export function checking(folder: Worktree | Submodule): boolean {
  return folder.pending && folder.changed === null && folder.statusError === null;
}

/** `next`, whose folders are pending, with what `prev` read of the same folders meanwhile:
 *  still pending, so that the new reading replaces it, but not blank while it comes. */
export function carryStates(next: History, prev: History | null): History {
  if (!prev) return next;
  const trees = new Map(prev.worktrees.filter((t) => !checking(t)).map((t) => [t.path, t]));
  const subs = new Map(prev.submodules.filter((s) => s.cloned && !checking(s)).map((s) => [s.path, s]));
  const worktrees = next.worktrees.map((t): Worktree => {
    const was = t.pending ? trees.get(t.path) : undefined;
    return was ? { ...t, changed: was.changed, statusError: was.statusError } : t;
  });
  const submodules = next.submodules.map((s): Submodule => {
    const was = s.pending ? subs.get(s.path) : undefined;
    if (!was) return s;
    const { changed, statusError, current, branch, ahead, behind, missing } = was;
    return { ...s, changed, statusError, current, branch, ahead, behind, missing };
  });
  return { ...next, worktrees, submodules };
}

/** `history` with `state` in its folder; the same history when it lists no such folder, e.g.
 *  one removed meanwhile. */
export function withState(history: History, state: FolderState): History {
  if (state.submodule) {
    const at = history.submodules.findIndex((s) => s.path === state.path);
    if (at < 0) return history;
    const submodules = history.submodules.slice();
    const checkout = state.checkout ?? { current: null, branch: null, ahead: 0, behind: 0, missing: false };
    submodules[at] = { ...submodules[at], ...checkout, changed: state.changed, statusError: state.statusError, pending: false };
    return { ...history, submodules };
  }
  const at = history.worktrees.findIndex((t) => t.path === state.path);
  if (at < 0) return history;
  const worktrees = history.worktrees.slice();
  worktrees[at] = { ...worktrees[at], changed: state.changed, statusError: state.statusError, pending: false };
  return { ...history, worktrees };
}

/** Shows an earlier reading, as reading it again failed: the count may be out of date. */
export function stale(folder: Worktree | Submodule): boolean {
  return !folder.pending && folder.changed !== null && folder.statusError !== null;
}

/** `history` with the folders still being read given up on: `error` says why none of them could
 *  be. One that shows an earlier reading keeps it, stale: the error says it wasn't read again. */
export function failChecking(history: History, error: string): History {
  const fail = <T extends Worktree | Submodule>(f: T): T => (f.pending ? { ...f, statusError: error, pending: false } : f);
  return { ...history, worktrees: history.worktrees.map(fail), submodules: history.submodules.map(fail) };
}
