// Split Commit: which changes of a commit go into the new commit right after it, picked by file
// or by line, and the split as an Edit Stack plan holds it. The rest keeps the commit's place
// and message. Type imports only, so the tests run on this file as it is.

import type { Draft, Item } from "./stack";
import type { DiffLine, FileDiff, Hunk, SplitFile, Stack, StackCommit, StackSplit } from "./types";

/** What is picked of one file: all of its changes, or some changed lines by their numbers. */
export type FilePick = { whole: true } | { removed: number[]; added: number[] };
/** Picks by path; a Map, as a file may be called `constructor` or `__proto__`. */
export type Picks = Map<string, FilePick>;

/** A split kept in an Edit Stack plan, with what the new commit's row shows. */
export interface DraftSplit extends StackSplit {
  additions: number;
  deletions: number;
}

/** The step id of a split's new commit, before it exists; the core names it the same. */
export const splitPart = (id: string) => `split:${id}`;
/** The commit a split part's step belongs to, or null for a commit of the stack. */
export const splitOf = (step: string) => (step.startsWith("split:") ? step.slice("split:".length) : null);

const changed = (line: DiffLine) => line.kind !== "context";

/** The changed lines of a file: removed by their old number, added by their new one. */
export function changedLines(diff: FileDiff): { removed: number[]; added: number[] } {
  const removed: number[] = [];
  const added: number[] = [];
  for (const line of diff.hunks.flatMap((h) => h.lines)) {
    if (line.kind === "removed" && line.oldLine !== null) removed.push(line.oldLine);
    if (line.kind === "added" && line.newLine !== null) added.push(line.newLine);
  }
  return { removed, added };
}

/** Single lines can be picked: a text file whose lines are shown. */
export const byLine = (diff: FileDiff) => diff.hunks.length > 0 && !diff.tooLarge && !diff.file.binary;

const lineKey = (line: DiffLine) => (line.kind === "removed" ? `-${line.oldLine}` : `+${line.newLine}`);

function pickedKeys(pick: FilePick | undefined, diff: FileDiff): Set<string> {
  if (!pick) return new Set();
  if ("whole" in pick) return new Set(diff.hunks.flatMap((h) => h.lines.filter(changed).map(lineKey)));
  return new Set([...pick.removed.map((n) => `-${n}`), ...pick.added.map((n) => `+${n}`)]);
}

/** How much of a file is picked. */
export function fileState(picks: Picks, diff: FileDiff): "none" | "some" | "all" {
  const pick = picks.get(diff.file.path);
  if (!pick) return "none";
  if ("whole" in pick) return "all";
  const all = changedLines(diff);
  const count = pick.removed.length + pick.added.length;
  return count === 0 ? "none" : count === all.removed.length + all.added.length ? "all" : "some";
}

/** A file's picked lines as a pick: none left takes the file out, all of them make it whole. */
function settle(picks: Picks, diff: FileDiff, keys: Set<string>): Picks {
  const next = new Map(picks);
  const all = changedLines(diff);
  const removed = all.removed.filter((n) => keys.has(`-${n}`));
  const added = all.added.filter((n) => keys.has(`+${n}`));
  if (!removed.length && !added.length) next.delete(diff.file.path);
  else if (removed.length === all.removed.length && added.length === all.added.length) next.set(diff.file.path, { whole: true });
  else next.set(diff.file.path, { removed, added });
  return next;
}

/** Pick a whole file, or take it out when it is all picked already. */
export function toggleFile(picks: Picks, diff: FileDiff): Picks {
  const next = new Map(picks);
  if (fileState(picks, diff) === "all") next.delete(diff.file.path);
  else next.set(diff.file.path, { whole: true });
  return next;
}

/** Pick or unpick changed lines of a file; `lines` all go the way the first one goes. */
export function toggleLines(picks: Picks, diff: FileDiff, lines: DiffLine[]): Picks {
  const own = lines.filter(changed);
  if (!own.length) return picks;
  const keys = pickedKeys(picks.get(diff.file.path), diff);
  const on = !keys.has(lineKey(own[0]));
  for (const line of own) {
    if (on) keys.add(lineKey(line));
    else keys.delete(lineKey(line));
  }
  return settle(picks, diff, keys);
}

/** Pick every changed line of a hunk, or unpick them all when they are all picked. */
export function toggleHunk(picks: Picks, diff: FileDiff, hunk: Hunk): Picks {
  const keys = pickedKeys(picks.get(diff.file.path), diff);
  const own = hunk.lines.filter(changed);
  const all = own.every((l) => keys.has(lineKey(l)));
  for (const line of own) {
    if (all) keys.delete(lineKey(line));
    else keys.add(lineKey(line));
  }
  return settle(picks, diff, keys);
}

/** The indexes in `hunk.lines` of the picked lines, for the diff's checkboxes. */
export function pickedInHunk(picks: Picks, diff: FileDiff, hunk: Hunk): number[] {
  const keys = pickedKeys(picks.get(diff.file.path), diff);
  return hunk.lines.flatMap((line, i) => (changed(line) && keys.has(lineKey(line)) ? [i] : []));
}

/** The picks as the core takes them, in the order of the commit's files. */
export function splitFiles(picks: Picks, diffs: FileDiff[]): SplitFile[] {
  return diffs.flatMap((diff): SplitFile[] => {
    const pick = picks.get(diff.file.path);
    if (!pick) return [];
    if ("whole" in pick || fileState(picks, diff) === "all") return [{ path: diff.file.path, lines: null }];
    return [{ path: diff.file.path, lines: { removed: [...pick.removed], added: [...pick.added] } }];
  });
}

/** The lines the new commit gets, counted as its row shows them. */
export function pickedCounts(picks: Picks, diffs: FileDiff[]): { files: number; additions: number; deletions: number } {
  let files = 0;
  let additions = 0;
  let deletions = 0;
  for (const diff of diffs) {
    const pick = picks.get(diff.file.path);
    if (!pick) continue;
    files++;
    if ("whole" in pick) {
      additions += diff.file.additions;
      deletions += diff.file.deletions;
    } else {
      additions += pick.added.length;
      deletions += pick.removed.length;
    }
  }
  return { files, additions, deletions };
}

/** Why the split can't be made yet, or null. */
export function splitProblem(picks: Picks, diffs: FileDiff[], message: string): string | null {
  if (!splitFiles(picks, diffs).length) return "Pick the changes that go into the new commit.";
  if (diffs.every((d) => fileState(picks, d) === "all")) return "Leave some changes in this commit: picking all of them moves nothing.";
  if (!message.trim()) return "Type a message for the new commit.";
  return null;
}

/** The picks a kept split was made with, to open its sheet again. */
export function picksOf(split: StackSplit): Picks {
  return new Map(split.files.map((f): [string, FilePick] => [f.path, f.lines ? { removed: [...f.lines.removed], added: [...f.lines.added] } : { whole: true }]));
}

/** The plan with `split` made: its new commit sits right above the commit it is split from. */
export function withSplit(draft: Draft, split: DraftSplit): Draft {
  const part = splitPart(split.id);
  const items: Item[] = draft.items.filter((i) => !(i.kind === "commit" && i.id === part));
  const at = items.findIndex((i) => i.kind === "commit" && i.id === split.id);
  if (at < 0) return draft;
  if (draft.splits?.[split.id]) {
    // Made again: its new commit stays where it was moved.
    const was = draft.items.findIndex((i) => i.kind === "commit" && i.id === part);
    if (was >= 0) return { ...draft, splits: { ...draft.splits, [split.id]: split } };
  }
  items.splice(at, 0, { kind: "commit", id: part });
  return { ...draft, items, splits: { ...(draft.splits ?? {}), [split.id]: split } };
}

/** The plan without the split of `id`: the new commit goes, its changes are back in the commit. */
export function withoutSplit(draft: Draft, id: string): Draft {
  const part = splitPart(id);
  const splits = { ...(draft.splits ?? {}) };
  delete splits[id];
  const acts = { ...draft.acts };
  delete acts[part];
  const messages = { ...draft.messages };
  delete messages[part];
  return { ...draft, items: draft.items.filter((i) => !(i.kind === "commit" && i.id === part)), acts, messages, splits };
}

/** The plan's splits, as the core takes them. */
export const plannedSplits = (draft: Draft): StackSplit[] => Object.values(draft.splits ?? {}).map(({ id, files, message }) => ({ id, files, message }));

/** The stack's commits with each split's new commit as a row of its own, and the split commit
 *  with what it keeps. */
export function commitsWithSplits(stack: Stack, draft: Draft): StackCommit[] {
  const splits = draft.splits ?? {};
  return stack.commits.flatMap((c): StackCommit[] => {
    const split = splits[c.id];
    if (!split) return [c];
    const part: StackCommit = {
      ...c,
      id: splitPart(c.id),
      summary: split.message.split("\n")[0],
      message: split.message,
      additions: split.additions,
      deletions: split.deletions,
      pushed: false,
    };
    const rest = { ...c, additions: Math.max(0, c.additions - split.additions), deletions: Math.max(0, c.deletions - split.deletions) };
    return [part, rest];
  });
}
