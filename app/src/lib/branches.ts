// Confirmations for checking out, creating, renaming and deleting branches.

import type { Part, Recovery, Request } from "./confirm.svelte";
import { remoteTrouble } from "./remote";
import type { Action, CommitBrief, DeletionCheck, Failure, History, HistoryRow, RefInfo } from "./types";
import { shortId } from "./format";
import { onlyPrefix, splitPrefix, type BranchPrefix } from "./branchPrefixes";
import { firstName, prefixesFor, prefixStart, similarNote } from "./branchPrefs";
import { prefs } from "./prefs.svelte";
import { elsewhereRequest, refusedForWorktree, worktreeOf } from "./worktrees";

export interface BranchContext {
  history: History;
  /** Lane color of a branch's latest commit, by local or remote name (`main`, `origin/main`). */
  colorOf: (name: string) => number;
  /** Files with uncommitted changes; 0 also when they couldn't be read: see `changesUnknown`. */
  uncommitted: number;
}

/** `git status` failed: whether there are uncommitted changes isn't known, so a sheet must not
 *  say there are none. */
export function changesUnknown(ctx: BranchContext): boolean {
  return !!ctx.history.rows[0]?.worktree?.unknown;
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
  if (onlyPrefix(name)) return "Type a name for the branch.";
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
function changesInTheWay(failure: Failure, action: Action & { kind: "switch" | "track" | "detach" }, ctx: BranchContext): Recovery | null {
  if (failure.kind !== "localChanges") return null;
  const target = action.kind === "detach" ? shortId(action.commit) : action.branch;
  // A branch that keeps detached commits was already created before the switch stopped.
  const retry: Action = action.kind === "switch" ? { ...action, stash: true, keep: null } : { ...action, stash: true };
  return {
    title: "Your uncommitted changes are in the way",
    body: [
      "Some files you changed are different ",
      ...(action.kind === "detach" ? ["in ", { code: target }] : ["on ", chip(ctx, action.kind === "track" ? `${action.remote}/${target}` : target)]),
      ", so switching would overwrite your changes. Stash and Switch puts all uncommitted files aside in the stash first. They wait there, under Stashes in the sidebar, until you bring them back.",
    ],
    icon: "warn",
    tone: "warn",
    button: {
      label: "Stash and Switch",
      action: retry,
      status: `Stashing your changes, then switching to ${target}…`,
      done: `Switched to ${target}. Your changes are in the stash.`,
    },
    close: "Cancel",
    note: "Nothing changed yet.",
  };
}

/** Commits made on a detached HEAD, which no branch keeps, newest first. */
export function onNoBranch(history: History): HistoryRow[] {
  return history.head.branch ? [] : history.rows.filter((r) => r.noBranch);
}

/** "…stay on no branch: History stops showing them…", for leaving a detached HEAD. */
function leftBehind(lost: HistoryRow[]): Part[] {
  const them = lost.length === 1 ? "it" : "them";
  return [
    ` The ${plural(lost.length, "commit")} you made without a branch, `,
    ...listCommits(lost.map((r) => ({ id: r.id, summary: r.summary, authorName: r.authorName, time: r.time }))),
    `, stay on no branch: History stops showing ${them}, and git deletes ${them} after about two weeks. To keep ${them}, create a branch first.`,
  ];
}

export function switchRequest(ctx: BranchContext, branch: string): Request {
  // Another worktree has it checked out: git refuses, so that folder is offered instead.
  const elsewhere = worktreeOf(ctx.history, branch);
  if (elsewhere) return elsewhereRequest(ctx, branch, elsewhere);
  const action: Action = { kind: "switch", branch, stash: false, keep: null };
  const lost = onNoBranch(ctx.history);
  if (lost.length) {
    return {
      title: `Leave ${plural(lost.length, "commit")} behind?`,
      body: ["Checks out ", chip(ctx, branch), " and makes it the current branch.", ...leftBehind(lost), comeAlong(ctx)],
      icon: "warn",
      tone: "warn",
      button: "Switch Anyway",
      alt: { label: "Create Branch First", request: () => keepRequest(ctx, branch) },
      status: `Switching to ${branch}…`,
      done: `Switched to ${branch}. ${plural(lost.length, "commit")} left behind.`,
      recover: (failure) => changesInTheWay(failure, action, ctx) ?? refusedForWorktree(failure, ctx),
      action,
    };
  }
  const body: Part[] = ["Checks out ", chip(ctx, branch), " and makes it the current branch, so new commits go there. "];
  if (ctx.history.head.branch) body.push("You leave ", here(ctx), " as it is.");
  else body.push("HEAD stops pointing straight at ", here(ctx), ".");
  body.push(comeAlong(ctx));
  return {
    title: `Switch to ${branch}?`,
    body,
    icon: "checkout",
    button: "Switch",
    status: `Switching to ${branch}…`,
    done: `Switched to ${branch}.`,
    recover: (failure) => changesInTheWay(failure, action, ctx) ?? refusedForWorktree(failure, ctx),
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

/** Check out a commit without a branch. `tag` names the tag it was picked by. */
export function detachRequest(ctx: BranchContext, row: HistoryRow, tag?: string): Request {
  const action: Action = { kind: "detach", commit: tag ?? row.id, stash: false };
  const sha = shortId(row.id);
  const lost = onNoBranch(ctx.history).filter((r) => r.id !== row.id);
  const body: Part[] = ["Moves HEAD to commit ", { code: sha }, " ", { quote: row.summary }];
  if (tag) body.push(", tagged ", { code: tag }, ", to build or test that version. You will be on no branch (a detached HEAD), so to change something create a branch from here first.");
  else body.push(". You will be on no branch, a “detached HEAD”: commits you make there belong to no branch until you create one.");
  if (lost.length) body.push(...leftBehind(lost));
  body.push(comeAlong(ctx));
  return {
    title: lost.length ? `Leave ${plural(lost.length, "commit")} behind?` : tag ? `Check out ${tag}?` : "Check out this commit without a branch?",
    body,
    icon: lost.length ? "warn" : "checkout",
    tone: lost.length ? "warn" : undefined,
    button: lost.length ? "Check Out Anyway" : "Check Out",
    status: `Checking out ${tag ?? sha}…`,
    done: `Checked out ${tag ?? sha}. HEAD is not on a branch now.`,
    recover: (failure) => changesInTheWay(failure, action, ctx),
    action,
  };
}

/** Create a branch at a detached HEAD, so its commits belong to it. With `then`, switch there afterwards. */
export function keepRequest(ctx: BranchContext, then?: string): Request {
  return keepSheet(ctx, then, firstName(prefixesFor(ctx.history)), false);
}

function keepSheet(ctx: BranchContext, then: string | undefined, typed: string, publishIt: boolean): Request {
  const again = (value: string, publish = publishIt) => keepSheet(ctx, then, value, publish);
  const name = typed.trim();
  const problem = nameProblem(ctx, name);
  const commit = ctx.history.head.commit;
  const lost = onNoBranch(ctx.history);
  const remote = ctx.history.defaultRemote;
  const publish = !then && publishIt && remote ? remote : null;
  const tag = ctx.history.refs.find((r) => r.kind === "tag" && r.target === commit)?.name;
  const nameChip = name && !problem ? chip(ctx, name) : "a new branch";
  const body: Part[] = ["Creates ", nameChip, " at ", { code: commit ? shortId(commit) : "HEAD" }];
  if (then) {
    body.push(`, where HEAD is now, so the ${plural(lost.length, "commit")} you made without a branch belong to it. Then checks out `, chip(ctx, then), ". Nothing is left behind.");
  } else {
    body.push(", where HEAD is now, and switches to it. ");
    if (lost.length) body.push(`The ${plural(lost.length, "commit")} you made without a branch now belong to it, so they show up in the sidebar and can be pushed. Your files do not change.`);
    else body.push("New commits go to this branch. Your files do not change.");
    if (publish) body.push(` Then it is published to ${publish}.`);
  }
  const action: Action = then
    ? { kind: "switch", branch: then, stash: false, keep: name }
    : { kind: "createBranch", name, start: null, switch: true, publish };
  return {
    title: then ? `Keep ${plural(lost.length, "commit")}, then switch` : lost.length ? `Keep ${plural(lost.length, "commit")} on a new branch` : `New branch at ${tag ?? (commit ? shortId(commit) : "HEAD")}`,
    body,
    icon: "branch",
    button: then ? "Create and Switch" : "Create Branch",
    fields: [
      {
        label: "Name",
        text: { value: typed, placeholder: "new-cache", prefixes: prefixesFor(ctx.history), edit: (value) => again(value) },
        error: !onlyPrefix(name) && problem ? problem : undefined,
        note: !problem ? (similarNote(ctx.history, name) ?? undefined) : undefined,
      },
    ],
    options:
      !then && remote
        ? [
            {
              label: `Publish it to ${remote}`,
              sub: "Others see it, and Push and Pull work without arguments",
              on: publishIt,
              toggle: () => again(typed, !publishIt),
            },
          ]
        : [],
    invalid: problem,
    status: `Creating ${name}…`,
    done: then ? `Created ${name} with your commits and switched to ${then}.` : `Created ${name}. HEAD is on a branch again.`,
    recover: (failure) =>
      action.kind === "switch"
        ? changesInTheWay(failure, action, ctx)
        : remoteTrouble(failure, action, "Publishing to", remote ?? "the remote", `Created and published ${name}.`),
    action,
  };
}

interface NewBranch {
  name: string;
  /** Key of the base the sheet opened with. */
  base: string;
  /** The base was picked by hand, or the sheet was opened on it: the prefix doesn't move it. */
  picked: boolean;
  switchTo: boolean;
  publish: boolean;
  /** Fetch first and start at the upstream's tip; null: as Settings says. */
  fetch: boolean | null;
  /** Leave the uncommitted files behind in a stash; null: as Settings says. */
  stash: boolean | null;
}

interface Base {
  key: string;
  label: string;
  /** What `git switch -c` gets; null for HEAD. */
  ref: string | null;
  sha: string | null;
  color: number;
}

/** The New Branch sheet. `from` is the branch it was opened on, if any; `at` a commit it was opened on. */
/** `switchTo` is the sheet's first choice: a branch that rescues lost work starts without one,
 *  so nothing but the new branch changes. */
export function newBranchRequest(ctx: BranchContext, from?: string, at?: HistoryRow, switchTo = true): Request {
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
  if (at && at.id !== head.commit) bases.push({ key: "commit", label: shortId(at.id), ref: at.id, sha: at.id, color: at.graph.color });
  const start = at && at.id !== head.commit ? "commit" : from === head.branch ? "head" : from === trunk ? "trunk" : from ? "from" : "head";
  const list = prefixesFor(ctx.history);
  return newBranchSheet(ctx, bases, list, { name: firstName(list), base: start, picked: !!from || !!at, switchTo, publish: false, fetch: null, stash: null });
}

function newBranchSheet(ctx: BranchContext, bases: Base[], list: BranchPrefix[], state: NewBranch): Request {
  const again = (change: Partial<NewBranch>) => newBranchSheet(ctx, bases, list, { ...state, ...change });
  const { history } = ctx;
  const name = state.name.trim();
  // hotfix/ and the like start from the production branch, until another base is picked.
  const production = !state.picked && prefixStart(splitPrefix(name, list).prefix, list) === "production" && history.trunk;
  const productionBase = production ? bases.find((b) => b.key === "trunk") ?? (history.head.branch === history.trunk ? bases[0] : undefined) : undefined;
  const base = productionBase ?? bases.find((b) => b.key === state.base) ?? bases[0];
  const remote = history.defaultRemote;
  const problem = nameProblem(ctx, name);
  const clash = remote && name && history.refs.some((r) => r.kind === "remote" && r.name === `${remote}/${name}`);
  const color = base.color;

  // Fetch first: a branch base with an upstream and no commits of its own starts at the upstream's tip.
  const baseBranch = base.key === "commit" ? null : (base.ref ?? history.head.branch);
  const tracking = baseBranch ? local(ctx, baseBranch)?.tracking : null;
  const canFetch = !!tracking && !tracking.gone && tracking.ahead === 0;
  const fetching = canFetch && (state.fetch ?? prefs.get("oxbow.branch.fetchFirst"));
  const upstream = tracking ? `${tracking.remote}/${tracking.branch}` : null;
  const leaving = state.switchTo && ctx.uncommitted > 0 && (state.stash ?? prefs.get("oxbow.branch.uncommitted") === "stash");

  const nameChip = name && !problem ? chip(ctx, name, color) : "a new branch";
  const body: Part[] = [];
  if (fetching && upstream) {
    body.push(`Fetches ${tracking!.remote}, then creates `, nameChip, " at ", chip(ctx, upstream, color), ", the latest commit there");
  } else {
    body.push("Creates ", nameChip, " at ");
    const named = base.key !== "commit" && (base.ref || history.head.branch);
    if (named) body.push(chip(ctx, base.ref ?? history.head.branch!, color));
    if (base.sha) body.push(named ? ", commit " : "commit ", { code: shortId(base.sha) });
  }
  if (state.switchTo) {
    body.push(", and switches to it.");
    if (leaving) body.push(` Your ${plural(ctx.uncommitted, "uncommitted file")} stay${ctx.uncommitted === 1 ? "s" : ""} on `, here(ctx), " in a stash.");
    else body.push(comeAlong(ctx));
  } else if (history.head.branch) body.push(". You stay on ", here(ctx), ".");
  else body.push(".");
  if (state.publish && remote) body.push(` Then it is published to ${remote}, so others can see it.`);

  const action: Action = {
    kind: "createBranch",
    name,
    start: fetching && upstream ? upstream : base.ref,
    switch: state.switchTo,
    publish: state.publish && remote ? remote : null,
    fetch: fetching && tracking ? tracking.remote : null,
    stash: leaving,
  };
  const similar = !clash && !problem ? similarNote(history, name) : null;
  return {
    title: "New Branch",
    body,
    icon: "branch",
    button: "Create Branch",
    fields: [
      {
        label: "Name",
        text: { value: state.name, placeholder: "csv-export", prefixes: list, edit: (value) => again({ name: value }) },
        error: !onlyPrefix(name) && problem ? problem : undefined,
        note: clash ? `${remote} already has ${name}. To work on it, use Check Out as Local Branch on ${remote}/${name} in the sidebar.` : (similar ?? undefined),
      },
      {
        label: "From",
        chips: bases.map((b) => ({ label: b.label, on: b.key === base.key, pick: () => again({ base: b.key, picked: true }) })),
        note: productionBase && productionBase.key !== state.base ? `${splitPrefix(name, list).prefix} starts from the production branch` : undefined,
      },
    ],
    options: [
      { label: "Switch to the new branch", on: state.switchTo, toggle: () => again({ switchTo: !state.switchTo }) },
      ...(canFetch && upstream
        ? [
            {
              label: `Fetch ${tracking!.remote} first`,
              sub: `Starts at ${upstream}’s latest commit, not where your ${baseBranch} is`,
              on: fetching,
              toggle: () => again({ fetch: !fetching }),
            },
          ]
        : []),
      ...(state.switchTo && ctx.uncommitted
        ? [
            {
              label: `Leave ${plural(ctx.uncommitted, "uncommitted file")} on ${history.head.branch ?? "HEAD"}`,
              sub: `Stashed as “Before switching to ${name || "…"}”: Apply it when you come back`,
              on: leaving,
              toggle: () => again({ stash: !leaving }),
            },
          ]
        : []),
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
    status: fetching ? `Fetching ${tracking!.remote}…` : `Creating ${name}…`,
    done: `Created ${name}${state.switchTo ? " and switched to it" : ""}.${leaving ? ` Your files are in the stash.` : ""}`,
    recover: (failure) =>
      fetching && tracking ? remoteTrouble(failure, action, "Fetching from", tracking.remote, `Created ${name}.`) : remoteTrouble(failure, action, "Publishing to", remote ?? "the remote", `Created and published ${name}.`),
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
    note: force ? (both ? `Undo brings the branch back here, not on ${up!.remote}.` : "Undo brings the branch back, from the toast or the Operation Log.") : undefined,
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
