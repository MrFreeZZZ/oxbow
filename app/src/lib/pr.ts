// Pull requests: the sheets that open, merge and change them, and the stack list Oxbow keeps in
// the description of each pull request of a stack. GitHub's side shows as curl (and the `gh`
// command that does the same); what happens here afterwards is ordinary git. A merge request on
// GitLab (one with `gitlab` facts) goes through the same sheets, in GitLab's words and calls.

import { api } from "./api";
import type { Part, Request } from "./confirm.svelte";
import type { BranchContext } from "./branches";
import { shortId } from "./format";
import { GITHUB, GITLAB, type Words } from "./forge";
import type { Action, Failure, MergeMethodName, Plan, PullCall, PullRequest, PullSummary } from "./types";

/** "Squash and Merge" as the start of a sentence: "Squash and merge". */
const sentence = (label: string) => label.charAt(0) + label.slice(1).toLowerCase();
const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
const chip = (ctx: BranchContext, name: string): Part => ({ branch: name, color: ctx.colorOf(name) });

/** GitHub's words, or GitLab's for a merge request. */
export const wordsOfPull = (pr: PullRequest): Words => (pr.gitlab ? GITLAB : GITHUB);
const preview = (w: Words, calls: PullCall[]) => (w.kind === "gitlab" ? api.gitlabCallsPreview(calls) : api.githubCallsPreview(calls));
const send = (w: Words, calls: PullCall[]) => (w.kind === "gitlab" ? api.gitlabRun(calls) : api.githubRun(calls));

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

export type MethodChoice = (typeof METHODS)[number];

/** The ways it may merge: what the repository allows on GitHub; on GitLab a merge (or a
 *  fast-forward) and a squash, as the project allows. */
export function methodsOf(pr: PullRequest): MethodChoice[] {
  const g = pr.gitlab;
  if (!g) return METHODS.filter((m) => (m.id === "merge" ? pr.settings.mergeCommit : m.id === "squash" ? pr.settings.squash : pr.settings.rebase));
  const ff = g.mergeMethod === "ff";
  const merge: MethodChoice = {
    id: "merge",
    label: ff ? "Fast-forward" : "Merge",
    button: "Merge",
    tip: ff ? "Move the target onto its commits, without a merge commit" : g.mergeMethod === "rebase_merge" ? "Keep every commit and add a merge commit, onto the newest target only" : "Keep every commit and add a merge commit",
  };
  const squash: MethodChoice = { ...METHODS[1], tip: "One commit on the target for the whole merge request" };
  return g.squashOption === "always" ? [squash] : g.squashOption === "never" ? [merge] : [merge, squash];
}

/** The way picked when none is: GitLab's own choice for the merge request, else Squash. */
export function defaultMethod(pr: PullRequest, methods: MethodChoice[]): MergeMethodName {
  const wanted = pr.gitlab ? (pr.gitlab.squash ? "squash" : "merge") : "squash";
  return (methods.find((m) => m.id === wanted) ?? methods[0])?.id ?? "merge";
}

/** The commit message GitHub is asked to write, as the screen previews it; `owner` is the
 *  repository's. GitLab writes its own from the project's templates. */
export function mergeMessage(owner: string, pr: PullRequest, method: MergeMethodName): { title: string | null; message: string | null } {
  if (pr.gitlab) return { title: null, message: null };
  if (method === "squash") return { title: `${pr.title} (#${pr.number})`, message: readBody(pr.body).text };
  if (method === "merge") return { title: `Merge pull request #${pr.number} from ${pr.headOwner ?? owner}/${pr.head}`, message: pr.title };
  return { title: null, message: null };
}

/** The first line of what git said, for a note. */
const firstLine = (err: unknown) => String(err).trim().split("\n").find((line) => line.trim())?.replace(/^(error|fatal): /, "") ?? "";

/** Merge on GitHub (or GitLab), then bring the merge here; deleting the branch comes last, and a
 *  delete that fails only adds a note: the merge is in by then. `place` is where it lives: the
 *  remote, and on GitHub the repository's owner. */
export async function mergeRequest(ctx: BranchContext, place: { remote: string; owner: string | null }, pr: PullRequest, method: MergeMethodName, deleteBranch: boolean): Promise<Request> {
  const w = wordsOfPull(pr);
  const lab = w.kind === "gitlab";
  const n = `${w.mark}${pr.number}`;
  const { title, message } = mergeMessage(place.owner ?? "", pr, method);
  const calls: PullCall[] = [{ kind: "merge", number: pr.number, method, title, message, sha: pr.headSha, deleteBranch: lab && deleteBranch }];
  const local = ctx.history.refs.find((r) => r.kind === "local" && r.name === pr.head);
  const fromFork = !!pr.headOwner && pr.headOwner !== place.owner;
  // A local branch with commits GitHub doesn't have stays, so nothing is lost. The delete
  // itself checks again: only while the branch is still at the merged commit.
  const deleteLocal = deleteBranch && local?.target === pr.headSha;
  const landed: Action = { kind: "pullRequestMerged", remote: place.remote, base: pr.base, branch: pr.head };
  const cleanup: { action: Action; failed: (err: unknown) => string }[] = [];
  if (deleteBranch && !fromFork && !pr.settings.deleteBranchOnMerge)
    cleanup.push({
      action: { kind: "deleteRemoteBranch", remote: place.remote, branch: pr.head, expect: pr.headSha },
      failed: (err) => `${pr.head} stays on ${place.remote}: ${firstLine(err)}`,
    });
  if (deleteLocal)
    cleanup.push({
      action: { kind: "deleteMergedBranch", base: pr.base, branch: pr.head, sha: pr.headSha },
      failed: (err) => `${pr.head} stays here: ${firstLine(err)}`,
    });
  // A delete that can't be done (uncommitted changes on the branch) is left out and said so.
  const [curls, plan, ...cleanupPlans] = await Promise.all([
    preview(w, calls),
    api.planAction(landed),
    ...cleanup.map((c) => api.planAction(c.action).catch((err: unknown) => String(err))),
  ]);
  const skipped = cleanupPlans.filter((p): p is string => typeof p === "string");
  const steps = cleanup.filter((_, i) => typeof cleanupPlans[i] !== "string");
  const commands = [plan, ...cleanupPlans.filter((p): p is Plan => typeof p !== "string")].flatMap((p) => p.commands);
  const ff = lab && method === "merge" && pr.gitlab!.mergeMethod === "ff";
  const verb = method === "squash" ? `squashes ${plural(pr.commitCount, "commit")} of ` : method === "rebase" ? `replays ${plural(pr.commitCount, "commit")} of ` : ff ? "fast-forwards " : "merges ";
  const body: Part[] = ff ? [`${w.name} fast-forwards `, chip(ctx, pr.base), " to ", chip(ctx, pr.head)] : [`${w.name} `, verb, chip(ctx, pr.head), " into ", chip(ctx, pr.base)];
  body.push(method === "squash" ? " as one commit" : method === "merge" && !ff ? " with a merge commit" : "", ", then your ", chip(ctx, pr.base), " catches up here, unless it has commits of its own.");
  if (deleteBranch && pr.settings.deleteBranchOnMerge) body.push(` ${w.name} deletes its branch there itself.`);
  if (deleteBranch && local && !deleteLocal) body.push(` The local branch stays: it has commits ${w.name} doesn’t.`);
  for (const why of skipped) body.push(` The local branch stays: ${why.replace(/^(error|fatal): /, "")}.`);
  const flags = lab ? `${method === "squash" ? " --squash" : ""}${deleteBranch ? " --remove-source-branch" : ""}` : ` --${method}${deleteBranch ? " --delete-branch" : ""}`;
  const button = lab ? methodsOf(pr).find((m) => m.id === method)!.button : METHODS.find((m) => m.id === method)!.button;
  return {
    title: `${sentence(button)} ${n}?`,
    body,
    icon: "merge",
    button,
    status: `Merging ${n} on ${w.name}…`,
    note: `Undo can’t take back a merge on ${w.name}. The steps here are in the Operation Log.`,
    local: {
      commands: [...curls, ...commands.map((c) => c.display)],
      comments: [`${w.cli} merge ${pr.number}${flags} does the same`, ...commands.map((c) => c.comment)],
      run: async () => {
        await send(w, calls);
        await perform(landed);
        const notes: string[] = [];
        for (const step of steps) {
          try {
            await api.performAction(step.action);
          } catch (err) {
            notes.push(step.failed((err as Failure)?.output ?? err));
          }
        }
        return [`Merged ${n} into ${pr.base}.`, ...notes].join(" ");
      },
    },
  };
}

/** Ask GitHub to merge it on its own once checks pass and it is approved, or stop asking; on
 *  GitLab, once the pipeline passes. */
export async function autoMergeRequest(pr: PullRequest, method: MergeMethodName | null): Promise<Request> {
  const w = wordsOfPull(pr);
  const lab = w.kind === "gitlab";
  const n = `${w.mark}${pr.number}`;
  const calls: PullCall[] = [{ kind: "autoMerge", number: pr.number, id: pr.nodeId, method }];
  const [curl] = await preview(w, calls);
  const on = method !== null;
  const how = method ? (lab ? methodsOf(pr) : METHODS).find((m) => m.id === method)!.label.toLowerCase() : "";
  const comment = lab
    ? on
      ? `${w.cli} merge ${pr.number} --auto-merge${method === "squash" ? " --squash" : ""} does the same`
      : "Cancel auto-merge on GitLab’s page does the same"
    : on
      ? `gh pr merge ${pr.number} --auto --${method} does the same`
      : `gh pr merge ${pr.number} --disable-auto does the same`;
  return {
    title: on ? `Merge ${n} automatically when it is ready?` : `Stop merging ${n} automatically?`,
    body: on
      ? [
          lab
            ? `GitLab merges it into ${pr.base} by ${how} as soon as the pipeline passes and it meets the project’s rules. Nothing changes here until then.`
            : `GitHub merges it into ${pr.base} by ${how} as soon as the required checks pass and it has the approvals it needs. Nothing changes here until then.`,
        ]
      : ["It stays open until someone merges it."],
    icon: "merge",
    button: on ? "Turn On" : "Turn Off",
    status: on ? "Turning on auto-merge…" : "Turning off auto-merge…",
    local: {
      commands: [curl],
      comment,
      run: async () => {
        await send(w, calls);
        return on ? `${n} merges once it is ready.` : `Auto-merge of ${n} is off.`;
      },
    },
  };
}

export async function readyRequest(pr: PullRequest): Promise<Request> {
  const w = wordsOfPull(pr);
  const n = `${w.mark}${pr.number}`;
  const calls: PullCall[] = [{ kind: "readyForReview", number: pr.number, id: pr.nodeId, title: pr.gitlab ? pr.title : null }];
  const [curl] = await preview(w, calls);
  return {
    title: `Mark ${n} ready for review?`,
    body: [w.kind === "gitlab" ? "It stops being a draft: GitLab takes Draft: off its title, and it can merge." : "It stops being a draft, and the reviewers it asks for hear about it."],
    icon: "edit",
    button: "Ready for Review",
    status: "Marking it ready…",
    local: {
      commands: [curl],
      comment: `${w.cli} ${w.kind === "gitlab" ? "update" : "ready"} ${pr.number}${w.kind === "gitlab" ? " --ready" : ""} does the same`,
      run: async () => {
        await send(w, calls);
        return `${n} is ready for review.`;
      },
    },
  };
}

/** Make it a draft again: it can't merge until it is marked ready. */
export async function draftRequest(pr: PullRequest): Promise<Request> {
  const w = wordsOfPull(pr);
  const n = `${w.mark}${pr.number}`;
  const calls: PullCall[] = [{ kind: "convertToDraft", number: pr.number, id: pr.nodeId, title: pr.gitlab ? pr.title : null }];
  const [curl] = await preview(w, calls);
  return {
    title: `Mark ${n} as a draft?`,
    body: [w.kind === "gitlab" ? "GitLab puts Draft: before its title, and it can’t merge until it is marked ready." : "It can’t merge until it is marked ready for review again; the reviews it has stay."],
    icon: "edit",
    button: "Mark as Draft",
    status: "Marking it as a draft…",
    local: {
      commands: [curl],
      comment: `${w.cli} ${w.kind === "gitlab" ? `update ${pr.number} --draft` : `ready ${pr.number} --undo`} does the same`,
      run: async () => {
        await send(w, calls);
        return `${n} is a draft.`;
      },
    },
  };
}

/** Close it, or open it again; its branch stays as it is. */
export async function closeRequest(pr: PullRequest, open: boolean): Promise<Request> {
  const w = wordsOfPull(pr);
  const n = `${w.mark}${pr.number}`;
  const calls: PullCall[] = [{ kind: "setState", number: pr.number, open }];
  const [curl] = await preview(w, calls);
  return {
    title: open ? `Reopen ${n}?` : `Do you really want to close ${n}?`,
    body: [open ? `It is open again on ${w.name}, with its conversation and reviews.` : `It stays on ${w.name}, closed, and can be reopened. Its branch stays.`],
    icon: "edit",
    button: open ? "Reopen" : "Close",
    status: open ? "Reopening it…" : "Closing it…",
    local: {
      commands: [curl],
      comment: `${w.cli} ${open ? "reopen" : "close"} ${pr.number} does the same`,
      run: async () => {
        await send(w, calls);
        return open ? `${n} is open again.` : `Closed ${n}.`;
      },
    },
  };
}

/** Point it at another branch to merge into, picked from `choices` or typed. */
export async function targetRequest(ctx: BranchContext, pr: PullRequest, choices: string[]): Promise<Request> {
  const w = wordsOfPull(pr);
  // The request once, with a stand-in for the branch the sheet then fills in as it is picked.
  const [shape] = await preview(w, [{ kind: "update", number: pr.number, base: TARGET }]);
  return targetSheet(ctx, pr, w, choices, "", shape);
}

const TARGET = "OXBOW_TARGET_BRANCH";

function targetSheet(ctx: BranchContext, pr: PullRequest, w: Words, choices: string[], target: string, shape: string): Request {
  const n = `${w.mark}${pr.number}`;
  const name = target.trim();
  const calls: PullCall[] = [{ kind: "update", number: pr.number, base: name }];
  const again = (next: string) => targetSheet(ctx, pr, w, choices, next, shape);
  return {
    title: `Change what ${n} merges into?`,
    body: ["It merges into ", chip(ctx, pr.base), ` now. ${w.name} shows its changes against the new target, and ${w.checks} ${w.kind === "gitlab" ? "runs" : "run"} again.`],
    icon: "merge",
    button: "Change Target",
    status: "Changing the target…",
    invalid: !name ? "Pick a branch" : name === pr.base ? `It merges into ${name} already` : name === pr.head ? "It can’t merge into itself" : null,
    fields: [
      {
        label: "Into",
        chips: choices.map((b) => ({ label: b, on: b === name, pick: () => again(b) })),
        text: { value: name, placeholder: "Branch name", edit: again },
      },
    ],
    local: {
      commands: [shape.replace(TARGET, JSON.stringify(name || "<branch>").slice(1, -1))],
      comment: `${w.cli} ${w.kind === "gitlab" ? "update" : "edit"} ${pr.number} --${w.kind === "gitlab" ? "target-branch" : "base"} ${name || "<branch>"} does the same`,
      run: async () => {
        await send(w, calls);
        return `${n} merges into ${name} now.`;
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

/** Force push the rewritten branches and point their pull requests at the trunk: the restack's
 *  last two steps, or the ones still to go. `failed` keeps the step that failed and why, for
 *  going on from it later; `done` runs once both went through. */
export async function restackPushRequest(
  ctx: BranchContext,
  w: Words,
  push: (Action & { kind: "pushBranches" }) | null,
  retarget: { number: number; base: string }[],
  failed: (step: "push" | "base", message: string) => Promise<void>,
  done: () => Promise<void>,
): Promise<Request | null> {
  const calls: PullCall[] = retarget.map((r) => ({ kind: "update", number: r.number, base: r.base }));
  if (!push && !calls.length) return null;
  const [curls, plan] = await Promise.all([calls.length ? preview(w, calls) : Promise.resolve([]), push ? api.planAction(push) : Promise.resolve(null)]);
  const pushed = plan?.commands ?? [];
  const body: Part[] = [];
  if (push) {
    body.push("Sends the restacked ");
    push.branches.forEach((b, i) => body.push(...(i ? [i === push.branches.length - 1 ? " and " : ", "] : []), chip(ctx, b.branch)));
    body.push(` to ${push.remote}, replacing the commits the rebase rewrote.`);
  }
  if (retarget.length) body.push(` ${retarget.map((r) => `${w.mark}${r.number}`).join(", ")} then ${retarget.length === 1 ? "goes" : "go"} into `, chip(ctx, retarget[0].base), ".");
  return {
    title: push ? `Push the restacked branches to ${push.remote}?` : `Point the ${w.noun}s at the trunk?`,
    body,
    icon: "push",
    tone: push ? "warn" : undefined,
    forcePush: !!push,
    button: push ? "Force Push" : "Change Base",
    status: "Pushing…",
    note: "If a step fails, the restack goes on from it later.",
    local: {
      commands: [...pushed.map((c) => c.display), ...curls],
      comments: [
        ...pushed.map((c) => c.comment),
        ...retarget.map((r) => (w.kind === "gitlab" ? `glab mr update ${r.number} --target-branch ${r.base} does the same` : `gh pr edit ${r.number} --base ${r.base} does the same`)),
      ],
      run: async () => {
        if (push)
          try {
            await perform(push);
          } catch (err) {
            await failed("push", firstLine(err)).catch(() => {});
            throw err;
          }
        if (calls.length)
          try {
            await send(w, calls);
          } catch (err) {
            await failed("base", firstLine(err)).catch(() => {});
            throw err;
          }
        await done().catch(() => {});
        return "Restacked.";
      },
    },
  };
}

// --- Getting it ready ---------------------------------------------------------------------------

/** Merge the base into the branch on GitHub, like its Update branch button; on GitLab rebase
 *  the branch onto the target, like its Rebase button. */
export async function updateBranchRequest(ctx: BranchContext, pr: PullRequest): Promise<Request> {
  const w = wordsOfPull(pr);
  const lab = w.kind === "gitlab";
  const calls: PullCall[] = [{ kind: "updateBranch", number: pr.number, sha: pr.headSha }];
  const [curl] = await preview(w, calls);
  const local = ctx.history.refs.find((r) => r.kind === "local" && r.name === pr.head);
  const ahead = local?.tracking && !local.tracking.gone ? local.tracking.ahead : 0;
  const body: Part[] = lab
    ? ["GitLab rebases ", chip(ctx, pr.head), " onto the newest ", chip(ctx, pr.base), ", and the pipeline runs again."]
    : ["GitHub merges the newest ", chip(ctx, pr.base), " into ", chip(ctx, pr.head), " with a merge commit, and the checks run again."];
  if (local) body.push(lab ? " Its commits get new ids there: pull it here afterwards with rebase." : " Pull it here afterwards to have the merge locally.");
  return {
    title: lab ? `Rebase ${pr.head} onto ${pr.base}?` : `Update ${pr.head} with ${pr.base}?`,
    body,
    icon: lab ? "rebase" : "merge",
    button: lab ? "Rebase" : "Update Branch",
    status: lab ? "Rebasing the branch…" : "Updating the branch…",
    warning: ahead
      ? { title: `${plural(ahead, "commit")} here aren’t on ${w.name} yet`, body: lab ? ["Push them first: after the rebase, ", chip(ctx, pr.head), " here and there no longer line up."] : ["Push them first, or pull after the update: ", chip(ctx, pr.head), " then has a merge to bring in."] }
      : undefined,
    local: {
      commands: [curl],
      comment: lab ? `glab mr rebase ${pr.number} does the same` : `gh pr update-branch ${pr.number} does the same`,
      run: async () => {
        await send(w, calls);
        return `${w.name} is ${lab ? "rebasing" : "updating"} ${pr.head}.`;
      },
    },
  };
}

/** Ask `logins` to review it. */
export async function reviewersRequest(pr: PullRequest, logins: string[]): Promise<Request> {
  const calls: PullCall[] = [{ kind: "requestReviewers", number: pr.number, reviewers: logins }];
  const [curl] = await api.githubCallsPreview(calls);
  return {
    title: `Ask ${logins.join(", ")} to review #${pr.number}?`,
    body: [`GitHub tells ${logins.length === 1 ? "them" : "each of them"} about it.`],
    icon: "push",
    button: "Ask for Review",
    status: "Asking for a review…",
    local: {
      commands: [curl],
      comment: `gh pr edit ${pr.number} ${logins.map((l) => `--add-reviewer ${l}`).join(" ")} does the same`,
      run: async () => {
        await api.githubRun(calls);
        return `Asked ${logins.join(", ")} to review #${pr.number}.`;
      },
    },
  };
}

export const describeSha = (sha: string | null) => (sha ? shortId(sha) : "");
