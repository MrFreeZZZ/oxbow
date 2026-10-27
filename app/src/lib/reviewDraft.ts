// A review of someone's pull request: where on a diff a comment can go, the draft written here,
// and the calls that send it to GitHub in one go.
import type { DiffLine, DiffSide, DraftComment, DraftReply, Hunk, PullCall, PullCommit, PullEntry, PullRequest, ReviewDraft, Spot, Verdict } from "./types";

export type Thread = Extract<PullEntry, { kind: "thread" }>;

/** Lines GitHub shows around a change: a comment goes on those and on the change itself. */
const CONTEXT = 3;

/** A line as GitHub numbers it: a deleted line by its old number on the left, any other by its
 *  new number on the right. */
export function placeOf(line: DiffLine): { side: DiffSide; line: number } | null {
  if (line.kind === "removed") return line.oldLine === null ? null : { side: "LEFT", line: line.oldLine };
  return line.newLine === null ? null : { side: "RIGHT", line: line.newLine };
}

/** Name of a line of a file, for what shows under it. */
export const lineKey = (path: string, side: DiffSide, line: number) => `${side}:${line}:${path}`;

/** Whether line `index` of `hunk` can take a comment: a change, or within three lines of one,
 *  as in the diff GitHub shows. Opened unchanged lines and the rest of a whole file can't. */
export function commentable(hunk: Hunk, index: number): boolean {
  const lines = hunk.lines;
  if (!placeOf(lines[index])) return false;
  for (let i = Math.max(0, index - CONTEXT); i <= Math.min(lines.length - 1, index + CONTEXT); i++) {
    if (lines[i].kind !== "context") return true;
  }
  return false;
}

/** The spot of lines `from` to `to` of a hunk: the side of the last one, from the first one that
 *  has a number on that side. */
export function spotOf(path: string, hunk: Hunk, from: number, to: number): Spot | null {
  const [a, b] = from <= to ? [from, to] : [to, from];
  const end = placeOf(hunk.lines[b]);
  if (!end) return null;
  let start: number | null = null;
  for (let i = a; i < b && start === null; i++) {
    const line = hunk.lines[i];
    const n = end.side === "LEFT" ? (line.kind === "added" ? null : line.oldLine) : line.kind === "removed" ? null : line.newLine;
    if (n !== null && n < end.line) start = n;
  }
  return { path, side: end.side, line: end.line, startLine: start };
}

/** Whether `spot` covers a line, for lighting the lines a comment is on. */
export function covers(spot: { side: DiffSide; line: number; startLine?: number | null }, side: DiffSide, line: number): boolean {
  return spot.side === side && line <= spot.line && line >= (spot.startLine ?? spot.line);
}

/** "line 9" or "lines 13–14". */
export function spotLabel(spot: { line: number | null; startLine?: number | null }): string {
  if (spot.line === null) return "";
  return spot.startLine && spot.startLine !== spot.line ? `lines ${spot.startLine}–${spot.line}` : `line ${spot.line}`;
}

export function emptyDraft(commit: string): ReviewDraft {
  return { commit, body: "", verdict: null, comments: [], replies: [] };
}

/** Oxbow's own id for a comment of the draft. */
export function newId(): string {
  return `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 7)}`;
}

/** Whether a draft holds nothing worth keeping. */
export const isEmpty = (draft: ReviewDraft) => !draft.comments.length && !draft.replies.length && !draft.body.trim() && !draft.verdict;

/** "2 comments and 1 reply", "1 comment", or "" for none. */
export function countWords(comments: number, replies: number): string {
  const parts = [];
  if (comments) parts.push(`${comments} ${comments === 1 ? "comment" : "comments"}`);
  if (replies) parts.push(`${replies} ${replies === 1 ? "reply" : "replies"}`);
  return parts.join(" and ");
}

/** The calls of sending a review: started on the commit its comments are on, then its comments
 *  and replies, then the verdict. Without a draft, on the pull request's head. */
export function submitCalls(pr: PullRequest, draft: ReviewDraft | null, verdict: Verdict, body: string): PullCall[] {
  const calls: PullCall[] = [{ kind: "startReview", id: pr.nodeId, commit: draft?.commit ?? pr.headSha }];
  for (const c of draft?.comments ?? []) calls.push({ kind: "reviewComment", spot: c.spot, body: c.body });
  for (const r of draft?.replies ?? []) calls.push({ kind: "reviewReply", thread: r.thread, body: r.body });
  calls.push({ kind: "submitReview", verdict, body: body.trim() });
  return calls;
}

/** Why the review can't be sent as it is, or null. */
export function submitProblem(verdict: Verdict, body: string, draft: ReviewDraft | null, mine: boolean): string | null {
  if (mine && verdict !== "comment") return "GitHub doesn’t take a verdict on your own pull request.";
  if (verdict === "comment" && !body.trim() && !draft?.comments.length && !draft?.replies.length) return "Write something, or add comments to the review.";
  if (verdict === "requestChanges" && !body.trim() && !draft?.comments.length) return "Say what needs to change.";
  return null;
}

/** The commits pushed after `commit`, or null when it is no longer among them: a force push. */
export function pushedSince(commits: PullCommit[], commit: string): PullCommit[] | null {
  const at = commits.findIndex((c) => c.sha === commit);
  return at < 0 ? null : commits.slice(at + 1);
}

/** "dmitry pushed 2 commits after you started." */
export function pushedText(pr: PullRequest, commit: string): string {
  const since = pushedSince(pr.commits, commit);
  if (!since) return `${pr.author.login} force pushed after you started.`;
  const who = [...new Set(since.map((c) => c.author))];
  const n = since.length;
  return `${who.length === 1 ? who[0] : pr.author.login} pushed ${n} ${n === 1 ? "commit" : "commits"} after you started.`;
}

/** The pull request's threads still on its latest lines. */
export const liveThreads = (pr: PullRequest): Thread[] => pr.timeline.filter((e): e is Thread => e.kind === "thread" && !e.outdated && e.line !== null);

export function withComment(draft: ReviewDraft, spot: Spot, body: string, time: number): ReviewDraft {
  const comment: DraftComment = { id: newId(), spot, body, time };
  return { ...draft, comments: [...draft.comments, comment] };
}

export function withReply(draft: ReviewDraft, thread: Thread, body: string, time: number): ReviewDraft {
  const reply: DraftReply = { id: newId(), to: thread.id, thread: thread.node ?? "", body, time };
  return { ...draft, replies: [...draft.replies, reply] };
}

/** The draft with one comment or reply written anew, or left out when `body` is null. */
export function withEdit(draft: ReviewDraft, id: string, body: string | null): ReviewDraft {
  const edit = <T extends { id: string; body: string }>(list: T[]) =>
    body === null ? list.filter((c) => c.id !== id) : list.map((c) => (c.id === id ? { ...c, body } : c));
  return { ...draft, comments: edit(draft.comments), replies: edit(draft.replies) };
}
