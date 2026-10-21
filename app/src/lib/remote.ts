// Confirmations for Fetch, Pull and Push, and the way out when one of them fails.

import type { Part, Recovery, Request, Step, TermLine } from "./confirm.svelte";
import { api } from "./api";
import { mac } from "./keys";
import { describe, stoppedOnConflicts } from "./merge";
import type { Action, Bisect, CommitBrief, Failure, HistoryRow, Operation, SshCheck, Tracking, UnpushedSubmodule } from "./types";
import { unpushedWarning } from "./submodules";
import { shortId } from "./format";
import { errorLine } from "./reads";

export interface RemoteContext {
  /** Checked-out branch; null when HEAD is detached. */
  branch: string | null;
  /** Lane color of the checked-out branch. */
  color: number;
  tracking: Tracking | null;
  /** The remote Fetch, Pull and Push talk to. */
  remote: string | null;
  remotes: string[];
  /** Commits of the branch that the remote does not have yet, newest first. */
  unpushed: HistoryRow[];
  /** A merge, rebase, cherry-pick, revert or stash apply waiting to be finished. */
  operation?: Operation | null;
  bisect?: Bisect | null;
  /** git's error when the upstreams couldn't be read: `tracking` null then says nothing. */
  unread?: string | null;
}

/** The way out offered next to a reason, done by the caller. */
export type SyncFix = "publish" | "resolve" | "continue" | "abort" | "newBranch" | "finishBisect" | "retry";

/** Why Pull or Push can't run now, shown when the grey button is clicked, with what fixes it. */
export interface SyncBlock {
  title: string;
  body: Part[];
  /** The last one is the main way out. */
  fixes: { label: string; fix: SyncFix }[];
}

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

/** Where the branch lives on the remote: its upstream, or a branch of the same name. */
function target(ctx: RemoteContext): { remote: string; upstream: string; publish: boolean } | null {
  if (!ctx.branch) return null;
  if (ctx.tracking && !ctx.tracking.gone) return { remote: ctx.tracking.remote, upstream: ctx.tracking.branch, publish: false };
  if (!ctx.remote) return null;
  return { remote: ctx.remote, upstream: ctx.branch, publish: true };
}

function chip(ctx: RemoteContext, name: string): Part {
  return { branch: name, color: ctx.color };
}

export function syncBlock(ctx: RemoteContext, kind: "pull" | "push"): SyncBlock | null {
  const bisect = ctx.bisect;
  if (bisect) {
    return {
      title: "Bisect in progress",
      body: [
        bisect.found ? "Bisect found the commit and waits to be finished. Finishing it goes back to " : "Git checks out commits one by one while you test them. Finishing it goes back to ",
        bisect.startIsBranch ? chip(ctx, bisect.start) : { code: shortId(bisect.start) },
        ".",
      ],
      fixes: [{ label: "Finish Bisect", fix: "finishBisect" }],
    };
  }
  const op = ctx.operation;
  if (op) {
    const words = describe(op);
    const stash = op.kind === "stashApply";
    const commits = op.kind === "merge" || op.kind === "squash";
    const finish = stash ? "Finish" : commits ? `Commit ${words.noun}` : `Continue ${words.noun}`;
    const on: Part = op.branch ? chip(ctx, op.branch) : { code: "HEAD" };
    const doing: Part[] =
      op.kind === "rebase"
        ? ["Push and Pull wait while ", on, " is being rebased."]
        : stash
          ? ["Push and Pull wait until the stash is applied to ", on, "."]
          : ["Push and Pull wait until the ", words.verb, " on ", on, " is finished."];
    return {
      title: stash ? "Finish applying the stash first" : `Finish the ${words.verb} first`,
      body: [
        ...doing,
        op.conflicted
          ? ` ${plural(op.conflicted, "file")} still ${op.conflicted === 1 ? "has" : "have"} conflicts.`
          : ` All conflicts are resolved: ${finish.toLowerCase()}, then ${kind}.`,
      ],
      fixes: [
        { label: stash ? "Undo Apply" : `Abort ${words.noun}`, fix: "abort" },
        op.conflicted ? { label: "Resolve Conflicts", fix: "resolve" } : { label: finish, fix: "continue" },
      ],
    };
  }
  if (!ctx.branch) {
    return {
      title: "You’re not on a branch",
      body: ["Push and Pull work with a branch. Commits made here belong to none: put them on a branch first."],
      fixes: [{ label: "Create Branch…", fix: "newBranch" }],
    };
  }
  const t = ctx.tracking;
  if (!t && ctx.unread) {
    return {
      title: "Couldn’t read the upstream",
      body: ["Oxbow doesn’t know where ", chip(ctx, ctx.branch), " pushes and pulls, so it doesn’t guess: ", { code: errorLine(ctx.unread) }],
      fixes: [{ label: "Retry", fix: "retry" }],
    };
  }
  if (kind === "pull" && t?.gone) {
    return {
      title: `${t.remote}/${t.branch} was deleted`,
      body: ["There is nothing left to pull. Push publishes ", chip(ctx, ctx.branch), ` to ${ctx.remote ?? t.remote} again.`],
      fixes: ctx.remote ? [{ label: "Publish Again…", fix: "publish" }] : [],
    };
  }
  if (kind === "pull" && !t) {
    return {
      title: "Nothing to pull from yet",
      body: ctx.remote
        ? [chip(ctx, ctx.branch), ` isn’t on ${ctx.remote}, so it has no upstream. Publish it, and Pull will take new commits from `, chip(ctx, `${ctx.remote}/${ctx.branch}`), "."]
        : [chip(ctx, ctx.branch), " has no upstream to pull from."],
      fixes: ctx.remote ? [{ label: "Publish…", fix: "publish" }] : [],
    };
  }
  if (kind === "push" && !target(ctx)) {
    return { title: "No remote to push to", body: ["Add one in Settings › Repository."], fixes: [] };
  }
  return null;
}

export function canPull(ctx: RemoteContext): boolean {
  return syncBlock(ctx, "pull") === null;
}

export function canPush(ctx: RemoteContext): boolean {
  return syncBlock(ctx, "push") === null;
}

export function fetchRequest(ctx: RemoteContext): Request {
  const all = ctx.remotes.length > 1;
  const remote = ctx.remote ?? ctx.remotes[0];
  return {
    title: all ? "Fetch from all remotes?" : `Fetch from ${remote}?`,
    body: [
      "Downloads new commits, branches and tags from ",
      all ? `${ctx.remotes.join(", ")}` : { code: remote },
      " and forgets remote branches that were deleted there.",
    ],
    icon: "fetch",
    button: "Fetch",
    note: "Your branches and files do not change.",
    status: all ? "Fetching all remotes…" : `Fetching ${remote}…`,
    done: all ? "Fetched all remotes." : `Fetched ${remote}.`,
    recover: (failure) => networkOrAuth(failure, { kind: "fetch", remote: all ? null : remote }, "Fetching", ctx, all ? "Fetched all remotes." : `Fetched ${remote}.`),
    action: { kind: "fetch", remote: all ? null : remote },
  };
}

/** Fetch what a shallow clone left out, so History reaches the first commit. */
export function unshallowRequest(remote: string): Request {
  const action: Action = { kind: "unshallow", remote };
  const done = `Fetched the full history of ${remote}.`;
  return {
    title: `Fetch the full history from ${remote}?`,
    body: [
      "This is a shallow clone: it has only the latest commits. Downloads the older commits from ",
      { code: remote },
      ", so History, search and blame reach back to the first commit. On a big repository this takes a while.",
    ],
    icon: "fetch",
    button: "Fetch Full History",
    note: "Your branches and files do not change.",
    status: `Fetching the full history from ${remote}…`,
    done,
    recover: (failure) => remoteTrouble(failure, action, "Fetching", remote, done),
    action,
  };
}

/** How Pull goes, from Settings › Git: rebase is the default. */
export interface PullSetup {
  mode: "rebase" | "merge" | "fastForward";
  autostash: boolean;
  /** Fetch the whole remote first, not only the branch being pulled. */
  fetchFirst?: boolean;
  /** Check out the commits the pulled commits record in the submodules too. */
  submodules?: boolean;
  /** The repository has submodules now, so the sheet mentions them; a pull that brings the
   *  first one clones it all the same. */
  hasSubmodules?: boolean;
}

export function pullRequest(ctx: RemoteContext, setup: PullSetup = { mode: "rebase", autostash: true }): Request {
  const t = ctx.tracking!;
  const branch = ctx.branch!;
  const upstream = `${t.remote}/${t.branch}`;
  // The mode goes along, so Merge… or Rebase… picked for this one pull is what runs.
  const action: Action = { kind: "pull", remote: t.remote, branch: t.branch, fetchFirst: !!setup.fetchFirst, submodules: !!setup.submodules, mode: setup.mode };
  const how: Part[] =
    setup.mode === "merge"
      ? [" and merges it into ", chip(ctx, branch), "; a merge commit joins the two when both have new commits."]
      : setup.mode === "fastForward"
        ? [" and moves ", chip(ctx, branch), " up to it. If the branch has commits of its own, nothing changes and Git says so."]
        : [" and replays your local commits of ", chip(ctx, branch), " on top of it."];
  return {
    title: `Pull into ${branch}?`,
    body: [
      ...(setup.fetchFirst ? ["Fetches every branch of ", { code: t.remote }, ", then takes "] : ["Fetches "]),
      chip(ctx, upstream),
      ...how,
      setup.autostash ? " Uncommitted files are stashed first and restored after." : " Uncommitted changes to the same files stop the pull.",
      ...(setup.submodules && setup.hasSubmodules ? [" Submodules then check out the commits the pulled commits record."] : []),
    ],
    icon: "pull",
    button: "Pull",
    note: t.rewritten
      ? `${upstream} still has the old versions of commits you rewrote: pulling brings them back. Push replaces them instead.`
      : t.behind
        ? `${upstream} had ${plural(t.behind, "new commit")} at the last fetch.`
        : undefined,
    status: `Pulling ${upstream} into ${branch}…`,
    done: `${branch} is up to date with ${upstream}.`,
    recover: (failure) => conflict(failure, setup.mode) ?? networkOrAuth(failure, action, "Pulling", ctx, `${branch} is up to date with ${upstream}.`),
    action,
  };
}

/** Push, or Publish; with submodule commits their remotes don't have, the sheet says so and
 *  pushes those first unless asked not to. */
export function pushRequest(ctx: RemoteContext, submodules: UnpushedSubmodule[] = []): Request {
  const request = plainPushRequest(ctx);
  const action = request.action;
  if (!submodules.length || action?.kind !== "push" || action.force) return request;
  const both: Action & { kind: "push" } = { ...action, submodules: true };
  const one = submodules.length === 1;
  const verb = action.setUpstream ? "Publish" : "Push";
  return {
    ...request,
    action: both,
    button: `${verb} Both`,
    warning: {
      title: one ? `${submodules[0].title} has a commit that isn’t pushed` : `${submodules.length} submodules have commits that aren’t pushed`,
      body: unpushedWarning(submodules, `${verb} Both`),
    },
    alt: { label: one ? `${verb} Without ${submodules[0].title}` : `${verb} Without Them`, request: () => request },
    recover: (failure) => pushRecovery(failure, ctx, both),
  };
}

function plainPushRequest(ctx: RemoteContext): Request {
  const to = target(ctx)!;
  const branch = ctx.branch!;
  const action: Action = {
    kind: "push",
    remote: to.remote,
    branch,
    upstream: to.upstream,
    setUpstream: to.publish,
    force: false,
    lease: null,
    noVerify: false,
  };
  const recover = (failure: Failure) => pushRecovery(failure, ctx, action);
  if (to.publish) {
    return {
      title: `Publish ${branch} to ${to.remote}?`,
      body: [
        "Creates ",
        chip(ctx, `${to.remote}/${branch}`),
        " with ",
        plural(ctx.unpushed.length, "commit"),
        " of ",
        chip(ctx, branch),
        " and remembers it as the upstream, so Pull and Push know where the branch lives.",
      ],
      icon: "push",
      button: "Publish",
      note: "After the push your team can see this branch.",
      status: `Publishing ${branch} to ${to.remote}…`,
      done: `Published ${branch} to ${to.remote}.`,
      recover,
      action,
    };
  }
  const commits = ctx.unpushed;
  const rewritten = ctx.tracking?.rewritten;
  const replay = ctx.tracking?.replay;
  if (rewritten && replay) return replayPushRequest(ctx, action, rewritten, replay);
  if (rewritten) return rewrittenPushRequest(ctx, action, to, rewritten);
  const body: Part[] = [`Uploads ${plural(commits.length, "commit")} of `, chip(ctx, branch), ` to ${to.remote}${commits.length ? ": " : ""}`];
  const listed = commits.slice(0, 3);
  listed.forEach((c, i) => {
    if (i) body.push(i === listed.length - 1 && commits.length === listed.length ? " and " : ", ");
    body.push({ code: shortId(c.id) }, " ", { quote: c.summary });
  });
  if (commits.length > listed.length) body.push(` and ${commits.length - listed.length} more`);
  body.push(".");
  return {
    title: `Push ${branch} to ${to.remote}?`,
    body,
    icon: "push",
    button: "Push",
    note: "After the push your team can see these commits.",
    status: `Pushing ${branch} to ${to.remote}…`,
    done: commits.length ? `Pushed ${plural(commits.length, "commit")} to ${to.remote}/${to.upstream}.` : `${to.remote}/${to.upstream} is up to date.`,
    recover,
    action,
  };
}

/** Push after commits already on the remote were rewritten here (an edited message, a squash,
 *  a drop, Edit Stack): a plain push is refused, and Pull and Push would bring the old versions
 *  back. So it replaces them, with --force-with-lease at the tip seen at the last fetch. */
function rewrittenPushRequest(ctx: RemoteContext, action: Action & { kind: "push" }, to: { remote: string; upstream: string }, tip: string): Request {
  const remoteBranch = `${to.remote}/${to.upstream}`;
  const branch = action.branch;
  // Try Again and Push Without Hook repeat this push, so they keep its force and lease too.
  const forced: Action & { kind: "push" } = { ...action, force: true, lease: tip };
  const old = ctx.tracking?.behind ?? 0;
  const fresh = Math.max(ctx.unpushed.length, ctx.tracking?.ahead ?? 0);
  // Undo Commit or a reset took pushed commits back, with nothing new in their place.
  const body: Part[] = fresh
    ? [
        "You rewrote commits that are already on ",
        chip(ctx, remoteBranch),
        `. The push replaces the old ${old === 1 ? "version" : `versions (${old})`} there with your ${plural(fresh, "commit")} of `,
        chip(ctx, branch),
        ". A normal push would be refused, and a pull would bring the old ones back.",
      ]
    : [
        `You took back ${old === 1 ? "a commit" : plural(old, "commit")} that ${old === 1 ? "is" : "are"} already on `,
        chip(ctx, remoteBranch),
        ". The push removes ",
        old === 1 ? "it" : "them",
        " there too, so the remote ends where ",
        chip(ctx, branch),
        " does. A normal push would be refused, and a pull would bring ",
        old === 1 ? "it" : "them",
        " back.",
      ];
  return {
    title: `Push ${branch} to ${to.remote} with force?`,
    body,
    icon: "push",
    tone: "warn",
    button: "Force Push",
    note: `If someone pushed to ${remoteBranch} after your last fetch, nothing is overwritten.`,
    status: `Force pushing ${branch} to ${to.remote}…`,
    done: `Pushed ${branch} to ${remoteBranch}, replacing the old commits.`,
    recover: (failure) => pushRecovery(failure, ctx, forced),
    action: forced,
  };
}

/** Push after commits already on the remote were rewritten here while someone pushed on top of
 *  their old versions. Pull and Push would drop an edited message (rebase sees the commit as
 *  already upstream), so the push copies their commits onto the branch first, then replaces
 *  the old versions with --force-with-lease at the tip seen at the last fetch. */
function replayPushRequest(ctx: RemoteContext, push: Action & { kind: "push" }, tip: string, replay: { base: string; commits: CommitBrief[] }): Request {
  const remoteBranch = `${push.remote}/${push.upstream}`;
  const branch = push.branch;
  const action: Action & { kind: "replayAndPush" } = {
    kind: "replayAndPush",
    remote: push.remote,
    branch,
    upstream: push.upstream,
    base: replay.base,
    lease: tip,
    noVerify: push.noVerify,
  };
  return {
    title: `Copy their commits and push ${branch} with force?`,
    body: [
      "You rewrote commits that are already on ",
      chip(ctx, remoteBranch),
      ", and ",
      ...whoPushed(replay.commits),
      " on top of the old versions. The push copies ",
      replay.commits.length === 1 ? "it" : "them",
      " onto ",
      chip(ctx, branch),
      ", then replaces the old versions on the remote. Your rewrite and their work both stay.",
    ],
    icon: "push",
    tone: "warn",
    button: "Copy and Push",
    note: "The copies get new SHAs: whoever pushed them pulls once to pick them up. If someone pushes again before you, nothing is overwritten.",
    status: `Copying commits and pushing ${branch}…`,
    done: `Pushed ${branch} to ${remoteBranch} with ${plural(replay.commits.length, "copied commit")}.`,
    recover: (failure) => replayRecovery(failure, ctx, push, action),
    action,
  };
}

function replayRecovery(failure: Failure, ctx: RemoteContext, push: Action & { kind: "push" }, action: Action & { kind: "replayAndPush" }): Recovery | null {
  if (failure.kind === "conflict") return stoppedOnConflicts(failure, "cherry-pick");
  if (failure.kind === "hook") {
    return {
      title: "The pre-push hook stopped the push",
      body: ["Their commits are copied onto ", chip(ctx, action.branch), ", but nothing was sent to ", { code: action.remote }, ". The hook’s output is below: fix what it reports, then try again."],
      icon: "hook",
      tone: "err",
      button: { label: "Try Again", action, status: `Pushing ${action.branch} to ${action.remote}…`, done: `Pushed ${action.branch}.` },
      alt: {
        label: "Push Without Hook…",
        danger: true,
        request: () => ({ ...noVerifyRequest(ctx, push), recover: (f) => replayRecovery(f, ctx, { ...push, noVerify: true }, { ...action, noVerify: true }), action: { ...action, noVerify: true } }),
      },
      close: "Close",
    };
  }
  if (failure.kind === "rejected" || failure.kind === "staleLease") return pushRecovery(failure, ctx, push);
  return networkOrAuth(failure, action, "Pushing", ctx, `Pushed ${action.branch}.`);
}

/** Who pushed what, e.g. "Kirill pushed 1a2b3c4 “Fix login”" or "Kirill and Maria pushed 3 commits". */
function whoPushed(incoming: CommitBrief[]): Part[] {
  if (incoming.length === 0) return ["Someone pushed new commits"];
  const people = [...new Set(incoming.map((c) => c.authorName))];
  const who = people.length <= 2 ? people.join(" and ") : `${people[0]} and ${people.length - 1} others`;
  if (incoming.length === 1) return [`${who} pushed `, { code: shortId(incoming[0].id) }, " ", { quote: incoming[0].summary }];
  return [`${who} pushed ${plural(incoming.length, "commit")}, the newest `, { code: shortId(incoming[0].id) }, " ", { quote: incoming[0].summary }];
}

function pushRecovery(failure: Failure, ctx: RemoteContext, action: Action & { kind: "push" }): Recovery | null {
  const remoteBranch = `${action.remote}/${action.upstream}`;
  const pullAndPush: Action = { kind: "pullAndPush", remote: action.remote, branch: action.branch, upstream: action.upstream, noVerify: action.noVerify };
  const n = Math.max(ctx.unpushed.length, ctx.tracking?.ahead ?? 0);
  const rewrite = failure.tracking;
  if ((failure.kind === "rejected" || failure.kind === "staleLease") && rewrite?.rewritten) {
    return rewrittenRecovery(failure, ctx, action, rewrite.rewritten, rewrite.replay);
  }
  if (failure.kind === "rejected" || failure.kind === "staleLease") {
    const count = failure.incoming.length;
    return {
      title: failure.kind === "staleLease" ? `${remoteBranch} changed again` : `${remoteBranch} has ${count === 1 ? "a commit" : count ? plural(count, "commit") : "commits"} you don’t have`,
      body: [
        failure.kind === "staleLease" ? "Nothing was overwritten: " : "The remote refused the push. ",
        ...whoPushed(failure.incoming),
        " to ",
        chip(ctx, remoteBranch),
        `. ${n ? `Your ${plural(n, "commit")}` : "Your commits"} can go on top of it: Pull and Push replays them after it, then pushes.${failure.kind === "staleLease" ? "" : " Nothing is overwritten."}`,
      ],
      icon: "warn",
      tone: "warn",
      button: { label: "Pull and Push", action: pullAndPush, status: `Pulling ${remoteBranch}, then pushing ${action.branch}…`, done: `Pulled and pushed ${action.branch}.` },
      alt: failure.remoteTip ? { label: "Force Push…", danger: true, request: () => forcePushRequest(ctx, action, failure, true) } : undefined,
      note: "If the commits clash, the pull stops and you can undo it.",
    };
  }
  if (failure.kind === "hook") {
    return {
      title: "The pre-push hook stopped the push",
      body: ["Nothing was sent to ", { code: action.remote }, ". The hook’s output is below: fix what it reports, then try again."],
      icon: "hook",
      tone: "err",
      button: { label: "Try Again", action, status: `Pushing ${action.branch} to ${action.remote}…`, done: `Pushed ${action.branch}.` },
      alt: { label: "Push Without Hook…", danger: true, request: () => noVerifyRequest(ctx, action) },
      close: "Close",
    };
  }
  // Copy and Push, run from this sheet's recovery, stops in a cherry-pick, not a pull.
  const copying = failure.output.includes("cherry-pick --continue");
  return (copying ? stoppedOnConflicts(failure, "cherry-pick") : conflict(failure)) ?? networkOrAuth(failure, action, "Pushing", ctx, `Pushed ${action.branch}.`);
}

/** A push of commits rewritten here was refused because the remote moved: someone pushed on
 *  top of the old versions. Pull and Push would drop an edited message, so the way out copies
 *  their commits over and pushes with the new lease. */
function rewrittenRecovery(failure: Failure, ctx: RemoteContext, action: Action & { kind: "push" }, tip: string, replay: { base: string; commits: CommitBrief[] } | null): Recovery {
  const remoteBranch = `${action.remote}/${action.upstream}`;
  // Its own failures go on from what it runs, with the new lease, not from the first push.
  const copy: Action & { kind: "replayAndPush" } | null = replay
    ? { kind: "replayAndPush", remote: action.remote, branch: action.branch, upstream: action.upstream, base: replay.base, lease: tip, noVerify: action.noVerify }
    : null;
  const forced: Action & { kind: "push" } = { ...action, force: true, lease: tip };
  const button: Step & { recover: (f: Failure) => Recovery | null } =
    replay && copy
      ? {
          action: copy,
          status: `Copying commits and pushing ${action.branch}…`,
          done: `Pushed ${action.branch} to ${remoteBranch} with ${plural(replay.commits.length, "copied commit")}.`,
          recover: (f) => replayRecovery(f, ctx, forced, copy),
        }
      : {
          action: forced,
          status: `Force pushing ${action.branch} to ${action.remote}…`,
          done: `Pushed ${action.branch} to ${remoteBranch}, replacing the old commits.`,
          recover: (f) => pushRecovery(f, ctx, forced),
        };
  return {
    title: `${remoteBranch} changed since your last fetch`,
    body: replay
      ? [
          "Nothing was overwritten: ",
          ...whoPushed(replay.commits),
          " on top of the old versions of the commits you rewrote. Copy and Push puts ",
          replay.commits.length === 1 ? "it" : "them",
          " onto ",
          chip(ctx, action.branch),
          ", then pushes. A pull would drop your rewrite instead.",
        ]
      : ["Nothing was overwritten. ", chip(ctx, action.branch), " already has everything new there, so the push can replace the old versions now."],
    icon: "warn",
    tone: "warn",
    button: { label: replay ? "Copy and Push" : "Force Push", ...button },
    alt: failure.remoteTip ? { label: "Overwrite Anyway…", danger: true, request: () => forcePushRequest(ctx, action, failure, true) } : undefined,
    note: replay ? "The copies get new SHAs: whoever pushed them pulls once to pick them up." : `Only if ${remoteBranch} is still ${shortId(tip)}.`,
  };
}

function forcePushRequest(ctx: RemoteContext, action: Action & { kind: "push" }, failure: Failure, lease: boolean): Request {
  const remoteBranch = `${action.remote}/${action.upstream}`;
  const tip = failure.remoteTip!;
  const lost = failure.incoming;
  const body: Part[] = ["Replaces ", chip(ctx, remoteBranch), " with your local ", chip(ctx, action.branch), ". "];
  if (lost.length === 1) body.push("The commit ", { code: shortId(lost[0].id) }, " ", { quote: lost[0].summary }, ` by ${lost[0].authorName} disappears from the remote.`);
  else if (lost.length) body.push(`${plural(lost.length, "commit")} disappear from the remote.`);
  if (!lease) body.push(" Without the check, anything pushed after you looked is overwritten too.");
  const forced: Action & { kind: "push" } = { ...action, force: true, lease: lease ? tip : null };
  return {
    title: `Overwrite ${action.upstream} on ${action.remote}?`,
    body,
    icon: "push",
    button: "Force Push",
    danger: true,
    note: "Whoever pushed those commits still has them, but has to sort it out.",
    status: `Force pushing ${action.branch}…`,
    done: `Force-pushed ${action.branch} to ${remoteBranch}.`,
    options: [
      {
        label: `Only if ${action.remote} still ends at ${shortId(tip)}`,
        sub: "If someone pushes again before you, nothing is overwritten",
        on: lease,
        toggle: () => forcePushRequest(ctx, action, failure, !lease),
      },
    ],
    recover: (f) => pushRecovery(f, ctx, forced),
    action: forced,
  };
}

function noVerifyRequest(ctx: RemoteContext, action: Action & { kind: "push" }): Request {
  return {
    title: "Push without the pre-push hook?",
    body: ["Sends ", chip(ctx, action.branch), " to ", { code: action.remote }, " without running the checks the hook does."],
    icon: "hook",
    button: "Push Anyway",
    danger: true,
    note: "The hook runs again on your next push.",
    status: `Pushing ${action.branch} to ${action.remote}…`,
    done: `Pushed ${action.branch} without the hook.`,
    recover: (f) => pushRecovery(f, ctx, { ...action, noVerify: true }),
    action: { ...action, noVerify: true },
  };
}

function conflict(failure: Failure, mode: PullSetup["mode"] = "rebase"): Recovery | null {
  return stoppedOnConflicts(failure, "pull", mode === "merge" ? "commit the merge." : "continue the rebase.");
}

/** Try Again after a remote could not be reached or refused the sign-in, for any action that talks to `remote`. */
export function remoteTrouble(failure: Failure, action: Action, verb: string, remote: string, done: string): Recovery | null {
  const ctx: RemoteContext = { branch: null, color: 0, tracking: null, remote, remotes: [remote], unpushed: [] };
  return networkOrAuth(failure, action, verb, ctx, done);
}

function networkOrAuth(failure: Failure, action: Action, verb: string, ctx: RemoteContext, done: string): Recovery | null {
  const remote = "remote" in action && action.remote ? action.remote : (ctx.remote ?? "the remote");
  const again = { label: "Try Again", action, status: `${verb} ${remote}…`, done };
  if (failure.kind === "auth" && failure.ssh) {
    const ssh = sshRecovery(failure.ssh, verbNoun(action), again);
    if (ssh) return ssh;
  }
  if (failure.kind === "auth") {
    return {
      title: `${remote} did not accept your sign-in`,
      body: [
        "Fetch, Pull and Push need a working SSH key or saved password for ",
        { code: remote },
        ". The output below says what failed. Nothing changed in your repository.",
      ],
      icon: "key",
      tone: "err",
      button: again,
      close: "Close",
    };
  }
  if (failure.kind === "network") {
    return {
      title: `Can’t reach ${remote}`,
      body: ["Check the internet connection or VPN, then try again. Nothing changed: your branch and files are as they were."],
      icon: "offline",
      tone: "warn",
      button: again,
      close: "Close",
    };
  }
  return null;
}

/** "Fetch", "Pull" or "Push", for "Fetch from origin connects over SSH…". */
function verbNoun(action: Action): string {
  if (action.kind === "fetch" || action.kind === "fetchTags" || action.kind === "unshallow") return "Fetch";
  if (action.kind === "pull") return "Pull";
  return "Push";
}

/** "GitHub" for github.com, as people say it; the host name for anything else. */
export function hostName(host: string): string {
  const known: Record<string, string> = { "github.com": "GitHub", "gitlab.com": "GitLab", "bitbucket.org": "Bitbucket", "codeberg.org": "Codeberg" };
  return known[host.toLowerCase()] ?? host;
}

/** What Oxbow ran to find out, for the sheet's terminal block. */
function checkedLines(check: SshCheck, fix: string[] = []): TermLine[] {
  const lines: TermLine[] = [];
  for (const step of check.steps) {
    lines.push({ kind: "cmd", text: step.command });
    for (const text of step.output.split("\n").filter(Boolean)) lines.push({ kind: step.bad ? "err" : "out", text });
  }
  if (fix.length) {
    lines.push({ kind: "hint", text: "# the fix:" });
    for (const text of fix) lines.push(text.startsWith("#") ? { kind: "hint", text } : { kind: "cmd", text });
  }
  return lines;
}

/** The way out when a remote refused the SSH connection, from what Oxbow found out. */
export function sshRecovery(check: SshCheck, verb: string, retry: Step & { label: string }): Recovery | null {
  const host = hostName(check.host);
  const how: Part[] = [`${verb} ${verb === "Push" ? "to" : "from"} `, { code: check.remote }, " connects over SSH as ", { code: check.login }];
  const https = check.httpsUrl ? { label: "Use HTTPS Instead…", request: () => httpsRequest(check) } : undefined;
  switch (check.problem) {
    case "agentEmpty": {
      return {
        title: `${host} didn’t accept your SSH key`,
        body: [
          ...how,
          ", and no key was offered: ssh-agent is empty, which usually happens after a restart. The key ",
          { code: check.key! },
          ` is still on disk. Adding it to the agent${mac ? " and to Keychain fixes this for good." : " fixes this until the next restart."}`,
        ],
        icon: "key",
        tone: "err",
        checked: checkedLines(check),
        button: { label: "Add Key to Agent", action: { kind: "addSshKey", key: check.keyPath! }, status: "Adding the key to ssh-agent…", next: retry },
        alt: https,
        note: "If the key has a passphrase, a small window asks for it once.",
        close: "Close",
      };
    }
    case "keyNotOnHost":
      return {
        title: `Your SSH key isn’t on ${host}`,
        body: [
          ...how,
          ". ",
          ...(check.key ? ["Your key ", { code: check.key } as Part, " was offered"] : ["A key from ssh-agent was offered"]),
          `, but no ${host} account has it. Add the public key to your account, then try again.`,
        ],
        icon: "key",
        tone: "err",
        checked: [...checkedLines(check), { kind: "hint", text: `# The key is loaded, but ${host} doesn’t know it.` }],
        act: check.keyPath
          ? {
              label: "Copy Public Key",
              run: async () => {
                await navigator.clipboard.writeText(await api.publicKey(check.keyPath!));
                return host === "GitHub"
                  ? "Copied the public key. Paste it in GitHub › Settings › SSH and GPG keys, then try again."
                  : `Copied the public key. Add it to your ${host} account, then try again.`;
              },
            }
          : undefined,
        button: check.keyPath ? undefined : retry,
        alt: https,
        note: "Nothing changed in your repository.",
        close: "Close",
      };
    case "noKey":
      return {
        title: "This computer has no SSH key yet",
        body: [...how, `, but there is no key in `, { code: "~/.ssh" }, ` and none in ssh-agent. Make one and add it to ${host}, or connect over HTTPS instead.`],
        icon: "key",
        tone: "err",
        checked: checkedLines(check, ["ssh-keygen -t ed25519", `# then add ~/.ssh/id_ed25519.pub to your ${host} account`]),
        button: retry,
        alt: https,
        close: "Close",
      };
    case "unknownHost":
      return {
        title: `ssh doesn’t trust ${check.host} yet`,
        body: [
          ...how,
          ", but ",
          { code: check.host },
          " is not in ",
          { code: "~/.ssh/known_hosts" },
          `, or its key changed. Try Again asks whether to trust it: compare the fingerprint with the one ${host} publishes before you answer yes.`,
        ],
        icon: "key",
        tone: "warn",
        checked: checkedLines(check),
        button: retry,
        alt: https,
        close: "Close",
      };
    case "works":
      return {
        title: `ssh gets into ${host} now`,
        body: ["When Oxbow checked, ", { code: check.login }, " let the key in, so the failure was probably passing. Try again."],
        icon: "key",
        tone: "warn",
        checked: checkedLines(check),
        button: retry,
        close: "Close",
      };
    default:
      return null;
  }
}

/** Point the remote at its HTTPS address instead of SSH. */
export function httpsRequest(check: SshCheck): Request {
  const url = check.httpsUrl!;
  return {
    title: `Connect ${check.remote} over HTTPS?`,
    body: [
      "Same repository, different way in: Fetch, Pull and Push then sign in with your ",
      hostName(check.host),
      " password or token, which Git’s credential helper keeps, instead of an SSH key. Nothing else changes.",
    ],
    icon: "key",
    button: "Switch to HTTPS",
    note: "Switch back any time in Settings › Repository › Remotes.",
    status: `Switching ${check.remote} to HTTPS…`,
    done: `${check.remote} now uses ${url}.`,
    action: { kind: "setRemoteUrl", name: check.remote, url },
  };
}
