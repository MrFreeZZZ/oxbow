import { blockers, nextStep, restackFinished, restackLines, stillToRetarget, type NextFacts } from "../src/lib/nextStep.ts";
import type { PullCheck, PullRequest, PullSummary, RestackState } from "../src/lib/types.ts";
import assert from "node:assert/strict";

const person = (login: string) => ({ login, avatarUrl: "" });
const check = (name: string, state: string): PullCheck => ({ name, state, detail: null, seconds: null, url: null }) as PullCheck;

function pull(over: Partial<PullRequest> = {}): PullRequest {
  return {
    number: 419,
    title: "Expose /v2/sessions endpoint",
    state: "open",
    draft: false,
    head: "auth/2-api",
    headSha: "9c1f2e7",
    headOwner: null,
    base: "main",
    htmlUrl: "",
    body: "",
    author: person("alexander"),
    created: 0,
    updated: 0,
    nodeId: "PR_1",
    additions: 0,
    deletions: 0,
    changedFiles: 0,
    commitCount: 1,
    commitsComplete: true,
    mergeable: true,
    mergeableState: "clean",
    mergedAt: null,
    mergeCommit: null,
    mergedBy: null,
    autoMerge: null,
    commits: [],
    reviews: [],
    requested: [],
    timeline: [],
    checks: [check("test", "ok")],
    requiredApprovals: 1,
    settings: { mergeCommit: true, squash: true, rebase: true, autoMerge: true, deleteBranchOnMerge: false, defaultBranch: "main", canPush: true },
    ...over,
  } as PullRequest;
}

const facts = (pr: PullRequest, over: Partial<NextFacts> = {}): NextFacts => ({ pr, mergedBelow: null, basePull: null, restack: null, trunk: "main", checkedOut: false, ...over });
const approved = { author: person("maria"), state: "approved", body: "", time: 0 } as PullRequest["reviews"][number];

// Approved, checks passed, no conflicts: merging is next.
{
  const next = nextStep(facts(pull({ reviews: [approved] })))!;
  assert.equal(next.kind, "merge");
  assert.equal(next.text, "1 check passed, approved by maria, no conflicts with main.");
}

// Behind its base under strict rules: updating comes first, the review after.
{
  const pr = pull({ mergeableState: "behind" });
  const blocked = blockers(facts(pr));
  assert.deepEqual(
    blocked.map((b) => b.kind),
    ["update", "review"],
  );
  assert.equal(nextStep(facts(pr))!.kind, "update");
  assert.equal(blocked[1].text, "Needs 1 approval, and nobody is asked to review it yet.");
}

// Blocked by running checks and a missing review: waiting for the checks is first.
{
  const pr = pull({ mergeableState: "blocked", checks: [check("test", "run"), check("clippy", "run")], requested: ["maria"] });
  const next = nextStep(facts(pr))!;
  assert.equal(next.kind, "waitChecks");
  assert.equal(next.text, "test and clippy are still running.");
  assert.equal(blockers(facts(pr))[1].text, "Needs 1 approval. Waiting on maria.");
}

// A failing check that blocks comes before the review.
{
  const pr = pull({ mergeableState: "blocked", checks: [check("test", "fail")], reviews: [approved] });
  assert.equal(nextStep(facts(pr))!.title, "Fix the failing check");
}

// Built on a merged pull request: restack before anything else.
{
  const merged = { number: 418, head: "auth/1-models", base: "main" } as PullSummary;
  const pr = pull({ draft: true });
  assert.deepEqual(
    blockers(facts(pr, { mergedBelow: merged })).map((b) => b.kind),
    ["restack", "ready", "review"],
  );
}

// Still waiting on GitHub: no step to take yet.
assert.equal(nextStep(facts(pull({ mergeableState: "unknown", reviews: [approved] })))!.kind, "checking");
assert.equal(nextStep(facts(pull({ state: "merged" }))), null);

// --- A restack under way ---

const kept = (over: Partial<RestackState> = {}): RestackState => ({
  restack: {
    merged: 418,
    base: "main",
    branches: ["auth/3-ui", "auth/2-api"],
    drop: ["5f0a1c2"],
    remote: "origin",
    push: [
      { branch: "auth/3-ui", upstream: "auth/3-ui" },
      { branch: "auth/2-api", upstream: "auth/2-api" },
    ],
    retarget: [419],
    failed: { step: "push", message: "Could not resolve host: github.com" },
  },
  rebasing: false,
  unpushed: ["auth/3-ui", "auth/2-api"],
  ...over,
});
const pulls = [{ number: 419, state: "open", base: "auth/1-models" }] as PullSummary[];

// The push failed: rebased, then the push with why, then the base waits.
{
  const lines = restackLines(kept(), pulls);
  assert.deepEqual(
    lines.map((l) => [l.step, l.state]),
    [
      ["rebase", "done"],
      ["push", "failed"],
      ["base", "next"],
    ],
  );
  assert.equal(lines[1].why, "Could not resolve host: github.com");
  assert.equal(lines[1].text, "Force push auth/3-ui and auth/2-api to origin");
  const next = nextStep(facts(pull(), { restack: lines }))!;
  assert.equal(next.kind, "continueRestack");
  assert.equal(next.text, "The branches are rebased here, but GitHub still has the old commits. The push failed.");
}

// Pushed since (here or elsewhere): the base is next, and done once GitHub says so.
{
  const pushed = kept({ unpushed: [] });
  assert.deepEqual(stillToRetarget(pushed, pulls), [419]);
  assert.deepEqual(
    restackLines(pushed, pulls).map((l) => l.state),
    ["done", "done", "now"],
  );
  const moved = [{ number: 419, state: "open", base: "main" }] as PullSummary[];
  assert.ok(restackFinished(restackLines(pushed, moved)));
}

// A rebase that stopped holds up the rest.
{
  const lines = restackLines(kept({ rebasing: true, restack: { ...kept().restack, failed: null } }), pulls);
  assert.deepEqual(
    lines.map((l) => l.state),
    ["stopped", "next", "next"],
  );
}

// GitLab: its own words, and its merge status decides what blocks it.
{
  const lab = { kind: "gitlab", name: "GitLab", mark: "!", noun: "merge request", check: "job", checks: "the pipeline", cli: "glab mr" } as const;
  const gitlab = (mergeStatus: string): PullRequest["gitlab"] => ({
    mergeStatus,
    mergeMethod: "merge",
    squashOption: "default_off",
    squash: false,
    removeSourceBranch: false,
    behind: null,
    pipeline: null,
    diff: null,
    lineCounts: false,
  });
  const thread = (resolved: boolean) => ({ kind: "thread", resolved }) as unknown as PullRequest["timeline"][number];

  // A job allowed to fail doesn't block, and open threads do when the project asks.
  const pr = pull({
    requiredApprovals: null,
    mergeableState: "blocked",
    gitlab: gitlab("discussions_not_resolved"),
    checks: [check("lint", "warn"), check("build", "ok")],
    timeline: [thread(false), thread(true), thread(false)],
  });
  const blocked = blockers(facts(pr, { words: lab }));
  assert.deepEqual(blocked.map((b) => b.kind), ["threads"]);
  assert.equal(blocked[0].text, "This project merges only once every thread is resolved. 2 threads are still open.");

  // A failing pipeline the project waits for: in jobs, and the pipeline runs again.
  const failing = pull({ mergeableState: "blocked", requiredApprovals: null, gitlab: gitlab("ci_must_pass"), checks: [check("rspec", "fail")] });
  const fix = nextStep(facts(failing, { words: lab }))!;
  assert.equal(fix.title, "Fix the failing job");
  assert.equal(fix.text, "rspec failed. Push a fix, and the pipeline runs again.");

  // Needs a rebase: GitLab rebases it.
  const behind = pull({ mergeableState: "behind", requiredApprovals: null, gitlab: gitlab("need_rebase") });
  assert.equal(nextStep(facts(behind, { words: lab }))!.title, "Rebase onto main");

  // Approvals GitLab asks for without saying how many; another merge request first.
  assert.deepEqual(
    blockers(facts(pull({ mergeableState: "blocked", requiredApprovals: null, gitlab: gitlab("not_approved") }), { words: lab })).map((b) => b.title),
    ["Get it approved"],
  );
  assert.equal(
    blockers(facts(pull({ mergeableState: "blocked", requiredApprovals: null, gitlab: gitlab("blocked_status") }), { words: lab }))[0].title,
    "Merge what it depends on first",
  );

  // Mergeable: jobs passed, the allowed failure said in GitLab's words.
  const ready = nextStep(facts(pull({ requiredApprovals: null, gitlab: gitlab("mergeable"), checks: [check("lint", "warn"), check("build", "ok")] }), { words: lab }))!;
  assert.equal(ready.text, "1 job passed, the failing ones may fail, no conflicts with main.");
}
