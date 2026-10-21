import { errorLine, loadingStates, reloadNews, retried, settle, stillFailing, troubled } from "../src/lib/reads.ts";
import type { History, ReadProblem, RefInfo, Tracking, Worktree } from "../src/lib/types.ts";
import assert from "node:assert/strict";

const tracking = (ahead: number, behind: number): Tracking => ({ remote: "origin", branch: "main", ahead, behind, gone: false, rewritten: null, replay: null });
const branch = (name: string, t: Tracking | null): RefInfo => ({ name, kind: "local", target: "a1", tracking: t }) as RefInfo;
const tree = (name: string, changed: number | null): Worktree => ({ path: `/src/${name}`, name, branch: name, head: "a1", main: false, current: false, locked: null, missing: false, changed, statusError: changed === null ? "fatal: index file corrupt" : null, pending: false });

function history(problems: ReadProblem[], over: Partial<History> = {}): History {
  return {
    head: { branch: "main", commit: "a1" },
    refs: [branch("main", tracking(2, 0))],
    tracking: tracking(2, 0),
    operation: null,
    bisect: null,
    worktrees: [tree("main", 0), tree("hotfix", 3)],
    submodules: [],
    problems,
    ...over,
  } as History;
}

const failed = (part: ReadProblem["part"]): ReadProblem => ({ part, error: "`git worktree list` failed: fatal: unable to read\nmore" });

// A first read that went well: every part is ready.
{
  const { states } = settle(history([]), null, null, 1000);
  assert.deepEqual(troubled(states), []);
  assert.equal(states.worktrees.phase, "ready");
  assert.equal(states.worktrees.at, 1000);
}

// A part that fails on the first read is failed: not known, and nothing stands in for it.
{
  const { history: shown, states } = settle(history([failed("worktrees")], { worktrees: [] }), null, null, 1000);
  assert.equal(states.worktrees.phase, "failed");
  assert.equal(states.worktrees.at, null);
  assert.deepEqual(shown.worktrees, []);
  assert.deepEqual(troubled(states), ["worktrees"]);
}

// A part that fails after a good read keeps that reading, marked stale, with when it was read.
{
  const first = settle(history([]), null, null, 1000);
  const next = history([failed("worktrees")], { worktrees: [] });
  const { history: shown, states } = settle(next, first.history, first.states, 5000);
  assert.equal(states.worktrees.phase, "stale");
  assert.equal(states.worktrees.at, 1000);
  assert.equal(shown.worktrees.length, 2);
  assert.equal(shown.worktrees[1].changed, 3);
  // Failing again keeps the first good reading's time.
  const again = settle(history([failed("worktrees")], { worktrees: [] }), shown, states, 9000);
  assert.equal(again.states.worktrees.at, 1000);
  assert.equal(again.history.worktrees.length, 2);
}

// The changes are never kept from before: their row says they aren't known.
{
  const first = settle(history([]), null, null, 1000);
  const { states } = settle(history([failed("changes")]), first.history, first.states, 5000);
  assert.equal(states.changes.phase, "failed");
}

// Upstreams that couldn't be read keep the earlier counts, not "not published".
{
  const first = settle(history([]), null, null, 1000);
  const next = history([failed("tracking")], { refs: [branch("main", null)], tracking: null });
  const { history: shown, states } = settle(next, first.history, first.states, 5000);
  assert.equal(states.tracking.phase, "stale");
  assert.equal(shown.tracking?.ahead, 2);
  assert.equal(shown.refs[0].tracking?.ahead, 2);
}

// Retry: a part read again on its own is ready, and no longer a problem.
{
  const first = settle(history([failed("worktrees")], { worktrees: [] }), null, null, 1000);
  const done = retried(first.history, first.states, "worktrees", { worktrees: [tree("hotfix", 1)] }, 2000);
  assert.equal(done.states.worktrees.phase, "ready");
  assert.equal(done.history.problems.length, 0);
  assert.equal(done.history.worktrees[0].changed, 1);
  // Failing again leaves a failed part failed and a stale one stale.
  assert.equal(stillFailing(first.states, "worktrees", "again").worktrees.phase, "failed");
  const stale = { ...loadingStates(), worktrees: { phase: "loading" as const, error: "x", at: 500 } };
  assert.equal(stillFailing(stale, "worktrees", "again").worktrees.phase, "stale");
}

// git's error in one line.
{
  assert.equal(errorLine("`git status --porcelain` failed: fatal: index file corrupt\n"), "fatal: index file corrupt");
  assert.equal(errorLine("warning: x\nerror: bad object"), "error: bad object");
  assert.equal(errorLine(null), "");
}

// Reload: a status that couldn't be read is never "No uncommitted changes".
{
  const at = (files: number, unknown = false, problems: ReadProblem[] = [], over: Partial<History> = {}) =>
    history(problems, {
      rows: [{ id: "worktree", worktree: { files, staged: 0, unstaged: files, untracked: 0, conflicted: 0, unknown } }, { id: "a1", worktree: null }],
      stashes: [],
      changes: null,
      changesStamp: 1,
      ...over,
    } as Partial<History>);
  const unread = [failed("changes")];
  // Two changed files, then git status failed.
  assert.deepEqual(reloadNews(at(2), at(0, true, unread)), { text: "Couldn’t read the uncommitted changes", partial: true });
  // Clean, then failed.
  assert.deepEqual(reloadNews(at(0), at(0, true, unread)), { text: "Couldn’t read the uncommitted changes", partial: true });
  // Not known, then read clean: that is news.
  assert.deepEqual(reloadNews(at(0, true, unread), at(0)), { text: "No uncommitted changes", partial: false });
  assert.deepEqual(reloadNews(at(0, true, unread), at(3)), { text: "3 uncommitted files", partial: false });
  // The branch moved while the status failed: both are said.
  const moved = at(0, true, [...unread, failed("worktrees")], { refs: [{ ...branch("main", null), target: "b2" }] });
  assert.deepEqual(reloadNews(at(2), moved), { text: "main moved · Couldn’t read the uncommitted changes and worktrees", partial: true });
  // All read, nothing changed.
  assert.deepEqual(reloadNews(at(2), at(2)), { text: "Nothing new", partial: false });
  assert.deepEqual(reloadNews(at(2), at(0)), { text: "No uncommitted changes", partial: false });
}
