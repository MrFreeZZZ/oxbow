<script lang="ts">
  import type { History, HistoryRow, RefInfo, RepoSummary } from "./types";
  import { lane, tint } from "./format";

  let {
    repo,
    history,
    selectedRow,
    onOpen,
    onPick,
  }: {
    repo: RepoSummary;
    history: History;
    selectedRow: HistoryRow | null;
    onOpen: () => void;
    onPick: (commit: string) => void;
  } = $props();

  /** Lists longer than this show only their first items until expanded. */
  const COLLAPSED = 10;

  // The ref clicked last, while its commit is still the selected one.
  let clicked = $state<RefInfo | null>(null);
  let expanded = $state<Record<string, boolean>>({});
  let list = $state<HTMLDivElement>();

  // Keep the highlighted item in view when the selection moves to another branch.
  $effect(() => {
    const name = focused;
    if (!name || !list) return;
    const item = [...list.querySelectorAll<HTMLElement>("[data-ref]")].find((el) => el.dataset.ref === name);
    item?.scrollIntoView({ block: "nearest" });
  });

  /** The branch or tag highlighted in the sidebar follows the selected commit. */
  const focused = $derived(
    clicked && clicked.target === selectedRow?.id ? clicked.name : (selectedRow?.graph.branch ?? null),
  );

  const rowOf = $derived(new Map(history.rows.map((row) => [row.id, row])));

  /** Local branches: the checked-out one first, the rest by latest commit, newest first. */
  const branches = $derived(
    history.refs
      .filter((r) => r.kind === "local")
      .map((r) => ({ ref: r, time: rowOf.get(r.target)?.time ?? 0, color: rowOf.get(r.target)?.graph.color ?? 0 }))
      .sort((a, b) => {
        const ha = a.ref.name === history.head.branch ? 1 : 0;
        const hb = b.ref.name === history.head.branch ? 1 : 0;
        return hb - ha || b.time - a.time || a.ref.name.localeCompare(b.ref.name);
      }),
  );
  const tags = $derived(
    history.refs
      .filter((r) => r.kind === "tag")
      .map((r) => ({ ref: r, time: rowOf.get(r.target)?.time ?? 0 }))
      .sort((a, b) => b.time - a.time || b.ref.name.localeCompare(a.ref.name)),
  );

  /** Select the branch's or tag's latest commit, as if it were clicked in the graph. */
  function focus(r: RefInfo) {
    clicked = r;
    onPick(r.target);
  }

  /** The first items of a list, plus the highlighted one if it would be hidden. */
  function shown<T extends { ref: RefInfo }>(key: string, items: T[]): T[] {
    if (expanded[key] || items.length <= COLLAPSED) return items;
    const head = items.slice(0, COLLAPSED);
    const current = items.slice(COLLAPSED).find((item) => item.ref.name === focused);
    return current ? [...head, current] : head;
  }
</script>

{#snippet more(key: string, count: number)}
  {#if count > COLLAPSED}
    <button class="more" onclick={() => (expanded[key] = !expanded[key])} aria-expanded={!!expanded[key]}>
      <svg class="icon small" viewBox="0 0 16 16"><path d={expanded[key] ? "M4.5 10 8 6.5l3.5 3.5" : "M4.5 6 8 9.5 11.5 6"} /></svg>
      {expanded[key] ? "Show less" : `Show all ${count}`}
    </button>
  {/if}
{/snippet}

<nav aria-label="Sidebar">
  <div class="lights" data-tauri-drag-region></div>

  <button class="repo" onclick={onOpen} title="Open another repository">
    <svg class="icon" viewBox="0 0 16 16"><path d="M2.5 4.5a1 1 0 0 1 1-1h3l1.5 1.5h4.5a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1z" /></svg>
    <span class="repo-text">
      <span class="repo-name">{repo.name}</span>
      <span class="repo-path">{repo.path}</span>
    </span>
    <svg class="icon small" viewBox="0 0 16 16"><path d="M5 6l3-3 3 3M5 10l3 3 3-3" /></svg>
  </button>

  <div class="scroll" bind:this={list}>
    <div class="heading">Workspace</div>
    <div class="item current">
      <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" /><path d="M8 4.5V8l2.5 1.5" /></svg>
      <span class="grow">History</span>
    </div>

    <div class="heading">Branches</div>
    {#each shown("branches", branches) as b (b.ref.name)}
      {@const head = b.ref.name === history.head.branch}
      <button
        class="item"
        data-ref={b.ref.name}
        style:background={focused === b.ref.name ? tint(b.color) : undefined}
        onclick={() => focus(b.ref)}
        title="Go to the latest commit on {b.ref.name}"
      >
        <span class="dot" style:background={lane(b.color)}></span>
        <span class="grow ellipsis" class:bold={head}>{b.ref.name}</span>
        {#if head}<span class="head">HEAD</span>{/if}
      </button>
    {/each}
    {@render more("branches", branches.length)}

    {#if history.remotes.length}
      <div class="heading">Remotes</div>
      {#each history.remotes as remote (remote)}
        {@const count = history.refs.filter((r) => r.kind === "remote" && r.remote === remote).length}
        <div class="item">
          <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" /><path d="M2 8h12M8 2c2 2 2 10 0 12M8 2c-2 2-2 10 0 12" /></svg>
          <span class="grow">{remote}</span>
          <span class="meta">{count} {count === 1 ? "branch" : "branches"}</span>
        </div>
      {/each}
    {/if}

    {#if tags.length}
      <div class="heading">Tags</div>
      {#each shown("tags", tags) as t (t.ref.name)}
        <button
          class="item"
          data-ref={t.ref.name}
          style:background={focused === t.ref.name ? "var(--side-sel)" : undefined}
          onclick={() => focus(t.ref)}
          title="Go to {t.ref.name}"
        >
          <svg class="icon" viewBox="0 0 16 16"><path d="M2.5 2.5h5l6 6-5 5-6-6z" /><circle cx="5.5" cy="5.5" r="0.8" /></svg>
          <span class="grow ellipsis">{t.ref.name}</span>
        </button>
      {/each}
      {@render more("tags", tags.length)}
    {/if}
  </div>
</nav>

<style>
  nav {
    position: absolute;
    left: 8px;
    top: 8px;
    bottom: 8px;
    width: 248px;
    border-radius: 18px;
    background: var(--side);
    border: 0.5px solid var(--side-border);
    box-shadow: var(--panel-shadow);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .lights {
    height: 44px;
    flex-shrink: 0;
  }
  .repo {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    margin: 0 8px 6px;
    padding: 0 10px;
    border-radius: 10px;
    background: var(--field);
    color: var(--icon);
    flex-shrink: 0;
  }
  .repo-text {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.2;
  }
  .repo-name {
    font-weight: 600;
    color: var(--text);
  }
  .repo-path {
    font-size: 11px;
    color: var(--text2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .small {
    width: 14px;
    height: 14px;
  }
  .scroll {
    flex-grow: 1;
    overflow-y: auto;
    padding-bottom: 10px;
  }
  .heading {
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
    padding: 14px 18px 4px;
  }
  .heading:first-child {
    padding-top: 8px;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: calc(100% - 16px);
    height: 28px;
    margin: 0 8px;
    padding: 0 10px;
    border-radius: 8px;
    color: var(--icon);
  }
  .item .grow {
    color: var(--text);
  }
  .item.current {
    background: var(--side-sel);
    font-weight: 600;
  }
  .grow {
    flex-grow: 1;
    min-width: 0;
  }
  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bold {
    font-weight: 600;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 4px;
    margin: 0 4px;
    flex-shrink: 0;
  }
  .head {
    font-size: 10px;
    font-weight: 700;
    color: var(--accent-text);
  }
  .meta {
    font-size: 11px;
    color: var(--text2);
  }
  .more {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    margin: 2px 8px 0;
    padding: 0 10px;
    border-radius: 8px;
    font-size: 12px;
    color: var(--text2);
  }
  .more:hover {
    color: var(--text);
  }
</style>
