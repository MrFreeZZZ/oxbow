<script lang="ts">
  // A command's keys in Settings › Keyboard Shortcuts: a capsule per key, which a click turns into
  // the recorder for a new one; + adds a key, the arrow puts the command's own keys back.

  import { keys } from "./keys";
  import { menuIcons } from "./Menu.svelte";
  import { shortcutEdit } from "./shortcutEdit.svelte";
  import { bindingsOf, changedKeys, strokeOf, type ShortcutId } from "./shortcuts";

  let { id }: { id: ShortcutId } = $props();

  const recording = $derived(shortcutEdit.recording?.id === id ? shortcutEdit.recording : null);
  const combos = $derived(bindingsOf(id));

  let recorder = $state<HTMLElement>();
  $effect(() => {
    if (recording) recorder?.focus();
  });

  function onKey(event: KeyboardEvent) {
    event.preventDefault();
    event.stopPropagation();
    const plain = !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey;
    if (plain && event.key === "Escape") shortcutEdit.cancel();
    else if (plain && event.key === "Enter") void shortcutEdit.save();
    else {
      const stroke = strokeOf(event);
      if (stroke) shortcutEdit.press(stroke);
    }
  }
</script>

{#snippet capsule(combo: string)}
  {#each keys(combo).split(" ") as part, k (k)}<kbd>{part}</kbd>{/each}
{/snippet}

{#if recording}
  <span class="recorder" class:bad={!!shortcutEdit.problem} class:warn={shortcutEdit.lost.length > 0} tabindex="0" role="textbox" aria-label="Press the new keys" bind:this={recorder} onkeydown={onKey}>
    {#if shortcutEdit.combo}{@render capsule(shortcutEdit.combo)}{:else}<span class="hint">Press keys…</span>{/if}
  </span>
  <button class="button small" onclick={() => shortcutEdit.cancel()}>Cancel</button>
  <button class="button small primary" disabled={!shortcutEdit.combo || !!shortcutEdit.problem} onclick={() => void shortcutEdit.save()}>Save</button>
{:else}
  <span class="keys">
    {#each combos as combo (combo)}
      <button class="combo" onclick={() => shortcutEdit.start(id, combo)} title="Change {keys(combo)}">{@render capsule(combo)}</button>
    {:else}
      <button class="none" onclick={() => shortcutEdit.start(id)}>Add Key</button>
    {/each}
  </span>
  <span class="tools">
    {#if combos.length}
      <button class="tool add" onclick={() => shortcutEdit.start(id)} aria-label="Add a key" title="Add a Key">
        <svg class="icon small" viewBox="0 0 16 16"><path d="M8 3.5v9M3.5 8h9" /></svg>
      </button>
    {/if}
    {#if changedKeys(id)}
      <button class="tool" onclick={() => shortcutEdit.reset(id)} aria-label="Reset to the default keys" title="Reset to Default">
        <svg class="icon small" viewBox="0 0 16 16"><path d={menuIcons.reset} /></svg>
      </button>
    {/if}
  </span>
{/if}

<style>
  .keys {
    display: flex;
    flex-shrink: 0;
    gap: 6px;
  }

  .combo,
  .none {
    display: inline-flex;
    gap: 3px;
    align-items: center;
    height: 24px;
    padding: 0 3px;
    border: none;
    border-radius: var(--radius-s);
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }

  .combo:hover,
  .combo:focus-visible {
    background: var(--accent-quiet);
  }

  .none {
    padding: 0 8px;
    font-size: var(--text-small);
    color: var(--text2);
  }

  .none:hover {
    color: var(--accent-ink);
  }

  kbd {
    font: inherit;
    font-size: var(--text-small);
    color: var(--text);
    background: var(--field);
    border-radius: var(--radius-s);
    padding: 2px 7px;
    min-width: 22px;
    text-align: center;
  }

  .tools {
    display: flex;
    flex-shrink: 0;
    gap: 2px;
    width: 52px;
    justify-content: flex-end;
  }

  .tool {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border: none;
    border-radius: var(--radius-pill);
    background: transparent;
    color: var(--text2);
    cursor: pointer;
  }

  .tool:hover {
    background: var(--field);
    color: var(--text);
  }

  /* + shows on the row under the pointer; ↺ stays while the keys are your own. */
  .tool.add {
    opacity: 0;
  }

  :global(.row:hover) .tool.add,
  .tool.add:focus-visible {
    opacity: 1;
  }

  .recorder {
    display: inline-flex;
    flex-shrink: 0;
    align-items: center;
    gap: 3px;
    min-width: 132px;
    height: 28px;
    padding: 0 8px;
    border-radius: var(--radius-s);
    background: var(--field);
    box-shadow: var(--focus-ring);
    outline: none;
  }

  .recorder.warn {
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--orange) 40%, transparent);
  }

  .recorder.bad {
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--danger) 40%, transparent);
  }

  .recorder kbd {
    background: var(--win);
  }

  .hint {
    font-size: var(--text-small);
    color: var(--text2);
  }
</style>
