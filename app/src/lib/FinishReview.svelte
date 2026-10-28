<script lang="ts">
  // Finish Review: the summary, the verdict, and Submit, which sends the review with its comments
  // and replies. The summary and verdict stay with the draft while it waits.
  import type { ReviewSession } from "./review.svelte";
  import type { Verdict } from "./types";
  import { countWords, submitProblem } from "./reviewDraft";
  import { shortId } from "./format";
  import { api } from "./api";

  let { session, mine, anchor }: { session: ReviewSession; mine: boolean; /** The button it opens from, which it hangs under. */ anchor: HTMLElement | undefined } = $props();

  /** Under the button, its right edge on the button's, kept inside the window. */
  const WIDTH = 380;
  let place = $state({ top: 0, left: 0 });
  $effect(() => {
    const r = anchor?.getBoundingClientRect();
    if (!r) return;
    place = { top: r.bottom + 6, left: Math.max(16, Math.min(r.right - WIDTH, window.innerWidth - WIDTH - 16)) };
  });

  const VERDICTS: { id: Verdict; label: string; text: string }[] = [
    { id: "comment", label: "Comment", text: "Feedback, without a verdict" },
    { id: "approve", label: "Approve", text: "Fine to merge as it is" },
    { id: "requestChanges", label: "Request Changes", text: "Needs changes before it merges" },
  ];

  const draft = $derived(session.draft);
  const verdict = $derived<Verdict>(draft?.verdict ?? "comment");
  const body = $derived(draft?.body ?? "");
  const what = $derived(countWords(draft?.comments.length ?? 0, draft?.replies.length ?? 0));
  const moved = $derived(!!draft && draft.commit !== session.pr.headSha);
  /** Commits came in after the files shown: the review goes on the ones read. */
  const behind = $derived(!draft && !!session.shown && session.shown !== session.pr.headSha);
  const problem = $derived(
    session.pending
      ? `A review you started on GitHub${session.pending.comments ? ` with ${countWords(session.pending.comments, 0)}` : ""} is waiting there: send or discard it on GitHub first.`
      : submitProblem(verdict, body, draft, mine),
  );

  let box = $state<HTMLTextAreaElement>();
  $effect(() => box?.focus());

  const close = () => (session.finishing = false);
</script>

<div class="catcher" role="presentation" onclick={close}></div>
<div class="pop" role="dialog" aria-label="Finish Review" style:top="{place.top}px" style:left="{place.left}px">
  <span class="title">Finish Review</span>
  <textarea bind:this={box} rows="3" placeholder="Sum it up (optional)" value={body} oninput={(e) => session.setBody(e.currentTarget.value)}></textarea>
  <div class="verdicts" role="radiogroup" aria-label="Verdict">
    {#each VERDICTS as v (v.id)}
      {@const off = mine && v.id !== "comment"}
      <button class="verdict" role="radio" aria-checked={verdict === v.id} disabled={off} title={off ? "Not on your own pull request" : undefined} onclick={() => session.setVerdict(v.id)}>
        <span class="radio" class:on={verdict === v.id}></span>
        <span class="words"><span class="label">{v.label}</span><span class="text">{v.text}</span></span>
      </button>
    {/each}
  </div>
  {#if what || problem || behind}
    <div class="info" class:warn={!!session.pending}>
      <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="5.5" /><path d="M8 7.5v3M8 5.3v.2" /></svg>
      <span>
        {#if session.pending}
          {problem}
        {:else if what}
          <b>{what}</b> go with it, on lines of {shortId(draft!.commit)}{moved ? ", the version you read. GitHub marks the ones whose lines changed since as outdated." : "."}
        {:else if problem}
          {problem}
        {:else}
          It goes on {shortId(session.shown!)}, the version you read. Newer commits aren’t in Files yet, so it doesn’t cover them.
        {/if}
      </span>
    </div>
  {/if}
  <div class="buttons">
    {#if session.pending}
      <button class="button small text" onclick={() => api.openGitHub(`${session.pr.htmlUrl}/files`).catch(() => {})}>Open on GitHub</button>
    {:else if draft}
      <button class="button small text" onclick={() => session.discard()}>Discard Review…</button>
    {/if}
    <span class="grow"></span>
    <button class="button small" onclick={close}>Cancel</button>
    <button class="button small primary" disabled={!!problem} title={problem ?? undefined} onclick={() => session.submit(verdict, body)}>Submit Review</button>
  </div>
</div>

<style>
  .catcher {
    position: fixed;
    inset: 0;
    z-index: 19;
  }
  .pop {
    position: fixed;
    z-index: 20;
    width: 380px;
    max-width: calc(100vw - 32px);
    padding: 16px;
    border-radius: var(--radius-xl);
    background: var(--sheet);
    -webkit-backdrop-filter: blur(30px) saturate(1.8);
    backdrop-filter: blur(30px) saturate(1.8);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--shadow-pop);
    display: flex;
    flex-direction: column;
    gap: 12px;
    text-align: left;
    color: var(--text);
    font-weight: 400;
  }
  .title {
    font-size: var(--text-heading);
    font-weight: 600;
  }
  textarea {
    width: 100%;
    resize: vertical;
    padding: 8px 10px;
    border-radius: var(--radius-m);
    border: 1px solid var(--sep);
    background: var(--win);
    color: var(--text);
    font-family: var(--font);
    font-size: var(--text-body);
    line-height: 1.45;
  }
  .verdicts {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .verdict {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 2px 0;
    text-align: left;
  }
  .verdict:disabled {
    opacity: 0.45;
  }
  .radio {
    width: 16px;
    height: 16px;
    margin-top: 1px;
    border-radius: 50%;
    border: 1.3px solid var(--lead-in);
    background: var(--win);
    flex-shrink: 0;
  }
  .radio.on {
    border: 5px solid var(--accent);
  }
  .words {
    display: flex;
    flex-direction: column;
  }
  .label {
    font-size: var(--text-body);
    font-weight: 600;
  }
  .text {
    font-size: var(--text-small);
    color: var(--text2);
  }
  .info {
    display: flex;
    gap: 8px;
    padding: 10px 12px;
    border-radius: var(--radius-m);
    background: var(--field);
    font-size: var(--text-small);
    line-height: 1.4;
  }
  .info.warn {
    background: var(--orange-soft);
  }
  .info .icon {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    margin-top: 1px;
    color: var(--text2);
  }
  .buttons {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .grow {
    flex-grow: 1;
  }
</style>
