import { commentable, countWords, pushedSince, spotOf, submitCalls, submitProblem, withEdit, withComment, emptyDraft } from "../src/lib/reviewDraft.ts";
import type { DiffLine, Hunk, PullCommit, PullRequest } from "../src/lib/types.ts";
import assert from "node:assert/strict";
import { test } from "node:test";

const line = (kind: DiffLine["kind"], oldLine: number | null, newLine: number | null): DiffLine => ({ kind, oldLine, newLine, text: "", words: null });

// Lines 1–8 unchanged, 9 removed, 9 added, then 10–17 unchanged.
const hunk: Hunk = {
  header: "",
  oldStart: 1,
  oldLines: 17,
  newStart: 1,
  newLines: 17,
  lines: [
    ...Array.from({ length: 8 }, (_, i) => line("context", i + 1, i + 1)),
    line("removed", 9, null),
    line("added", null, 9),
    ...Array.from({ length: 8 }, (_, i) => line("context", i + 10, i + 10)),
  ],
};

test("comments go on changes and the three lines around them, as GitHub shows", () => {
  const ok = hunk.lines.map((_, i) => commentable(hunk, i));
  assert.deepEqual(
    ok.map((o, i) => (o ? i : -1)).filter((i) => i >= 0),
    [5, 6, 7, 8, 9, 10, 11, 12],
  );
});

test("a range takes the side of its last line", () => {
  assert.deepEqual(spotOf("a.rs", hunk, 6, 9), { path: "a.rs", side: "RIGHT", line: 9, startLine: 7 });
  assert.deepEqual(spotOf("a.rs", hunk, 8, 8), { path: "a.rs", side: "LEFT", line: 9, startLine: null });
  // From the deleted line to the added one: the deleted line has no number on the right.
  assert.deepEqual(spotOf("a.rs", hunk, 8, 9), { path: "a.rs", side: "RIGHT", line: 9, startLine: null });
});

const pr = { number: 421, nodeId: "PR_421", headSha: "new" } as PullRequest;

test("a review goes as start, comments, replies, verdict", () => {
  let draft = withComment(emptyDraft("old"), { path: "a.rs", side: "RIGHT", line: 9 }, "Why?", 1);
  draft = { ...draft, replies: [{ id: "r", to: 7, thread: "PRRT_7", body: "Fine.", time: 2 }] };
  const kinds = submitCalls(pr, draft, "approve", " ok ").map((c) => c.kind);
  assert.deepEqual(kinds, ["startReview", "reviewComment", "reviewReply", "submitReview"]);
  assert.deepEqual(submitCalls(pr, draft, "approve", "")[0], { kind: "startReview", id: "PR_421", commit: "old" });
  assert.deepEqual(submitCalls(pr, null, "approve", "")[0], { kind: "startReview", id: "PR_421", commit: "new" });
  assert.equal(withEdit(draft, draft.comments[0].id, null).comments.length, 0);
});

test("what a review needs before it is sent", () => {
  assert.ok(submitProblem("comment", " ", null, false));
  assert.equal(submitProblem("approve", "", null, false), null);
  assert.ok(submitProblem("approve", "", null, true));
  assert.equal(countWords(2, 1), "2 comments and 1 reply");
});

test("commits pushed after the review started, or none found after a force push", () => {
  const c = (sha: string) => ({ sha }) as PullCommit;
  assert.deepEqual(pushedSince([c("a"), c("b"), c("c")], "a")?.map((x) => x.sha), ["b", "c"]);
  assert.equal(pushedSince([c("b")], "a"), null);
});
