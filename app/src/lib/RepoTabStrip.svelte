<script lang="ts">
  import Menu, { menuIcons, type MenuEntry } from "./Menu.svelte";
  import type { RepoSessionId } from "./api";
  import { withKeys } from "./keys";
  import { repoTabActions, type OpenTab } from "./repoTabs";
  import { combo } from "./shortcuts";
  import { keyFocus } from "./tabs";
  import { tick } from "svelte";

  let { tabs, activeId, openMenu, openRecent, copyPath, activate, close, closeOthers }: {
    tabs: readonly OpenTab[];
    activeId: RepoSessionId;
    openMenu: (event: MouseEvent) => void;
    openRecent: () => void;
    copyPath: (path: string) => void;
    activate: (id: RepoSessionId) => Promise<void>;
    close: (id: RepoSessionId) => Promise<void>;
    closeOthers: (id: RepoSessionId) => Promise<void>;
  } = $props();

  let list = $state<HTMLDivElement>();
  let focused = $state<RepoSessionId | null>(null);
  let menu = $state<{ x: number; y: number; tab: OpenTab; anchor: HTMLButtonElement } | null>(null);
  const activeSelector = '[data-repo-tab-list] [role="tab"][aria-selected="true"]';

  $effect(() => {
    const node = list;
    const tab = node?.querySelector(`[data-tab-id="${activeId}"]`);
    if (!node || !tab) return;
    const reveal = () => tab.scrollIntoView({ block: "nearest", inline: "nearest" });
    reveal();
    const resize = new ResizeObserver(reveal);
    resize.observe(node);
    return () => resize.disconnect();
  });

  async function withTabFocus(action: Promise<void>) {
    await action;
    await tick();
    if (!document.hasFocus()) return;
    const tab = document.querySelector<HTMLButtonElement>(activeSelector);
    if (tab && (document.activeElement === document.body || list?.contains(document.activeElement))) keyFocus(tab);
  }

  function showContext(event: MouseEvent | KeyboardEvent, tab: OpenTab) {
    event.preventDefault();
    const anchor = event.currentTarget as HTMLButtonElement;
    const box = anchor.getBoundingClientRect();
    menu = event instanceof MouseEvent && (event.clientX || event.clientY)
      ? { x: event.clientX, y: event.clientY, tab, anchor }
      : { x: box.left, y: box.bottom + 4, tab, anchor };
  }

  function hideContext() {
    const anchor = menu?.anchor;
    menu = null;
    if (document.hasFocus() && document.activeElement?.closest('[role="menu"]')) anchor?.focus();
  }

  function contextEntries(tab: OpenTab): MenuEntry[] {
    const actions = repoTabActions(tab, tabs.length, {
      copyPath,
      close: (id) => void withTabFocus(close(id)),
      closeOthers: (id) => void withTabFocus(closeOthers(id)),
    });
    return [
      { kind: "item", icon: menuIcons.copy, ...actions.copy },
      { kind: "sep" },
      { kind: "item", icon: menuIcons.bad, ...actions.close },
      { kind: "item", icon: menuIcons.bad, ...actions.others },
    ];
  }

  function tabKey(event: KeyboardEvent, tab: OpenTab) {
    if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
      showContext(event, tab);
      return;
    }
    if (event.altKey || event.metaKey || event.ctrlKey || event.shiftKey) return;
    const at = tabs.indexOf(tab);
    const last = tabs.length - 1;
    const next = event.key === "ArrowLeft" ? (at + last) % tabs.length :
      event.key === "ArrowRight" ? (at + 1) % tabs.length :
      event.key === "Home" ? 0 : event.key === "End" ? last : -1;
    if (next < 0) return;
    event.preventDefault();
    const button = list?.querySelector<HTMLButtonElement>(`[data-tab-id="${tabs[next].id}"]`);
    if (button) keyFocus(button);
  }
</script>

<div class="strip" data-tauri-drag-region>
  <button class="control repository" onclick={openMenu} aria-label="Repository menu" aria-haspopup="menu" title="Repository menu">
    <svg class="icon" viewBox="0 0 16 16"><path d="M4 6l4 4 4-4" /></svg>
  </button>
  <div class="list" data-repo-tab-list role="tablist" aria-label="Repository tabs" bind:this={list}
    style:--tab-count={tabs.length}
    onfocusout={(event) => { if (!list?.contains(event.relatedTarget as Node | null)) focused = null; }}>
    {#each tabs as tab (tab.id)}
      {@const selected = tab.id === activeId}
      <div class="item" class:selected role="presentation">
        <button class="tab-button" role="tab" aria-selected={selected} aria-controls={`repo-panel-${tab.id}`}
          tabindex={tab.id === (focused ?? activeId) ? 0 : -1} data-tab-id={tab.id} title={tab.repo.path}
          onclick={(event) => {
            const picking = activate(tab.id);
            if (event.detail === 0) void withTabFocus(picking);
          }}
          onfocus={() => focused = tab.id} onkeydown={(event) => tabKey(event, tab)}
          oncontextmenu={(event) => showContext(event, tab)}>
          <svg class="icon folder" viewBox="0 0 16 16"><path d={menuIcons.folder} /></svg>
          <span>{tab.repo.name}</span>
        </button>
        <button class="control close" tabindex={selected ? 0 : -1} aria-label={`Close ${tab.repo.name} tab`}
          title={`Close ${tab.repo.name} tab`} oncontextmenu={(event) => showContext(event, tab)}
          onclick={(event) => {
            const closing = close(tab.id);
            if (event.detail === 0) void withTabFocus(closing);
          }}>
          <svg class="icon" viewBox="0 0 16 16"><path d={menuIcons.bad} /></svg>
        </button>
      </div>
    {/each}
  </div>
  <button class="control add" onclick={openRecent} aria-label="Open recent repository" aria-haspopup="dialog"
    title={withKeys("Open Recent…", combo("openRecent"))}>
    <svg class="icon" viewBox="0 0 16 16"><path d="M8 3v10M3 8h10" /></svg>
  </button>
</div>
{#if menu}
  <Menu x={menu.x} y={menu.y} label={menu.tab.repo.name} entries={contextEntries(menu.tab)} onClose={hideContext} />
{/if}

<style>
  .strip {
    --track: color-mix(in srgb, var(--accent) 15%, var(--win));
    --tab-width: 166px;
    --curve: var(--radius-m);
    height: var(--repo-tabs-height, 40px);
    display: flex;
    align-items: flex-end;
    gap: var(--space-1);
    padding-inline: var(--repo-tabs-leading) var(--space-2);
    background: var(--track);
    box-shadow: inset 0 -1px var(--sep);
  }
  .list {
    display: flex;
    width: calc(var(--tab-count) * var(--tab-width));
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .item {
    position: relative;
    flex: 1 0 128px;
    min-width: 0;
    height: 34px;
    display: flex;
    align-items: center;
    padding-inline: var(--space-2);
  }
  .item:not(.selected):not(:has(+ .selected))::after {
    content: "";
    position: absolute;
    right: 0;
    height: 15px;
    width: 1px;
    background: color-mix(in srgb, var(--text2) 26%, transparent);
    pointer-events: none;
  }
  .item.selected::before {
    content: "";
    position: absolute;
    inset: 0 var(--curve);
    border-radius: var(--radius-l) var(--radius-l) 0 0;
    background: var(--win);
    pointer-events: none;
  }
  .item.selected::after {
    content: "";
    position: absolute;
    inset: auto 0 0;
    height: var(--curve);
    background:
      radial-gradient(circle at 0 0, transparent calc(var(--curve) - .5px), var(--win) var(--curve)) left / var(--curve) 100% no-repeat,
      radial-gradient(circle at 100% 0, transparent calc(var(--curve) - .5px), var(--win) var(--curve)) right / var(--curve) 100% no-repeat;
    pointer-events: none;
  }
  .item:not(.selected)::before {
    content: "";
    position: absolute;
    inset: 3px var(--space-1);
    border-radius: var(--radius-m);
    pointer-events: none;
  }
  .item:not(.selected):hover::before {
    background: var(--hover-wash);
  }
  .item:not(.selected):has(.tab-button:active)::before {
    background: var(--press-wash);
  }
  .item:not(.selected):has(.tab-button:is(:focus-visible, :global(.key-focus)))::before {
    box-shadow: var(--focus-ring);
  }
  .tab-button,
  .control {
    position: relative;
    display: flex;
    align-items: center;
    border: 0;
    border-radius: var(--radius-s);
    background-color: transparent;
    color: var(--text2);
    font: inherit;
  }
  .control:hover {
    background-image: linear-gradient(var(--hover-wash), var(--hover-wash));
  }
  .control:active {
    background-image: linear-gradient(var(--press-wash), var(--press-wash));
  }
  .tab-button {
    align-self: stretch;
    flex: 1;
    min-width: 0;
    gap: var(--space-2);
    padding: 0 var(--space-1) 0 var(--space-2);
    color: var(--text);
    text-align: left;
  }
  .tab-button:is(:focus-visible, :global(.key-focus)) {
    outline-style: none;
  }
  .selected .tab-button:is(:focus-visible, :global(.key-focus)) span {
    text-decoration: underline;
    text-underline-offset: 3px;
  }
  .tab-button span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .folder {
    flex: 0 0 16px;
    color: var(--text2);
  }
  .selected .tab-button { font-weight: 500; }
  .selected .folder { color: var(--accent); }
  .control {
    flex: none;
    justify-content: center;
    padding: 0;
    width: 28px;
    height: 28px;
    margin-bottom: 4px;
  }
  .repository {
    background-color: color-mix(in srgb, var(--accent) 11%, var(--track));
    color: var(--text);
    border-radius: var(--radius-m);
  }
  .close {
    width: 24px;
    height: 24px;
    margin: 0;
    padding: var(--space-1);
    border-radius: var(--radius-pill);
    background-clip: content-box;
  }
  .close .icon { width: 12px; height: 12px; }
  .selected .close, .add { color: var(--text); }
</style>
