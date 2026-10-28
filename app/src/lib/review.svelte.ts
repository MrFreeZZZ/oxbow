// Reviewing one pull request here: the files ticked Viewed on GitHub, the draft review kept in
// `.git/oxbow/reviews` until it is sent, the one comment being written, and the requests that
// send comments, replies and the review itself.

import { api } from "./api";
import { github } from "./github.svelte";
import { confirm, type Request } from "./confirm.svelte";
import type { PullCall, PullRequest, ReviewDraft, Spot, Verdict, Viewing } from "./types";
import { countWords, emptyDraft, isEmpty, spotLabel, submitCalls, withComment, withEdit, withReply, type Thread } from "./reviewDraft";

/** The comment being written: a new one on lines, a reply to a thread, or one of the draft's again. */
export type Editing = { kind: "comment"; spot: Spot; body: string } | { kind: "reply"; thread: number; body: string } | { kind: "edit"; id: string; body: string };

/** How long typing in the review's summary waits before it is kept. */
const KEEP_AFTER = 500;

export class ReviewSession {
  readonly number: number;
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
  #run: (request: Request | Promise<Request>) => Promise<boolean>;
  #changed: () => void;
  #timer: ReturnType<typeof setTimeout> | undefined;

  constructor(pr: PullRequest, run: (request: Request | Promise<Request>) => Promise<boolean>, changed: () => void) {
    this.number = pr.number;
    this.pr = pr;
    this.files = pr.changedFiles;
    this.#run = run;
    this.#changed = changed;
    this.loadViewing().catch(() => {});
    api.reviewDraft(pr.number).then(
      (d) => (this.draft = d),
      (err) => confirm.fail(err),
    );
  }

  /** Comments and replies of the draft, for the button that finishes it. */
  get count(): number {
    return (this.draft?.comments.length ?? 0) + (this.draft?.replies.length ?? 0);
  }

  /** Viewed ticks and a review started on GitHub, read again. */
  async loadViewing() {
    const v = await api.githubViewing(this.number);
    this.viewed = new Set(v.viewed);
    this.dismissed = new Set(v.dismissed);
    this.pending = v.pending;
  }

  /** Tick or untick a file's Viewed box, here at once and then on GitHub. */
  setViewed(path: string, on: boolean, failed: (err: unknown) => void) {
    const before = { viewed: this.viewed, dismissed: this.dismissed };
    const v = new Set(this.viewed);
    const d = new Set(this.dismissed);
    if (on) v.add(path);
    else v.delete(path);
    d.delete(path);
    this.viewed = v;
    this.dismissed = d;
    api.githubSend({ kind: "viewed", id: this.pr.nodeId, path, viewed: on }).catch((err) => {
      this.viewed = before.viewed;
      this.dismissed = before.dismissed;
      failed(err);
    });
  }

  /** Keep `draft`, or put it away when it holds nothing. */
  #keep(draft: ReviewDraft | null) {
    clearTimeout(this.#timer);
    const kept = draft && !isEmpty(draft) ? draft : null;
    this.draft = kept;
    api.saveReviewDraft(this.number, kept).catch((err) => confirm.fail(err));
  }

  #started(): ReviewDraft {
    return this.draft ?? emptyDraft(this.shown ?? this.pr.headSha);
  }

  addComment(spot: Spot, body: string) {
    this.#keep(withComment(this.#started(), spot, body.trim(), Math.floor(Date.now() / 1000)));
    this.editing = null;
  }

  addReply(thread: Thread, body: string) {
    this.#keep(withReply(this.#started(), thread, body.trim(), Math.floor(Date.now() / 1000)));
    this.editing = null;
  }

  /** A comment or reply of the draft written anew, or left out with null. */
  edit(id: string, body: string | null) {
    if (this.draft) this.#keep(withEdit(this.draft, id, body === null ? null : body.trim()));
    this.editing = null;
  }

  /** The review's summary as it is typed, kept a moment later. */
  setBody(body: string) {
    const draft = { ...this.#started(), body };
    this.draft = draft;
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => this.#keep(draft), KEEP_AFTER);
  }

  setVerdict(verdict: Verdict) {
    this.#keep({ ...this.#started(), verdict });
  }

  /** Send one call at once, as the request the sheet or Activity shows. */
  async #send(call: PullCall, request: Omit<Request, "local" | "icon"> & { icon?: Request["icon"] }, done: string): Promise<boolean> {
    const [curl] = await api.githubCallsPreview([call]);
    const ok = await this.#run({
      icon: "edit",
      ...request,
      local: {
        commands: [curl],
        run: async () => {
          await api.githubRun([call]);
          return done;
        },
      },
    });
    if (ok) this.#changed();
    return ok;
  }

  async commentNow(spot: Spot, body: string) {
    const ok = await this.#send(
      { kind: "comment", number: this.number, commit: this.pr.headSha, spot, body: body.trim() },
      { title: `Comment on ${spot.path}?`, body: [`It goes to GitHub now, on ${spotLabel(spot)}, outside a review.`], button: "Comment", status: "Commenting…" },
      `Commented on ${spot.path}.`,
    );
    if (ok) this.editing = null;
  }

  async replyNow(thread: Thread, body: string) {
    const ok = await this.#send(
      { kind: "reply", number: this.number, to: thread.id, body: body.trim() },
      { title: `Reply on ${thread.path}?`, body: ["It goes to GitHub now, outside a review."], button: "Reply", status: "Replying…" },
      `Replied on ${thread.path}.`,
    );
    if (ok) this.editing = null;
  }

  resolve(thread: Thread) {
    if (!thread.node) return;
    const again = !!thread.resolved;
    void this.#send(
      { kind: "resolve", thread: thread.node, resolved: !again },
      { title: `${again ? "Unresolve" : "Resolve"} the thread on ${thread.path}?`, body: [again ? "It opens again." : "It folds away on GitHub."], button: again ? "Unresolve" : "Resolve", status: again ? "Unresolving…" : "Resolving…" },
      again ? "The thread is open again." : "The thread is resolved.",
    );
  }

  /** Send the review with its comments and replies, then put the draft away. */
  async submit(verdict: Verdict, body: string) {
    const draft = this.draft;
    const calls = submitCalls(this.pr, draft, verdict, body, this.shown);
    const curls = await api.githubCallsPreview(calls);
    const what = countWords(draft?.comments.length ?? 0, draft?.replies.length ?? 0);
    const said = verdict === "approve" ? "Approved" : verdict === "requestChanges" ? "Asked for changes on" : "Reviewed";
    const ok = await this.#run({
      title: `Send your review of #${this.number}?`,
      body: [`${verdict === "approve" ? "It approves the pull request" : verdict === "requestChanges" ? "It asks for changes" : "It comments, without a verdict"}${what ? `, with ${what}` : ""}.`],
      icon: "edit",
      button: "Submit Review",
      status: "Sending the review…",
      local: {
        commands: curls,
        comment: "a pending review, its comments and replies, then the verdict; if one fails, the pending review is deleted",
        run: async () => {
          await api.githubRun(calls);
          await api.saveReviewDraft(this.number, null);
          return `${said} #${this.number}.`;
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
    const draft = this.draft;
    if (!draft) return;
    const what = countWords(draft.comments.length, draft.replies.length);
    const ok = await this.#run({
      title: `Do you really want to discard your review of #${this.number}?`,
      body: [`${what ? `Its ${what} aren’t sent` : "Nothing of it was sent"}. A copy stays in .git/oxbow/removed for 30 days.`],
      icon: "discard",
      danger: true,
      button: "Discard Review",
      past: `Discarded the review of #${this.number}`,
      local: {
        commands: [`mv .git/oxbow/reviews/${github.repo ? `${github.repo.owner}-${github.repo.name}-` : ""}${this.number}.json .git/oxbow/removed/`],
        run: async () => {
          await api.saveReviewDraft(this.number, null);
          return "The review is discarded.";
        },
      },
    });
    if (!ok) return;
    clearTimeout(this.#timer);
    this.draft = null;
    this.finishing = false;
  }
}
