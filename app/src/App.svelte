<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { api, settings } from "./lib/api";
  import type { History, RepoSummary } from "./lib/types";
  import Sidebar from "./lib/Sidebar.svelte";
  import HistoryList from "./lib/HistoryList.svelte";
  import CommitPanel from "./lib/CommitPanel.svelte";
  import Welcome from "./lib/Welcome.svelte";

  let repo = $state<RepoSummary | null>(null);
  let history = $state<History | null>(null);
  let selected = $state<string | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let list = $state<HistoryList>();
  let panelWidth = $state(640);

  const branchCount = $derived(history ? history.refs.filter((r) => r.kind === "local").length : 0);
  const rowsById = $derived(new Map(history?.rows.map((r) => [r.id, r]) ?? []));
  const selectedRow = $derived(selected ? (rowsById.get(selected) ?? null) : null);
  // Children of every commit, newest first (rows are already newest first).
  const childrenOf = $derived.by(() => {
    const map = new Map<string, string[]>();
    for (const r of history?.rows ?? []) {
      for (const parent of r.parents) {
        const list = map.get(parent);
        if (list) list.push(r.id);
        else map.set(parent, [r.id]);
      }
    }
    return map;
  });

  async function load(path: string) {
    loading = true;
    error = null;
    try {
      repo = await api.openRepo(path);
      history = await api.history();
      // Start on the checked-out commit, like the design: HEAD's latest commit is selected.
      selected = history.head.commit ?? history.rows[0]?.id ?? null;
      requestAnimationFrame(() => selected && list?.reveal(selected));
    } catch (err) {
      error = String(err);
    } finally {
      loading = false;
    }
  }

  async function chooseRepo() {
    const path = await open({ directory: true, multiple: false, title: "Open Repository" });
    if (typeof path === "string") await load(path);
  }

  function select(id: string) {
    selected = id;
    list?.reveal(id);
  }

  function startResize(event: PointerEvent) {
    const startX = event.clientX;
    const startWidth = panelWidth;
    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);
    const move = (e: PointerEvent) => {
      panelWidth = Math.min(Math.max(startWidth - (e.clientX - startX), 360), window.innerWidth - 560);
    };
    const up = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
      api.setSetting(settings.detailsWidth, Math.round(panelWidth)).catch(() => {});
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  }

  $effect(() => {
    api.getSetting<number>(settings.detailsWidth).then((width) => {
      if (typeof width === "number") panelWidth = Math.min(Math.max(width, 360), window.innerWidth - 560);
    });
    api.initialRepo().then((path) => {
      if (path) load(path);
    });
  });
</script>

{#if !repo || !history}
  <Welcome {loading} {error} onOpen={chooseRepo} />
{:else}
  <div class="window">
    <Sidebar {repo} {history} {selectedRow} onOpen={chooseRepo} onPick={select} />
    <div class="main">
      <header data-tauri-drag-region>
        <div class="title" data-tauri-drag-region>
          <span class="name">History</span>
          <span class="sub">{branchCount} {branchCount === 1 ? "branch" : "branches"} · {history.head.branch ?? "detached HEAD"}</span>
        </div>
        <span class="capsule" title="Checked-out branch">
          <svg class="icon" viewBox="0 0 16 16"><circle cx="4.5" cy="3.5" r="1.5" /><circle cx="4.5" cy="12.5" r="1.5" /><circle cx="11.5" cy="5.5" r="1.5" /><path d="M4.5 5v6M11.5 7c0 3-7 2-7 4" /></svg>
          <span>{history.head.branch ?? "Detached"}</span>
        </span>
        <span class="spacer" data-tauri-drag-region></span>
        {#if error}<span class="error" role="alert">{error}</span>{/if}
        <button class="capsule" onclick={() => repo && load(repo.path)} aria-label="Reload history" title="Reload">
          <svg class="icon" viewBox="0 0 16 16"><path d="M13 8a5 5 0 1 1-1.5-3.5M13 2.5V5h-2.5" /></svg>
        </button>
      </header>
      <div class="content">
        <section class="history" aria-label="Commit history">
          <div class="columns"><span>Description</span>{#if history.truncated}<span class="note">Showing the latest {history.rows.length.toLocaleString()} commits</span>{/if}</div>
          <HistoryList bind:this={list} {history} {selected} onSelect={select} />
        </section>
        <aside class="panel" style:width="{panelWidth}px" aria-label="Commit details">
          <button class="grip" onpointerdown={startResize} aria-label="Resize commit details" title="Drag to resize"></button>
          {#if selectedRow}
            <CommitPanel row={selectedRow} childIds={childrenOf.get(selectedRow.id) ?? []} lookup={(id) => rowsById.get(id)} onSelect={select} />
          {/if}
        </aside>
      </div>
    </div>
  </div>
{/if}

<style>
  .window {
    position: relative;
    height: 100%;
    color: var(--text);
  }
  .main {
    position: absolute;
    left: 264px;
    top: 0;
    right: 0;
    bottom: 0;
    display: flex;
    flex-direction: column;
  }
  header {
    height: 56px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 12px 0 14px;
    flex-shrink: 0;
  }
  .title {
    display: flex;
    flex-direction: column;
    line-height: 1.2;
    margin-right: 4px;
  }
  .name {
    font-size: 15px;
    font-weight: 700;
  }
  .sub {
    font-size: 11px;
    color: var(--text2);
  }
  .capsule {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 34px;
    padding: 0 12px;
    border-radius: 17px;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    color: var(--icon);
    font-weight: 500;
  }
  .capsule span {
    color: var(--text);
  }
  .spacer {
    flex-grow: 1;
    align-self: stretch;
  }
  .error {
    color: var(--red);
    font-size: 12px;
    max-width: 360px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .content {
    flex-grow: 1;
    display: flex;
    min-height: 0;
  }
  .history {
    flex-grow: 1;
    min-width: 0;
    background: var(--graph-bg);
    display: flex;
    flex-direction: column;
  }
  .columns {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 28px;
    flex-shrink: 0;
    padding: 0 18px 0 40px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
    border-bottom: 1px solid var(--sep);
  }
  .columns span:first-child {
    flex-grow: 1;
  }
  .note {
    font-weight: 400;
  }
  .panel {
    position: relative;
    flex-shrink: 0;
    border-left: 1px solid var(--sep);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .grip {
    position: absolute;
    left: -4px;
    top: 50%;
    width: 7px;
    height: 36px;
    margin-top: -18px;
    border-radius: 4px;
    background: var(--win);
    border: 1px solid var(--glass-border);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.08);
    z-index: 2;
    cursor: col-resize;
  }
</style>
