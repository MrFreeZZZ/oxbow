import assert from "node:assert/strict";
import { test } from "node:test";
import { OverviewReader, trackingNote } from "../src/lib/overview.ts";
import type { OverviewRow, RepoOverview } from "../src/lib/types.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

test("a refresh cancels the earlier read and only publishes its own list", async () => {
  const first = deferred<OverviewRow[]>(), second = deferred<OverviewRow[]>();
  const reads: string[] = [], stopped: string[] = [], applied: unknown[] = [];
  const reader = new OverviewReader(id => { reads.push(id); return reads.length === 1 ? first.promise : second.promise; }, async id => { stopped.push(id); });
  const old = reader.refresh((rows, error) => applied.push({ rows, error }));
  const current = reader.refresh((rows, error) => applied.push({ rows, error }));
  const newest = [{ repository: { name: "new", path: "/new" } }] as OverviewRow[];
  second.resolve(newest); await current;
  first.resolve([{ repository: { name: "removed", path: "/old" } }] as OverviewRow[]); await old;
  assert.deepEqual(stopped, [reads[0]]);
  assert.deepEqual(applied, [{ rows: newest, error: null }]);
});

test("leaving drops late success and failure and cancels exactly its read", async () => {
  for (const failure of [false, true]) {
    const request = deferred<OverviewRow[]>();
    const stopped: string[] = [];
    let applied = false;
    const reader = new OverviewReader(() => request.promise, async id => { stopped.push(id); });
    const pending = reader.refresh(() => { applied = true; });
    reader.stop(); reader.stop();
    if (failure) request.reject(new Error("late error")); else request.resolve([]);
    await pending;
    assert.equal(applied, false);
    assert.equal(stopped.length, 1);
  }
});

test("a list error is reported and can be retried", async () => {
  let fail = true;
  const applied: unknown[] = [];
  const reader = new OverviewReader(async () => { if (fail) throw new Error("can't read"); return []; }, async () => {});
  await reader.refresh((rows, error) => applied.push({ rows, error }));
  fail = false;
  await reader.refresh((rows, error) => applied.push({ rows, error }));
  assert.deepEqual(applied, [{ rows: null, error: "Error: can't read" }, { rows: [], error: null }]);
});

test("unknown counts and special HEAD states are never labelled up to date", () => {
  const summary: RepoOverview = { color: 0, stashes: 0, operation: null, branch: "main", commit: "abc", upstream: null, ahead: null, behind: null, changed: 0, conflicts: 0, fetchedAt: null, filtersDisabled: false };
  assert.equal(trackingNote(summary), "No upstream");
  assert.equal(trackingNote({ ...summary, commit: null }), "No commits");
  assert.equal(trackingNote({ ...summary, branch: null }), "Detached HEAD");
  assert.equal(trackingNote({ ...summary, upstream: "origin/main" }), "origin/main · comparison unavailable");
  assert.equal(trackingNote({ ...summary, upstream: "origin/main", ahead: 0, behind: 0 }), "origin/main");
});

test("inspector selection follows exact paths through reorder, filtering and removal", async () => {
  const { selectedOverview } = await import("../src/lib/overview.ts");
  const a = { repository: { name: "api", path: "/catalog/api" } } as OverviewRow;
  const b = { repository: { name: "api", path: "/payments/api" } } as OverviewRow;
  assert.equal(selectedOverview([a, b], b.repository.path), b);
  assert.equal(selectedOverview([b, a], b.repository.path), b);
  assert.equal(selectedOverview([a, b], null, b.repository.path), b);
  assert.equal(selectedOverview([a], b.repository.path), a);
  assert.equal(selectedOverview([], b.repository.path), null);
});

test("inspector facts preserve unavailable tracking and exact zero counts", async () => {
  const { overviewFacts } = await import("../src/lib/overview.ts");
  const { text } = await import("../src/lib/settingsRows.ts");
  const summary: RepoOverview = { color: 0, stashes: 0, operation: null, branch: "main", commit: "abc", upstream: "origin/main", ahead: 0, behind: 1, changed: 7, conflicts: 2, fetchedAt: null, filtersDisabled: false };
  const values = (s: RepoOverview) => overviewFacts(s).map(r => r.control?.type === "value" ? text(r.control.text) : "");
  assert.deepEqual(values(summary), ["7", "0 commits", "1 commit"]);
  assert.deepEqual(values({ ...summary, ahead: null, behind: null }), ["7", "—", "—"]);
  assert.equal(text(overviewFacts(summary)[1].label), "Ahead of upstream");
});

test("Welcome keeps stash and operation information without calling local refs server-fresh", async () => {
  const { overviewStatus } = await import("../src/lib/overview.ts");
  const repository = { name: "same", path: "/repo", opened: 0, missing: false };
  const summary: RepoOverview = { color: 0, stashes: 2, operation: null, branch: "main", commit: "abc", upstream: "origin/main", ahead: 0, behind: 0, changed: 0, conflicts: 0, fetchedAt: null, filtersDisabled: false };
  assert.equal(overviewStatus(repository, summary).text, "2 stashes");
  assert.equal(overviewStatus(repository, { ...summary, stashes: 0 }).text, "Up to date with local refs");
  const conflict = overviewStatus(repository, { ...summary, operation: "Merge paused", conflicts: 1 });
  assert.equal(conflict.warn, true);
  assert.match(conflict.text, /Merge paused.*1 conflict.*2 stashes/);
  assert.equal(overviewStatus(repository, { ...summary, stashes: 0, ahead: null, behind: null }).text, "Comparison unavailable");
});
