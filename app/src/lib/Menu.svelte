<script lang="ts" module>
  export type MenuEntry =
    | { kind: "item"; label: string; icon: string; danger?: boolean; run: () => void }
    | { kind: "header"; label: string }
    | { kind: "note"; label: string }
    | { kind: "sep" };

  /** Icon paths of menu items, from the Branches design. */
  export const menuIcons = {
    checkout: "M2.5 8h8M7.5 4.5 11 8l-3.5 3.5M13.5 3v10",
    branch: "M3 3.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0-3 0M3 12.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0-3 0M10 5.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0-3 0M4.5 5v6M11.5 7c0 3-7 2-7 4",
    edit: "M10.5 2.5l3 3L6 13H3v-3z",
    drop: "M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.7 9h5.6l.7-9",
    copy: "M5.5 5.5h7v8h-7zM3.5 10.5v-8h7",
    push: "M8 12V3M4.5 6.5 8 3l3.5 3.5M3 14h10",
    merge: "M4.5 2v12M4.5 4.5c0 3.5 7 2.5 7 6v3.5M2.5 12 4.5 14l2-2",
    rebase: "M4.5 14V7M4.5 7c0-3 7-1 7-5M11.5 2v12M9.5 4 11.5 2l2 2",
  };
</script>

<script lang="ts">
  // A context menu at the pointer, in the glass style of the design.

  let { x, y, label, entries, onClose }: { x: number; y: number; label: string; entries: MenuEntry[]; onClose: () => void } = $props();

  let box = $state<HTMLDivElement>();
  let left = $state(0);
  let top = $state(0);

  // Open at the pointer, kept inside the window.
  $effect(() => {
    if (!box) return;
    const { width, height } = box.getBoundingClientRect();
    left = Math.max(8, Math.min(x, window.innerWidth - width - 8));
    top = Math.max(8, Math.min(y, window.innerHeight - height - 8));
    box.querySelector<HTMLButtonElement>("button")?.focus();
  });

  function choose(run: () => void) {
    onClose();
    run();
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const items = [...(box?.querySelectorAll<HTMLButtonElement>("button") ?? [])];
      const at = items.indexOf(document.activeElement as HTMLButtonElement);
      const next = event.key === "ArrowDown" ? (at + 1) % items.length : (at - 1 + items.length) % items.length;
      items[next]?.focus();
    }
  }
</script>

<svelte:window onkeydown={onKey} onblur={onClose} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="catcher"
  onclick={onClose}
  oncontextmenu={(e) => {
    e.preventDefault();
    onClose();
  }}
></div>
<div class="menu" role="menu" aria-label={label} bind:this={box} style:left="{left}px" style:top="{top}px">
  {#each entries as entry, i (i)}
    {#if entry.kind === "header"}
      <div class="header">{entry.label}</div>
    {:else if entry.kind === "sep"}
      <div class="sep"></div>
    {:else if entry.kind === "note"}
      <div class="note">{entry.label}</div>
    {:else}
      {@const run = entry.run}
      <button role="menuitem" class:danger={entry.danger} onclick={() => choose(run)}>
        <svg class="icon" viewBox="0 0 16 16"><path d={entry.icon} /></svg>
        <span>{entry.label}</span>
      </button>
    {/if}
  {/each}
</div>

<style>
  .catcher {
    position: fixed;
    inset: 0;
    z-index: 30;
  }
  .menu {
    position: fixed;
    z-index: 31;
    width: max-content;
    min-width: 230px;
    max-width: 380px;
    padding: 5px;
    border-radius: 14px;
    background: var(--sheet);
    -webkit-backdrop-filter: blur(30px) saturate(1.8);
    backdrop-filter: blur(30px) saturate(1.8);
    border: 0.5px solid var(--glass-border);
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.18);
    display: flex;
    flex-direction: column;
  }
  .header {
    height: 24px;
    padding: 0 10px;
    font-size: 11px;
    font-weight: 600;
    line-height: 24px;
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sep {
    height: 1px;
    margin: 5px 10px;
    background: var(--sep);
  }
  .note {
    padding: 1px 10px 4px 34px;
    font-size: 11px;
    line-height: 1.35;
    color: var(--text2);
  }
  button {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 26px;
    padding: 0 10px;
    border-radius: 8px;
    color: var(--text);
    text-align: left;
    outline: none;
  }
  button .icon {
    width: 15px;
    height: 15px;
    color: var(--icon);
  }
  button span {
    flex-grow: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  button:hover,
  button:focus-visible {
    background: var(--side-sel);
  }
  button.danger,
  button.danger .icon {
    color: var(--red);
  }
</style>
