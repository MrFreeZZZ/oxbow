// Edit Stack: a stack's commits and branch lines as a plan the user rearranges, the plan as git
// gets it, and the sheets that carry it out. The commit menu's Drop, Squash into Previous, Edit
// Message of an older commit and Add Staged Changes are one-line plans of the same kind.

import { api } from "./api";
import type { Part, Request } from "./confirm.svelte";
import type { BranchContext } from "./branches";
import { stoppedOnConflicts } from "./merge";
import { shortId } from "./format";
import type { HistoryRow, Stack, StackPlan, StackPreview, StackStep, StepAction } from "./types";

const plural = (n: number, word: string) => `${n} ${n === 1 ? word : word === "branch" ? "branches" : `${word}s`}`;

/** A line of the plan on screen, newest first: a commit, or the line of a branch above its commits. */
export type Item = { kind: "branch"; name: string } | { kind: "commit"; id: string };

export interface Draft {
  items: Item[];
  /** What happens to each commit; a missing one is picked. */
  acts: Record<string, StepAction>;
  /** New messages: a reword's, or the combined message of squashes into a commit. */
  messages: Record<string, string>;
  /** Replay the stack on the trunk's newest commit. */
  onto: boolean;
}

export const ACTIONS: { id: StepAction; label: string; key: string; hint: string }[] = [
  { id: "pick", label: "Pick", key: "P", hint: "keep as is" },
  { id: "reword", label: "Reword", key: "R", hint: "new message" },
  { id: "edit", label: "Edit", key: "E", hint: "stop to amend" },
  { id: "squash", label: "Squash", key: "S", hint: "into the one below" },
  { id: "fixup", label: "Fixup", key: "F", hint: "squash, drop message" },
  { id: "drop", label: "Drop", key: "D", hint: "remove commit" },
];

export const itemKey = (item: Item) => (item.kind === "branch" ? `b:${item.name}` : `c:${item.id}`);

/** The stack as it is: each branch line right above its newest commit. */
export function initialItems(stack: Stack): Item[] {
  const items: Item[] = [];
  for (const commit of stack.commits) {
    for (const b of stack.branches.filter((b) => b.tip === commit.id)) items.push({ kind: "branch", name: b.name });
    items.push({ kind: "commit", id: commit.id });
  }
  return items;
}

export function freshDraft(stack: Stack, onto = false): Draft {
  return { items: initialItems(stack), acts: {}, messages: {}, onto };
}

const actOf = (draft: Draft, id: string): StepAction => draft.acts[id] ?? "pick";
const folds = (a: StepAction) => a === "squash" || a === "fixup";

/** The commit a squash or fixup at `id` melts into: the next kept commit below it, or the one
 *  that one melts into. Null when nothing below is left. */
export function foldTarget(draft: Draft, id: string): string | null {
  const commits = draft.items.filter((i): i is { kind: "commit"; id: string } => i.kind === "commit").map((i) => i.id);
  for (let j = commits.indexOf(id) + 1; j < commits.length; j++) {
    const a = actOf(draft, commits[j]);
    if (a === "drop") continue;
    if (folds(a)) return foldTarget(draft, commits[j]);
    return commits[j];
  }
  return null;
}

/** Squashes and fixups that melt into `id`, newest first. */
/** Where a typed combined message for the commit `id` is kept in `Draft.messages`. */
export const squashKey = (id: string) => `squash:${id}`;

export function foldsInto(draft: Draft, id: string): string[] {
  return draft.items.flatMap((i) => (i.kind === "commit" && folds(actOf(draft, i.id)) && foldTarget(draft, i.id) === id ? [i.id] : []));
}

/** The commit's message as it is now. */
const messageOf = (stack: Stack, id: string) => stack.commits.find((c) => c.id === id)?.message ?? "";

/** The message of the commit that squashes melt into, as git would combine it. */
export function combinedMessage(stack: Stack, draft: Draft, target: string): string {
  const squashed = foldsInto(draft, target)
    .filter((id) => actOf(draft, id) === "squash")
    .reverse();
  const own = draft.messages[target] && actOf(draft, target) === "reword" ? draft.messages[target] : messageOf(stack, target);
  return [own, ...squashed.map((id) => messageOf(stack, id))].map((m) => m.trim()).join("\n\n");
}

export function planOf(stack: Stack, draft: Draft): StackPlan {
  const steps: StackStep[] = [...draft.items].reverse().map((item) => {
    if (item.kind === "branch") return { kind: "branch", name: item.name };
    const action = actOf(draft, item.id);
    // A commit that squashes melt into gets the combined message, as typed or as git makes it.
    const squashed = foldsInto(draft, item.id).some((id) => actOf(draft, id) === "squash");
    const message = squashed
      ? (draft.messages[squashKey(item.id)] ?? combinedMessage(stack, draft, item.id))
      : action === "reword"
        ? (draft.messages[item.id] ?? null)
        : null;
    return { kind: "commit", id: item.id, action, message };
  });
  return {
    top: stack.top,
    base: stack.base.id,
    onto: draft.onto ? stack.trunkTip.id : stack.base.id,
    ontoName: draft.onto ? stack.trunk : null,
    head: stack.head,
    steps,
  };
}

/** Commits the plan keeps as commits of their own. */
export function keptCount(draft: Draft): number {
  return draft.items.filter((i) => i.kind === "commit" && actOf(draft, i.id) !== "drop" && !(folds(actOf(draft, i.id)) && foldTarget(draft, i.id))).length;
}

/** Move the item at `index` one place up (-1) or down (+1). The top branch stays on top. */
export function moveItem(items: Item[], index: number, dir: -1 | 1, top: string): Item[] | null {
  const to = index + dir;
  if (to < 0 || to >= items.length) return null;
  const isTop = (i: Item) => i.kind === "branch" && i.name === top;
  if (isTop(items[index]) || isTop(items[to])) return null;
  const next = [...items];
  [next[index], next[to]] = [next[to], next[index]];
  return next;
}

/** Move the item at `from` so it lands at `to` (an index in the list without it). */
export function dragItem(items: Item[], from: number, to: number, top: string): Item[] | null {
  const next = [...items];
  const [item] = next.splice(from, 1);
  const at = Math.max(1, Math.min(to, next.length));
  if (item.kind === "branch" && item.name === top) return null;
  next.splice(at, 0, item);
  return next.every((i, k) => k === 0 || !(i.kind === "branch" && i.name === top)) ? next : null;
}

// --- The sheets -------------------------------------------------------------------------------

const chip = (ctx: BranchContext, name: string): Part => ({ branch: name, color: ctx.colorOf(name) });

/** "a, b and c" as branch chips. */
function chips(ctx: BranchContext, names: string[]): Part[] {
  const parts: Part[] = [];
  names.forEach((name, i) => {
    if (i) parts.push(i === names.length - 1 ? " and " : ", ");
    parts.push(chip(ctx, name));
  });
  return parts;
}

/** What the rebase changes, in a sentence or two, from the dry run. */
function consequences(ctx: BranchContext, stack: Stack, preview: StackPreview): { parts: Part[]; warn: boolean } {
  const parts: Part[] = [];
  const moving = preview.branches.filter((b) => b.moves).map((b) => b.name);
  if (moving.length) parts.push(" ", ...chips(ctx, moving), moving.length === 1 ? " moves with its commits." : " move with their commits.");
  const force = preview.branches.filter((b) => b.forcePush).map((b) => b.name);
  if (force.length) parts.push(" ", ...chips(ctx, force), ` ${force.length === 1 ? "is" : "are"} already pushed, so the next push has to be a force push.`);
  if (preview.emptied.length) parts.push(` ${plural(preview.emptied.length, "commit")} ${stack.trunk} already has ${preview.emptied.length === 1 ? "is" : "are"} left out.`);
  if (stack.leftBehind.length) parts.push(" ", ...chips(ctx, stack.leftBehind), ` fork${stack.leftBehind.length === 1 ? "s" : ""} off the stack and stay${stack.leftBehind.length === 1 ? "s" : ""} where ${stack.leftBehind.length === 1 ? "it is" : "they are"}.`);
  if (preview.conflict) {
    parts.push(" The dry run stops on a conflict in ", { quote: preview.conflict.summary }, ": the rebase will stop there so you can resolve it.");
  }
  return { parts, warn: !!force.length || !!preview.conflict };
}

/** Rebase Stack: the plan from the Edit Stack screen. */
export function editStackRequest(ctx: BranchContext, stack: Stack, draft: Draft, preview: StackPreview): Request {
  const plan = planOf(stack, draft);
  const { parts, warn } = consequences(ctx, stack, preview);
  const body: Part[] = ["Rewrites ", plural(preview.rewritten + preview.squashed, "commit"), " of the stack"];
  if (draft.onto) body.push(" and replays it on top of ", chip(ctx, stack.trunk), ` ${shortId(stack.trunkTip.id)}`);
  body.push(", oldest first, by the list below.", ...parts);
  if (preview.edits) body.push(` It stops at ${plural(preview.edits, "commit")} for you to change; Continue Rebase goes on.`);
  return {
    title: `Rebase the ${stack.top} stack?`,
    body,
    icon: "rebase",
    tone: warn ? "warn" : undefined,
    button: "Rebase Stack",
    status: "Rebasing the stack…",
    done: "Stack updated.",
    note: "Nothing is pushed. Undo is in the Operation Log.",
    resolveConflicts: true,
    recover: (failure) => stoppedOnConflicts(failure, "rebase"),
    action: { kind: "editStack", plan },
  };
}

/** Push the branches the rebase rewrote, each to its upstream. */
export function pushBranchesRequest(ctx: BranchContext, stack: Stack, names: string[]): Request | null {
  const branches = stack.branches.filter((b) => names.includes(b.name) && b.upstream);
  if (!branches.length) return null;
  const remote = branches[0].upstream!.remote;
  const same = branches.filter((b) => b.upstream!.remote === remote);
  return {
    title: `Push ${plural(same.length, "branch")} to ${remote}?`,
    body: ["Sends ", ...chips(ctx, same.map((b) => b.name)), ` to ${remote}, replacing the commits the rebase rewrote. If someone else pushed to one of them since your last fetch, that one is refused and nothing of theirs is lost.`],
    icon: "push",
    tone: "warn",
    button: "Force Push",
    status: `Pushing to ${remote}…`,
    done: `Pushed ${plural(same.length, "branch")} to ${remote}.`,
    action: { kind: "pushBranches", remote, branches: same.map((b) => ({ branch: b.name, upstream: b.upstream!.branch })) },
  };
}

// --- One-step edits from the commit menu ------------------------------------------------------

/** The checked-out branch's stack with one commit's action changed, and what that does. */
async function oneStep(id: string, act: StepAction, message: string | null): Promise<{ stack: Stack; draft: Draft; preview: StackPreview }> {
  const stack = await api.stack(null);
  const draft = freshDraft(stack);
  draft.acts[id] = act;
  if (message !== null) draft.messages[id] = message;
  const preview = await api.stackPreview(planOf(stack, draft));
  return { stack, draft, preview };
}

function oneStepRequest(ctx: BranchContext, stack: Stack, draft: Draft, preview: StackPreview, id: string, opening: Part[], extra: Partial<Request>): Request {
  const { parts, warn } = consequences(ctx, stack, preview);
  // Newest first, so its place is the number of commits above it.
  const above = stack.commits.findIndex((c) => c.id === id);
  const body: Part[] = [...opening];
  if (above > 0) body.push(` ${plural(above, "commit")} above it get${above === 1 ? "s" : ""} a new SHA.`);
  body.push(...parts);
  return {
    title: "",
    body,
    icon: "rebase",
    tone: warn ? "warn" : undefined,
    button: "",
    note: "Nothing is pushed. Undo is in the Operation Log.",
    resolveConflicts: true,
    recover: (failure) => stoppedOnConflicts(failure, "rebase"),
    action: { kind: "editStack", plan: planOf(stack, draft) },
    ...extra,
  };
}

const commitParts = (row: { id: string; summary: string }): Part[] => [{ code: shortId(row.id) }, " ", { quote: row.summary }];

export async function dropCommitRequest(ctx: BranchContext, row: HistoryRow): Promise<Request> {
  const { stack, draft, preview } = await oneStep(row.id, "drop", null);
  const branch = stack.commits.find((c) => c.id === row.id)?.branch ?? stack.top;
  return oneStepRequest(ctx, stack, draft, preview, row.id, ["Removes ", ...commitParts(row), " from ", chip(ctx, branch), ". Its changes go with it."], {
    title: "Drop this commit?",
    danger: true,
    icon: "drop",
    button: "Drop Commit",
    status: `Dropping ${shortId(row.id)}…`,
    done: `Dropped ${shortId(row.id)}.`,
  });
}

export async function squashRequest(ctx: BranchContext, row: HistoryRow): Promise<Request> {
  const { stack, draft, preview } = await oneStep(row.id, "squash", null);
  const target = foldTarget(draft, row.id);
  const into = stack.commits.find((c) => c.id === target);
  if (!into) throw new Error("There is no commit before this one in the stack to squash it into.");
  const sheet = (typed: string): Request => {
    draft.messages[squashKey(into.id)] = typed;
    return oneStepRequest(ctx, stack, draft, preview, row.id, ["Melts ", ...commitParts(row), " into the commit before it, ", ...commitParts(into), ", as one commit with both changes."], {
      title: "Squash into the previous commit?",
      icon: "merge",
      button: "Squash",
      fields: [{ label: "Message", text: { value: typed, multiline: true, placeholder: "Commit message", edit: (value) => sheet(value) } }],
      invalid: typed.trim() ? null : "Type a message for the combined commit.",
      status: `Squashing ${shortId(row.id)}…`,
      done: `Squashed ${shortId(row.id)} into ${shortId(into.id)}.`,
    });
  };
  return sheet(combinedMessage(stack, draft, into.id));
}

export async function rewordRequest(ctx: BranchContext, row: HistoryRow): Promise<Request> {
  const stack = await api.stack(null);
  const commit = stack.commits.find((c) => c.id === row.id);
  if (!commit) throw new Error("This commit is not in the checked-out branch’s stack.");
  // What a new message does to the stack doesn't depend on its words: one dry run will do.
  const { draft, preview } = await oneStep(row.id, "reword", `${commit.message}\n\n(reworded)`);
  const sheet = (typed: string): Request => {
    draft.messages[row.id] = typed;
    const changed = !!typed.trim() && typed.trim() !== commit.message.trim();
    return oneStepRequest(ctx, stack, draft, preview, row.id, ["Replaces the message of ", { code: shortId(row.id) }, " on ", chip(ctx, commit.branch), ". Its changes stay as they are."], {
      title: "Edit the message of this commit?",
      icon: "edit",
      button: "Save Message",
      fields: [{ label: "Message", text: { value: typed, multiline: true, placeholder: "Commit message", edit: (value) => sheet(value) } }],
      invalid: !typed.trim() ? "Type a commit message." : !changed ? "Change the message first." : null,
      status: "Saving the message…",
      done: "Saved the new message.",
    });
  };
  return sheet(commit.message);
}

export function addToCommitRequest(ctx: BranchContext, row: HistoryRow): Request {
  const staged = ctx.history.rows[0]?.worktree?.staged ?? 0;
  const head = ctx.history.head.branch ?? "HEAD";
  return {
    title: "Add the staged changes to this commit?",
    body: [
      `Your ${plural(staged, "staged file")} go${staged === 1 ? "es" : ""} into `,
      ...commitParts(row),
      " as if they had always been part of it: a fixup commit, then a rebase that melts it in. Commits above it on ",
      chip(ctx, head),
      " get new SHAs; unstaged changes stay as they are.",
    ],
    icon: "commit",
    button: "Add to Commit",
    invalid: staged ? null : "Stage the changes to add first.",
    note: "Nothing is pushed. Undo is in the Operation Log.",
    status: `Adding to ${shortId(row.id)}…`,
    done: `Added the staged changes to ${shortId(row.id)}.`,
    resolveConflicts: true,
    recover: (failure) => stoppedOnConflicts(failure, "rebase"),
    action: { kind: "addToCommit", commit: row.id },
  };
}
