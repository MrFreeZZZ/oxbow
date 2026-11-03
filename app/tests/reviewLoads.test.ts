import assert from "node:assert/strict";
import fs from "node:fs";
import ts from "typescript";
import { keepDetails, keepList } from "../src/lib/reviewLoads.ts";
import { blockers, nextStep } from "../src/lib/nextStep.ts";
import type { PullList, PullRequest, PullSummary } from "../src/lib/types.ts";

const issue = (section: string, state: "partial" | "unavailable" = "unavailable") => ({ section, state, message: "Request failed" });
const pull = (number: number, state = "open") => ({ number, state, headSha: "head" }) as PullSummary;
const prior = [pull(1), pull(2, "closed"), pull(3, "merged")];
assert.deepEqual(keepList(prior, { items: [], issues: [] }), [], "complete empty replaces old data");
assert.deepEqual(keepList(prior, { items: [], issues: [issue("Open")] }), [prior[0]], "failed open read preserves its snapshot");
assert.deepEqual(keepList(prior, { items: [pull(4)], issues: [issue("Open", "partial")] }), [pull(4)], "partial snapshot is already preserved by core");

const old = { ...pull(1), timeline: [{ kind: "thread", id: 1, time: 1 }], reviews: [{ state: "approved" }], checks: [{ name: "build", state: "ok" }], commits: [{ sha: "old" }] } as PullRequest;
const fresh = { ...old, timeline: [{ kind: "event", time: 2, text: "new" }], checks: [{ name: "lint", state: "ok" }], loadIssues: [issue("Comments"), issue("Checks")] } as PullRequest;
const kept = keepDetails(old, fresh);
assert.equal(kept.timeline.length, 2, "keep old comments alongside successful new timeline");
assert.equal(kept.checks.length, 2, "keep successful status results alongside cached checks");
assert.equal(keepDetails(old, { ...fresh, headSha: "changed" }).timeline.length, 1, "cached data never crosses heads");
assert.equal(keepDetails(old, { ...fresh, number: 2 }).timeline.length, 1, "cached data never crosses requests");

for (const gitlab of [null, { mergeStatus: "mergeable" }]) {
  for (const section of ["Threads", "Checks", "Statuses", "Discussions", "Approvals", "Timeline", "Comments"]) {
    const pr = { ...fresh, gitlab, draft: false, reviews: [], checks: [], requested: [], mergeable: true, mergeableState: "clean", settings: { canPush: true }, loadIssues: [issue(section)] } as PullRequest;
    const facts = { pr, mergedBelow: null, basePull: null, restack: null, trunk: "main", checkedOut: false };
    assert.equal(blockers(facts)[0].kind, "checking");
    assert.notEqual(nextStep(facts)?.kind, "merge", `${section} failure must not report ready to merge`);
  }
}

function deferred<T>() { let resolve!: (v: T) => void, reject!: (e: unknown) => void; const promise = new Promise<T>((r, no) => { resolve = r; reject = no; }); return { promise, resolve, reject }; }
const turn = () => new Promise((r) => setTimeout(r, 0));
function state() {
  const gh: ReturnType<typeof deferred<PullList>>[] = [], gl: ReturnType<typeof deferred<any>>[] = [];
  const api = {
    githubRepo: async () => ({ owner: "acme", name: "api" }),
    gitlabProjects: async () => [{ host: "gitlab.com", path: "acme/api" }],
    githubPulls: () => { const d = deferred<PullList>(); gh.push(d); return d.promise; },
    gitlabMergeRequests: () => { const d = deferred<any>(); gl.push(d); return d.promise; },
    githubPullStatuses: async () => [], githubAccount: async () => ({ login: "me" }),
    gitlabAccounts: async () => [], gitlabStatuses: async () => [],
  };
  const source = fs.readFileSync(new URL("../src/lib/github.svelte.ts", import.meta.url), "utf8").replace(/^import .*;\n/gm, "").replace(/export /g, "");
  const js = ts.transpile(source, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None });
  const value = Function("api", "$state", "keepList", `${js}; return github;`)(api, (v: unknown) => v, keepList);
  return { value, gh, gl };
}
// Repositories, account changes and refreshes can resolve in reverse order on both providers.
for (const change of ["repository", "account", "refresh"]) {
  const s = state();
  const first = s.value.load("A", true); await turn();
  const second = change === "account" ? s.value.accountChanged() : change === "refresh" ? s.value.refresh() : s.value.load("B", true);
  await turn();
  s.gh[1].resolve({ items: [pull(2)], issues: [] });
  s.gl[1].resolve({ project: { host: "gitlab.com", path: "acme/api" }, mergeRequests: { items: [pull(20)], issues: [] } });
  await second; await turn();
  s.gh[0].resolve({ items: [pull(1)], issues: [issue("Open")] });
  s.gl[0].resolve({ project: { host: "old" }, mergeRequests: { items: [pull(10)], issues: [issue("Open")] } });
  await first; await turn();
  assert.deepEqual(s.value.pulls.map((p: PullSummary) => p.number), [2]);
  assert.deepEqual(s.value.mergeRequests.map((p: PullSummary) => p.number), [20]);
  assert.deepEqual(s.value.issues, []);
  assert.deepEqual(s.value.mrIssues, []);
  assert.equal(s.value.mrProject.host, "gitlab.com");
}

// Exercise the actual component loader, including catch/finally and delayed response guards.
function detailsLoader() {
  const requests: { forge: string; number: number; answer: ReturnType<typeof deferred<PullRequest>> }[] = [];
  const api = {
    stopReads: async () => {},
    githubPull: (number: number) => request("github", number),
    gitlabMergeRequest: (number: number) => request("gitlab", number),
  };
  function request(forge: string, number: number) { const answer = deferred<PullRequest>(); requests.push({forge, number, answer}); return answer.promise; }
  const source = fs.readFileSync(new URL("../src/lib/PullRequestView.svelte", import.meta.url), "utf8");
  const start = source.indexOf("  async function load(number: number)");
  const body = source.slice(start, source.indexOf("\n  $effect", start));
  const js = ts.transpile(body, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None });
  const loader = Function("api", "keepDetails", `
    let loadId=0, checksAgain=0, forge='github', lab=false, nav={repo:'A'}, summary={number:1};
    let pr=null, error=null, loading=false;
    ${js}
    return {load, select(repo, of, number) { nav.repo=repo; forge=of; lab=of==='gitlab'; summary={number}; },
      get pr(){return pr}, get error(){return error}, get loading(){return loading}};
  `)(api, keepDetails);
  return { loader, requests };
}
for (const provider of ["github", "gitlab"]) {
  for (const switchTo of ["refresh", "repository", "provider", "number"]) {
    const { loader, requests } = detailsLoader();
    loader.select("A", provider, 1);
    const first = loader.load(1); await turn();
    const of = switchTo === "provider" ? (provider === "github" ? "gitlab" : "github") : provider;
    const number = switchTo === "number" ? 2 : 1;
    loader.select(switchTo === "repository" ? "B" : "A", of, number);
    const second = loader.load(number); await turn();
    const newest = { ...fresh, number, title: "newest", mergeableState: "clean", loadIssues: [] };
    requests[1].answer.resolve(newest); await second;
    requests[0].answer.resolve({ ...fresh, title: "stale", mergeableState: "clean" }); await first;
    assert.equal(loader.pr.title, "newest", `${provider}: ${switchTo} cannot accept a stale detail response`);
    assert.equal(loader.loading, false);
    assert.equal(loader.error, null);
  }
}

// An older error cannot replace a newer success; a current error leaves details available
// and the next Retry clears it without losing the review session's request identity.
for (const provider of ["github", "gitlab"]) {
  const { loader, requests } = detailsLoader();
  loader.select("A", provider, 1);
  const first = loader.load(1); await turn();
  const second = loader.load(1); await turn();
  requests[1].answer.resolve({ ...fresh, title: "kept", mergeableState: "clean", loadIssues: [] }); await second;
  requests[0].answer.reject(new Error("old network error")); await first;
  assert.equal(loader.error, null);
  const failed = loader.load(1); await turn();
  requests[2].answer.reject(new Error("current network error")); await failed;
  assert.equal(loader.pr.title, "kept");
  assert.match(loader.error, /current network error/);
  const retry = loader.load(1); await turn();
  requests[3].answer.resolve({ ...fresh, title: "retried", mergeableState: "clean", loadIssues: [] }); await retry;
  assert.equal(loader.pr.title, "retried");
  assert.equal(loader.error, null);
}
