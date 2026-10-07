<script lang="ts" module>
  export type MenuEntry =
    | { kind: "item"; label: string; icon: string; danger?: boolean; run: () => void }
    /** Opens a submenu to the side, e.g. Reset ▸ Soft / Mixed / Hard. */
    | { kind: "sub"; label: string; icon: string; entries: SubEntry[] }
    | { kind: "header"; label: string }
    | { kind: "note"; label: string }
    | { kind: "sep" };

  /** An item of a submenu: a label, and a grey hint on the right. */
  export type SubEntry = { label: string; hint?: string; danger?: boolean; run: () => void };

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
    cherry: "M3 12a2 2 0 1 0 4 0a2 2 0 1 0-4 0M9 11a2 2 0 1 0 4 0a2 2 0 1 0-4 0M5 10c.5-4 3-6.5 6-8M11 9c-.5-2.5-.5-5 0-7",
    undo: "M3 6.5h7a3.5 3.5 0 0 1 0 7H6M5.5 4 3 6.5 5.5 9",
    revert: "M12.5 8a4.5 4.5 0 1 1-1.3-3.2M11.5 2v3h-3",
    reset: "M3.5 8a4.5 4.5 0 1 0 1.3-3.2M4.5 2v3h3",
    stash: "M2.5 3.5h11v3h-11zM3.5 6.5v6h9v-6M6.5 9h3",
    tag: "M2.5 2.5h5l6 6-5 5-6-6zM5.5 4.7a.8.8 0 1 0 0 1.6a.8.8 0 1 0 0-1.6",
    compare: "M2.5 5.5h9M9 3l2.5 2.5L9 8M13.5 10.5h-9M7 8l-2.5 2.5L7 13",
    fetch: "M8 3v9M4.5 8.5 8 12l3.5-3.5M3 14h10",
    pop: "M2.5 8.5v4.5h11V8.5M8 10.5V2.5M5.5 5 8 2.5 10.5 5",
  };
</script>

<script lang="ts">
  // A context menu at the pointer, in the glass style of the design.

  let { x, y, label, entries, onClose }: { x: number; y: number; label: string; entries: MenuEntry[]; onClose: () => void } = $props();

  let box = $state<HTMLDivElement>();
  let left = $state(0);
  let top = $state(0);
  /** The open submenu: its entry, and the item it hangs off. */
  let sub = $state<{ index: number; item: HTMLElement } | null>(null);
  let subBox = $state<HTMLDivElement>();
  let subLeft = $state(0);
  let subTop = $state(0);

  // Open at the pointer, kept inside the window.
  $effect(() => {
    if (!box) return;
    const { width, height } = box.getBoundingClientRect();
    left = Math.max(8, Math.min(x, window.innerWidth - width - 8));
    top = Math.max(8, Math.min(y, window.innerHeight - height - 8));
    box.querySelector<HTMLButtonElement>("button")?.focus();
  });

  // Beside the menu, on the right unless there is no room there.
  $effect(() => {
    if (!sub || !subBox || !box) return;
    const menu = box.getBoundingClientRect();
    const item = sub.item.getBoundingClientRect();
    const { width, height } = subBox.getBoundingClientRect();
    subLeft = menu.right - 4 + width > window.innerWidth - 8 ? menu.left + 4 - width : menu.right - 4;
    subTop = Math.max(8, Math.min(item.top - 5, window.innerHeight - height - 8));
  });

  function openSub(index: number, item: HTMLElement, focus = false) {
    sub = { index, item };
    if (focus) requestAnimationFrame(() => subBox?.querySelector<HTMLButtonElement>("button")?.focus());
  }

  function choose(run: () => void) {
    onClose();
    run();
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    } else if (event.key === "ArrowRight" && document.activeElement instanceof HTMLElement && document.activeElement.dataset.sub) {
      event.preventDefault();
      openSub(Number(document.activeElement.dataset.sub), document.activeElement, true);
    } else if (event.key === "ArrowLeft" && sub && subBox?.contains(document.activeElement)) {
      event.preventDefault();
      sub.item.focus();
      sub = null;
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const inSub = !!subBox?.contains(document.activeElement);
      const items = [...((inSub ? subBox : box)?.querySelectorAll<HTMLButtonElement>("button") ?? [])];
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
    {:else if entry.kind === "sub"}
      <button
        role="menuitem"
        aria-haspopup="menu"
        aria-expanded={sub?.index === i}
        class:open={sub?.index === i}
        data-sub={i}
        onmouseenter={(e) => openSub(i, e.currentTarget)}
        onclick={(e) => openSub(i, e.currentTarget, true)}
      >
        <svg class="icon" viewBox="0 0 16 16"><path d={entry.icon} /></svg>
        <span>{entry.label}</span>
        <svg class="icon chevron" viewBox="0 0 16 16"><path d="M6 3.5 10.5 8 6 12.5" /></svg>
      </button>
    {:else}
      {@const run = entry.run}
      <button role="menuitem" class:danger={entry.danger} onmouseenter={() => (sub = null)} onclick={() => choose(run)}>
        <svg class="icon" viewBox="0 0 16 16"><path d={entry.icon} /></svg>
        <span>{entry.label}</span>
      </button>
    {/if}
  {/each}
</div>
{#if sub}
  {@const parent = entries[sub.index]}
  {#if parent?.kind === "sub"}
    <div class="menu submenu" role="menu" aria-label={parent.label} bind:this={subBox} style:left="{subLeft}px" style:top="{subTop}px">
      {#each parent.entries as entry (entry.label)}
        <button role="menuitem" class:danger={entry.danger} onclick={() => choose(entry.run)}>
          <span>{entry.label}</span>
          {#if entry.hint}<span class="hint">{entry.hint}</span>{/if}
        </button>
      {/each}
    </div>
  {/if}
{/if}

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
  button:focus-visible,
  button.open {
    background: var(--side-sel);
  }
  button .chevron {
    width: 12px;
    height: 12px;
    flex-shrink: 0;
    color: var(--text2);
  }
  .submenu {
    min-width: 0;
  }
  .submenu button {
    gap: 28px;
  }
  .submenu .hint {
    flex-grow: 0;
    font-size: 12px;
    color: var(--text2);
  }
  button.danger,
  button.danger .icon {
    color: var(--red);
  }
</style>
