import assert from "node:assert/strict";
import { test } from "node:test";
import { bisectMarkRequest, bisectStartRequest } from "../src/lib/bisect.ts";
import type { BranchContext } from "../src/lib/branches.ts";
import type { HistoryRow } from "../src/lib/types.ts";

test("bisect uses the commits shown in the range when a branch and tag share a name", () => {
  const row = (digit: string, parent?: HistoryRow) => ({
    id: digit.repeat(40),
    parents: parent ? [parent.id] : [],
    labels: [],
  }) as HistoryRow;
  const root = row("a");
  const good = row("b", root);
  good.labels = [{ name: "preview", kind: "tag", color: 0, head: false }];
  const middle = row("c", good);
  const bad = row("d", middle);
  const ctx = {
    history: {
      rows: [bad, middle, good, root],
      refs: [
        { name: "preview", kind: "local", target: bad.id },
        { name: "preview", kind: "tag", target: good.id },
      ],
      head: { branch: "preview", commit: bad.id },
      trunk: null,
    },
    uncommitted: 0,
    colorOf: () => 0,
  } as unknown as BranchContext;
  for (const from of [{ bad }, { good }]) {
    const request = bisectStartRequest(ctx, from);
    assert.equal(request.invalid, null);
    assert.ok(request.body.some((part) => typeof part === "string" && part.includes("2 commits")));
    assert.deepEqual(request.action, {
      kind: "bisectStart", good: good.id, bad: bad.id, stash: false, command: null,
    });
  }
});

test("marking another commit sends its whole id, not the short one shown", () => {
  const row = (digit: string) => ({ id: digit.repeat(40), summary: digit, parents: [], labels: [] }) as unknown as HistoryRow;
  const testing = row("a");
  const other = row("b");
  const ctx = {
    history: { rows: [testing, other], refs: [], head: { branch: null, commit: testing.id }, trunk: null, bisect: null },
    uncommitted: 0,
    colorOf: () => 0,
  } as unknown as BranchContext;
  assert.deepEqual(bisectMarkRequest(ctx, "good", other).action, { kind: "bisectMark", mark: "good", commit: other.id });
  assert.deepEqual(bisectMarkRequest(ctx, "bad").action, { kind: "bisectMark", mark: "bad", commit: null });
});
