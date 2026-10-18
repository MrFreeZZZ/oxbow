<script lang="ts">
  // Fix a Mistake: what happened, in the words people search for. Picking a row opens that
  // fix's sheet, planned from the repository as it is; a row that can't be used says why.

  import type { Part } from "./confirm.svelte";
  import type { FixChoice, FixId } from "./fixes";
  import { actionIcons as icons } from "./icons";
  import { plate, tint } from "./format";
  import { withKeys } from "./keys";

  let { choices, summary, onPick, onClose }: { choices: FixChoice[]; summary: Part[]; onPick: (id: FixId) => void; onClose: () => void } = $props();

  let box = $state<HTMLDivElement>();
  $effect(() => {
    box?.querySelector<HTMLButtonElement>("button.fix:not(:disabled)")?.focus();
  });

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const rows = [...(box?.querySelectorAll<HTMLButtonElement>("button.fix:not(:disabled)") ?? [])];
    if (!rows.length) return;
    event.preventDefault();
    const at = rows.indexOf(document.activeElement as HTMLButtonElement);
    const next = event.key === "ArrowDown" ? (at + 1) % rows.length : (at - 1 + rows.length) % rows.length;
    rows[next].focus();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="dim">
  <div class="sheet" role="dialog" aria-modal="true" aria-labelledby="fix-title" bind:this={box}>
    <div class="top">
      <span class="tile"><svg class="icon" viewBox="0 0 16 16"><path d={icons.fix} /></svg></span>
      <div class="words">
        <span class="title" id="fix-title">Fix a Mistake</span>
        <span class="body">Pick what happened. Oxbow looks at your repository and shows the steps, with their git commands, before anything runs.</span>
        <span class="state">
          {#each summary as part, i (i)}
            {#if typeof part === "string"}{part}{:else if "branch" in part}<span class="chip" style:background={tint(part.color, "label")} style:color={plate(part.color)}>{part.branch}</span
              >{:else if "code" in part}<span class="chip mono">{part.code}</span>{:else}“{part.quote}”{/if}
          {/each}
        </span>
      </div>
    </div>
    <div class="fixes">
      {#each choices as choice (choice.id)}
        <button class="fix" class:off={!!choice.off} disabled={!!choice.off} onclick={() => onPick(choice.id)}>
          <span class="fix-tile"><svg class="icon" viewBox="0 0 16 16"><path d={icons[choice.icon]} /></svg></span>
          <span class="fix-words">
            <span class="fix-title">{choice.title}</span>
            {#if choice.off}
              <span class="fix-sub why"><svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="5.75" /><path d="M8 7.2v3.6M8 5.2v.01" /></svg>{choice.off}</span>
            {:else}
              <span class="fix-sub">{choice.sub}</span>
            {/if}
          </span>
          <svg class="icon chevron" viewBox="0 0 16 16"><path d="M6 3.5 10.5 8 6 12.5" /></svg>
        </button>
      {/each}
    </div>
    <div class="back">
      <svg class="icon" viewBox="0 0 16 16"><path d={icons.undo} /></svg>
      <span>Every fix goes into the Operation Log, so Undo can take it back.</span>
    </div>
    <div class="foot">
      <button class="button" onclick={onClose} title={withKeys("Cancel", "Escape")}>Cancel</button>
    </div>
  </div>
</div>

<style>
  .dim {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: var(--dim);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .sheet {
    width: min(620px, calc(100vw - 48px));
    max-height: calc(100vh - 48px);
    overflow-y: auto;
    padding: 22px 22px 18px;
    border-radius: var(--radius-sheet);
    background: var(--sheet);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--shadow-sheet);
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .top {
    display: flex;
    gap: 14px;
    align-items: flex-start;
  }
  .tile {
    width: 44px;
    height: 44px;
    flex-shrink: 0;
    border-radius: var(--radius-l);
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--lane-0) var(--tint-label), transparent);
    color: var(--plate-0);
  }
  .tile .icon {
    width: 22px;
    height: 22px;
    stroke-width: 1.4;
  }
  .words {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .title {
    font-size: var(--text-heading);
    font-weight: 700;
    line-height: 1.3;
  }
  .body {
    line-height: 1.75;
  }
  .state {
    font-size: var(--text-small);
    line-height: 1.7;
    color: var(--text2);
  }
  .chip {
    padding: 1px 6px;
    border-radius: var(--radius-s);
    background: var(--field);
    font-weight: 500;
  }
  .chip.mono {
    font-family: var(--mono);
    font-weight: 400;
  }
  .fixes {
    border-radius: var(--radius-l);
    background: var(--panel);
    border: 0.5px solid var(--panel-border);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .fix {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 10px 12px;
    white-space: normal;
  }
  .fix + .fix {
    border-top: 0.5px solid var(--sep);
  }
  .fix:not(:disabled):hover,
  .fix:focus-visible {
    background: color-mix(in srgb, var(--accent) 9%, transparent);
    outline: none;
  }
  .fix-tile {
    width: 30px;
    height: 30px;
    flex-shrink: 0;
    border-radius: var(--radius-m);
    display: grid;
    place-items: center;
    background: color-mix(in srgb, var(--lane-0) var(--tint-label), transparent);
    color: var(--plate-0);
  }
  .fix-words {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .fix-title {
    font-weight: 600;
  }
  .fix-sub {
    font-size: var(--text-small);
    line-height: 1.4;
    color: var(--text2);
  }
  .fix-sub.why {
    display: flex;
    gap: 5px;
    align-items: flex-start;
  }
  .fix-sub.why .icon {
    width: 13px;
    height: 13px;
    flex-shrink: 0;
    margin-top: 1px;
  }
  .chevron {
    flex-shrink: 0;
    color: var(--text2);
  }
  .fix.off .fix-tile {
    background: var(--field);
    color: var(--text2);
  }
  .fix.off .fix-title {
    color: var(--text2);
  }
  .fix.off .chevron {
    visibility: hidden;
  }
  .back {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 0 2px;
    font-size: var(--text-small);
    line-height: 1.4;
    color: var(--text2);
  }
  .back .icon {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    margin-top: 1px;
  }
  .foot {
    display: flex;
    justify-content: flex-end;
    padding-top: 2px;
  }

  .button {
    min-width: 96px;
    padding: 0 16px;
  }
</style>
