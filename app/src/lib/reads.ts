// What History knows of each part it reads besides the commits: the uncommitted changes, the
// upstreams, an operation or bisect under way, the worktrees and the submodules. Each can fail
// on its own while the commits still show, and a part that failed must not look like an empty
// one: "0 changed", "no changes" or "not published" are things git said, not what is shown when
// it said nothing. A part that failed after a good reading keeps that reading, marked stale.

import type { History, ReadPart, RefInfo } from "./types";

/** loading: being read again (Retry); ready: from the last read; stale: the last read failed,
 *  so this is an earlier one; failed: never read well, so not known at all. */
export type ReadPhase = "loading" | "ready" | "stale" | "failed";

export interface ReadState {
  phase: ReadPhase;
  /** git's words for the last failure; null once a read went well. */
  error: string | null;
  /** When what is shown was read, in ms; null when nothing was. */
  at: number | null;
}

export type ReadStates = Record<ReadPart, ReadState>;

export const READ_PARTS: ReadPart[] = ["changes", "tracking", "operation", "bisect", "worktrees", "submodules"];

/** How each part is called in a sentence. */
export const PART_NAMES: Record<ReadPart, string> = {
  changes: "uncommitted changes",
  tracking: "upstream branches",
  operation: "merge or rebase state",
  bisect: "bisect state",
  worktrees: "worktrees",
  submodules: "submodules",
};

/** Before the first read: nothing known yet. */
export function loadingStates(): ReadStates {
  return Object.fromEntries(READ_PARTS.map((part) => [part, { phase: "loading", error: null, at: null }])) as ReadStates;
}

/** The history to show for `next`, the newest read, and the state of each of its parts. A part
 *  that failed keeps `prev`'s reading when that one was good or stale, and is stale; without one
 *  it is failed. The changes are never kept: their row says they aren't known instead, so no
 *  file is staged or committed from an old list. */
export function settle(next: History, prev: History | null, states: ReadStates | null, now: number): { history: History; states: ReadStates } {
  const out: ReadStates = loadingStates();
  let history = next;
  for (const part of READ_PARTS) {
    const problem = next.problems.find((p) => p.part === part);
    if (!problem) {
      out[part] = { phase: "ready", error: null, at: now };
      continue;
    }
    const before = states?.[part];
    const kept = prev && part !== "changes" && before && before.at !== null && before.phase !== "failed";
    if (kept) {
      history = keep(history, prev, part);
      out[part] = { phase: "stale", error: problem.error, at: before.at };
    } else out[part] = { phase: "failed", error: problem.error, at: null };
  }
  return { history, states: out };
}

/** `next` with `prev`'s reading of `part`. */
function keep(next: History, prev: History, part: ReadPart): History {
  switch (part) {
    case "worktrees":
      return { ...next, worktrees: prev.worktrees };
    case "submodules":
      return { ...next, submodules: prev.submodules };
    case "operation":
      return { ...next, operation: prev.operation };
    case "bisect":
      return { ...next, bisect: prev.bisect };
    case "tracking": {
      const was = new Map(prev.refs.filter((r) => r.kind === "local").map((r) => [r.name, r.tracking]));
      const refs = next.refs.map((r): RefInfo => (r.kind === "local" && !r.tracking && was.get(r.name) ? { ...r, tracking: was.get(r.name)! } : r));
      const tracking = next.tracking ?? (next.head.branch && next.head.branch === prev.head.branch ? prev.tracking : null);
      return { ...next, refs, tracking };
    }
    default:
      return next;
  }
}

/** A part read again on its own (Retry) went well: the history with it, and it is ready. */
export function retried(history: History, states: ReadStates, part: ReadPart, patch: Partial<History>, now: number): { history: History; states: ReadStates } {
  return {
    history: { ...history, ...patch, problems: history.problems.filter((p) => p.part !== part) },
    states: { ...states, [part]: { phase: "ready", error: null, at: now } },
  };
}

/** A part read again on its own (Retry) failed again: it stays as it was, with the new error. */
export function stillFailing(states: ReadStates, part: ReadPart, error: string): ReadStates {
  const was = states[part];
  return { ...states, [part]: { phase: was.at === null ? "failed" : "stale", error, at: was.at } };
}

/** Parts not known now: failed or stale, for the status line. */
export function troubled(states: ReadStates | null): ReadPart[] {
  if (!states) return [];
  return READ_PARTS.filter((part) => states[part].phase === "failed" || states[part].phase === "stale");
}

/** "4 min ago" for a stale part: how old what it shows is. */
export function age(state: ReadState, now: number): string {
  if (state.at === null) return "never";
  const minutes = Math.round((now - state.at) / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  return `${hours} ${hours === 1 ? "hour" : "hours"} ago`;
}

/** "Read 4 min ago" for a stale part. */
export function readAgo(state: ReadState, now: number): string {
  return state.at === null ? "Never read" : `Read ${age(state, now)}`;
}

/** git's error in one line, for a tooltip or a note: its first line that says something. */
export function errorLine(error: string | null): string {
  if (!error) return "";
  const lines = error.split("\n").map((l) => l.trim()).filter(Boolean);
  const said = lines.find((l) => /^(fatal|error):/.test(l)) ?? lines[0] ?? "";
  return said.replace(/^`[^`]*` failed: /, "");
}
