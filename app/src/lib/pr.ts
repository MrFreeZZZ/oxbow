// Pull requests: the sheets that open, merge and change them, and the stack list Oxbow keeps in
// the description of each pull request of a stack. GitHub's side shows as curl (and the `gh`
// command that does the same); what happens here afterwards is ordinary git.

import { api } from "./api";
import type { Part, Request } from "./confirm.svelte";
import type { BranchContext } from "./branches";
import { shortId } from "./format";
import type { Action, Failure, GitHubRepo, MergeMethodName, Plan, PullCall, PullRequest, PullSummary } from "./types";

/** "Squash and Merge" as the start of a sentence: "Squash and merge". */
const sentence = (label: string) => label.charAt(0) + label.slice(1).toLowerCase();
const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
const chip = (ctx: BranchContext, name: string): Part => ({ branch: name, color: ctx.colorOf(name) });

/** A failed git step says what git said. */
async function perform(action: Action) {
  try {
    await api.performAction(action);
  } catch (err) {
    throw (err as Failure)?.output ?? String(err);
  }
}

// --- The stack list in descriptions -----------------------------------------------------------

const STACK_START = "<!-- oxbow-stack -->";
const STACK_END = "<!-- /oxbow-stack -->";
/** Where the new pull request's own number goes, before GitHub gave it one. */
export const NEW_NUMBER = "#$NUMBER";

/** A pull request of a stack, oldest (closest to the trunk) first. */
export interface StackLine {
  branch: string;
  /** "#418", NEW_NUMBER for the one being made, or null for a branch with none yet. */
  number: string | null;
  merged: boolean;
}

/** The list as it goes into the description of `current`'s pull request. */
export function stackSection(lines: StackLine[], current: string): string {
  const rows = lines.map((line) => {
    const name = line.number ?? `\`${line.branch}\` (no pull request yet)`;
    return `- ${name}${line.merged ? " · merged" : ""}${line.branch === current ? " ← this one" : ""}`;
  });
  return [STACK_START, "**Stack** · kept up to date by Oxbow", "", ...rows, STACK_END].join("\n");
}

/** `body` with its stack list replaced by `section`, or with `section` added at the end. */
export function withStack(body: string, section: string): string {
  const start = body.indexOf(STACK_START);
  const end = body.indexOf(STACK_END);
  if (start >= 0 && end > start) return body.slice(0, start) + section + body.slice(end + STACK_END.length);
  return body.trim() ? `${body.trimEnd()}\n\n${section}` : section;
}

/** A description as the screen shows it: the words, without HTML comments, and the rows of the
 *  stack list Oxbow keeps in it. */
export function readBody(body: string): { text: string; stack: string[] } {
  const start = body.indexOf(STACK_START);
  const end = body.indexOf(STACK_END);
  let stack: string[] = [];
  let text = body;
  if (start >= 0 && end > start) {
    stack = body
      .slice(start + STACK_START.length, end)
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.startsWith("- "))
      .map((line) => line.slice(2));
    text = body.slice(0, start) + body.slice(end + STACK_END.length);
  }
  text = text.replace(/<!--[\s\S]*?-->/g, "").trim();
  return { text, stack };
}

// --- Merging -----------------------------------------------------------------------------------

export const METHODS: { id: MergeMethodName; label: string; button: string; tip: string }[] = [
  { id: "merge", label: "Merge", button: "Create Merge Commit", tip: "Keep every commit and add a merge commit" },
  { id: "squash", label: "Squash", button: "Squash and Merge", tip: "One commit on the base for the whole pull request" },
  { id: "rebase", label: "Rebase", button: "Rebase and Merge", tip: "Replay each commit onto the base, no merge commit" },
];

/** The commit message GitHub is asked to write, as the screen previews it. */
export function mergeMessage(gh: GitHubRepo, pr: PullRequest, method: MergeMethodName): { title: string | null; message: string | null } {
  if (method === "squash") return { title: `${pr.title} (#${pr.number})`, message: readBody(pr.body).text };
  if (method === "merge") return { title: `Merge pull request #${pr.number} from ${pr.headOwner ?? gh.owner}/${pr.head}`, message: pr.title };
  return { title: null, message: null };
}

/** The first line of what git said, for a note. */
const firstLine = (err: unknown) => String(err).trim().split("\n").find((line) => line.trim())?.replace(/^(error|fatal): /, "") ?? "";

/** Merge on GitHub, then bring the merge here; deleting the branch comes last, and a delete
 *  that fails only adds a note: the merge is in by then. */
export async function mergeRequest(ctx: BranchContext, gh: GitHubRepo, pr: PullRequest, method: MergeMethodName, deleteBranch: boolean): Promise<Request> {
  const { title, message } = mergeMessage(gh, pr, method);
  const calls: PullCall[] = [{ kind: "merge", number: pr.number, method, title, message, sha: pr.headSha }];
  const local = ctx.history.refs.find((r) => r.kind === "local" && r.name === pr.head);
  const fromFork = !!pr.headOwner && pr.headOwner !== gh.owner;
  // A local branch with commits GitHub doesn't have stays, so nothing is lost. The delete
  // itself checks again: only while the branch is still at the merged commit.
  const deleteLocal = deleteBranch && local?.target === pr.headSha;
  const landed: Action = { kind: "pullRequestMerged", remote: gh.remote, base: pr.base, branch: pr.head };
  const cleanup: { action: Action; failed: (err: unknown) => string }[] = [];
  if (deleteBranch && !fromFork && !pr.settings.deleteBranchOnMerge)
    cleanup.push({
      action: { kind: "deleteRemoteBranch", remote: gh.remote, branch: pr.head, expect: pr.headSha },
      failed: (err) => `${pr.head} stays on ${gh.remote}: ${firstLine(err)}`,
    });
  if (deleteLocal)
    cleanup.push({
      action: { kind: "deleteMergedBranch", base: pr.base, branch: pr.head, sha: pr.headSha },
      failed: (err) => `${pr.head} stays here: ${firstLine(err)}`,
    });
  // A delete that can't be done (uncommitted changes on the branch) is left out and said so.
  const [curls, plan, ...cleanupPlans] = await Promise.all([
    api.githubCallsPreview(calls),
    api.planAction(landed),
    ...cleanup.map((c) => api.planAction(c.action).catch((err: unknown) => String(err))),
  ]);
  const skipped = cleanupPlans.filter((p): p is string => typeof p === "string");
  const steps = cleanup.filter((_, i) => typeof cleanupPlans[i] !== "string");
  const commands = [plan, ...cleanupPlans.filter((p): p is Plan => typeof p !== "string")].flatMap((p) => p.commands);
  const verb = method === "squash" ? `squashes ${plural(pr.commitCount, "commit")} of ` : method === "rebase" ? `replays ${plural(pr.commitCount, "commit")} of ` : "merges ";
  const body: Part[] = ["GitHub ", verb, chip(ctx, pr.head), " into ", chip(ctx, pr.base)];
  body.push(method === "squash" ? " as one commit" : method === "merge" ? " with a merge commit" : "", ", then your ", chip(ctx, pr.base), " catches up here.");
  if (deleteBranch && pr.settings.deleteBranchOnMerge) body.push(" GitHub deletes its branch there itself.");
  if (deleteBranch && local && !deleteLocal) body.push(" The local branch stays: it has commits GitHub doesn’t.");
  for (const why of skipped) body.push(` The local branch stays: ${why.replace(/^(error|fatal): /, "")}.`);
  const flags = `--${method}${deleteBranch ? " --delete-branch" : ""}`;
  return {
    title: `${sentence(METHODS.find((m) => m.id === method)!.button)} #${pr.number}?`,
    body,
    icon: "merge",
    button: METHODS.find((m) => m.id === method)!.button,
    status: `Merging #${pr.number} on GitHub…`,
    note: "Undo can’t take back a merge on GitHub. The steps here are in the Operation Log.",
    local: {
      commands: [...curls, ...commands.map((c) => c.display)],
      comments: [`gh pr merge ${pr.number} ${flags} does the same`, ...commands.map((c) => c.comment)],
      run: async () => {
        await api.githubRun(calls);
        await perform(landed);
        const notes: string[] = [];
        for (const step of steps) {
          try {
            await api.performAction(step.action);
          } catch (err) {
            notes.push(step.failed((err as Failure)?.output ?? err));
          }
        }
        return [`Merged #${pr.number} into ${pr.base}.`, ...notes].join(" ");
      },
    },
  };
}

/** Ask GitHub to merge it on its own once checks pass and it is approved, or stop asking. */
export async function autoMergeRequest(pr: PullRequest, method: MergeMethodName | null): Promise<Request> {
  const calls: PullCall[] = [{ kind: "autoMerge", number: pr.number, id: pr.nodeId, method }];
  const [curl] = await api.githubCallsPreview(calls);
  const on = method !== null;
  return {
    title: on ? `Merge #${pr.number} automatically when it is ready?` : `Stop merging #${pr.number} automatically?`,
    body: on
      ? [`GitHub merges it into ${pr.base} by ${METHODS.find((m) => m.id === method)!.label.toLowerCase()} as soon as the required checks pass and it has the approvals it needs. Nothing changes here until then.`]
      : ["It stays open until someone merges it."],
    icon: "merge",
    button: on ? "Turn On" : "Turn Off",
    status: on ? "Turning on auto-merge…" : "Turning off auto-merge…",
    local: {
      commands: [curl],
      comment: on ? `gh pr merge ${pr.number} --auto --${method} does the same` : `gh pr merge ${pr.number} --disable-auto does the same`,
      run: async () => {
        await api.githubRun(calls);
        return on ? `#${pr.number} merges once it is ready.` : `Auto-merge of #${pr.number} is off.`;
      },
    },
  };
}

export async function readyRequest(pr: PullRequest): Promise<Request> {
  const calls: PullCall[] = [{ kind: "readyForReview", number: pr.number, id: pr.nodeId }];
  const [curl] = await api.githubCallsPreview(calls);
  return {
    title: `Mark #${pr.number} ready for review?`,
    body: ["It stops being a draft, and the reviewers it asks for hear about it."],
    icon: "edit",
    button: "Ready for Review",
    status: "Marking it ready…",
    local: {
      commands: [curl],
      comment: `gh pr ready ${pr.number} does the same`,
      run: async () => {
        await api.githubRun(calls);
        return `#${pr.number} is ready for review.`;
      },
    },
  };
}

// --- Opening one -------------------------------------------------------------------------------

export interface NewPull {
  branch: string;
  base: string;
  title: string;
  /** The description as typed; the stack list goes in on its own. */
  body: string;
  draft: boolean;
  reviewers: string[];
  /** The push that has to come first, when GitHub doesn't have the commits yet. */
  push: (Action & { kind: "push" }) | null;
  /** The stack list for the new one, when it is in a stack. */
  stack: StackLine[] | null;
  /** Other pull requests of the stack whose list gets the new one. */
  others: PullSummary[];
  /** Commits it brings, for the sheet. */
  commits: number;
}

/** The description the new pull request gets. */
export function newBody(pull: NewPull): string {
  return pull.stack ? withStack(pull.body, stackSection(pull.stack, pull.branch)) : pull.body;
}

/** Each other stack pull request's description with the new one in its list. */
function stackUpdates(pull: NewPull, number: string): PullCall[] {
  if (!pull.stack) return [];
  const lines = pull.stack.map((line) => (line.branch === pull.branch ? { ...line, number } : line));
  return pull.others.map((other) => ({ kind: "update", number: other.number, body: withStack(other.body, stackSection(lines, other.head)) }));
}

export async function createRequest(ctx: BranchContext, pull: NewPull): Promise<Request> {
  const create: PullCall = { kind: "create", head: pull.branch, base: pull.base, title: pull.title.trim(), body: newBody(pull), draft: pull.draft };
  const reviewers: PullCall[] = pull.reviewers.length ? [{ kind: "requestReviewers", number: 0, reviewers: pull.reviewers }] : [];
  const later = stackUpdates(pull, NEW_NUMBER);
  const [curls, plan] = await Promise.all([api.githubCallsPreview([create, ...reviewers, ...later]), pull.push ? api.planAction(pull.push) : Promise.resolve(null)]);
  const pushed = plan?.commands ?? [];
  const body: Part[] = [];
  if (pull.push) body.push("Pushes ", chip(ctx, pull.branch), ` to ${pull.push.remote}, then opens`);
  else body.push("Opens");
  body.push(` a ${pull.draft ? "draft " : ""}pull request of ${plural(pull.commits, "commit")} into `, chip(ctx, pull.base), ".");
  if (pull.reviewers.length) body.push(` Asks ${pull.reviewers.join(", ")} to review it.`);
  if (pull.others.length) body.push(` Adds it to the stack list of ${pull.others.map((o) => `#${o.number}`).join(", ")}.`);
  const flags = [`--base ${pull.base}`, pull.draft ? "--draft" : "", ...pull.reviewers.map((r) => `--reviewer ${r}`)].filter(Boolean).join(" ");
  return {
    title: `Open a pull request for ${pull.branch}?`,
    body,
    icon: "push",
    button: pull.push ? "Push and Create" : "Create Pull Request",
    status: pull.push ? `Pushing ${pull.branch}…` : "Opening the pull request…",
    invalid: pull.title.trim() ? null : "Give it a title",
    local: {
      commands: [...pushed.map((c) => c.display), ...curls],
      comments: [
        ...pushed.map((c) => c.comment),
        `gh pr create ${flags} does the same`,
        ...reviewers.map(() => "$NUMBER is the new pull request’s"),
        ...later.map(() => "its stack list gets the new pull request"),
      ],
      run: async () => {
        if (pull.push) await perform(pull.push);
        const made = await api.githubRun([create, ...reviewers]);
        const number = made?.number;
        if (number && pull.stack) {
          const own = stackSection(
            pull.stack.map((line) => (line.branch === pull.branch ? { ...line, number: `#${number}` } : line)),
            pull.branch,
          );
          await api.githubRun([{ kind: "update", number, body: withStack(create.body, own) }, ...stackUpdates(pull, `#${number}`)]);
        }
        return number ? `Opened #${number}.` : "Opened the pull request.";
      },
    },
  };
}

// --- After the base of a stack merged ---------------------------------------------------------

/** Force push the rewritten branches and point their pull requests at the trunk. */
export async function restackPushRequest(
  ctx: BranchContext,
  push: (Action & { kind: "pushBranches" }) | null,
  retarget: { number: number; base: string }[],
): Promise<Request | null> {
  const calls: PullCall[] = retarget.map((r) => ({ kind: "update", number: r.number, base: r.base }));
  if (!push && !calls.length) return null;
  const [curls, plan] = await Promise.all([calls.length ? api.githubCallsPreview(calls) : Promise.resolve([]), push ? api.planAction(push) : Promise.resolve(null)]);
  const pushed = plan?.commands ?? [];
  const body: Part[] = [];
  if (push) {
    body.push("Sends the restacked ");
    push.branches.forEach((b, i) => body.push(...(i ? [i === push.branches.length - 1 ? " and " : ", "] : []), chip(ctx, b.branch)));
    body.push(` to ${push.remote}, replacing the commits the rebase rewrote.`);
  }
  if (retarget.length) body.push(` ${retarget.map((r) => `#${r.number}`).join(", ")} then ${retarget.length === 1 ? "goes" : "go"} into `, chip(ctx, retarget[0].base), ".");
  return {
    title: push ? `Push the restacked branches to ${push.remote}?` : "Point the pull requests at the trunk?",
    body,
    icon: "push",
    tone: push ? "warn" : undefined,
    button: push ? "Force Push" : "Change Base",
    status: "Pushing…",
    local: {
      commands: [...pushed.map((c) => c.display), ...curls],
      comments: [...pushed.map((c) => c.comment), ...retarget.map((r) => `gh pr edit ${r.number} --base ${r.base} does the same`)],
      run: async () => {
        if (push) await perform(push);
        if (calls.length) await api.githubRun(calls);
        return "Restacked.";
      },
    },
  };
}

export const describeSha = (sha: string | null) => (sha ? shortId(sha) : "");
