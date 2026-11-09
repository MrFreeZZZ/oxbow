<script lang="ts">
  import type { FileChange } from "./types";
  import type { ListKey } from "./changelists";
  import { BIG_FILE, bytes } from "./unusual";
  import { changeBadge as badge, keysTo } from "./changesList";
  import ChangesCheckbox from "./ChangesCheckbox.svelte";
  import PathName from "./PathName.svelte";

  import type { ChangesFileEvents } from "./changesList";

  let { file, group, nested, on, open, moving, dragging, counted,
    partialTitle, share, modClick, events }: {
    file: FileChange; group: ListKey | "conflicted"; nested: boolean;
    on: boolean; open: boolean; moving: boolean; dragging: boolean;
    counted: FileChange | null; partialTitle: string | null;
    share: { runs: number; total: number; title: string } | null; modClick: string;
    events: ChangesFileEvents;
  } = $props();
  const b = $derived(badge[file.status]);
  const unstaged = $derived(group !== "staged" && group !== "conflicted");
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="file diff-list-row diff-list-file"
  class:open
  class:nested
  class:bin={unstaged}
  class:moving={moving}
  class:ticked={on && !open}
  data-group={group}
  data-path={file.path}
  oncontextmenu={events.menu}
>
  {#if group !== "conflicted"}
    <ChangesCheckbox {on} label="{on ? 'Unselect' : 'Select'} {file.path}"
      title="Select for {group === 'staged' ? 'Unstage' : 'Stage'} and the file menu · Shift-click selects a range"
      onclick={(e) => { keysTo(e, ".name"); events.tick(e); }} />
  {/if}
  <button
    class="name diff-list-row-control"
    onclick={events.open}
    onpointerdown={events.drag}
    onkeydown={events.keyDown}
    onkeyup={events.keyUp}
    aria-current={open ? "true" : undefined}
    title={dragging ? undefined : `${file.oldPath ? `${file.oldPath} → ${file.path}` : file.path}${group === "conflicted" ? "" : ` · ${modClick} to select, Shift-click to select a range, drag to ${group === "staged" ? "unstage" : "stage"}`}`}
  >
    <span class="diff-list-badge {b.tone}" title={b.title}>{b.text}</span>
    <PathName path={file.path} folders={!nested} />
    {#if share}
      <span class="note partial" title={share.title}>
        <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="5.5" /><path class="fill" d="M8 2.5a5.5 5.5 0 0 0 0 11z" /></svg>{share.runs} of {share.total}
      </span>
    {/if}
    {#if partialTitle}
      <span class="note partial" title={partialTitle}>
        <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="5.5" /><path class="fill" d="M8 2.5a5.5 5.5 0 0 0 0 11z" /></svg>Partial
      </span>
    {/if}
    {#if file.submodule}
      <span class="note" title="Another repository in a folder here">Submodule{file.submodule.changed && !file.submodule.moved ? " · changed inside" : ""}</span>
    {:else if !counted}
      <span class="counts mono" title="Counting the changed lines…">…</span>
    {:else if counted.lfs}
      <span class="note" title="Kept in Git LFS: git stores a small pointer">LFS{counted.newSize !== undefined ? ` · ${bytes(counted.newSize)}` : ""}</span>
    {:else if (counted.newSize ?? 0) >= BIG_FILE}
      <span class="note warn" title="Too big for a git repository: GitHub refuses files over 100 MB">{bytes(counted.newSize!)}</span>
    {:else if !counted.binary && !counted.uncounted && file.status !== "conflicted"}
      <span class="counts mono"><span class="add">+{counted.additions}</span> <span class="del">−{counted.deletions}</span></span>
    {/if}
  </button>
  {#if unstaged}
    <button class="discard" onclick={events.discard} aria-label="{file.status === 'untracked' ? 'Delete' : 'Discard changes in'} {file.path}" title={file.status === "untracked" ? "Delete this new file" : "Discard changes"}>
      <svg class="icon" viewBox="0 0 16 16"><path d="M2.5 4.5h11M6 4.5V3a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1v1.5M4 4.5l.7 9a1 1 0 0 0 1 .9h4.6a1 1 0 0 0 1-.9l.7-9" /></svg>
    </button>
  {/if}
</div>

<style>
  .file.moving {
    opacity: 0.4;
  }
  .file {
    position: relative;
  }
  .file.ticked {
    background: var(--side-sel);
  }
  .name {
    flex-grow: 1;
    min-width: 0;
    height: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .note {
    font-size: var(--text-caption);
    color: var(--text2);
    flex-shrink: 0;
  }
  .note.warn {
    color: var(--orange);
  }
  .partial {
    display: flex;
    align-items: center;
    gap: 3px;
  }
  .partial .icon {
    width: 11px;
    height: 11px;
  }
  .partial .fill {
    fill: currentColor;
    stroke: none;
  }
  .counts {
    font-size: var(--text-caption);
    flex-shrink: 0;
    white-space: nowrap;
  }
  .add {
    color: var(--green);
  }
  .del {
    color: var(--red);
  }
  .discard {
    position: absolute;
    right: 2px;
    top: 2px;
    width: 24px;
    height: 24px;
    border-radius: var(--radius-pill);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text2);
    opacity: 0;
  }
  .discard .icon {
    width: 14px;
    height: 14px;
  }
  .file:hover .discard,
  .discard:focus-visible {
    opacity: 1;
    background: var(--win);
  }
  .file.bin:hover .counts {
    visibility: hidden;
  }
  .discard:hover {
    color: var(--red);
  }
  .name { cursor: var(--changes-row-cursor, default); }
</style>
