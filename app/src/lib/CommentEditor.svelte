<script lang="ts">
  // Writing a comment or a reply: it goes into the review, or to GitHub at once.
  import { onMount } from "svelte";

  let {
    value = $bindable(""),
    placeholder = "Leave a comment",
    now = null,
    add = "Add to Review",
    addOff = null,
    onCancel,
    onNow,
    onAdd,
  }: {
    value?: string;
    placeholder?: string;
    /** The button that sends it at once, e.g. Comment Now; none for a draft written again. */
    now?: string | null;
    add?: string;
    /** Why it can't go into the review, e.g. the file changed since the review started. */
    addOff?: string | null;
    onCancel: () => void;
    onNow?: () => void;
    onAdd: () => void;
  } = $props();

  let box = $state<HTMLTextAreaElement>();
  // Focused where it is: a focus that scrolls would jump the diff away from the line.
  onMount(() => {
    box?.focus({ preventScroll: true });
    box?.scrollIntoView({ block: "nearest" });
  });
  const empty = $derived(!value.trim());
</script>

<div class="editor">
  <textarea bind:this={box} bind:value {placeholder} rows="3" spellcheck="true"></textarea>
  <div class="buttons">
    {#if addOff}<span class="why">{addOff}</span>{/if}
    <button class="button small text" onclick={onCancel}>Cancel</button>
    {#if now && onNow}<button class="button small" disabled={empty} onclick={onNow}>{now}</button>{/if}
    <button class="button small primary" disabled={empty || !!addOff} title={addOff ?? undefined} onclick={onAdd}>{add}</button>
  </div>
</div>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  textarea {
    width: 100%;
    min-height: 64px;
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
  .buttons {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 6px;
  }
  .why {
    flex-grow: 1;
    min-width: 0;
    font-size: var(--text-caption);
    color: var(--text2);
  }
</style>
