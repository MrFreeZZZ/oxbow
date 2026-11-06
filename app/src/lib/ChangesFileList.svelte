<script lang="ts">
  import type { Snippet } from "svelte";
  import type { FileChange } from "./types";
  import type { ListKey } from "./changelists";
  import { keysTo } from "./changesList";
  import ChangesCheckbox from "./ChangesCheckbox.svelte";

  let { files, group, folders, folded, cut, limit, step, ticks, row, onFold, onTickAll, onMore }: {
    files: FileChange[]; group: ListKey | "conflicted"; folders: boolean; folded: Set<string>;
    cut: { folders: { folder: { dir: string; files: FileChange[] }; room: number }[]; hidden: number } | null;
    limit: number; step: number; ticks: Set<string>; row: Snippet<[FileChange, boolean]>;
    onFold: (key: string) => void; onTickAll: (paths: string[]) => void; onMore: () => void;
  } = $props();
</script>

{#snippet moreRows(left: number)}
  <button class="more-rows" onclick={onMore}
    >Show {Math.min(left, step).toLocaleString()} more {Math.min(left, step) === 1 ? "file" : "files"}</button
  >
{/snippet}

  {#if folders && cut}
    {#each cut.folders as { folder, room } (folder.dir)}
      {@const key = `${group}:${folder.dir}`}
      {@const open = !folded.has(key)}
      {#if room > 0}
      <div class="folder">
        {#if group !== "conflicted"}<ChangesCheckbox on={folder.files.length > 0 && folder.files.every((f) => ticks.has(f.path))}
          some={folder.files.some((f) => ticks.has(f.path)) && !folder.files.every((f) => ticks.has(f.path))}
          label={`Select the files in ${folder.dir || "the top folder"}`} title={folder.files.length > 0 && folder.files.every((f) => ticks.has(f.path)) ? "Select none" : "Select all"}
          disabled={!folder.files.length} onclick={(e) => { keysTo(e, ".fold"); onTickAll(folder.files.map((f) => f.path)); }} />{/if}
        <button class="fold" aria-expanded={open} onclick={() => onFold(key)} title={folder.dir || "The repository's top folder"}>
          <svg class="chevron" class:open viewBox="0 0 16 16"><path d="M6 4l4 4-4 4" /></svg>
          <svg class="icon folder-icon" viewBox="0 0 16 16"><path d="M1.5 4.5a1 1 0 0 1 1-1H6l1.5 1.5h6a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" /></svg>
          <span class="dirname">{folder.dir ? folder.dir.slice(0, -1) : "Top folder"}</span>
          <span class="n">{folder.files.length}</span>
        </button>
      </div>
      {#if open}{#each folder.files.slice(0, room) as file (file.path)}{@render row(file, true)}{/each}{/if}
      {/if}
    {/each}
    {#if cut.hidden > 0}{@render moreRows(cut.hidden)}{/if}
  {:else}
    {#each files.slice(0, limit) as file (file.path)}{@render row(file, false)}{/each}
    {#if files.length > limit}{@render moreRows(files.length - limit)}{/if}
  {/if}

<style>
  .n {
    font-weight: 400;
  }
  .more-rows {
    display: block;
    width: 100%;
    height: 26px;
    border-radius: var(--radius-s);
    color: var(--text2);
    font-size: var(--text-small);
    text-align: center;
  }
  .more-rows:hover {
    background: var(--field);
  }
  .fold:focus-visible,
  .fold:global(.key-focus) {
    outline: none;
  }
  .fold:focus-visible::after,
  .fold:global(.key-focus)::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: var(--radius-s);
    outline: 2px solid var(--accent);
    outline-offset: -2px;
    pointer-events: none;
  }
  .folder {
    position: relative;
    display: flex;
    align-items: center;
    height: 26px;
    padding: 0 var(--space-1) 0 22px;
    border-radius: var(--radius-s);
  }
  .folder:hover {
    background: var(--field);
  }
  .fold {
    flex-grow: 1;
    min-width: 0;
    height: 100%;
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .chevron {
    width: 10px;
    height: 10px;
    flex-shrink: 0;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
    transition: transform 0.15s;
  }
  .chevron.open {
    transform: rotate(90deg);
  }
  .folder-icon {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
  }
  .dirname {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
    color: var(--text);
  }
  :global(:root[data-contrast="more"]) .fold:focus-visible::after { outline-width: 3px; }
</style>
