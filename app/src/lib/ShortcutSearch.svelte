<script lang="ts">
  // The search bar of Settings › Keyboard Shortcuts: words or keys pressed with Record Keys on,
  // only the changed commands, and Reset All.

  import { confirm } from "./confirm.svelte";
  import { keys } from "./keys";
  import { menuIcons } from "./Menu.svelte";
  import { KEYBOARD, keySearch, searchKey } from "./settingsKeys.svelte";
  import { anyChanged, resetAllRequest } from "./shortcutEdit.svelte";

  /** No command matches the search. */
  let { empty }: { empty: boolean } = $props();

  let keyField = $state<HTMLInputElement>();
</script>

<div class="key-tools">
  <label class="key-search" class:recording={keySearch.record}>
    <svg class="icon small" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5" /><path d="M10.5 10.5 14 14" /></svg>
    {#if keySearch.record}
      <input bind:this={keyField} aria-label="Press keys to find their command" placeholder="Press keys to find their command" value={keySearch.stroke ? keys(keySearch.stroke) : ""} onkeydown={searchKey} readonly />
    {:else}
      <input bind:this={keyField} type="search" aria-label="Search commands" placeholder="Search commands or keys" bind:value={keySearch.query} spellcheck="false" />
    {/if}
  </label>
  <button
    class="button pill"
    class:on={keySearch.record}
    aria-pressed={keySearch.record}
    title="Find a command by pressing its keys"
    onclick={() => {
      keySearch.record = !keySearch.record;
      keySearch.stroke = null;
      keySearch.query = "";
      requestAnimationFrame(() => keyField?.focus());
    }}
  >
    <svg class="icon small" viewBox="0 0 16 16"><path d={KEYBOARD} /></svg>Record Keys
  </button>
  <button class="button pill" class:on={keySearch.changedOnly} aria-pressed={keySearch.changedOnly} onclick={() => (keySearch.changedOnly = !keySearch.changedOnly)} title="Only the commands you gave your own keys">Changed</button>
  <span class="spacer"></span>
  <button class="button pill" disabled={!anyChanged()} onclick={() => void confirm.run(resetAllRequest())} title={anyChanged() ? "Every command back to the keys it came with, as in VS Code" : "Every command has the keys it came with"}>
    <svg class="icon small" viewBox="0 0 16 16"><path d={menuIcons.reset} /></svg>Reset All
  </button>
</div>
{#if empty}<p class="empty">{keySearch.changedOnly && !keySearch.query && !keySearch.stroke ? "Every command has the keys it came with, as in VS Code." : `No commands ${keySearch.stroke ? `with ${keys(keySearch.stroke)}` : `match “${keySearch.query.trim()}”`}.`}</p>{/if}

<style>
  /* Stays on top while the list scrolls under it. */
  .key-tools {
    position: sticky;
    top: -6px;
    z-index: 2;
    display: flex;
    align-items: center;
    gap: 8px;
    margin: -6px -30px 0 -28px;
    padding: 6px 30px 8px 28px;
    background: var(--win);
  }

  .key-search {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 280px;
    height: 30px;
    padding: 0 10px;
    border-radius: var(--radius-pill);
    background: var(--field);
    color: var(--text2);
  }

  .key-search:focus-within {
    box-shadow: var(--focus-ring);
  }

  .key-search input {
    flex-grow: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    font: inherit;
    color: var(--text);
    caret-color: transparent;
    box-shadow: none;
  }

  .key-search input[type="search"] {
    caret-color: auto;
  }

  .key-search.recording input::placeholder {
    color: var(--accent-ink);
  }

  .pill.on {
    background: var(--accent-quiet);
    color: var(--accent-ink);
  }

  .pill {
    gap: 7px;
    padding: 0 14px 0 11px;
  }

  .pill .icon {
    color: var(--icon);
  }

  .icon.small {
    width: 14px;
    height: 14px;
  }

  .spacer {
    flex-grow: 1;
    align-self: stretch;
  }

  .empty {
    margin: 0;
    padding: 8px 12px 0;
    font-size: var(--text-body);
    line-height: 1.4;
    color: var(--text2);
  }
</style>
