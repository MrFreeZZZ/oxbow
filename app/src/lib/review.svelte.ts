// Reviewing one pull request here: the files ticked Viewed on GitHub, the draft review kept in
// `.git/oxbow/reviews` until it is sent, the one comment being written, and the requests that
// send comments, replies and the review itself. A merge request on GitLab the same way: its
// Viewed ticks stay on this computer, as GitLab keeps none for others to read.

import { api } from "./api";
import { github } from "./github.svelte";
import { confirm, type Request } from "./confirm.svelte";
import type { PullCall, PullRequest, ReviewDraft, ReviewTarget, Spot, Verdict, Viewing } from "./types";
import { addProblem, approvedBy, commentCall, countWords, emptyDraft, isEmpty, spotLabel, submitCalls, withComment, withEdit, withReply, type Thread, type Writing } from "./reviewDraft";
import { wordsOf, type ForgeKind, type Words } from "./forge";
import { ReviewSaves } from "./reviewSaves";
import { applyRequest, suggestedRange, type BodyPart } from "./suggestion";

/** The comment being written: a new one on lines, a reply to a thread, or one of the draft's again. */
export type Editing = ({ kind: "comment" } & Writing) | { kind: "reply"; thread: number; body: string } | { kind: "edit"; id: string; body: string };

/** How long typing in the review's summary waits before it is kept. */
const KEEP_AFTER = 500;

export class ReviewSession {
  readonly number: number;
  /** A merge request on GitLab. */
  readonly lab: boolean;
  readonly words: Words;
  pr = $state.raw<PullRequest>(null!);
  /** The review written here, or null before its first comment. */
  draft = $state.raw<ReviewDraft | null>(null);
  viewed = $state.raw(new Set<string>());
  dismissed = $state.raw(new Set<string>());
  /** A review this account started on GitHub's website and hasn't sent. */
  pending = $state.raw<Viewing["pending"]>(null);
  editing = $state<Editing | null>(null);
  /** The Finish Review popover is open. */
  finishing = $state(false);
  /** Files the pull request changes, as the Files tab counts them. */
  files = $state(0);
  /** The head commit whose files the Files tab shows; it can be behind `pr` while newer ones
   *  load. A review begins on it, never on a head nobody saw. */
  shown = $state<string | null>(null);
  /** Its branch is the one checked out here, so a suggestion can go into its files. */
  onBranch = $state(false);
  #run: (request: Request | Promise<Request>) => Promise<boolean>;
  #changed: () => void;
  readonly saves: ReviewSaves;
  #viewing = 0;
  #viewedVersion = 0;
  #viewedPaths = new Map<string, { version: number; confirmed: { viewed: boolean; dismissed: boolean }; pending: number; queue: Promise<void> }>();
  #disposed = false;

  constructor(pr: PullRequest, run: (request: Request | Promise<Request>) => Promise<boolean>, changed: () => void, target: ReviewTarget, forge: ForgeKind = "github") {
    this.number = pr.number;
    this.lab = forge === "gitlab";
    this.words = wordsOf(forge);
    this.pr = pr;
    this.files = pr.changedFiles;
    this.#run = run;
    this.#changed = changed;
    this.saves = new ReviewSaves(api, target, pr.number, draft => { this.draft = draft; }, err => confirm.fail(err));
    this.loadViewing().catch(() => {});
  }

  /** Comments and replies of the draft, for the button that finishes it. */
  get count(): number {
    return (this.draft?.comments.length ?? 0) + (this.draft?.replies.length ?? 0);
  }

  /** Viewed ticks and a review started on GitHub, read again. */
  async loadViewing() {
    const mine = ++this.#viewing;
    const version = this.#viewedVersion;
    const v = await (this.lab ? api.gitlabViewing(this.number, this.saves.target) : api.githubViewing(this.number, this.saves.target));
    if (this.#disposed || mine !== this.#viewing) return;
    const viewed = new Set(v.viewed), dismissed = new Set(v.dismissed);
    for (const [path, state] of this.#viewedPaths) {
      if (state.version > version || state.pending) {
        this.#put(viewed, path, this.viewed.has(path));
        this.#put(dismissed, path, this.dismissed.has(path));
      } else state.confirmed = { viewed: viewed.has(path), dismissed: dismissed.has(path) };
    }
    this.viewed = viewed;
    this.dismissed = dismissed;
    this.pending = v.pending;
  }

  #put(set: Set<string>, path: string, on: boolean) { if (on) set.add(path); else set.delete(path); }

  /** Requests of one path are ordered; failure rolls back only that path to its last success. */
  setViewed(path: string, on: boolean, failed: (err: unknown) => void) {
    const state = this.#viewedPaths.get(path) ?? {
      version: 0, confirmed: { viewed: this.viewed.has(path), dismissed: this.dismissed.has(path) }, pending: 0, queue: Promise.resolve(),
    };
    const version = state.version = ++this.#viewedVersion;
    state.pending++;
    this.#viewedPaths.set(path, state);
    const viewed = new Set(this.viewed), dismissed = new Set(this.dismissed);
    this.#put(viewed, path, on);
    dismissed.delete(path);
    this.viewed = viewed;
    this.dismissed = dismissed;
    state.queue = state.queue.then(async () => {
      try {
        if (this.lab) await api.gitlabSetViewed(this.number, path, on, this.saves.target);
        else await api.githubSend({ kind: "viewed", id: this.pr.nodeId, path, viewed: on }, this.saves.target);
        state.confirmed = { viewed: on, dismissed: false };
      } catch (err) {
        if (!this.#disposed && state.version === version) {
          const viewed = new Set(this.viewed), dismissed = new Set(this.dismissed);
          this.#put(viewed, path, state.confirmed.viewed);
          this.#put(dismissed, path, state.confirmed.dismissed);
          this.viewed = viewed;
          this.dismissed = dismissed;
        }
        failed(err);
      } finally { state.pending--; }
    });
  }

  #keep(change: (draft: ReviewDraft | null) => ReviewDraft | null, pause = 0) {
    this.saves.edit(draft => {
      const next = change(draft);
      return next && !isEmpty(next) ? next : null;
    }, pause);
  }

  dispose() {
    this.#disposed = true;
    this.#viewing++;
    this.saves.dispose();
  }

  #preview(calls: PullCall[]): Promise<string[]> {
    return this.lab ? api.gitlabCallsPreview(calls, this.saves.target) : api.githubCallsPreview(calls, this.saves.target);
  }

  #sendCalls(calls: PullCall[]): Promise<unknown> {
    return this.lab ? api.gitlabRun(calls, this.saves.target) : api.githubRun(calls, this.saves.target);
  }

  /** How the draft's file is named in `.git/oxbow/reviews`. */
  #draftFile(): string {
    return this.saves.state?.file ?? "review.json";
  }

  addComment(writing: Writing) {
    // Its commit must be checked against the saved review before a comment is attached to it.
    if (!this.saves.state) {
      void this.saves.ready().then(() => { if (!this.#disposed) this.addComment(writing); }, err => confirm.fail(err));
      return;
    }
    // Whether its file is the same on the newest commit is for the Files tab to say, before Add
    // is offered; a comment on an older commit than the review's never goes in.
    if (addProblem(this.draft, writing, this.pr.headSha, true)) return;
    // A review not started yet starts on the commit the comment was written on.
    const comment = withComment(emptyDraft(writing.commit), writing.spot, writing.body.trim(), Math.floor(Date.now() / 1000)).comments[0];
    this.#keep(current => {
      const draft = current ?? emptyDraft(writing.commit);
      return { ...draft, comments: [...draft.comments, comment] };
    });
    this.editing = null;
  }

  addReply(thread: Thread, body: string) {
    const start = emptyDraft(this.shown ?? this.pr.headSha);
    const reply = withReply(start, thread, body.trim(), Math.floor(Date.now() / 1000)).replies[0];
    this.#keep(current => {
      const draft = current ?? start;
      return { ...draft, replies: [...draft.replies, reply] };
    });
    this.editing = null;
  }

  /** A comment or reply of the draft written anew, or left out with null. */
  edit(id: string, body: string | null) {
    this.#keep(current => current ? withEdit(current, id, body === null ? null : body.trim()) : null);
    this.editing = null;
  }

  /** The review's summary as it is typed, kept a moment later. */
  setBody(body: string) {
    const start = emptyDraft(this.shown ?? this.pr.headSha);
    this.#keep(current => ({ ...(current ?? start), body }), KEEP_AFTER);
  }

  setVerdict(verdict: Verdict) {
    const start = emptyDraft(this.shown ?? this.pr.headSha);
    this.#keep(current => ({ ...(current ?? start), verdict }));
  }

  /** Send one call at once, as the request the sheet or Activity shows. */
  async #send(call: PullCall, request: Omit<Request, "local" | "icon"> & { icon?: Request["icon"] }, done: string): Promise<boolean> {
    const [curl] = await this.#preview([call]);
    const ok = await this.#run({
      icon: "edit",
      ...request,
      local: {
        commands: [curl],
        run: async () => {
          await this.#sendCalls([call]);
          return done;
        },
      },
    });
    if (ok) this.#changed();
    return ok;
  }

  async commentNow(writing: Writing) {
    const { spot } = writing;
    const ok = await this.#send(
      commentCall(this.number, writing),
      { title: `Comment on ${spot.path}?`, body: [`It goes to ${this.words.name} now, on ${spotLabel(spot)}, outside a review.`], button: "Comment", status: "Commenting…" },
      `Commented on ${spot.path}.`,
    );
    if (ok) this.editing = null;
  }

  async replyNow(thread: Thread, body: string) {
    const ok = await this.#send(
      this.lab ? { kind: "reply", number: this.number, to: thread.id, thread: thread.node, body: body.trim() } : { kind: "reply", number: this.number, to: thread.id, body: body.trim() },
      { title: `Reply on ${thread.path || `${this.words.mark}${this.number}`}?`, body: [`It goes to ${this.words.name} now, outside a review.`], button: "Reply", status: "Replying…" },
      `Replied on ${thread.path}.`,
    );
    if (ok) this.editing = null;
  }

  resolve(thread: Thread) {
    if (!thread.node) return;
    const again = !!thread.resolved;
    void this.#send(
      this.lab ? { kind: "resolve", thread: thread.node, resolved: !again, number: this.number } : { kind: "resolve", thread: thread.node, resolved: !again },
      { title: `${again ? "Unresolve" : "Resolve"} the thread on ${thread.path || `${this.words.mark}${this.number}`}?`, body: [again ? "It opens again." : `It folds away on ${this.words.name}.`], button: again ? "Unresolve" : "Resolve", status: again ? "Unresolving…" : "Resolving…" },
      again ? "The thread is open again." : "The thread is resolved.",
    );
  }

  /** Apply Suggestion: `who`'s suggested lines in place of the ones the thread is on, in the
   *  file on disk, from the commit it was on (the newest without word from GitHub), fetched
   *  first if it isn't here. */
  async applySuggestion(thread: Thread, part: Extract<BodyPart, { kind: "suggestion" }>, who: string) {
    const range = suggestedRange(part, thread, this.lab);
    if (!range || !thread.path) return;
    const commit = thread.commit || this.pr.headSha;
    await (this.lab ? api.gitlabFetch(this.number, this.pr.base, this.saves.target) : api.pullFetch(this.number, this.pr.base, this.saves.target)).catch(() => {});
    await this.#run(applyRequest(thread.path, commit, range, part.lines, who));
  }

  /** Send the review with its comments and replies, then put the draft away. */
  async submit(verdict: Verdict, body: string) {
    await this.saves.flush();
    const draft = this.draft;
    const revoke = this.lab && approvedBy(this.pr, github.mrMe);
    const calls = submitCalls(this.pr, draft, verdict, body, this.shown, this.lab ? { revoke } : null);
    const curls = await this.#preview(calls);
    const what = countWords(draft?.comments.length ?? 0, draft?.replies.length ?? 0);
    const said = verdict === "approve" ? "Approved" : verdict === "requestChanges" ? "Asked for changes on" : "Reviewed";
    const { mark, noun } = this.words;
    const ok = await this.#run({
      title: `Send your review of ${mark}${this.number}?`,
      body: [
        `${verdict === "approve" ? `It approves the ${noun}` : verdict === "requestChanges" ? "It asks for changes" : "It comments, without a verdict"}${what ? `, with ${what}` : ""}.`,
        ...(revoke && verdict === "requestChanges" ? [" It takes back your approval."] : []),
      ],
      icon: "edit",
      button: "Submit Review",
      status: "Sending the review…",
      local: {
        commands: curls,
        comment: this.lab
          ? "draft comments and replies, then publishing them with the verdict; if one fails, the drafts sent are deleted"
          : "a pending review, its comments and replies, then the verdict; if one fails, the pending review is deleted",
        run: async () => {
          await this.#sendCalls(calls);
          await this.saves.clear();
          return `${said} ${mark}${this.number}.`;
        },
      },
    });
    if (!ok) return;
    this.draft = null;
    this.finishing = false;
    this.#changed();
  }

  /** Throw the draft away; a copy stays in `.git/oxbow/removed` for 30 days. */
  async discard() {
    await this.saves.flush();
    const draft = this.draft;
    if (!draft) return;
    const what = countWords(draft.comments.length, draft.replies.length);
    const ok = await this.#run({
      title: `Do you really want to discard your review of ${this.words.mark}${this.number}?`,
      body: [`${what ? `Its ${what} aren’t sent` : "Nothing of it was sent"}. A copy stays in .git/oxbow/removed for 30 days.`],
      icon: "discard",
      danger: true,
      button: "Discard Review",
      past: `Discarded the review of ${this.words.mark}${this.number}`,
      local: {
        commands: [`mv .git/oxbow/reviews/${this.#draftFile()} .git/oxbow/removed/`],
        run: async () => {
          await this.saves.clear();
          return "The review is discarded.";
        },
      },
    });
    if (!ok) return;
    this.draft = null;
    this.finishing = false;
  }
}
