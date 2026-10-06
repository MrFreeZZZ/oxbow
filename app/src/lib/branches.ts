// Confirmations for checking out, creating, renaming and deleting branches.

import type { Part, Recovery, Request } from "./confirm.svelte";
import { remoteTrouble } from "./remote";
import type { Action, CommitBrief, DeletionCheck, Failure, History, RefInfo } from "./types";
import { shortId } from "./format";

export interface BranchContext {
  history: History;
  /** Lane color of a branch's latest commit, by local or remote name (`main`, `origin/main`). */
  colorOf: (name: string) => number;
  /** Files with uncommitted changes. */
  uncommitted: number;
}

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

const chip = (ctx: BranchContext, name: string, color = ctx.colorOf(name)): Part => ({ branch: name, color });

/** The checked-out branch's chip, or the detached commit. */
function here(ctx: BranchContext): Part {
  const { branch, commit } = ctx.history.head;
  return branch ? chip(ctx, branch) : { code: commit ? shortId(commit) : "HEAD" };
}

/** "Your 4 uncommitted files come along." */
function comeAlong(ctx: BranchContext): string {
  return ctx.uncommitted ? ` Your ${plural(ctx.uncommitted, "uncommitted file")} come${ctx.uncommitted === 1 ? "s" : ""} along.` : "";
}

const local = (ctx: BranchContext, name: string) => ctx.history.refs.find((r) => r.kind === "local" && r.name === name);

/** The branch's upstream, unless it was deleted on the remote. */
export function upstreamOf(ref: RefInfo) {
  return ref.tracking && !ref.tracking.gone ? { remote: ref.tracking.remote, branch: ref.tracking.branch } : null;
}

/** Why `name` can't be a new branch, or null. `except` is the branch being renamed. */
export function nameProblem(ctx: BranchContext, name: string, except?: string): string | null {
  if (!name) return "Type a name for the branch.";
  if (/\s/.test(name)) return "Branch names can’t contain spaces.";
  if (/[~^:?*[\\\x00-\x1f\x7f]/.test(name)) return "Branch names can’t contain ~ ^ : ? * [ or \\.";
  if (
    name === "HEAD" ||
    name === "@" ||
    /^[-/.]|[/.]$|\.lock$|\.\.|\/\/|@\{|\/\.|\.lock\//.test(name)
  ) {
    return "That isn’t a valid branch name.";
  }
  if (name !== except && local(ctx, name)) return `A branch named ${name} already exists.`;
  return null;
}

/** Commits in a sentence: `1a2b3c4 “Fix login”, 5d6e7f8 “Add test” and 2 more`. */
function listCommits(commits: CommitBrief[]): Part[] {
  const parts: Part[] = [];
  const shown = commits.slice(0, 3);
  shown.forEach((c, i) => {
    if (i) parts.push(i === shown.length - 1 && commits.length === shown.length ? " and " : ", ");
    parts.push({ code: shortId(c.id) }, " ", { quote: c.summary });
  });
  if (commits.length > shown.length) parts.push(` and ${commits.length - shown.length} more`);
  return parts;
}

/** Stash and Switch, when uncommitted files are in the way of a checkout. */
function changesInTheWay(failure: Failure, action: Action & { kind: "switch" | "track" }, ctx: BranchContext): Recovery | null {
  if (failure.kind !== "localChanges") return null;
  const branch = action.branch;
  return {
    title: "Your uncommitted changes are in the way",
    body: [
      "Some files you changed are different on ",
      chip(ctx, action.kind === "track" ? `${action.remote}/${branch}` : branch),
      ", so switching would overwrite your changes. Stash and Switch puts all uncommitted files aside in the stash first. They wait there, under Stashes in the sidebar, until you bring them back.",
    ],
    icon: "warn",
    tone: "warn",
    button: {
      label: "Stash and Switch",
      action: { ...action, stash: true },
      status: `Stashing your changes, then switching to ${branch}…`,
      done: `Switched to ${branch}. Your changes are in the stash.`,
    },
    close: "Cancel",
    note: "Nothing changed yet.",
  };
}

export function switchRequest(ctx: BranchContext, branch: string): Request {
  const action: Action = { kind: "switch", branch, stash: false };
  const body: Part[] = ["Checks out ", chip(ctx, branch), " and makes it the current branch, so new commits go there. "];
  if (ctx.history.head.branch) body.push("You leave ", here(ctx), " as it is.");
  else body.push("You leave the commit ", here(ctx), ", which is on no branch.");
  body.push(comeAlong(ctx));
  return {
    title: `Switch to ${branch}?`,
    body,
    icon: "checkout",
    button: "Switch",
    status: `Switching to ${branch}…`,
    done: `Switched to ${branch}.`,
    recover: (failure) => changesInTheWay(failure, action, ctx),
    action,
  };
}

/** Check out a remote branch (`origin/x`) as a local branch that tracks it. */
export function trackRequest(ctx: BranchContext, ref: RefInfo, author: string): Request {
  const remote = ref.remote!;
  const branch = ref.name.slice(remote.length + 1);
  const action: Action = { kind: "track", remote, branch, stash: false };
  return {
    title: `Check out ${branch}?`,
    body: [
      "Creates a local branch ",
      chip(ctx, branch, ctx.colorOf(ref.name)),
      " from ",
      chip(ctx, ref.name),
      `${author ? ` by ${author}` : ""} and switches to it. It tracks the remote, so Pull and Push work right away.${comeAlong(ctx)}`,
    ],
    icon: "checkout",
    button: "Check Out",
    status: `Checking out ${branch}…`,
    done: `Created ${branch} from ${remote} and switched to it.`,
    recover: (failure) => changesInTheWay(failure, action, ctx),
    action,
  };
}

interface NewBranch {
  name: string;
  /** Key of the chosen base. */
  base: string;
  switchTo: boolean;
  publish: boolean;
}

interface Base {
  key: string;
  label: string;
  /** What `git switch -c` gets; null for HEAD. */
  ref: string | null;
  sha: string | null;
  color: number;
}

const PREFIXES = ["feature/", "fix/", "chore/"];

/** The New Branch sheet. `from` is the branch it was opened on, if any. */
export function newBranchRequest(ctx: BranchContext, from?: string): Request {
  const { head, trunk } = ctx.history;
  const target = (name: string) => local(ctx, name)?.target ?? null;
  const bases: Base[] = [
    {
      key: "head",
      label: head.branch ? `${head.branch} (HEAD)` : "HEAD",
      ref: null,
      sha: head.commit,
      color: head.branch ? ctx.colorOf(head.branch) : 0,
    },
  ];
  if (trunk && trunk !== head.branch && local(ctx, trunk)) bases.push({ key: "trunk", label: trunk, ref: trunk, sha: target(trunk), color: ctx.colorOf(trunk) });
  if (from && from !== head.branch && from !== trunk) bases.push({ key: "from", label: from, ref: from, sha: target(from), color: ctx.colorOf(from) });
  const start = from === head.branch ? "head" : from === trunk ? "trunk" : from ? "from" : "head";
  return newBranchSheet(ctx, bases, { name: "", base: start, switchTo: true, publish: false });
}

function newBranchSheet(ctx: BranchContext, bases: Base[], state: NewBranch): Request {
  const again = (change: Partial<NewBranch>) => newBranchSheet(ctx, bases, { ...state, ...change });
  const name = state.name.trim();
  const base = bases.find((b) => b.key === state.base) ?? bases[0];
  const remote = ctx.history.defaultRemote;
  const problem = nameProblem(ctx, name);
  const clash = remote && name && ctx.history.refs.some((r) => r.kind === "remote" && r.name === `${remote}/${name}`);
  const color = base.color;

  const body: Part[] = ["Creates ", name && !problem ? chip(ctx, name, color) : "a new branch", " at "];
  if (base.ref || ctx.history.head.branch) body.push(chip(ctx, base.ref ?? ctx.history.head.branch!, color));
  if (base.sha) body.push(base.ref || ctx.history.head.branch ? ", commit " : "commit ", { code: shortId(base.sha) });
  if (state.switchTo) body.push(`, and switches to it.${comeAlong(ctx)}`);
  else if (ctx.history.head.branch) body.push(". You stay on ", here(ctx), ".");
  else body.push(".");
  if (state.publish && remote) body.push(` Then it is published to ${remote}, so others can see it.`);

  const action: Action = {
    kind: "createBranch",
    name,
    start: base.ref,
    switch: state.switchTo,
    publish: state.publish && remote ? remote : null,
  };
  const withPrefix = (prefix: string) => {
    const bare = PREFIXES.reduce((n, p) => (n.startsWith(p) ? n.slice(p.length) : n), state.name);
    return state.name.startsWith(prefix) ? bare : prefix + bare;
  };
  return {
    title: "New Branch",
    body,
    icon: "branch",
    button: "Create Branch",
    fields: [
      {
        label: "Name",
        text: { value: state.name, placeholder: "feature/csv-export", edit: (value) => again({ name: value }) },
        chips: PREFIXES.map((p) => ({ label: p, on: state.name.startsWith(p), pick: () => again({ name: withPrefix(p) }) })),
        error: name && problem ? problem : undefined,
        note: clash ? `${remote} already has ${name}. To work on it, use Check Out as Local Branch on ${remote}/${name} in the sidebar.` : undefined,
      },
      {
        label: "From",
        chips: bases.map((b) => ({ label: b.label, on: b.key === base.key, pick: () => again({ base: b.key }) })),
      },
    ],
    options: [
      { label: "Switch to the new branch", on: state.switchTo, toggle: () => again({ switchTo: !state.switchTo }) },
      ...(remote
        ? [
            {
              label: `Publish it to ${remote}`,
              sub: "Others see it, and Push and Pull work without arguments",
              on: state.publish,
              toggle: () => again({ publish: !state.publish }),
            },
          ]
        : []),
    ],
    invalid: problem,
    status: `Creating ${name}…`,
    done: `Created ${name}${state.switchTo ? " and switched to it" : ""}.`,
    recover: (failure) => remoteTrouble(failure, action, "Publishing to", remote ?? "the remote", `Created and published ${name}.`),
    action,
  };
}

/** The Rename sheet for a local branch. */
export function renameRequest(ctx: BranchContext, ref: RefInfo): Request {
  return renameSheet(ctx, ref, ref.name, !!upstreamOf(ref) && !ref.tracking?.behind);
}

function renameSheet(ctx: BranchContext, ref: RefInfo, to: string, onRemote: boolean): Request {
  const from = ref.name;
  const name = to.trim();
  const up = upstreamOf(ref);
  const problem = name === from ? "Type the new name." : nameProblem(ctx, name, from);
  const color = ctx.colorOf(from);
  const body: Part[] = ["Renames ", chip(ctx, from), " to ", name && !problem ? chip(ctx, name, color) : "a new name", ". Commits and history stay the same."];
  // Deleting the old name on the remote would take commits the local branch doesn't have with it.
  const behind = ref.tracking?.behind ?? 0;
  if (up && behind) {
    body.push(
      " ",
      chip(ctx, `${up.remote}/${up.branch}`),
      ` has ${plural(behind, "commit")} that ${from} doesn’t, so it keeps its name. Pull first to rename it there too.`,
    );
  } else if (up && !onRemote) body.push(" ", chip(ctx, `${up.remote}/${up.branch}`), " keeps the old name until you push the new one.");
  if (up && onRemote) body.push(` On ${up.remote}, the new name is pushed and the old one deleted.`);
  const action: Action = { kind: "renameBranch", from, to: name, upstream: up && onRemote ? up : null };
  return {
    title: `Rename ${from}?`,
    body,
    icon: "edit",
    button: "Rename",
    fields: [
      {
        label: "New name",
        text: { value: to, edit: (value) => renameSheet(ctx, ref, value, onRemote) },
        error: name && name !== from && problem ? problem : undefined,
      },
    ],
    options: up && !behind
      ? [
          {
            label: `Also rename it on ${up.remote}`,
            sub: "Teammates see the new name after their next fetch",
            on: onRemote,
            toggle: () => renameSheet(ctx, ref, to, !onRemote),
          },
        ]
      : [],
    invalid: problem,
    status: `Renaming ${from}…`,
    done: `Renamed ${from} to ${name}.`,
    recover: (failure) => (up ? remoteTrouble(failure, action, "Renaming on", up.remote, `Renamed ${from} to ${name}.`) : null),
    action,
  };
}

/** The Delete sheet for a local branch, after asking git what would be lost. */
export function deleteRequest(ctx: BranchContext, ref: RefInfo, check: DeletionCheck, onRemote = false): Request {
  const name = ref.name;
  const up = upstreamOf(ref);
  const both = !!up && onRemote;
  const lost = both ? check.lostWithUpstream : check.lost;
  const force = lost.length > 0;
  const body: Part[] = ["Deletes the local branch ", chip(ctx, name), ". "];
  if (ref.tracking?.gone) body.push("Its branch on the remote was already deleted, usually after its pull request was merged. ");
  const are = lost.length === 1 ? "is" : "are";
  if (force && both) {
    body.push(`${plural(lost.length, "commit")} ${are} on no other branch, so deleting it here and on ${up!.remote} loses ${lost.length === 1 ? "it" : "them"}: `, ...listCommits(lost), ".");
  } else if (force) {
    body.push(`Its ${plural(lost.length, "commit")} ${are} on no other branch and never pushed: `, ...listCommits(lost), ".");
  } else if (up && !both && check.lost.length === 0 && check.lostWithUpstream.length) {
    body.push("Its commits stay safe on ", chip(ctx, `${up.remote}/${up.branch}`), ".");
  } else {
    body.push("All its commits are also on other branches, so nothing is lost.");
  }
  if (both) body.push(` It is also deleted on ${up!.remote}, for everyone.`);
  const action: Action = { kind: "deleteBranch", name, force, upstream: both ? up : null };
  return {
    title: `Delete ${name}?`,
    body,
    icon: "drop",
    button: force ? "Delete Anyway" : "Delete",
    danger: true,
    options: up
      ? [
          {
            label: `Also delete ${up.remote}/${up.branch}`,
            sub: "Removes it on the remote for everyone",
            on: onRemote,
            toggle: () => deleteRequest(ctx, ref, check, !onRemote),
          },
        ]
      : [],
    note: force ? "Oxbow can’t bring deleted commits back yet." : undefined,
    status: `Deleting ${name}…`,
    done: `Deleted ${name}${both ? ` here and on ${up!.remote}` : ""}.`,
    recover: (failure) => notMerged(failure, ctx, action) ?? (up ? remoteTrouble(failure, action, "Deleting on", up.remote, `Deleted ${name}.`) : null),
    action,
  };
}

/** `git branch -d` refused although no commit would be lost: offer -D. */
function notMerged(failure: Failure, ctx: BranchContext, action: Action & { kind: "deleteBranch" }): Recovery | null {
  if (failure.kind !== "notMerged") return null;
  return {
    title: `git calls ${action.name} not merged`,
    body: [
      "git deletes a branch with -d only when it is merged into the current branch or its upstream. The commits of ",
      chip(ctx, action.name),
      " are on other branches, so deleting it loses nothing.",
    ],
    icon: "warn",
    tone: "warn",
    button: { label: "Delete Anyway", danger: true, action: { ...action, force: true }, status: `Deleting ${action.name}…`, done: `Deleted ${action.name}.` },
    close: "Cancel",
  };
}

/** Delete a remote branch (`origin/x`) on its remote. */
export function deleteRemoteRequest(ctx: BranchContext, ref: RefInfo, author: string, lost: CommitBrief[]): Request {
  const remote = ref.remote!;
  const branch = ref.name.slice(remote.length + 1);
  const action: Action = { kind: "deleteRemoteBranch", remote, branch };
  return {
    title: `Delete ${branch} on ${remote}?`,
    body: [
      "Deletes ",
      chip(ctx, ref.name),
      ` on ${remote} for everyone.${author ? ` Its latest commit is by ${author}.` : ""} Your local branches are not touched.`,
      ...(lost.length
        ? [` ${plural(lost.length, "commit")} ${lost.length === 1 ? "is" : "are"} on no other branch and ${lost.length === 1 ? "is" : "are"} lost unless someone still has ${lost.length === 1 ? "it" : "them"}: `, ...listCommits(lost), "."]
        : []),
    ],
    icon: "drop",
    button: lost.length ? "Delete Anyway" : `Delete on ${remote}`,
    danger: true,
    note: "Anyone who still has the branch can push it back.",
    status: `Deleting ${branch} on ${remote}…`,
    done: `Deleted ${ref.name}.`,
    recover: (failure) => remoteTrouble(failure, action, "Deleting on", remote, `Deleted ${ref.name}.`),
    action,
  };
}
