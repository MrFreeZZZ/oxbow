<script lang="ts">
  // A comment or reply of the review written here, not sent yet: Pending, to edit or take out.
  import Avatar from "./Avatar.svelte";
  import CommentEditor from "./CommentEditor.svelte";
  import type { ReviewSession } from "./review.svelte";
  import { relativeTime } from "./format";

  let { session, id, body, time, me, label = "" }: { session: ReviewSession; id: string; body: string; time: number; me: string; label?: string } = $props();

  const editing = $derived(session.editing?.kind === "edit" && session.editing.id === id ? session.editing : null);
</script>

<div class="note">
  <Avatar login={me} size={22} />
  <div class="main">
    <div class="head">
      <span class="who">{me}</span>
      {#if label}<span class="muted">{label}</span>{/if}
      <span class="muted">{relativeTime(time).toLowerCase()}</span>
      <span class="pending">Pending</span>
      <span class="grow"></span>
      {#if !editing}
        <button class="button mini text" onclick={() => (session.editing = { kind: "edit", id, body })}>Edit</button>
        <button class="button mini text" onclick={() => session.edit(id, null)} title="Take it out of the review">Delete</button>
      {/if}
    </div>
    {#if editing}
      <CommentEditor bind:value={editing.body} add="Save" onCancel={() => (session.editing = null)} onAdd={() => session.edit(id, editing.body)} />
    {:else}
      <p class="body selectable">{body}</p>
    {/if}
  </div>
</div>

<style>
  .note {
    display: flex;
    gap: 10px;
    padding: 10px 12px;
  }
  .main {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 22px;
    font-size: var(--text-small);
  }
  .who {
    font-weight: 600;
  }
  .muted {
    color: var(--text2);
  }
  .grow {
    flex-grow: 1;
  }
  .pending {
    padding: 1px 7px;
    border-radius: var(--radius-pill);
    background: var(--accent-quiet);
    color: var(--accent-ink);
    font-size: var(--text-caption);
    font-weight: 600;
  }
  .body {
    margin: 0;
    font-size: var(--text-body);
    line-height: 1.45;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
</style>
