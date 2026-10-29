<script lang="ts">
  // A thread of review comments on GitHub or GitLab with the replies of the review written here, and a
  // reply: into the review, or sent at once. Resolve folds it away on GitHub or GitLab.
  import Avatar from "./Avatar.svelte";
  import CommentEditor from "./CommentEditor.svelte";
  import DraftNote from "./DraftNote.svelte";
  import type { ReviewSession } from "./review.svelte";
  import type { Thread } from "./reviewDraft";
  import { relativeTime } from "./format";

  let { thread, session, me, resolve = true }: { thread: Thread; session: ReviewSession | null; me: string | null; resolve?: boolean } = $props();

  const replies = $derived(session?.draft?.replies.filter((r) => r.to === thread.id) ?? []);
  const writing = $derived(session?.editing?.kind === "reply" && session.editing.thread === thread.id ? session.editing : null);
  const open = $derived(session?.pr.state === "open");
</script>

<div class="thread">
  {#each thread.comments as c, i (i)}
    <div class="comment">
      <Avatar login={c.author.login} url={c.author.avatarUrl} size={22} />
      <div class="main">
        <div class="head"><span class="who">{c.author.login}</span><span class="muted">{relativeTime(c.time).toLowerCase()}</span></div>
        <p class="body selectable">{c.body}</p>
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
  .body {
    margin: 0;
    font-size: var(--text-body);
    line-height: 1.45;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
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
