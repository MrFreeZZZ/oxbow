<script lang="ts">
  // Every change between the two sides of Compare, as one diff: Summary with the files, then a
  // tab per file, like the commit details.

  import { api } from "./api";
  import { diffSettings, wholeByDefault } from "./prefs.svelte";
  import type { Comparison, FileDiff } from "./types";
  import { lane, plate, shortId, splitPath, tabLabels, tint } from "./format";
  import DiffView from "./DiffView.svelte";

  let {
    comparison: c,
    base,
    target,
    color,
    onPickCommit,
  }: {
    comparison: Comparison;
    base: string;
    target: string;
    color: number;
    /** Show one commit of the compared side. */
    onPickCommit: (id: string) => void;
  } = $props();

  let tab = $state<string | null>(null);
  let whole = $state(wholeByDefault());
  let diffs = $state<FileDiff[]>([]);
  let error = $state<string | null>(null);
  let scroller = $state<HTMLDivElement>();

  const labels = $derived(tabLabels(c.files.map((f) => f.path)));
  const totals = $derived(c.files.reduce((t, f) => ({ add: t.add + f.additions, del: t.del + f.deletions }), { add: 0, del: 0 }));
  const authors = $derived([...new Set(c.ahead.map((a) => a.authorName))]);
  const commits = $derived(c.mode === "split" ? c.aheadCount : c.aheadCount + c.behindCount);

  // A new comparison starts on Summary.
  $effect(() => {
    c.from;
    c.targetId;
    tab = null;
  });

  $effect(() => {
    const from = c.from;
    const to = c.targetId;
    const path = tab;
    const wholeFile = whole;
    diffSettings();
    diffs = [];
    error = null;
    api.compareDiff(from, to, path, wholeFile).then(
      (d) => {
        if (c.from === from && c.targetId === to && tab === path && whole === wholeFile) diffs = d;
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
    <h1>
      {#if c.mode === "split"}
        What {target} changed since it split from {base}
      {:else}
        How {target} differs from {base}
      {/if}
    </h1>
    <p class="sub">
      {[
        authors.length ? `By ${authors.slice(0, 3).join(", ")}${authors.length > 3 ? ` and ${authors.length - 3} more` : ""}` : null,
        `${c.files.length} ${c.files.length === 1 ? "file" : "files"}`,
        `${commits} ${commits === 1 ? "commit" : "commits"}`,
      ]
        .filter(Boolean)
        .join(" · ")}
    </p>
    {#if c.mode === "tips" && c.behindCount}
      <p class="warn">
        Also shows the {c.behindCount} newer {c.behindCount === 1 ? "commit" : "commits"} on {base} undone, because the tips are compared directly.
        {#if c.mergeBase}Since Split leaves them out.{/if}
      </p>
    {/if}
  </div>

  <div class="tabs" role="tablist" aria-label="Changes">
    <button
      role="tab"
      aria-selected={tab === null}
      class="tab"
      class:on={tab === null}
      style:background={tab === null ? tint(color) : undefined}
      style:border-color={tab === null ? `color-mix(in srgb, ${lane(color)} 40%, transparent)` : undefined}
      style:color={tab === null ? plate(color) : undefined}
      onclick={() => openTab(null)}>Summary</button
    >
    {#if c.files.length}
      <span class="divider" aria-hidden="true"></span>
      <div class="file-tabs">
        {#each c.files as file (file.path)}
          <button role="tab" aria-selected={tab === file.path} class="tab neutral" class:on={tab === file.path} onclick={() => openTab(file.path)} title={file.path}>{labels.get(file.path)}</button>
        {/each}
      </div>
    {/if}
  </div>

  <div class="scroll" bind:this={scroller}>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if tab === null}
      {#if c.files.length}
        <div class="files">
          <div class="count">
            <span class="grow">{c.files.length} {c.files.length === 1 ? "file" : "files"} changed</span>
            <span class="add">+{totals.add}</span>
            <span class="del">−{totals.del}</span>
          </div>
          {#each c.files as file (file.path)}
            {@const p = splitPath(file.path)}
            <div class="file">
              <button class="open" onclick={() => openTab(file.path)}>
                <span class="badge {file.status}">{statusIcon[file.status]}</span>
                <span class="grow ellipsis"><span class="dir">{p.dir}</span>{p.name}</span>
              </button>
              {#if file.onlyBase}
                <span class="meta mono" title="Only {base}'s newer commits changed it">only on {base}</span>
              {:else if file.last}
                <button class="sha mono" onclick={() => onPickCommit(file.last!)} title="The latest commit on {target} that changed it">{shortId(file.last)}</button>
              {/if}
              {#if file.binary}
                <span class="meta">binary</span>
              {:else}
                <span class="mono add">+{file.additions}</span>
                <span class="mono del num">−{file.deletions}</span>
              {/if}
            </div>
          {/each}
        </div>
      {:else}
        <p class="empty">No differences: both sides have the same files.</p>
      {/if}
    {/if}
    <DiffView {diffs} {color} {whole} onToggleWhole={() => (whole = !whole)} />
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
    margin: 4px 12px 0;
    padding: 14px 16px 14px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    border: 1px solid var(--sep);
    border-radius: 12px;
  }
  h1 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
    line-height: 1.3;
  }
  .sub {
    margin: 0;
    color: var(--text2);
    font-size: 12px;
  }
  .warn {
    margin: 4px 0 0;
    padding: 8px 12px;
    border-radius: 9px;
    background: var(--orange-soft);
    color: var(--orange);
    font-size: 12px;
    line-height: 1.45;
  }
  .tabs {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 10px 0 10px 12px;
    flex-shrink: 0;
  }
  .divider {
    width: 1px;
    height: 16px;
    background: var(--sep);
    flex-shrink: 0;
  }
  .file-tabs {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    padding-right: 12px;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .file-tabs::-webkit-scrollbar {
    display: none;
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
    margin: 12px 12px 6px;
    padding: 8px 4px 6px;
    border-radius: 12px;
    border: 1px solid var(--sep);
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
    height: 26px;
    padding: 0 8px 0 0;
    border-radius: 7px;
  }
  .file:hover {
    background: var(--field);
  }
  .open {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-grow: 1;
    min-width: 0;
    height: 26px;
    padding-left: 8px;
    text-align: left;
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
  .mono {
    font-family: var(--mono);
  }
  .sha {
    font-size: 11px;
    color: var(--text2);
    padding: 1px 5px;
    border-radius: 5px;
  }
  .sha:hover {
    background: var(--side-sel);
    color: var(--text);
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
    white-space: nowrap;
  }
  .empty,
  .error {
    margin: 16px 20px;
    color: var(--text2);
  }
  .error {
    color: var(--red);
  }
</style>
