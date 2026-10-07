<script lang="ts">
  import { keys, withKeys } from "./keys";
  // Find in file, in File History's toolbar: the text, a letter-case switch, which match is
  // current among how many, and arrows to the previous and next one.

  import { nav } from "./nav.svelte";

  let input = $state<HTMLInputElement>();
  const active = $derived(!!nav.find.text);

  export function focus() {
    input?.focus();
    input?.select();
  }

  function clear() {
    nav.find = { ...nav.find, text: "" };
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "Enter") {
      event.preventDefault();
      nav.findStep(event.shiftKey ? -1 : 1);
    } else if (event.key === "Escape") {
      event.preventDefault();
      if (nav.find.text) clear();
      else input?.blur();
    }
  }
</script>

<div class="field" class:active>
  <svg class="icon glass" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5" /><path d="M10.3 10.3 14 14" /></svg>
  <input
    bind:this={input}
    value={nav.find.text}
    oninput={(e) => (nav.find = { ...nav.find, text: e.currentTarget.value })}
    onkeydown={onKey}
    placeholder="Find in file"
    spellcheck="false"
    autocomplete="off"
    aria-label="Find in file"
    title={withKeys("Find in File", "Mod+F")}
  />
  <button
    class="case"
    class:on={nav.find.matchCase}
    onclick={() => (nav.find = { ...nav.find, matchCase: !nav.find.matchCase })}
    aria-pressed={nav.find.matchCase}
    aria-label="Match case"
    title={nav.find.matchCase ? "Matching letter case" : "Ignoring letter case"}>Aa</button
  >
  {#if active}
    <span class="place" aria-live="polite">{nav.found.count ? `${nav.found.at + 1} of ${nav.found.count}` : "None"}</span>
    <button class="arrow" onclick={() => nav.findStep(-1)} disabled={!nav.found.count} aria-label="Previous match" title={withKeys("Previous Match", "Shift+Enter")}>
      <svg class="icon" viewBox="0 0 16 16"><path d="M4.5 9.5 8 6l3.5 3.5" /></svg>
    </button>
    <button class="arrow" onclick={() => nav.findStep(1)} disabled={!nav.found.count} aria-label="Next match" title={withKeys("Next Match", "Enter")}>
      <svg class="icon" viewBox="0 0 16 16"><path d="M4.5 6.5 8 10l3.5-3.5" /></svg>
    </button>
    <button class="clear" onclick={clear} aria-label="Clear">
      <svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" /><path d="M6 6l4 4M10 6l-4 4" /></svg>
    </button>
  {/if}
</div>

<style>
  .field {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 220px;
    flex-shrink: 1;
    min-width: 180px;
    height: 34px;
    padding: 0 6px 0 10px;
    border-radius: 17px;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    color: var(--icon);
    transition: width 0.15s ease;
  }
  .field.active {
    width: 300px;
  }
  .field:focus-within {
    box-shadow:
      var(--glass-shadow),
      0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent);
  }
  .glass {
    flex-shrink: 0;
    color: var(--text2);
  }
  input {
    flex-grow: 1;
    min-width: 40px;
    border: 0;
    outline: none;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 13px;
    user-select: text;
    -webkit-user-select: text;
  }
  input::placeholder {
    color: var(--text3, var(--text2));
  }
  .case {
    height: 22px;
    padding: 0 6px;
    border-radius: 7px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
    flex-shrink: 0;
  }
  .case:hover {
    background: var(--field);
  }
  .case.on {
    background: var(--side-sel);
    color: var(--text);
  }
  .place {
    font-size: 11px;
    color: var(--text2);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
    padding: 0 2px;
  }
  .arrow {
    width: 22px;
    height: 22px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 11px;
    flex-shrink: 0;
  }
  .arrow:disabled {
    opacity: 0.35;
  }
  .arrow:not(:disabled):hover {
    background: var(--side-sel);
  }
  .clear {
    display: flex;
    flex-shrink: 0;
  }
  .clear svg {
    width: 15px;
    height: 15px;
    fill: var(--text2);
    stroke: var(--win);
    stroke-width: 1.5;
    stroke-linecap: round;
    opacity: 0.7;
  }
</style>
