<script lang="ts">
  import type { FileChange } from "./types";
  import { tint } from "./format";
  import { changeBadge as badge } from "./changesList";
  import PathName from "./PathName.svelte";

  let { files, total, opened, color, onOpen, onKeyDown, onKeyUp }: {
    files: FileChange[]; total: number; opened: string | null; color: number;
    onOpen: (path: string) => void; onKeyDown: (event: KeyboardEvent) => void; onKeyUp: (event: KeyboardEvent) => void;
  } = $props();
</script>

<!-- Amend: the commit it replaces keeps its files, and the staged ones join them. -->
<div class="from-last" title="Files of the commit Amend replaces: they stay in it, with the staged changes added">
  <span class="grow">From last commit</span><span class="n">{total}</span>
</div>
{#each files as file (file.path)}
  {@const b = badge[file.status]}
  {@const open = opened === file.path}
  <div class="file" class:open style:background={open ? tint(color) : undefined}>
    <button class="name" onkeydown={onKeyDown} onkeyup={onKeyUp} onclick={() => onOpen(file.path)} aria-current={open ? "true" : undefined} title="{file.path} · its changes in the last commit">
      <span class="badge {b.tone}" title={b.title}>{b.text}</span>
      <PathName path={file.path} />
    </button>
  </div>
{/each}

<style>
  .from-last {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    margin-top: 2px;
    padding: 0 var(--space-2) 0 22px;
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .n {
    font-weight: 400;
  }
  .grow {
    flex-grow: 1;
    min-width: 0;
  }
  .file {
    position: relative;
    display: flex;
    align-items: center;
    height: 28px;
    padding: 0 var(--space-2) 0 22px;
    border-radius: var(--radius-m);
  }
  .file.open {
    box-shadow: inset 0 0 0 1px var(--selection-border);
  }
  .file:hover {
    background: var(--field);
  }
  .name:focus-visible,
  .name:global(.key-focus) {
    outline: none;
  }
  .name:focus-visible::after,
  .name:global(.key-focus)::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: var(--radius-s);
    outline: 2px solid var(--accent);
    outline-offset: -2px;
    pointer-events: none;
  }
  .name:focus-visible::after,
  .name:global(.key-focus)::after {
    border-radius: var(--radius-m);
  }
  .file.open .name::after {
    content: none;
  }
  .name {
    flex-grow: 1;
    min-width: 0;
    height: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .badge {
    width: 16px;
    height: 16px;
    border-radius: var(--radius-xs);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: var(--text-caption);
    font-weight: 700;
    flex-shrink: 0;
  }
  .badge.orange {
    background: var(--orange-soft);
    color: var(--orange);
  }
  .badge.green {
    background: var(--green-soft);
    color: var(--green);
  }
  .badge.red {
    background: var(--red-soft);
    color: var(--red);
  }
  .name { cursor: var(--changes-row-cursor, default); }
  :global(:root[data-contrast="more"]) .name:focus-visible::after { outline-width: 3px; }
</style>
