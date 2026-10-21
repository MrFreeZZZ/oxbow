<script lang="ts">
  // Lines of code to type in, with line numbers: a plain text field over a copy of its lines,
  // which draws the numbers and each line's tint. Both wrap the same way, so they stay in step.

  import { onMount, untrack } from "svelte";

  let {
    value,
    first = 1,
    tintOf = () => undefined,
    label,
    focus = false,
    onedit,
    rows = $bindable(),
  }: {
    /** The starting text; what is typed then goes to `onedit`. */
    value: string;
    /** The number of the first line. */
    first?: number;
    /** A background for line `i`, from 0. */
    tintOf?: (i: number, line: string) => string | undefined;
    label: string;
    /** Takes the focus when it first shows, with the caret at the end. */
    focus?: boolean;
    onedit: (value: string) => void;
    /** The drawn lines, one element each, for scrolling to a line. */
    rows?: HTMLElement;
  } = $props();

  let text = $state(untrack(() => value));
  let field: HTMLTextAreaElement;
  const lines = $derived(text.split("\n"));

  onMount(() => {
    if (!focus) return;
    field.focus({ preventScroll: true });
    field.setSelectionRange(text.length, text.length);
  });

  function keydown(event: KeyboardEvent) {
    // Tab types a tab, as in an editor; Escape leaves the field, so the keyboard isn't stuck.
    if (event.key === "Tab" && !event.shiftKey && !event.metaKey && !event.ctrlKey && !event.altKey) {
      event.preventDefault();
      // Through the editing commands, so ⌘Z takes it back like any typing.
      if (!document.execCommand("insertText", false, "\t")) field.setRangeText("\t", field.selectionStart, field.selectionEnd, "end");
      text = field.value;
      onedit(text);
    } else if (event.key === "Escape") {
      event.stopPropagation();
      field.blur();
    }
  }
</script>

<div class="box">
  <div class="rows" aria-hidden="true" bind:this={rows}>
    {#each lines as line, i (i)}
      <div class="row" style:background={tintOf(i, line)}><span class="no">{first + i}</span><span class="text">{line === "" ? "​" : line}</span></div>
    {/each}
  </div>
  <textarea
    bind:this={field}
    bind:value={text}
    oninput={() => onedit(text)}
    onkeydown={keydown}
    aria-label={label}
    spellcheck="false"
    autocomplete="off"
    autocapitalize="off"
    class="selectable"
  ></textarea>
</div>

<style>
  .box {
    position: relative;
    border-radius: var(--radius-m);
  }
  .box:focus-within {
    box-shadow: 0 0 0 1.5px color-mix(in srgb, var(--accent) 55%, transparent);
  }
  .row {
    display: grid;
    grid-template-columns: 36px 1fr;
  }
  .row:first-child {
    border-radius: var(--radius-m) var(--radius-m) 0 0;
  }
  .row:last-child {
    border-radius: 0 0 var(--radius-m) var(--radius-m);
  }
  .row:only-child {
    border-radius: var(--radius-m);
  }
  .no {
    color: var(--text2);
    opacity: 0.7;
    text-align: right;
    padding-right: 10px;
    user-select: none;
    -webkit-user-select: none;
  }
  /* The copy only takes up the room; the field on top shows the text. */
  .text {
    white-space: pre-wrap;
    word-break: break-all;
    padding-right: 8px;
    color: transparent;
  }
  textarea {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    padding: 0 8px 0 36px;
    border: 0;
    outline: none;
    resize: none;
    overflow: hidden;
    background: transparent;
    font: inherit;
    line-height: inherit;
    letter-spacing: inherit;
    tab-size: inherit;
    color: var(--code);
    caret-color: var(--text);
    white-space: pre-wrap;
    word-break: break-all;
  }
</style>
