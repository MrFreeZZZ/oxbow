<script lang="ts">
  // The title bar of a repository open Restricted: a capsule that says so, and under it what
  // git runs with off, with the way to trust it.

  import { restrictedLines } from "./trust";
  import type { TrustFinding } from "./types";

  let { findings, onTrust }: { findings: TrustFinding[]; onTrust: () => void } = $props();

  let open = $state(false);
  const lines = $derived(restrictedLines(findings));
  let box = $state<HTMLDivElement>();
  $effect(() => {
    if (open) box?.querySelector<HTMLButtonElement>("button.primary")?.focus();
  });

  function onKey(event: KeyboardEvent) {
    if (open && event.key === "Escape") {
      event.preventDefault();
      open = false;
    }
  }

  function trust() {
    open = false;
    onTrust();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="place">
  <button class="restricted" class:open onclick={() => (open = !open)} aria-expanded={open}>
    <svg class="icon" viewBox="0 0 16 16"><path d="M8 1.6 2.8 3.5v4c0 3.2 2.2 5.7 5.2 6.9 3-1.2 5.2-3.7 5.2-6.9v-4z" /></svg>Restricted
  </button>
  {#if open}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="catcher" onclick={() => (open = false)}></div>
    <div class="pop" role="dialog" aria-label="Restricted" bind:this={box}>
      <span class="title">Restricted</span>
      <span class="body">Oxbow runs Git here with this repository’s programs off. Your own Git settings still apply.</span>
      <ul>
        {#each lines as line (line.label)}
          <li><span class="label">{line.label}</span><span class="as">{line.as}</span></li>
        {/each}
      </ul>
      <span class="buttons">
        <button class="button small" onclick={trust}>Show Details…</button>
        <button class="button small primary" onclick={trust}>Trust This Repository…</button>
      </span>
    </div>
  {/if}
</div>

<style>
  .place {
    position: relative;
    flex-shrink: 0;
  }
  .restricted {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 26px;
    padding: 0 10px 0 8px;
    border-radius: var(--radius-pill);
    border: 0;
    font-size: var(--text-small);
    white-space: nowrap;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    font-weight: 500;
  }
  .restricted .icon {
    width: 14px;
    height: 14px;
  }
  .restricted.open {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .catcher {
    position: fixed;
    inset: 0;
    z-index: 19;
  }
  .pop {
    position: absolute;
    left: 0;
    top: 38px;
    z-index: 20;
    width: 340px;
    max-width: calc(100vw - 32px);
    padding: 14px 14px 12px;
    border-radius: var(--radius-xl);
    background: var(--sheet);
    -webkit-backdrop-filter: blur(30px) saturate(1.8);
    backdrop-filter: blur(30px) saturate(1.8);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--shadow-pop);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .title {
    font-weight: 600;
  }
  .body {
    font-size: var(--text-small);
    line-height: 1.45;
    color: var(--text2);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  li {
    display: flex;
    gap: 8px;
    font-size: var(--text-small);
  }
  .label {
    overflow-wrap: anywhere;
  }
  .as {
    margin-left: auto;
    color: var(--text2);
    white-space: nowrap;
  }
  .buttons {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    padding-top: 4px;
  }
</style>
