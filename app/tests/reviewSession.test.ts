import assert from "node:assert/strict";
import fs from "node:fs";
import ts from "typescript";
import { ReviewSaves, settleReviewSaves } from "../src/lib/reviewSaves.ts";
import * as drafts from "../src/lib/reviewDraft.ts";
import type { ReviewDraft, ReviewDraftState, ReviewTarget } from "../src/lib/types.ts";

function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (err: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const target = (): ReviewTarget => ({ workdir: "/repo/A", place: { gitlab: false, host: "github.com", project: "acme/api" } });
const state = (draft: ReviewDraft | null = null): ReviewDraftState => ({ draft, revision: "r0", headRef: "scoped/head", baseRef: "scoped/base", file: "scoped-7.json" });
const turn = () => new Promise<void>(resolve => setTimeout(resolve, 0));

function session() {
  const read = deferred<ReviewDraftState>();
  const viewing = deferred<{ viewed: string[]; dismissed: string[]; pending: null }>();
  const writes: { target: ReviewTarget; number: number; draft: ReviewDraft | null; expected: string }[] = [];
  const sent: { call: any; answer: ReturnType<typeof deferred<unknown>> }[] = [];
  const errors: unknown[] = [];
  const api = {
    reviewDraft: () => read.promise,
    saveReviewDraft: async (target: ReviewTarget, number: number, draft: ReviewDraft | null, expected: string) => {
      writes.push({ target, number, draft, expected }); return `r${writes.length}`;
    },
    githubViewing: () => viewing.promise,
    githubSend: (call: any) => { const answer = deferred<unknown>(); sent.push({ call, answer }); return answer.promise; },
  };
  const source = fs.readFileSync(new URL("../src/lib/review.svelte.ts", import.meta.url), "utf8")
    .replace(/^import .*;\n/gm, "").replace(/export /g, "");
  const js = ts.transpile(source, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None });
  const plain = (value: unknown) => value;
  const globals = { ...drafts, $state: Object.assign(plain, { raw: plain }), api, ReviewSaves,
    confirm: { fail: (err: unknown) => errors.push(err) }, github: {}, wordsOf: () => ({}) };
  const C = Function(...Object.keys(globals), `${js}; return ReviewSession;`)(...Object.values(globals));
  const address = target();
  const editor = new C({ number: 7, headSha: "head", changedFiles: 2 }, async () => true, () => {}, address);
  return { editor, address, read, viewing, writes, sent, errors, api };
}

// Switching the window, or changing the source address object, cannot redirect its delayed save.
{
  const s = session();
  s.read.resolve(state()); await s.editor.saves.ready();
  s.editor.setBody("typed in A");
  s.address.workdir = "/repo/B";
  s.address.place.project = "another/project";
  s.editor.dispose(); await s.editor.saves.flush();
  assert.equal(s.writes.length, 1);
  assert.equal(s.writes[0].target.workdir, "/repo/A");
  assert.equal(s.writes[0].target.place.project, "acme/api");
  assert.equal(s.writes[0].draft?.body, "typed in A");
}

// A late initial read keeps local intentions AND comments the editor had not read yet.
{
  const s = session();
  s.editor.setBody("new body");
  s.editor.setVerdict("approve");
  s.editor.addComment({ commit: "head", spot: { path: "new.rs", side: "RIGHT", line: 1, startLine: null }, body: "new comment" });
  const disk = drafts.withComment(drafts.emptyDraft("head"), { path: "old.rs", side: "RIGHT", line: 1, startLine: null }, "saved comment", 1);
  s.read.resolve(state({ ...disk, body: "old body" }));
  await s.editor.saves.flush();
  assert.equal(s.editor.draft.body, "new body");
  assert.equal(s.editor.draft.verdict, "approve");
  assert.deepEqual(s.editor.draft.comments.map((c: any) => c.body), ["saved comment", "new comment"]);
  assert.equal(s.writes.at(-1)?.draft?.comments.length, 2);
}

// A write still running finishes before the latest revision is sent; failure is retryable.
{
  const s = session();
  const first = deferred<string>();
  let attempts = 0;
  s.api.saveReviewDraft = async (target, number, draft, expected) => {
    s.writes.push({ target, number, draft, expected });
    if (++attempts === 1) return first.promise;
    return `r${attempts}`;
  };
  s.read.resolve(state()); await s.editor.saves.ready();
  s.editor.setBody("first"); const flushing = s.editor.saves.flush(); await turn();
  s.editor.setBody("latest"); const latest = s.editor.saves.flush();
  assert.equal(s.writes.length, 1);
  first.resolve("r1"); await Promise.all([flushing, latest]);
  assert.deepEqual(s.writes.map(w => [w.draft?.body, w.expected]), [["first", "r0"], ["latest", "r1"]]);
  await s.editor.saves.clear();
  assert.equal(s.writes.at(-1)?.draft, null);
  assert.equal(s.writes.at(-1)?.expected, "r2");
}
{
  const s = session();
  let fail = true;
  s.api.saveReviewDraft = async () => { if (fail) throw new Error("disk full"); return "retry"; };
  s.read.resolve(state()); await s.editor.saves.ready();
  s.editor.setBody("keep me");
  assert.match((await settleReviewSaves(100))!, /disk full/);
  assert.equal(s.editor.draft.body, "keep me");
  fail = false;
  assert.equal(await settleReviewSaves(100), null);
}

// A failed Viewed request cannot undo another file's successful request or a later refresh.
{
  const s = session();
  s.read.resolve(state()); await s.editor.saves.ready();
  s.editor.setViewed("first.rs", true, () => {});
  s.editor.setViewed("second.rs", true, () => {});
  await turn();
  s.sent[1].answer.resolve(null); await turn();
  s.sent[0].answer.reject(new Error("first failed")); await turn();
  assert.deepEqual([...s.editor.viewed], ["second.rs"]);
  s.viewing.resolve({ viewed: [], dismissed: [], pending: null }); await turn();
  assert.deepEqual([...s.editor.viewed], ["second.rs"], "late initial viewing must not erase an edit");
}

// Even when both toggles fail, the final rollback uses the last confirmed server state.
{
  const s = session();
  s.read.resolve(state()); await s.editor.saves.ready();
  s.editor.setViewed("file.rs", true, () => {});
  s.editor.setViewed("file.rs", false, () => {});
  await turn(); assert.equal(s.sent.length, 1);
  s.sent[0].answer.reject(new Error("first failed")); await turn();
  assert.equal(s.sent.length, 2);
  s.sent[1].answer.reject(new Error("second failed")); await turn();
  assert.equal(s.editor.viewed.has("file.rs"), false);
}

// A refused clear preserves the editor's text; a transient initial read can be retried.
{
  const s = session();
  s.read.resolve(state(drafts.emptyDraft("head"))); await s.editor.saves.ready();
  s.editor.setBody("unsent text"); await s.editor.saves.flush();
  s.api.saveReviewDraft = async () => { throw new Error("another window changed it"); };
  await assert.rejects(s.editor.saves.clear(), /another window/);
  assert.equal(s.editor.draft.body, "unsent text");
}
{
  let reads = 0, saved: ReviewDraft | null = null;
  const errors: unknown[] = [];
  const save = new ReviewSaves({
    reviewDraft: async () => { if (++reads === 1) throw new Error("temporary read failure"); return state(); },
    saveReviewDraft: async (_t, _n, draft) => { saved = draft; return "r1"; },
  }, target(), 7, () => {}, err => errors.push(err));
  await turn();
  save.edit(() => ({ ...drafts.emptyDraft("head"), body: "typed while the read failed" }), 500);
  await save.flush();
  assert.equal(reads, 2);
  assert.equal((saved as ReviewDraft | null)?.body, "typed while the read failed");
}
