<script lang="ts">
  import { api } from "./api";
  import type { CommitDetail, FileDiff, HistoryRow } from "./types";
  import { fullDate, initials, lane, personColor, plate, shortId, splitPath, tint } from "./format";
  import DiffView from "./DiffView.svelte";

  let { row, onSelect }: { row: HistoryRow; onSelect: (id: string) => void } = $props();

  let detail = $state<CommitDetail | null>(null);
  let diffs = $state<FileDiff[]>([]);
  let tab = $state<string | null>(null); // null = Summary, otherwise a file path
  let whole = $state(false);
  let error = $state<string | null>(null);
  let scroller = $state<HTMLDivElement>();

  const color = $derived(row.graph.color);
  const totals = $derived(
    detail?.files.reduce((t, f) => ({ add: t.add + f.additions, del: t.del + f.deletions }), { add: 0, del: 0 }) ?? { add: 0, del: 0 },
  );

  // A new commit resets the tab; the diff mode is kept.
  $effect(() => {
    const id = row.id;
    tab = null;
    detail = null;
    error = null;
    api.commitDetail(id).then(
      (d) => {
        if (row.id === id) detail = d;
      },
      (err) => (error = String(err)),
    );
  });

  $effect(() => {
    const id = row.id;
    const path = tab;
    const wholeFile = whole;
    diffs = [];
    api.commitDiff(id, path, wholeFile).then(
      (d) => {
        if (row.id === id && tab === path && whole === wholeFile) diffs = d;
      },
      (err) => (error = String(err)),
    );
  });

  function openTab(path: string | null) {
    tab = path;
    if (scroller) scroller.scrollTop = 0;
  }

  const statusIcon: Record<string, string> = { added: "A", deleted: "D", modified: "M", renamed: "R", copied: "C" };
</script>

<div class="panel">
  <div class="head">
    <div class="message">
      <h1 class="selectable">{detail?.summary ?? row.summary}</h1>
      {#if detail?.body}<p class="body selectable">{detail.body}</p>{/if}
    </div>
    <div class="people">
      <span class="avatar" style:background={lane(personColor(row.authorEmail))}>{initials(row.authorName)}</span>
      <span class="who">
        <span class="author" title={row.authorEmail}>{row.authorName}</span>
        <span class="date">{detail ? fullDate(detail.author.time, detail.author.offset) : ""}</span>
      </span>
      <span class="key">commit</span>
      <span class="sha mono selectable" title={row.id}>{shortId(row.id)}</span>
      {#if row.parents.length}
        <span class="key">{row.parents.length === 1 ? "parent" : "parents"}</span>
        {#each row.parents as parent (parent)}
          <button class="sha mono link" onclick={() => onSelect(parent)} title="Go to {parent}">{shortId(parent)}</button>
        {/each}
      {/if}
    </div>
  </div>

  <div class="tabs" role="tablist" aria-label="Changes in this commit">
    <button
      role="tab"
      aria-selected={tab === null}
      class="tab"
      style:background={tab === null ? tint(color) : undefined}
      style:border-color={tab === null ? `color-mix(in srgb, ${lane(color)} 40%, transparent)` : undefined}
      style:color={tab === null ? plate(color) : undefined}
      class:on={tab === null}
      onclick={() => openTab(null)}>Summary</button
    >
    {#each detail?.files ?? [] as file (file.path)}
      <button role="tab" aria-selected={tab === file.path} class="tab neutral" class:on={tab === file.path} onclick={() => openTab(file.path)} title={file.path}
        >{splitPath(file.path).name}</button
      >
    {/each}
  </div>

  <div class="scroll" bind:this={scroller}>
    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}
    {#if tab === null && detail}
      <div class="files">
        <div class="count">
          <span class="grow">{detail.files.length} {detail.files.length === 1 ? "file" : "files"} changed</span>
          <span class="add">+{totals.add}</span>
          <span class="del">−{totals.del}</span>
        </div>
        {#each detail.files as file (file.path)}
          {@const p = splitPath(file.path)}
          <button class="file" onclick={() => openTab(file.path)}>
            <span class="badge {file.status}">{statusIcon[file.status]}</span>
            <span class="grow ellipsis"><span class="dir">{p.dir}</span>{p.name}</span>
            {#if file.binary}
              <span class="meta">binary</span>
            {:else}
              <span class="mono add">+{file.additions}</span>
              <span class="mono del num">−{file.deletions}</span>
            {/if}
          </button>
        {/each}
      </div>
    {/if}
    <DiffView {diffs} {whole} onToggleWhole={() => (whole = !whole)} />
  </div>
</div>

<style>
  .panel {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .head {
    padding: 14px 20px 16px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    border-bottom: 1px solid var(--sep);
  }
  .message {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  h1 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
    line-height: 1.3;
  }
  .body {
    margin: 0;
    color: var(--text2);
    line-height: 1.45;
    white-space: pre-wrap;
    max-height: 160px;
    overflow-y: auto;
  }
  .people {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .avatar {
    width: 30px;
    height: 30px;
    border-radius: 15px;
    color: #fff;
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 600;
    font-size: 12px;
    flex-shrink: 0;
  }
  .who {
    flex-grow: 1;
    display: flex;
    flex-direction: column;
    line-height: 1.25;
    min-width: 140px;
  }
  .author {
    font-weight: 600;
  }
  .date,
  .key {
    font-size: 11px;
    color: var(--text2);
  }
  .sha {
    font-size: 11.5px;
    padding: 2px 7px;
    border-radius: 6px;
    background: var(--field);
  }
  .link {
    color: var(--accent-text);
  }
  .tabs {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--sep);
    overflow-x: auto;
    flex-shrink: 0;
    scrollbar-width: none;
  }
  .tab {
    display: flex;
    align-items: center;
    height: 26px;
    padding: 0 11px;
    border-radius: 13px;
    border: 1px solid var(--sep);
    font-size: 12px;
    font-weight: 500;
    color: var(--text2);
    flex-shrink: 0;
  }
  .tab.on {
    font-weight: 600;
  }
  .tab.neutral.on {
    background: var(--side-sel);
    border-color: transparent;
    color: var(--text);
  }
  .scroll {
    flex-grow: 1;
    overflow-y: auto;
    min-height: 0;
  }
  .files {
    padding: 10px 12px 8px;
    border-bottom: 1px solid var(--sep);
    margin-bottom: 6px;
  }
  .count {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px 6px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
  }
  .file {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 26px;
    padding: 0 8px;
    border-radius: 7px;
  }
  .file:hover {
    background: var(--field);
  }
  .badge {
    width: 16px;
    height: 16px;
    border-radius: 5px;
    font-size: 10px;
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--field);
    color: var(--text2);
    flex-shrink: 0;
  }
  .badge.added {
    color: var(--green);
  }
  .badge.deleted {
    color: var(--red);
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
  .dir {
    color: var(--text2);
  }
  .add {
    color: var(--green);
    font-size: 11px;
  }
  .del {
    color: var(--red);
    font-size: 11px;
  }
  .num {
    min-width: 22px;
    text-align: right;
  }
  .meta {
    font-size: 11px;
    color: var(--text2);
  }
  .error {
    color: var(--red);
    margin: 12px 20px;
  }
</style>
