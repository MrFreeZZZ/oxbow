import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";

const source = readFileSync(new URL("../src/lib/worktrees.ts", import.meta.url), "utf8");
const request = source.slice(source.indexOf("export async function pullWorktreeRequest"), source.indexOf("/** Branch chips"));
const setup = Function("api", "newWorktreeRequest", "newWorktreeSheet", "freeBranches", "freeFolder", "tilde",
  `${ts.transpile(request.replace("export ", ""), { target: ts.ScriptTarget.ES2022 })}; return pullWorktreeRequest;`);

test("Review worktrees use the fetched head unless they can use a local or tracking branch", async () => {
  const fetches: unknown[][] = [];
  const sha = "a".repeat(40);
  let tips: { base: string; head: string } | null = { base: "b".repeat(40), head: sha };
  let fetchError: Error | undefined;
  const fetch = (forge: string) => async (number: number, base: string) => {
    fetches.push([forge, number, base]);
    if (fetchError) throw fetchError;
    return { tips, stale: null };
  };
  const worktree = setup({ pullFetch: fetch("GitHub"), gitlabFetch: fetch("GitLab") },
    (_ctx, _path, branch) => ({ branch, create: false }),
    (_ctx, _path, _shown, _free, state) => state,
    () => [], async () => "/worktree", (path) => path);
  const pull = { number: 7, base: "main", head: "feature", headSha: "c".repeat(40) };
  const local = { kind: "local", name: "feature" };
  const remote = { kind: "remote", name: "origin/feature" };
  const make = (refs = [], fork = false, gitlab = false) => worktree({ history: { refs } }, "/repo", pull, "origin", fork, gitlab);

  assert.deepEqual(await make([local, remote]), { branch: "feature", create: false });
  assert.deepEqual(fetches, []);
  assert.equal((await make([remote])).from, "refs/remotes/origin/feature");
  assert.equal((await make()).from, sha);
  for (const [gitlab, branch] of [[false, "pr/7"], [true, "mr/7"]] as const) {
    const state = await make([local, remote], true, gitlab);
    assert.equal(state.branch, branch);
    assert.equal(state.from, sha);
  }
  assert.deepEqual(fetches, [["GitHub", 7, "main"], ["GitHub", 7, "main"], ["GitHub", 7, "main"], ["GitLab", 7, "main"]]);

  tips = null;
  await assert.rejects(make(), /Could not find the fetched review head/);
  fetchError = new Error("fetch failed");
  await assert.rejects(make([], true, true), fetchError);
});
