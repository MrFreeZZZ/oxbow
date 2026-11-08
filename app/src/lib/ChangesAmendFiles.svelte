<script lang="ts">
  import type { FileChange } from "./types";
  import { changeBadge as badge } from "./changesList";
  import PathName from "./PathName.svelte";

  let { files, total, opened, onOpen, onKeyDown, onKeyUp }: {
    files: FileChange[]; total: number; opened: string | null;
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
  <div class="file diff-list-row diff-list-file">
    <button class="name diff-list-row-control" onkeydown={onKeyDown} onkeyup={onKeyUp} onclick={() => onOpen(file.path)} aria-current={open ? "true" : undefined} title="{file.path} · its changes in the last commit">
      <span class="diff-list-badge {b.tone}" title={b.title}>{b.text}</span>
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
  }
  .name {
    flex-grow: 1;
    min-width: 0;
    height: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .name { cursor: var(--changes-row-cursor, default); }
</style>
