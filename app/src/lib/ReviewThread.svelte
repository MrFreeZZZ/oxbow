<script lang="ts">
  // A thread of review comments on GitHub or GitLab with the replies of the review written here, and a
  // reply: into the review, or sent at once. Resolve folds it away on GitHub or GitLab. A suggested
  // change in a comment shows as one, with Apply Suggestion while its lines are the newest.
  import Avatar from "./Avatar.svelte";
  import CommentBody from "./CommentBody.svelte";
  import CommentEditor from "./CommentEditor.svelte";
  import DraftNote from "./DraftNote.svelte";
  import type { ReviewSession } from "./review.svelte";
  import type { Thread } from "./reviewDraft";
  import { relativeTime } from "./format";
  import { suggestedRange, suggestionBlock, threadText, type BodyPart } from "./suggestion";

  let { thread, session, me, resolve = true }: { thread: Thread; session: ReviewSession | null; me: string | null; resolve?: boolean } = $props();

  const replies = $derived(session?.draft?.replies.filter((r) => r.to === thread.id) ?? []);
  const writing = $derived(session?.editing?.kind === "reply" && session.editing.thread === thread.id ? session.editing : null);
  const open = $derived(session?.pr.state === "open");

  /** On the newest version of new lines: a suggestion there can be written and applied. */
  const onNewLines = $derived(!!thread.path && thread.side === "RIGHT" && !thread.outdated && thread.line !== null);
  const lab = $derived(!!session?.lab);
  const before = (part: Extract<BodyPart, { kind: "suggestion" }>) => {
    const range = onNewLines ? suggestedRange(part, thread, lab) : null;
    return range ? threadText(thread.hunk, range.start, range.end) : null;
  };
  /** Apply Suggestion shows to whoever has the branch checked out, and to the author. */
  const canApply = $derived(!!session && onNewLines && open && (session.onBranch || (!!me && session.pr.author.login === me)));
  const applyOff = $derived(session && !session.onBranch ? `Check out ${session.pr.head} to apply it` : null);
  /** Suggest Change in a reply: the thread's lines, when they show here. */
  const suggest = $derived.by(() => {
    if (!onNewLines || thread.line === null) return null;
    const start = Math.min(thread.startLine ?? thread.line, thread.line);
    const lines = threadText(thread.hunk, start, thread.line);
    return lines ? () => suggestionBlock(lines, lab, thread.line! - start + 1) : null;
  });
</script>

<div class="thread">
  {#each thread.comments as c, i (i)}
    <div class="comment">
      <Avatar login={c.author.login} url={c.author.avatarUrl} size={22} />
      <div class="main">
        <div class="head"><span class="who">{c.author.login}</span><span class="muted">{relativeTime(c.time).toLowerCase()}</span></div>
        <CommentBody body={c.body} {before} onApply={canApply ? (part) => session?.applySuggestion(thread, part, c.author.login) : undefined} {applyOff} />
      </div>
    </div>
  {/each}
  {#if session && me}
    {#each replies as r (r.id)}
      <DraftNote {session} id={r.id} body={r.body} time={r.time} {me} />
    {/each}
    {#if open}
      <div class="foot">
        {#if writing}
          <CommentEditor
            bind:value={writing.body}
            placeholder="Reply"
            now="Reply Now"
            addOff={thread.node ? null : `${session.words.name} didn’t name this thread, so the reply can only go now.`}
            onCancel={() => (session.editing = null)}
            onNow={() => session.replyNow(thread, writing.body)}
            onAdd={() => session.addReply(thread, writing.body)}
            {suggest}
          />
        {:else}
          <Avatar login={me} size={22} />
          <button class="reply" onclick={() => (session.editing = { kind: "reply", thread: thread.id, body: "" })}>Reply…</button>
          {#if resolve && thread.node}
            <button class="button small text" onclick={() => session.resolve(thread)} title={thread.resolved ? "Open the thread again" : `Fold the thread away on ${session.words.name}`}>
              <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="5.5" /><path d="M5.8 8.2 7.3 9.7 10.4 6.4" /></svg>
              {thread.resolved ? "Unresolve" : "Resolve"}
            </button>
          {/if}
        {/if}
      </div>
    {/if}
  {/if}
</div>

<style>
  .thread {
    display: flex;
    flex-direction: column;
    font-family: var(--font);
    color: var(--text);
  }
  .comment {
    display: flex;
    gap: 10px;
    padding: 10px 12px;
  }
  .comment + .comment,
  .thread > :global(.note) {
    border-top: 1px solid var(--sep);
  }
  .main {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .head {
    display: flex;
    align-items: baseline;
    gap: 6px;
    font-size: var(--text-small);
  }
  .who {
    font-weight: 600;
  }
  .muted {
    color: var(--text2);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border-top: 1px solid var(--sep);
  }
  .foot > :global(.editor) {
    flex-grow: 1;
  }
  .reply {
    flex-grow: 1;
    height: 28px;
    padding: 0 10px;
    border-radius: var(--radius-m);
    background: var(--field);
    color: var(--text2);
    text-align: left;
    font-size: var(--text-body);
  }
  .icon {
    width: 13px;
    height: 13px;
  }
</style>
