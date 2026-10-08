// Confirmations for Fetch, Pull and Push, and the way out when one of them fails.

import type { Part, Recovery, Request, Step, TermLine } from "./confirm.svelte";
import { api } from "./api";
import { mac } from "./keys";
import { stoppedOnConflicts } from "./merge";
import type { Action, CommitBrief, Failure, HistoryRow, SshCheck, Tracking } from "./types";
import { shortId } from "./format";

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

export function canPull(ctx: RemoteContext): boolean {
  return !!ctx.branch && !!ctx.tracking && !ctx.tracking.gone;
}

export function canPush(ctx: RemoteContext): boolean {
  return target(ctx) !== null;
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

/** How Pull goes, from Settings › Git: rebase is the default. */
export interface PullSetup {
  mode: "rebase" | "merge" | "fastForward";
  autostash: boolean;
}

export function pullRequest(ctx: RemoteContext, setup: PullSetup = { mode: "rebase", autostash: true }): Request {
  const t = ctx.tracking!;
  const branch = ctx.branch!;
  const upstream = `${t.remote}/${t.branch}`;
  const action: Action = { kind: "pull", remote: t.remote, branch: t.branch };
  const how: Part[] =
    setup.mode === "merge"
      ? [" and merges it into ", chip(ctx, branch), "; a merge commit joins the two when both have new commits."]
      : setup.mode === "fastForward"
        ? [" and moves ", chip(ctx, branch), " up to it. If the branch has commits of its own, nothing changes and Git says so."]
        : [" and replays your local commits of ", chip(ctx, branch), " on top of it."];
  return {
    title: `Pull into ${branch}?`,
    body: [
      "Fetches ",
      chip(ctx, upstream),
      ...how,
      setup.autostash ? " Uncommitted files are stashed first and restored after." : " Uncommitted changes to the same files stop the pull.",
    ],
    icon: "pull",
    button: "Pull",
    note: t.behind ? `${upstream} had ${plural(t.behind, "new commit")} at the last fetch.` : undefined,
    status: `Pulling ${upstream} into ${branch}…`,
    done: `${branch} is up to date with ${upstream}.`,
    recover: (failure) => conflict(failure) ?? networkOrAuth(failure, action, "Pulling", ctx, `${branch} is up to date with ${upstream}.`),
    action,
  };
}

export function pushRequest(ctx: RemoteContext): Request {
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
  return conflict(failure) ?? networkOrAuth(failure, action, "Pushing", ctx, `Pushed ${action.branch}.`);
}

function forcePushRequest(ctx: RemoteContext, action: Action & { kind: "push" }, failure: Failure, lease: boolean): Request {
  const remoteBranch = `${action.remote}/${action.upstream}`;
  const tip = failure.remoteTip!;
  const lost = failure.incoming;
  const body: Part[] = ["Replaces ", chip(ctx, remoteBranch), " with your local ", chip(ctx, action.branch), ". "];
  if (lost.length === 1) body.push("The commit ", { code: shortId(lost[0].id) }, " ", { quote: lost[0].summary }, ` by ${lost[0].authorName} disappears from the remote.`);
  else if (lost.length) body.push(`${plural(lost.length, "commit")} disappear from the remote.`);
  if (!lease) body.push(" Without the check, anything pushed after you looked is overwritten too.");
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
    recover: (f) => pushRecovery(f, ctx, action),
    action: { ...action, force: true, lease: lease ? tip : null },
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

function conflict(failure: Failure): Recovery | null {
  return stoppedOnConflicts(failure, "pull");
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
  if (action.kind === "fetch" || action.kind === "fetchTags") return "Fetch";
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
