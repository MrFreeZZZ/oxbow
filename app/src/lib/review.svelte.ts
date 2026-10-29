// Reviewing one pull request here: the files ticked Viewed on GitHub, the draft review kept in
// `.git/oxbow/reviews` until it is sent, the one comment being written, and the requests that
// send comments, replies and the review itself. A merge request on GitLab the same way: its
// Viewed ticks stay on this computer, as GitLab keeps none for others to read.

import { api } from "./api";
import { github } from "./github.svelte";
import { confirm, type Request } from "./confirm.svelte";
import type { PullCall, PullRequest, ReviewDraft, Spot, Verdict, Viewing } from "./types";
import { addProblem, approvedBy, commentCall, countWords, emptyDraft, isEmpty, spotLabel, submitCalls, withComment, withEdit, withReply, type Thread, type Writing } from "./reviewDraft";
import { wordsOf, type ForgeKind, type Words } from "./forge";
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
  #timer: ReturnType<typeof setTimeout> | undefined;

  constructor(pr: PullRequest, run: (request: Request | Promise<Request>) => Promise<boolean>, changed: () => void, forge: ForgeKind = "github") {
    this.number = pr.number;
    this.lab = forge === "gitlab";
    this.words = wordsOf(forge);
    this.pr = pr;
    this.files = pr.changedFiles;
    this.#run = run;
    this.#changed = changed;
    this.loadViewing().catch(() => {});
    api.reviewDraft(pr.number, this.lab).then(
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
    const v = await (this.lab ? api.gitlabViewing(this.number) : api.githubViewing(this.number));
    this.viewed = new Set(v.viewed);
    this.dismissed = new Set(v.dismissed);
    this.pending = v.pending;
  }

  /** Tick or untick a file's Viewed box, here at once and then on GitHub (GitLab's stay here). */
  setViewed(path: string, on: boolean, failed: (err: unknown) => void) {
    const before = { viewed: this.viewed, dismissed: this.dismissed };
    const v = new Set(this.viewed);
    const d = new Set(this.dismissed);
    if (on) v.add(path);
    else v.delete(path);
    d.delete(path);
    this.viewed = v;
    this.dismissed = d;
    const sent = this.lab ? api.gitlabSetViewed(this.number, path, on) : api.githubSend({ kind: "viewed", id: this.pr.nodeId, path, viewed: on });
    sent.catch((err) => {
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
    api.saveReviewDraft(this.number, kept, this.lab).catch((err) => confirm.fail(err));
  }

  #preview(calls: PullCall[]): Promise<string[]> {
    return this.lab ? api.gitlabCallsPreview(calls) : api.githubCallsPreview(calls);
  }

  #sendCalls(calls: PullCall[]): Promise<unknown> {
    return this.lab ? api.gitlabRun(calls) : api.githubRun(calls);
  }

  /** How the draft's file is named in `.git/oxbow/reviews`. */
  #filePrefix(): string {
    if (this.lab) return github.mrProject ? `gitlab-${github.mrProject.path.replaceAll("/", "-")}-` : "";
    return github.repo ? `${github.repo.owner}-${github.repo.name}-` : "";
  }

  #started(): ReviewDraft {
    return this.draft ?? emptyDraft(this.shown ?? this.pr.headSha);
  }

  addComment(writing: Writing) {
    // Whether its file is the same on the newest commit is for the Files tab to say, before Add
    // is offered; a comment on an older commit than the review's never goes in.
    if (addProblem(this.draft, writing, this.pr.headSha, true)) return;
    // A review not started yet starts on the commit the comment was written on.
    const draft = this.draft ?? emptyDraft(writing.commit);
    this.#keep(withComment(draft, writing.spot, writing.body.trim(), Math.floor(Date.now() / 1000)));
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
    await (this.lab ? api.gitlabFetch(this.number, this.pr.base) : api.pullFetch(this.number, this.pr.base)).catch(() => {});
    await this.#run(applyRequest(thread.path, commit, range, part.lines, who));
  }

  /** Send the review with its comments and replies, then put the draft away. */
  async submit(verdict: Verdict, body: string) {
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
          await api.saveReviewDraft(this.number, null, this.lab);
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
        commands: [`mv .git/oxbow/reviews/${this.#filePrefix()}${this.number}.json .git/oxbow/removed/`],
        run: async () => {
          await api.saveReviewDraft(this.number, null, this.lab);
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
