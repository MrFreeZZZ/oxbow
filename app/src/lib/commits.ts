// Actions on one commit from its menu in History: cherry-pick it onto the checked-out branch,
// revert it, reset the branch to it, undo the last commit, and edit the last commit's message.

import { api } from "./api";
import type { Part, Recovery, Request } from "./confirm.svelte";
import type { BranchContext } from "./branches";
import { nothingToPick, stoppedOnConflicts } from "./merge";
import type { Failure, History, HistoryRow, ResetMode } from "./types";
import { shortId } from "./format";

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
const chip = (ctx: BranchContext, name: string): Part => ({ branch: name, color: ctx.colorOf(name) });

/** The checked-out branch, or HEAD when it is detached. */
const hereName = (ctx: BranchContext) => ctx.history.head.branch ?? "HEAD";
const here = (ctx: BranchContext): Part => (ctx.history.head.branch ? chip(ctx, ctx.history.head.branch) : { code: "HEAD" });

/** The commits at `starts` and every commit before them, as far as History has loaded. */
function reachable(history: History, starts: string[]): Set<string> {
  const byId = new Map(history.rows.map((r) => [r.id, r]));
  const seen = new Set<string>();
  const stack = [...starts];
  while (stack.length) {
    const next = stack.pop()!;
    if (seen.has(next)) continue;
    seen.add(next);
    stack.push(...(byId.get(next)?.parents ?? []));
  }
  return seen;
}

/** `id` and every commit before it. */
export const ancestors = (history: History, id: string | null) => reachable(history, id ? [id] : []);

/** Commits of the checked-out branch that a reset to `target` takes off it, newest first. */
function leaving(history: History, target: string): HistoryRow[] {
  const kept = ancestors(history, target);
  const now = ancestors(history, history.head.commit);
  return history.rows.filter((r) => now.has(r.id) && !kept.has(r.id));
}

/** Commits some branch or tag other than the checked-out branch keeps. */
function keptElsewhere(history: History): Set<string> {
  const others = history.refs.filter((r) => r.kind !== "stash" && r.kind !== "head" && !(r.kind === "local" && r.name === history.head.branch));
  return reachable(
    history,
    others.map((r) => r.target),
  );
}

/** " 2 of them are already on origin…", when rewriting pushed commits. */
function pushedWarning(ctx: BranchContext, commits: HistoryRow[], one = false): string {
  const remote = ctx.history.tracking?.remote;
  const pushed = commits.filter((r) => !r.unpushed).length;
  if (!remote || !pushed) return "";
  const which = one ? "It is" : pushed === commits.length ? (pushed === 1 ? "It is" : "They are") : `${pushed} of them are`;
  return ` ${which} already on ${remote}, so the next push has to be a force push.`;
}

const commitParts = (row: HistoryRow): Part[] => [{ code: shortId(row.id) }, " ", { quote: row.summary }];

/** Git stopped before changing anything because uncommitted edits are in the way. */
function changesInTheWay(failure: Failure, what: string): Recovery | null {
  if (failure.kind !== "localChanges") return null;
  return {
    title: "Your uncommitted changes are in the way",
    body: [`The ${what} changes files you have uncommitted edits in, so git stopped before changing anything. Commit or stash those edits, then try again.`],
    icon: "warn",
    tone: "warn",
    close: "OK",
    note: "Nothing changed.",
  };
}

export function cherryPickRequest(ctx: BranchContext, row: HistoryRow): Request {
  const sha = shortId(row.id);
  const from = row.graph.branch;
  const body: Part[] = ["Copies commit ", ...commitParts(row), ` by ${row.authorName}`];
  if (from && from !== ctx.history.head.branch) body.push(" from ", chip(ctx, from));
  body.push(" and adds it as a new commit on top of ", here(ctx), ". The original commit stays where it is.");
  if (row.parents.length > 1) body.push(" It is a merge, so the new commit gets the changes it brought into its first parent.");
  return {
    title: `Cherry-pick this commit onto ${hereName(ctx)}?`,
    body,
    icon: "cherry",
    button: "Cherry-Pick",
    note: "Nothing is pushed.",
    status: `Cherry-picking ${sha}…`,
    done: `Cherry-picked ${sha} onto ${hereName(ctx)}.`,
    recover: (failure) => nothingToPick(ctx, failure) ?? changesInTheWay(failure, "cherry-pick") ?? stoppedOnConflicts(failure, "cherry-pick"),
    action: { kind: "cherryPick", commit: row.id },
  };
}

export function revertRequest(ctx: BranchContext, row: HistoryRow): Request {
  const sha = shortId(row.id);
  const body: Part[] = ["Adds a new commit on top of ", here(ctx), " that undoes ", ...commitParts(row), "."];
  if (row.parents.length > 1) {
    body.push(
      " It is a merge: the new commit takes out everything the merge brought in. Merging that branch again later won’t bring those changes back unless you revert this revert first.",
    );
  } else {
    body.push(" The original commit stays in history, so this is safe on a branch that is already pushed.");
  }
  return {
    title: `Revert ${sha}?`,
    body,
    icon: "revert",
    button: "Revert",
    note: "Nothing is pushed.",
    status: `Reverting ${sha}…`,
    done: `Reverted ${sha} on ${hereName(ctx)}.`,
    recover: (failure) => changesInTheWay(failure, "revert") ?? stoppedOnConflicts(failure, "revert"),
    action: { kind: "revert", commit: row.id },
  };
}

const MODES: { mode: ResetMode; label: string; keeps: string }[] = [
  { mode: "soft", label: "Soft", keeps: "keep changes staged" },
  { mode: "mixed", label: "Mixed", keeps: "keep changes unstaged" },
  { mode: "hard", label: "Hard", keeps: "discard changes" },
];

/** The modes of Reset, for its submenu. */
export const resetModes = MODES;

export function resetRequest(ctx: BranchContext, row: HistoryRow, mode: ResetMode): Request {
  const sha = shortId(row.id);
  const name = hereName(ctx);
  const gone = leaving(ctx.history, row.id);
  const body: Part[] = ["Points ", here(ctx), " at commit ", ...commitParts(row), ". "];
  if (gone.length) {
    const they = gone.length === 1 ? "its" : "their";
    body.push(`${plural(gone.length, "commit")} now on ${name} will no longer be on the branch; `);
    if (mode === "soft") body.push(`${they} changes stay staged, ready to commit again.`);
    else if (mode === "mixed") body.push(`${they} changes stay in your files, unstaged.`);
    else body.push(`${they} changes are discarded`);
  } else if (mode === "hard") {
    body.push("Your files are set to that commit");
  } else {
    body.push("No commits leave the branch.");
  }
  if (mode === "hard") body.push(ctx.uncommitted ? ` together with your ${plural(ctx.uncommitted, "uncommitted file")}.` : ".");
  else if (ctx.uncommitted) body.push(` Your ${plural(ctx.uncommitted, "uncommitted file")} stay as they are.`);
  const warning = pushedWarning(ctx, gone);
  if (warning) body.push(warning);
  const elsewhere = keptElsewhere(ctx.history);
  const orphans = gone.filter((r) => !elsewhere.has(r.id)).length;
  if (orphans) {
    const them = orphans === gone.length ? (orphans === 1 ? "It is" : "They are") : `${orphans} of them are`;
    body.push(` ${them} on no other branch, so History stops showing ${orphans === 1 ? "it" : "them"}; git keeps such commits for about two weeks.`);
  }
  const discards = mode === "hard" && (gone.length > 0 || ctx.uncommitted > 0);
  return {
    title: `Reset ${name} to ${sha}?`,
    body,
    icon: "reset",
    danger: discards,
    tone: warning ? "warn" : undefined,
    button: discards ? "Reset and Discard" : "Reset",
    fields: [
      {
        label: "Mode",
        chips: MODES.map((m) => ({ label: m.label, on: m.mode === mode, pick: () => resetRequest(ctx, row, m.mode) })),
        note: MODES.find((m) => m.mode === mode)!.keeps.replace(/^\w/, (c) => c.toUpperCase()) + ".",
      },
    ],
    note: discards ? "Discarded changes can’t be brought back from Oxbow." : "Nothing is pushed.",
    status: `Resetting ${name} to ${sha}…`,
    done: `${name} is at ${sha} now.`,
    action: { kind: "reset", commit: row.id, mode },
  };
}

/** Take the last commit off the branch, its changes back to staged. */
export function undoCommitRequest(ctx: BranchContext, row: HistoryRow): Request {
  const sha = shortId(row.id);
  const warning = pushedWarning(ctx, [row], true);
  return {
    title: "Undo the last commit?",
    body: ["Takes ", ...commitParts(row), " off ", here(ctx), ". Its changes go back to staged, so you can change them and commit again.", warning],
    icon: "undo",
    tone: warning ? "warn" : undefined,
    button: "Undo Commit",
    status: `Undoing ${sha}…`,
    done: `Undid ${sha}. Its changes are staged.`,
    action: { kind: "reset", commit: "HEAD~1", mode: "soft" },
  };
}

/** Change the message of the last commit. */
export async function editMessageRequest(ctx: BranchContext, row: HistoryRow): Promise<Request> {
  const detail = await api.commitDetail(row.id);
  const original = [detail.summary, detail.body.trim()].filter(Boolean).join("\n\n");
  return editMessageSheet(ctx, row, original, original);
}

function editMessageSheet(ctx: BranchContext, row: HistoryRow, original: string, typed: string): Request {
  const warning = pushedWarning(ctx, [row], true);
  const staged = ctx.history.rows[0]?.worktree?.staged ?? 0;
  const body: Part[] = ["Replaces the message of ", { code: shortId(row.id) }, " on ", here(ctx), ". Its changes stay as they are"];
  body.push(staged ? `, and your ${plural(staged, "staged file")} stay${staged === 1 ? "s" : ""} staged.` : ".", " The commit gets a new SHA.", warning);
  return {
    title: "Edit the message of the last commit?",
    body,
    icon: "edit",
    tone: warning ? "warn" : undefined,
    button: "Save Message",
    fields: [
      {
        label: "Message",
        text: { value: typed, placeholder: "Commit message", multiline: true, edit: (value) => editMessageSheet(ctx, row, original, value) },
      },
    ],
    invalid: !typed.trim() ? "Type a commit message." : typed.trim() === original.trim() ? "Change the message first." : null,
    status: "Saving the message…",
    done: "Saved the new message.",
    action: { kind: "reword", message: typed },
  };
}
