import { addProblem, commentCall, commentable, countWords, pushedSince, spotOf, submitCalls, submitProblem, withEdit, withComment, emptyDraft } from "../src/lib/reviewDraft.ts";
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
  // New commits came in while the files shown were still the earlier ones: those are approved.
  assert.deepEqual(submitCalls(pr, null, "approve", "", "seen")[0], { kind: "startReview", id: "PR_421", commit: "seen" });
  assert.deepEqual(submitCalls(pr, draft, "approve", "", "seen")[0], { kind: "startReview", id: "PR_421", commit: "old" });
  assert.equal(withEdit(draft, draft.comments[0].id, null).comments.length, 0);
});

test("Comment Now goes on the commit whose diff the line was picked on", () => {
  // The line is picked while the files of commit A are shown.
  const writing = { spot: { path: "file.txt", side: "RIGHT" as const, line: 9 }, commit: "A", body: " Why? " };
  // B is pushed and loads before Comment Now: the comment still goes on A's line 9.
  const later = { ...pr, headSha: "B" };
  assert.deepEqual(commentCall(later.number, writing), { kind: "comment", number: 421, commit: "A", spot: writing.spot, body: "Why?" });
});

test("a comment written on one commit never goes into a review on another", () => {
  const spot = { path: "file.txt", side: "RIGHT" as const, line: 9 };
  const onA = { spot, commit: "A", body: "Why?" };
  // B loaded while the comment on A was open, and the review was then started on B.
  const onB = emptyDraft("B");
  assert.match(addProblem(onB, onA, "B", null) ?? "", /^This comment is on A and your review on B/);
  assert.match(addProblem(onB, onA, "B", true) ?? "", /^This comment is on A/);
  // Comment Now still sends it, on A.
  assert.equal(commentCall(421, onA).commit, "A");
  // On the review's own commit, or with no review yet, it goes in.
  assert.equal(addProblem(onB, { ...onA, commit: "B" }, "B", null), null);
  assert.equal(addProblem(null, onA, "B", null), null);
  // A review started on A, the comment written on the newest, B: only where the file is the same.
  const onAReview = emptyDraft("A");
  const onNewest = { ...onA, commit: "B" };
  assert.equal(addProblem(onAReview, onNewest, "B", true), null);
  assert.match(addProblem(onAReview, onNewest, "B", false) ?? "", /^file.txt changed since your review started on A/);
  assert.match(addProblem(onAReview, onNewest, "B", null) ?? "", /^Working out/);
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
