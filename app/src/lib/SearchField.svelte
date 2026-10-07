<script lang="ts">
  // The toolbar search field: a mode token (Message, Code, Author, File, SHA), the text, and the
  // place of the selected commit among the matches with arrows to the previous and next one.

  import type { HistoryRow } from "./types";
  import { MODES, search, type Mode } from "./search.svelte";
  import { nav } from "./nav.svelte";

  let {
    rows,
    matches,
    selected,
    onGo,
  }: {
    rows: HistoryRow[];
    /** Matching commits in graph order. */
    matches: string[];
    selected: string | null;
    onGo: (id: string) => void;
  } = $props();

  let input = $state<HTMLInputElement>();
  let menu = $state(false);

  const parsed = $derived(search.parsed);
  const label = $derived(MODES.find((m) => m.mode === parsed.mode)?.label ?? "Message");
  const at = $derived(selected ? matches.indexOf(selected) : -1);
  const modes = $derived(MODES.filter((m) => m.mode !== "sha" || search.looksLikeSha || parsed.mode === "sha"));

  export function focus() {
    input?.focus();
    input?.select();
  }

  /** The next match below the selected commit, or the previous one above it. */
  function step(by: 1 | -1) {
    if (!matches.length) return;
    let next: number;
    if (at >= 0) next = (at + by + matches.length) % matches.length;
    else {
      // The selected commit isn't a match: go to the nearest one in that direction.
      const index = selected ? rows.findIndex((r) => r.id === selected) : -1;
      const order = new Map(rows.map((r, i) => [r.id, i]));
      const below = matches.findIndex((id) => (order.get(id) ?? 0) > index);
      next = by === 1 ? (below < 0 ? 0 : below) : below <= 0 ? matches.length - 1 : below - 1;
    }
    onGo(matches[next]);
    search.remember();
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "Enter") {
      event.preventDefault();
      step(event.shiftKey ? -1 : 1);
    } else if (event.key === "Escape") {
      event.preventDefault();
      if (menu) menu = false;
      else if (search.input) search.clear();
      else input?.blur();
    } else if (event.key === "ArrowDown" && (event.metaKey || event.ctrlKey || event.altKey)) {
      event.preventDefault();
      step(1);
    } else if (event.key === "ArrowUp" && (event.metaKey || event.ctrlKey || event.altKey)) {
      event.preventDefault();
      step(-1);
    }
  }

  function openMenu() {
    menu = !menu;
    if (menu) search.countModes(rows);
  }

  function pick(mode: Mode) {
    search.pick(mode);
    menu = false;
    input?.focus();
  }

  const count = (n: number | null | undefined) => (n === undefined ? "" : n === null ? "…" : n === 1 ? "1 commit" : `${n.toLocaleString()} commits`);
</script>

<svelte:window
  onkeydown={(event) => {
    if (menu && event.key === "Escape") {
      event.preventDefault();
      menu = false;
      input?.focus();
    }
  }}
/>

<div class="field" class:active={search.active}>
  <svg class="icon glass" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5" /><path d="M10.3 10.3 14 14" /></svg>
  <button class="token" onclick={openMenu} aria-haspopup="menu" aria-expanded={menu} aria-label="Search in {label}">
    {label}
    <svg class="icon chevron" viewBox="0 0 16 16"><path d="M4.5 6.5 8 10l3.5-3.5" /></svg>
  </button>
  <input
    bind:this={input}
    bind:value={search.input}
    onkeydown={onKey}
    oninput={() => search.takePrefix()}
    onblur={() => search.remember()}
    placeholder="Search history"
    spellcheck="false"
    autocomplete="off"
    aria-label="Search history"
  />
  {#if search.active}
    <span class="place" aria-live="polite">
      {#if search.busy && !search.result}
        <span class="spinner" aria-label="Searching"></span>
      {:else if matches.length}
        {at >= 0 ? `${at + 1} of ${matches.length}` : `${matches.length}`}
      {:else}
        None
      {/if}
    </span>
    <button class="arrow" onclick={() => step(-1)} disabled={!matches.length} aria-label="Previous match" title="Previous match (⇧Enter)">
      <svg class="icon" viewBox="0 0 16 16"><path d="M4.5 9.5 8 6l3.5 3.5" /></svg>
    </button>
    <button class="arrow" onclick={() => step(1)} disabled={!matches.length} aria-label="Next match" title="Next match (Enter)">
      <svg class="icon" viewBox="0 0 16 16"><path d="M4.5 6.5 8 10l3.5-3.5" /></svg>
    </button>
    <button class="clear" onclick={() => search.clear()} aria-label="Clear search">
      <svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" /><path d="M6 6l4 4M10 6l-4 4" /></svg>
    </button>
  {/if}

  {#if menu}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="catcher" onclick={() => (menu = false)}></div>
    <div class="pop" role="menu" aria-label="Search in">
      <div class="heading">{parsed.text ? `Search “${parsed.text}” in` : "Search in"}</div>
      {#each modes as m (m.mode)}
        <button class="mode" class:on={m.mode === parsed.mode} role="menuitemradio" aria-checked={m.mode === parsed.mode} onclick={() => pick(m.mode)}>
          <span class="check">{m.mode === parsed.mode ? "✓" : ""}</span>
          <span class="what">
            <span class="name">{m.menu}</span>
            <span class="sub">{m.sub}</span>
          </span>
          {#if parsed.text}<span class="n">{count(search.counts[m.mode])}</span>{/if}
          <span class="flag">{m.flag}</span>
        </button>
      {/each}
      <div class="sep"></div>
      <button
        class="mode"
        role="menuitem"
        onclick={() => {
          menu = false;
          nav.quickOpen = true;
        }}
      >
        <span class="check"></span>
        <span class="what">
          <span class="name">Find in a file…</span>
          <span class="sub">pick a file, then search its text and history</span>
        </span>
        <span class="flag">{navigator.platform.startsWith("Mac") ? "⌘P" : "Ctrl+P"}</span>
      </button>
      {#if search.recent.length}
        <div class="sep"></div>
        <div class="heading">Recent searches</div>
        {#each search.recent as r (r.mode + r.text)}
          <button
            class="recent"
            role="menuitem"
            onclick={() => {
              search.recall(r);
              menu = false;
              input?.focus();
            }}
          >
            <svg class="icon clock" viewBox="0 0 16 16"><circle cx="8" cy="8" r="5.5" /><path d="M8 5v3.2l2 1.3" /></svg>
            <span class="chip">{MODES.find((m) => m.mode === r.mode)?.label}</span>
            <span class="name">{r.text}</span>
          </button>
        {/each}
      {/if}
      <div class="sep"></div>
      <div class="tip">Or start with a prefix: <code>code:</code>, <code>author:</code>, <code>path:</code>, <code>msg:</code></div>
    </div>
  {/if}
</div>

<style>
  .field {
    position: relative;
    display: flex;
    align-items: center;
    gap: 4px;
    width: 300px;
    flex-shrink: 1;
    min-width: 200px;
    height: 34px;
    padding: 0 6px 0 10px;
    border-radius: 17px;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    color: var(--icon);
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
  .token {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 22px;
    padding: 0 5px 0 8px;
    border-radius: 11px;
    background: var(--side-sel);
    color: var(--text);
    font-size: 12px;
    font-weight: 600;
    flex-shrink: 0;
  }
  .chevron {
    width: 11px;
    height: 11px;
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
  .spinner {
    display: inline-block;
    width: 11px;
    height: 11px;
    border-radius: 50%;
    border: 1.5px solid var(--sep);
    border-top-color: var(--text2);
    animation: spin 0.8s linear infinite;
    vertical-align: -2px;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .catcher {
    position: fixed;
    inset: 0;
    z-index: 19;
  }
  .pop {
    position: absolute;
    right: 0;
    top: 40px;
    z-index: 20;
    width: 380px;
    padding: 6px;
    border-radius: 16px;
    background: var(--sheet);
    -webkit-backdrop-filter: blur(30px) saturate(1.8);
    backdrop-filter: blur(30px) saturate(1.8);
    border: 0.5px solid var(--glass-border);
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.18);
    display: flex;
    flex-direction: column;
    color: var(--text);
  }
  .heading {
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
    padding: 6px 10px 4px;
  }
  .mode,
  .recent {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px 0 6px;
    border-radius: 9px;
    text-align: left;
  }
  .mode {
    min-height: 40px;
  }
  .recent {
    height: 28px;
  }
  .mode:hover,
  .recent:hover {
    background: var(--side-sel);
  }
  .mode.on {
    background: var(--side-sel);
  }
  .check {
    width: 14px;
    font-size: 12px;
    font-weight: 700;
    flex-shrink: 0;
    text-align: center;
  }
  .what {
    flex-grow: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    line-height: 1.25;
  }
  .what .name {
    font-weight: 600;
  }
  .sub {
    font-size: 11px;
    color: var(--text2);
  }
  .n {
    font-size: 11px;
    color: var(--text2);
    white-space: nowrap;
  }
  .flag {
    font-family: var(--mono);
    font-size: 11px;
    color: var(--text2);
    background: var(--field);
    border-radius: 6px;
    padding: 2px 6px;
    min-width: 52px;
    text-align: center;
  }
  .clock {
    color: var(--text2);
    flex-shrink: 0;
  }
  .chip {
    font-size: 11px;
    font-weight: 600;
    background: var(--field);
    border-radius: 6px;
    padding: 1px 7px;
  }
  .recent .name {
    flex-grow: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sep {
    height: 1px;
    background: var(--sep);
    margin: 5px 8px;
  }
  .tip {
    font-size: 11px;
    color: var(--text2);
    padding: 4px 10px 6px;
  }
  .tip code {
    font-family: var(--mono);
    color: var(--text);
  }
</style>
