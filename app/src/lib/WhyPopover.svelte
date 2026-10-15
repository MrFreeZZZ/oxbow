<script lang="ts">
  // Under a grey toolbar button that was clicked: why it can't run now, and the way out.

  import type { Part } from "./confirm.svelte";
  import { plate, tint } from "./format";

  let {
    title,
    body,
    fixes,
    onFix,
    onClose,
  }: {
    title: string;
    body: Part[];
    /** The last one is the main way out. */
    fixes: { label: string }[];
    onFix: (index: number) => void;
    onClose: () => void;
  } = $props();

  let box = $state<HTMLDivElement>();
  $effect(() => {
    box?.querySelector<HTMLButtonElement>("button.go")?.focus();
  });

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="catcher" onclick={onClose}></div>
<div class="pop" role="dialog" aria-label={title} bind:this={box}>
  <span class="title">
    <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" /><path d="M8 7.2v3.6M8 5.1v.1" /></svg>
    {title}
  </span>
  <span class="body">
    {#each body as part, i (i)}
      {#if typeof part === "string"}{part}{:else if "branch" in part}<span class="chip" style:background={tint(part.color, "label")} style:color={plate(part.color)}>{part.branch}</span
        >{:else if "code" in part}<span class="chip mono">{part.code}</span>{:else}<span class="quote">“{part.quote}”</span>{/if}
    {/each}
  </span>
  {#if fixes.length}
    <span class="buttons">
      {#each fixes as fix, i (i)}
        <button class:go={i === fixes.length - 1} onclick={() => onFix(i)}>{fix.label}</button>
      {/each}
    </span>
  {/if}
</div>

<style>
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
    width: 300px;
    max-width: calc(100vw - 32px);
    padding: 14px 14px 12px;
    border-radius: 18px;
    background: var(--sheet);
    -webkit-backdrop-filter: blur(30px) saturate(1.8);
    backdrop-filter: blur(30px) saturate(1.8);
    border: 0.5px solid var(--glass-border);
    box-shadow: 0 16px 44px rgba(0, 0, 0, 0.22);
    display: flex;
    flex-direction: column;
    gap: 6px;
    text-align: left;
    color: var(--text);
  }
  .title {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 13px;
    font-weight: 600;
  }
  .title .icon {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    color: var(--text2);
  }
  .body {
    font-size: 12px;
    line-height: 1.45;
    color: var(--text2);
  }
  .chip {
    padding: 1px 6px;
    border-radius: 7px;
    background: var(--field);
    font-weight: 500;
    white-space: pre-wrap;
  }
  .chip.mono {
    font-family: var(--mono);
    font-weight: 400;
    font-size: 11px;
  }
  .quote {
    font-weight: 600;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    margin-top: 6px;
  }
  button {
    height: 26px;
    padding: 0 12px;
    border-radius: 13px;
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
    flex-shrink: 0;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    color: var(--text);
  }
  button.go {
    background: var(--accent);
    border-color: transparent;
    color: #fff;
    font-weight: 600;
  }
</style>
