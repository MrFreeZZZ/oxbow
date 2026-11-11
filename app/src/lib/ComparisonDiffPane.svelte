<script lang="ts">
  import { tick, type Snippet } from "svelte";
  import { diffScroll } from "./diffScroll";
  import { lane, plate, tint } from "./format";
  import { filesWidth, resizeFiles } from "./listView.svelte";
  import { pressed } from "./shortcuts";
  import { fileTabs, keyFocus, tabStepper } from "./tabs";
  import type { FileChange } from "./types";
  import DiffTools from "./DiffTools.svelte";
  import FileBadge from "./FileBadge.svelte";
  import FileFilter from "./FileFilter.svelte";
  import FoldAll from "./FoldAll.svelte";
  import PathName from "./PathName.svelte";

  let {
    files, color, path, shownPath, onOpen, whole, onToggleWhole, onFold, totals,
    counted, metadata, children,
  }: {
    files: FileChange[];
    color: number;
    path: string | null;
    shownPath: string | null;
    onOpen: (path: string | null) => void;
    whole: boolean;
    onToggleWhole: () => void;
    onFold?: (fold: boolean) => void;
    totals?: { add: number; del: number } | null;
    counted?: (file: FileChange) => boolean;
    metadata?: Snippet<[string]>;
    children: Snippet;
  } = $props();

  const LIST_STEP = 300;
  const statusIcon: Record<string, string> = { added: "A", deleted: "D", modified: "M", renamed: "R", copied: "C" };
  const tone: Record<string, string> = { added: "green", deleted: "red" };
  let width = $state(0);
  let filter = $state("");
  let listed = $state(LIST_STEP);
  let list = $state<HTMLDivElement>();
  let scroller = $state<HTMLDivElement>();

  const stacked = $derived(width > 0 && width < 600);
  const needle = $derived(filter.trim().toLowerCase());
  const shown = $derived(needle ? files.filter((file) => file.path.toLowerCase().includes(needle) || file.oldPath?.toLowerCase().includes(needle)) : files);
  const paths = $derived(shown.map((file) => file.path));
  const fileIndex = $derived(shown.findIndex((file) => file.path === shownPath));

  $effect(() => {
    if (path !== null && !paths.includes(path)) onOpen(null);
  });

  function open(next: string | null) {
    const index = next === null ? -1 : paths.indexOf(next);
    if (index >= listed) listed = index + 1;
    onOpen(next);
  }

  function rowClick(event: MouseEvent, next: string | null) {
    // WebKit on macOS does not focus clicked buttons.
    (event.currentTarget as HTMLButtonElement).focus({ preventScroll: true });
    open(next);
  }

  $effect(() => {
    shownPath;
    if (scroller) scroller.scrollTop = 0;
  });

  $effect(() => {
    path;
    listed;
    requestAnimationFrame(() => list?.querySelector('[aria-current="true"]')?.scrollIntoView({ block: "nearest" }));
  });

  function stepFile(by: 1 | -1, wrap: boolean, rowPath = path) {
    const focused = document.activeElement;
    const fromRow = focused?.closest("[data-file-row]");
    if (!fileTabs(paths, rowPath, open)(by, wrap)) return false;
    if (fromRow && list?.contains(fromRow)) void tick().then(() => {
      if (document.activeElement !== focused) return;
      const active = list?.querySelector<HTMLElement>('[aria-current="true"]');
      if (active) keyFocus(active);
    });
    return true;
  }

  function onRowKey(event: KeyboardEvent, rowPath: string | null) {
    const by = pressed("nextFile", event) ? 1 : pressed("previousFile", event) ? -1 : 0;
    if (!by) return;
    event.preventDefault();
    stepFile(by, false, rowPath);
  }
</script>

<div class="body" class:stacked bind:clientWidth={width}>
  <div class="files" aria-label="Changed files" data-comparison-files data-steps use:tabStepper={stepFile} style:width={stacked ? undefined : `${filesWidth(width)}px`}>
    <div class="filter"><FileFilter bind:value={filter} {color} /></div>
    <div class="list" bind:this={list} style:--selection-fill={tint(color)} style:--selection-border={`color-mix(in srgb, ${lane(color)} 40%, transparent)`}>
      <button class="diff-list-row diff-list-group" data-file-row aria-current={path === null ? "true" : undefined} onclick={(event) => rowClick(event, null)} onkeydown={(event) => onRowKey(event, null)}>
        <span class="group-title" style:color={plate(color)}>All Changes</span>
        <span>{needle ? `${shown.length} of ${files.length}` : files.length.toLocaleString()}</span>
        <span class="grow"></span>
        {#if totals}
          <span class="add">+{totals.add}</span>
          <span class="del">−{totals.del}</span>
        {:else if files.length}
          <span title="Counting the changed lines">Counting…</span>
        {/if}
      </button>
      {#each shown.slice(0, listed) as file (file.path)}
        <!-- svelte-ignore a11y_no_static_element_interactions (delegates keys from the row buttons) -->
        <div class="diff-list-row diff-list-file file" data-file-row onkeydown={(event) => onRowKey(event, file.path)}>
          <button class="diff-list-row-control name" aria-current={path === file.path ? "true" : undefined} onclick={(event) => rowClick(event, file.path)} title={file.oldPath ? `${file.oldPath} → ${file.path}` : file.path}>
            <span class="diff-list-badge {tone[file.status] ?? "orange"}">{statusIcon[file.status] ?? "M"}</span>
            <PathName path={file.path} />
            {#if !counted || counted(file)}<FileBadge {file} />{/if}
          </button>
          {#if metadata}{@render metadata(file.path)}{/if}
        </div>
      {/each}
      {#if shown.length > listed}
        <button class="diff-list-row diff-list-file more" onclick={() => (listed += LIST_STEP)}>Show {Math.min(shown.length - listed, LIST_STEP).toLocaleString()} more {shown.length - listed === 1 ? "file" : "files"}</button>
      {/if}
      {#if needle && !shown.length}<p class="empty">No files match the filter.</p>{/if}
    </div>
  </div>
  {#if !stacked}<button class="edge" tabindex="-1" style:left={`${filesWidth(width) - 4}px`} onpointerdown={(event) => resizeFiles(event, width)} aria-label="Resize the files" title="Drag to resize"></button>{/if}
  <div class="diff" aria-label="Diff">
    <div class="dhead diff-toolbar">
      <span class="caption grow">
        {#if fileIndex >= 0}File {fileIndex + 1} of {shown.length.toLocaleString()}
        {:else}{files.length.toLocaleString()} {files.length === 1 ? "file" : "files"}{/if}
      </span>
      {#if onFold}<FoldAll {onFold} />{/if}
      <DiffTools {whole} {onToggleWhole} />
    </div>
    <div class="scroll" bind:this={scroller} use:diffScroll={color}>
      {@render children()}
    </div>
  </div>
</div>

<style>
  .body { position: relative; display: flex; flex: 1; min-height: 0; }
  .body.stacked { flex-direction: column; }
  .files { flex-shrink: 0; min-height: 0; display: flex; flex-direction: column; border-right: 1px solid var(--sep); }
  .stacked .files { max-height: 50%; border-right: 0; border-bottom: 1px solid var(--sep); }
  .filter { display: flex; align-items: center; min-height: 46px; padding: var(--space-2) var(--space-3); flex-shrink: 0; }
  .list { flex: 1; min-height: 0; overflow-y: auto; padding: var(--space-2); }
  .group-title { font-weight: 700; letter-spacing: 0.05em; text-transform: uppercase; }
  .add { color: var(--green); font-family: var(--mono); }
  .del { color: var(--red); font-family: var(--mono); }
  .file { position: relative; }
  .name { display: flex; align-items: center; gap: 8px; flex: 1; min-width: 0; height: 100%; }
  .more { justify-content: center; padding: 0; color: var(--text2); font-size: var(--text-small); }
  .edge {
    position: absolute;
    top: 50%;
    width: 7px;
    height: 36px;
    margin-top: -18px;
    border-radius: var(--radius-xs);
    background: var(--win);
    border: 1px solid var(--glass-border);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.08);
    z-index: 2;
    cursor: col-resize;
  }
  .diff { display: flex; flex-direction: column; flex: 1; min-width: 0; min-height: 0; }
  .dhead { display: flex; align-items: center; gap: 10px; min-height: 46px; flex-shrink: 0; }
  .caption, .empty { color: var(--text2); font-size: var(--text-small); }
  .caption { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .grow { flex: 1; min-width: 0; }
  .scroll { flex: 1; overflow-y: auto; min-height: 0; }
  .empty { margin: var(--space-2); }
</style>
