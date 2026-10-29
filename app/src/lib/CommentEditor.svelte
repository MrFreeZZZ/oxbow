<script lang="ts">
  // Writing a comment or a reply: it goes into the review, or to GitHub at once. On new lines,
  // Suggest Change writes the lines it is on as a suggestion, to edit into what they should be.
  import { onMount, tick } from "svelte";
  import { insertBlock } from "./suggestion";

  let {
    value = $bindable(""),
    placeholder = "Leave a comment",
    now = null,
    add = "Add to Review",
    addOff = null,
    onCancel,
    onNow,
    onAdd,
    suggest = null,
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
    /** The suggestion block of the lines it is on; none where a suggestion can't go. */
    suggest?: (() => string | null) | null;
  } = $props();

  let box = $state<HTMLTextAreaElement>();
  // Focused where it is: a focus that scrolls would jump the diff away from the line.
  onMount(() => {
    box?.focus({ preventScroll: true });
    box?.scrollIntoView({ block: "nearest" });
  });
  const empty = $derived(!value.trim());

  async function suggestChange() {
    const block = suggest?.();
    if (!block || !box) return;
    const from = box.selectionStart ?? value.length;
    const put = insertBlock(value, from, block);
    value = put.value;
    await tick();
    // Tall enough for the suggested lines, up to a screenful.
    box.style.height = `${Math.min(box.scrollHeight + 2, 360)}px`;
    // The caret at the end of the last suggested line, ready to change it.
    const at = put.start + block.lastIndexOf("\n");
    box.focus({ preventScroll: true });
    box.setSelectionRange(at, at);
  }
</script>

<div class="editor">
  <textarea bind:this={box} bind:value {placeholder} rows="3" spellcheck="true"></textarea>
  <div class="buttons">
    {#if suggest}
      <button class="button small text suggest" onclick={suggestChange} title="Write the lines as a suggestion, then change them to what they should be">
        <svg class="icon" viewBox="0 0 16 16"><path d="M4 5.5h8M8 1.5v8M4 13.5h8" /></svg>
        Suggest Change
      </button>
    {/if}
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
  .suggest {
    margin-right: auto;
  }
  .icon {
    width: 13px;
    height: 13px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
    stroke-linecap: round;
  }
  .why {
    flex-grow: 1;
    min-width: 0;
    font-size: var(--text-caption);
    color: var(--text2);
  }
</style>
