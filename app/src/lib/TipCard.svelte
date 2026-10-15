<script lang="ts">
  // A one-time Git tip (tips.svelte.ts): shown until Got It, or until tips are turned off.

  import { tips, TIPS, type TipId } from "./tips.svelte";

  /** `wide`: one strip across the window, with the buttons at its right end. */
  let { id, wide = false }: { id: TipId; wide?: boolean } = $props();

  const tip = $derived(TIPS[id]);
</script>

{#snippet buttons()}
  <span class="buttons">
    <button class="off" onclick={() => tips.off()}>Turn Off Tips</button>
    <button class="ok" onclick={() => tips.gotIt(id)}>Got It</button>
  </span>
{/snippet}

{#if tips.shows(id)}
  <div class="tip" class:wide role="note" aria-label="Git tip: {tip.title}">
    <span class="bulb" aria-hidden="true">
      <svg viewBox="0 0 16 16"><path d="M6 12.5h4M6.5 14.5h3M5.8 10.6C4.7 9.8 4 8.6 4 7.2a4 4 0 0 1 8 0c0 1.4-.7 2.6-1.8 3.4l-.2 1.4H6z" /></svg>
    </span>
    <div class="words">
      <span class="title">{tip.title}</span>
      <span class="body">
        {#each tip.body as part, i (i)}{#if typeof part === "string"}{part}{:else if "code" in part}<span class="code mono">{part.code}</span>{/if}{/each}
      </span>
      {#if !wide}{@render buttons()}{/if}
    </div>
    {#if wide}{@render buttons()}{/if}
  </div>
{/if}

<style>
  .tip {
    display: flex;
    gap: 10px;
    padding: 10px 12px 10px 10px;
    border-radius: 12px;
    background: color-mix(in srgb, var(--accent) 7%, var(--win));
    border: 0.5px solid color-mix(in srgb, var(--accent) 22%, transparent);
    text-align: left;
    color: var(--text);
    flex-shrink: 0;
    animation: show 0.18s ease-out;
  }
  @keyframes show {
    from {
      opacity: 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .tip {
      animation: none;
    }
  }
  .bulb {
    width: 24px;
    height: 24px;
    border-radius: 7px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent);
  }
  .bulb svg {
    width: 15px;
    height: 15px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.3;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .words {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
    flex-grow: 1;
  }
  .title {
    font-size: 12.5px;
    font-weight: 600;
    line-height: 24px;
    height: 24px;
    margin-bottom: -3px;
  }
  .body {
    font-size: 12px;
    line-height: 1.45;
    color: var(--text2);
  }
  .code {
    font-size: 11px;
    padding: 0 4px;
    border-radius: 5px;
    background: var(--field);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    margin-top: 5px;
  }
  .wide {
    margin: 0 12px 8px;
    align-items: center;
    gap: 12px;
  }
  .wide .buttons {
    margin-top: 0;
    align-self: center;
  }
  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 24px;
    min-width: 72px;
    padding: 0 11px;
    border-radius: 12px;
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
    flex-shrink: 0;
  }
  .off {
    color: var(--text2);
    background: transparent;
  }
  .off:hover {
    background: var(--hover-wash);
  }
  .ok {
    background: var(--accent);
    color: #fff;
    font-weight: 600;
  }
</style>
