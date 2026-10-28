// The one thing a pull request waits for next, out of everything that keeps it from merging, and
// where a restack after a merged pull request stands: rebase here, push, then the new base on
// GitHub, each step done, failed, or still to go.

import type { PullRequest, PullSummary, RestackState } from "./types";
import type { Words } from "./forge";

/** GitHub's words, when none are given (forge.ts has them too; tests load this file alone). */
const GITHUB: Words = { kind: "github", name: "GitHub", mark: "#", noun: "pull request", check: "check", checks: "the checks", cli: "gh pr" };

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
const list = (words: string[]) => (words.length < 2 ? words.join("") : `${words.slice(0, -1).join(", ")} and ${words[words.length - 1]}`);

// --- Restack -----------------------------------------------------------------------------------

export type StepState = "done" | "now" | "next" | "failed" | "stopped";

export interface RestackLine {
  step: "rebase" | "push" | "base";
  state: StepState;
  text: string;
  /** Why it failed or stopped. */
  why: string | null;
}

/** Pull requests the restack still has to point at its base. */
export function stillToRetarget(s: RestackState, pulls: PullSummary[]): number[] {
  return s.restack.retarget.filter((n) => {
    const p = pulls.find((x) => x.number === n);
    return !!p && p.state === "open" && p.base !== s.restack.base;
  });
}

/** The restack's steps in the order they run; the first one not done is "now", unless it
 *  failed or stopped. */
export function restackLines(s: RestackState, pulls: PullSummary[]): RestackLine[] {
  const r = s.restack;
  const lines: RestackLine[] = [
    {
      step: "rebase",
      state: s.rebasing ? "stopped" : "done",
      text: `Rebase ${list(r.branches)} onto ${r.base}, leaving out ${plural(r.drop.length, "commit")} ${r.base} has now`,
      why: s.rebasing ? "Stopped on a conflict: resolve it, then Continue Rebase" : null,
    },
  ];
  if (r.push.length) {
    const done = !s.unpushed.length;
    const failed = !done && r.failed?.step === "push";
    lines.push({
      step: "push",
      state: done ? "done" : failed ? "failed" : "next",
      text: `Force push ${list(r.push.map((p) => p.branch))} to ${r.remote}`,
      why: failed ? r.failed!.message : null,
    });
  }
  if (r.retarget.length) {
    const done = !stillToRetarget(s, pulls).length;
    const failed = !done && r.failed?.step === "base";
    lines.push({
      step: "base",
      state: done ? "done" : failed ? "failed" : "next",
      text: `Change the base of ${r.retarget.map((n) => `#${n}`).join(", ")} to ${r.base}`,
      why: failed ? r.failed!.message : null,
    });
  }
  // A step after one that stopped or failed waits for it.
  const first = lines.find((l) => l.state !== "done");
  if (first?.state === "next") first.state = "now";
  return lines;
}

export const restackFinished = (lines: RestackLine[]) => lines.every((l) => l.state === "done");

/** Whether the restack moves `pr`'s branch or changes its base. */
export const restackTouches = (s: RestackState, pr: PullSummary | PullRequest) => s.restack.branches.includes(pr.head) || s.restack.retarget.includes(pr.number);

// --- The next step -----------------------------------------------------------------------------

export type NextKind =
  | "continueRestack"
  | "restack"
  | "waitBase"
  | "ready"
  | "conflicts"
  | "update"
  | "fixChecks"
  | "changes"
  | "threads"
  | "waitChecks"
  | "review"
  | "noPush"
  | "rules"
  | "checking"
  | "merge";

export interface Next {
  kind: NextKind;
  title: string;
  text: string;
}

export interface NextFacts {
  pr: PullRequest;
  /** The merged pull request it is still built on, before a restack. */
  mergedBelow: PullSummary | null;
  /** The open pull request of its base branch, which has to merge first. */
  basePull: PullSummary | null;
  /** The restack under way for it, by its steps. */
  restack: RestackLine[] | null;
  /** The trunk's name, e.g. "main". */
  trunk: string;
  /** Its branch is checked out here. */
  checkedOut: boolean;
  /** GitHub's words, or GitLab's for a merge request. */
  words?: Words;
}

interface Blocker extends Next {
  /** One line in the list of what else keeps it from merging. */
  short: string | null;
}

/** What GitLab's other merge rules ask for, in words, by its `detailed_merge_status`. */
const GITLAB_RULES: Record<string, { title: string; text: string }> = {
  blocked_status: { title: "Merge what it depends on first", text: "It depends on other merge requests that aren’t merged yet." },
  commits_status: { title: "Push its commits", text: "GitLab has no commits for its source branch, or the branch is gone there." },
  external_status_checks: { title: "Wait for the status checks", text: "GitLab waits for the external status checks of this project to pass." },
  jira_association_missing: { title: "Name its Jira issue", text: "This project asks for a Jira issue key in the title or the description." },
  locked_paths: { title: "Wait for locked files", text: "Someone locked files it changes; they have to unlock them first." },
  locked_lfs_files: { title: "Wait for locked files", text: "Someone locked LFS files it changes; they have to unlock them first." },
  merge_time: { title: "Wait for its merge time", text: "It is set to merge no earlier than a given time." },
  security_policy_violations: { title: "Answer the security policy", text: "A security policy of this project blocks it for now." },
  title_regex: { title: "Change its title", text: "Its title doesn’t match the pattern this project asks for." },
  requested_changes: { title: "Answer the requested changes", text: "A reviewer asked for changes. Push the fixes, then ask them to look again." },
};

/** Everything that keeps `pr` from merging, the one to do first first. */
export function blockers(f: NextFacts): Blocker[] {
  const { pr, mergedBelow, basePull, restack, trunk } = f;
  const w = f.words ?? GITHUB;
  const n = (number: number) => `${w.mark}${number}`;
  if (pr.state !== "open") return [];
  const out: Blocker[] = [];
  const status = pr.gitlab?.mergeStatus ?? null;
  // GitLab waits for the pipeline only when the project asks it to.
  const strict = status !== null ? status === "ci_must_pass" || status === "ci_still_running" : pr.mergeableState === "blocked";
  const failing = pr.checks.filter((c) => c.state === "fail");
  const running = pr.checks.filter((c) => c.state === "run");
  const approvals = pr.reviews.filter((r) => r.state === "approved").length;
  const checkWord = (count: number) => (count === 1 ? w.check : `${w.check}s`);
  if (restack && !restackFinished(restack)) {
    const now = restack.find((l) => l.state !== "done")!;
    const text =
      now.step === "rebase"
        ? "The rebase stopped on a conflict. Resolve it and continue the rebase; the push and the new base come after."
        : now.step === "push"
          ? `The branches are rebased here, but ${w.name} still has the old commits.${now.state === "failed" ? " The push failed." : ""}`
          : `The branches are pushed, but ${n(pr.number)} still goes into the merged branch.${now.state === "failed" ? " Changing it failed." : ""}`;
    out.push({ kind: "continueRestack", title: "Finish the restack", text, short: null });
  } else if (mergedBelow)
    out.push({
      kind: "restack",
      title: `Restack onto ${trunk}`,
      text: `${n(mergedBelow.number)} is merged, but ${pr.head} still has its commits. A restack moves it onto ${trunk} without them.`,
      short: `Restack onto ${trunk}, ${n(mergedBelow.number)} is merged`,
    });
  if (basePull)
    out.push({
      kind: "waitBase",
      title: `Merge ${n(basePull.number)} first`,
      text: `It goes into ${pr.base}, the branch of ${n(basePull.number)}. Once ${n(basePull.number)} is in ${basePull.base}, this one can merge.`,
      short: `Merge ${n(basePull.number)} first, it is the base of this ${w.noun}`,
    });
  if (pr.draft)
    out.push({
      kind: "ready",
      title: "Mark it ready for review",
      text: "It is a draft: reviewers aren’t told about it yet, and it can’t merge.",
      short: "Draft, mark it ready for review first",
    });
  if (pr.mergeable === false || pr.mergeableState === "dirty")
    out.push({
      kind: "conflicts",
      title: `Resolve the conflicts with ${pr.base}`,
      text: f.checkedOut
        ? `${w.name} can’t merge it as it is. Merge ${pr.base} into ${pr.head} here, resolve the conflicts, then push.`
        : `${w.name} can’t merge it as it is. Switch to ${pr.head} to merge ${pr.base} into it and resolve them here.`,
      short: `Conflicts with ${pr.base}: resolve them first`,
    });
  if (pr.mergeableState === "behind")
    out.push(
      w.kind === "gitlab"
        ? {
            kind: "update",
            title: `Rebase onto ${pr.base}`,
            text: `${pr.base} has newer commits, and this project merges only onto the newest one. GitLab rebases ${pr.head} onto it, and the pipeline runs again.`,
            short: `${pr.base} has newer commits: rebase first`,
          }
        : {
            kind: "update",
            title: "Update the branch",
            text: `${pr.base} has newer commits, and its rules ask for them before merging. GitHub merges ${pr.base} into ${pr.head}, and the checks run again.`,
            short: `${pr.base} has newer commits: update the branch first`,
          },
    );
  if (strict && failing.length)
    out.push({
      kind: "fixChecks",
      title: `Fix the failing ${checkWord(failing.length)}`,
      text: `${list(failing.map((c) => c.name))} failed. Push a fix, and ${w.checks} ${w.kind === "gitlab" ? "runs" : "run"} again.`,
      short: `${plural(failing.length, w.check)} failing: ${failing.map((c) => c.name).join(", ")}`,
    });
  const asked = pr.reviews.filter((r) => r.state === "changesRequested").map((r) => r.author.login);
  if (asked.length)
    out.push({
      kind: "changes",
      title: "Answer the requested changes",
      text: `${list(asked)} asked for changes. Push the fixes and reply in the conversation.`,
      short: `Changes requested by ${asked.join(", ")}`,
    });
  if (status === "discussions_not_resolved") {
    const open = pr.timeline.filter((e) => e.kind === "thread" && e.resolved === false).length;
    out.push({
      kind: "threads",
      title: "Resolve the threads",
      text: `This project merges only once every thread is resolved.${open ? ` ${plural(open, "thread")} ${open === 1 ? "is" : "are"} still open.` : ""}`,
      short: open ? `${plural(open, "thread")} to resolve` : "Threads to resolve",
    });
  }
  if (strict && running.length)
    out.push({
      kind: "waitChecks",
      title: w.kind === "gitlab" ? "Wait for the pipeline" : "Wait for the checks",
      text: `${list(running.map((c) => c.name))} ${running.length === 1 ? "is" : "are"} still running.`,
      short: w.kind === "gitlab" ? "The pipeline is running" : "Checks are running",
    });
  if (pr.requiredApprovals && approvals < pr.requiredApprovals) {
    const need = plural(pr.requiredApprovals - approvals, "approval");
    const waiting = pr.requested.filter((login) => !pr.reviews.some((r) => r.author.login === login));
    out.push({
      kind: "review",
      title: "Get a review",
      text: waiting.length ? `Needs ${need}. Waiting on ${list(waiting)}.` : `Needs ${need}, and nobody is asked to review it yet.`,
      short: `Needs ${need}`,
    });
  } else if (status === "not_approved")
    out.push({ kind: "review", title: "Get it approved", text: "This project merges it only once it is approved.", short: "Needs approval" });
  if (!pr.settings.canPush)
    out.push(
      w.kind === "gitlab"
        ? { kind: "noPush", title: "Ask someone who can merge", text: `Your account may not merge into ${pr.base} on GitLab.`, short: `Your account may not merge into ${pr.base}` }
        : {
            kind: "noPush",
            title: "Ask someone who can merge",
            text: "Your account can’t push to this repository, so it can’t merge it.",
            short: "Your account can’t push to this repository, so it can’t merge",
          },
    );
  if (status === "ci_must_pass" && !failing.length && !running.length)
    out.push({
      kind: "rules",
      title: "Run a pipeline",
      text: "This project merges only once a pipeline passes, and its newest commit has none that passed.",
      short: "Needs a pipeline that passes",
    });
  const rule = status ? GITLAB_RULES[status] : undefined;
  if (rule && !(status === "requested_changes" && asked.length)) out.push({ kind: "rules", ...rule, short: rule.title });
  const blocked = status !== null ? pr.mergeableState === "blocked" : strict;
  if (blocked && !out.length)
    out.push({ kind: "rules", title: `Wait for ${w.name}’s rules`, text: `${w.name}’s rules for ${pr.base} block merging for now.`, short: `${w.name}’s rules for ${pr.base} block merging for now` });
  return out;
}

/** The step to take next: the first blocker, or merging once there is none. */
export function nextStep(f: NextFacts, blocked: Blocker[] = blockers(f)): Next | null {
  const { pr } = f;
  const w = f.words ?? GITHUB;
  if (pr.state !== "open") return null;
  if (blocked.length) return blocked[0];
  if (pr.mergeableState === "unknown") return { kind: "checking", title: `${w.name} is checking it`, text: "Whether it merges cleanly is known in a moment." };
  const passed = pr.checks.filter((c) => c.state === "ok").length;
  const approvers = pr.reviews.filter((r) => r.state === "approved").map((r) => r.author.login);
  const allowed = w.kind === "gitlab" ? "the failing ones may fail" : "the failing ones aren’t required";
  const facts = [
    ...(pr.checks.length ? [pr.checks.some((c) => c.state === "fail" || c.state === "warn") ? `${plural(passed, w.check)} passed, ${allowed}` : `${plural(passed, w.check)} passed`] : []),
    ...(approvers.length ? [`approved by ${list(approvers)}`] : []),
    `no conflicts with ${pr.base}`,
  ];
  const text = facts.join(", ");
  return { kind: "merge", title: "Merge it", text: `${text.charAt(0).toUpperCase()}${text.slice(1)}.` };
}
