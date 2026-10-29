<script lang="ts">
  // The diff column of the Changes panel: what is shown over it (a file's side, or every file
  // of a side), the diffs with the actions of each hunk and file, or the way on when a side has
  // nothing left.

  import { api } from "./api";
  import { hunkRequest, linesRequest, stashMessageFor, type LinesKind } from "./changes";
  import { stashLinesRequest } from "./stashLines";
  import type { Shown } from "./changesView";
  import { confirm, type Request } from "./confirm.svelte";
  import DiffTools from "./DiffTools.svelte";
  import DiffView, { type LinePicker } from "./DiffView.svelte";
  import FoldAll from "./FoldAll.svelte";
  import { shortcut } from "./keys";
  import Menu, { menuIcons, type MenuEntry } from "./Menu.svelte";
  import { updateFolderRequest } from "./submodules";
  import TipCard from "./TipCard.svelte";
  import type { FileChange, FileDiff, Hunk, Side } from "./types";

  let {
    color,
    shown,
    diffs,
    diffsOf,
    pending,
    error,
    lastOpen,
    lastDiffs,
    needle,
    whole = $bindable(),
    sel = $bindable(),
    scroller = $bindable(),
    openUnstaged,
    openStaged,
    sideFiles,
    counted,
    staged,
    tracked,
    untracked,
    conflicted,
    stagedByPath,
    picker,
    onShow,
    onMore,
    onResolve,
    act,
    stage,
    unstage,
    discard,
    stageAndNext,
    hunkChip = null,
    onHunkChip,
    fileCaption = null,
    listTitle = null,
  }: {
    color: number;
    /** What the column shows: a file on one side, or every file of a side. */
    shown: Shown | null;
    /** The diffs shown, and what they were read for. */
    diffs: FileDiff[];
    diffsOf: Shown | null;
    /** Files still to come, past the pages read. */
    pending: number;
    error: string | null;
    /** A file of the commit Amend replaces, its diff in that commit shown instead. */
    lastOpen: string | null;
    lastDiffs: FileDiff[];
    /** The filter of the lists, lower case. */
    needle: string;
    whole: boolean;
    /** Lines picked in one hunk, with the one clicked last for Shift-click. */
    sel: { hunk: Hunk; lines: number[]; anchor: number } | null;
    scroller?: HTMLDivElement;
    /** The open file's unstaged and staged parts. */
    openUnstaged: FileChange | null;
    openStaged: FileChange | null;
    /** Files of every file of a side shown, for the heading over them. */
    sideFiles: FileChange[];
    /** The lines of the files are counted: until then their totals show "…", not 0. */
    counted: boolean;
    staged: FileChange[];
    tracked: FileChange[];
    untracked: FileChange[];
    conflicted: FileChange[];
    stagedByPath: Map<string, FileChange>;
    picker: LinePicker;
    onShow: (next: Shown) => void;
    /** The next page of the diffs of every file of a side. */
    onMore: () => void;
    /** Opens the Conflicts screen. */
    onResolve?: () => void;
    act: (request: Request) => Promise<boolean>;
    stage: (files: FileChange[]) => unknown;
    unstage: (files: FileChange[]) => unknown;
    discard: (files: FileChange[]) => unknown;
    stageAndNext: (file: FileChange) => unknown;
    /** With changelists, the chip on an unstaged hunk's bar: the list its changes are in. */
    hunkChip?: ((d: FileDiff, hunk: Hunk) => { label: string; listed: boolean }) | null;
    /** The chip's menu, which moves the hunk's changes to another list. */
    onHunkChip?: (event: MouseEvent, d: FileDiff, hunk: Hunk) => void;
    /** Under the switch of an unstaged file in a changelist: what of it the list holds. */
    fileCaption?: string | null;
    /** The changelist whose files are all shown. */
    listTitle?: string | null;
  } = $props();

  let diffView = $state<DiffView>();
  let menu = $state<{ x: number; y: number; entries: MenuEntry[]; label: string } | null>(null);

  const openFile = $derived(shown?.kind === "file" ? shown : null);
  const sideTotals = $derived(sideFiles.reduce((sum, f) => ({ add: sum.add + f.additions, del: sum.del + f.deletions }), { add: 0, del: 0 }));

  /** What the open file's side holds, under the switch. */
  function caption(file: Shown & { kind: "file" }): string {
    if (file.side === "staged") return "Goes into the commit";
    const f = openUnstaged;
    if (f?.status === "conflicted") return "Has conflicts to resolve";
    if (f?.status === "untracked") return "New file git does not track yet";
    return "Stays out of the commit";
  }

  const hunkAction = (kind: LinesKind, path: string, hunk: Hunk) => act(hunkRequest(kind, path, hunk));

  function linesAction(kind: LinesKind, path: string, hunk: Hunk) {
    if (sel?.hunk === hunk) act(linesRequest(kind, path, hunk, [...sel.lines].sort((a, b) => a - b)));
  }

  /** Stash the hunk, or its picked lines when some are. */
  function stashLines(path: string, hunk: Hunk) {
    const lines = sel?.hunk === hunk && sel.lines.length ? [...sel.lines].sort((a, b) => a - b) : null;
    act(stashLinesRequest(path, hunk, lines, stashMessageFor(path)));
  }

  /** Stage's menu on the bar of the open file: Stage & Next File first, as the way through a
   *  review, then Stage itself, and Discard the Rest for a file staged in part. */
  function stageMenu(event: MouseEvent, file: FileChange, label: string) {
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const entries: MenuEntry[] = [
      { kind: "item", label: "Stage & Next File", icon: menuIcons.skip, keys: shortcut("stageAndNext"), run: () => stageAndNext(file) },
      { kind: "item", label, icon: menuIcons.stage, keys: shortcut("stageFile"), run: () => stage([file]) },
    ];
    if (stagedByPath.has(file.path)) {
      entries.push({ kind: "sep" });
      entries.push({ kind: "item", label: "Discard the Rest…", icon: menuIcons.drop, danger: true, run: () => discard([file]) });
    }
    menu = { x: box.right - 240, y: box.bottom + 4, entries, label: "Stage menu" };
  }
</script>

{#snippet hunkBar(d: FileDiff, hunk: Hunk, side: Side)}
  {#if !whole && d.file.status !== "untracked" && d.file.status !== "conflicted"}
    {@const n = sel?.hunk === hunk ? sel.lines.length : 0}
    {@const lineWord = n === 1 ? "Line" : "Lines"}
    <div class="hunk">
      <span class="header">{hunk.header}</span>
      {#if n}
        <button class="state picked" onclick={() => (sel = null)} title="Clear the picked lines">{n} of {hunk.lines.filter((l) => l.kind !== "context").length} lines</button>
        {#if side === "unstaged" && hunkChip}
          <button class="state chip" onclick={(e) => onHunkChip?.(e, d, hunk)} aria-haspopup="menu" title="Move the changes of the picked lines to another changelist">
            Move<svg class="icon" viewBox="0 0 16 16"><path d="M4.5 6.5 8 10l3.5-3.5" /></svg>
          </button>
        {/if}
        {#if side === "unstaged"}
          <button class="button mini text" onclick={() => linesAction("discardHunk", d.file.path, hunk)}>Discard {n} {lineWord}</button>
          <button class="button mini text" onclick={() => stashLines(d.file.path, hunk)} title="Put the picked lines aside in a new stash">Stash {n} {lineWord}</button>
          <button class="button mini quiet" onclick={() => linesAction("stageHunk", d.file.path, hunk)}>Stage {n} {lineWord}</button>
        {:else}
          <button class="button mini text" onclick={() => linesAction("unstageHunk", d.file.path, hunk)}>Unstage {n} {lineWord}</button>
        {/if}
      {:else}
        {#if side === "unstaged" && hunkChip}
          {@const chip = hunkChip(d, hunk)}
          <button class="state chip" class:listed={chip.listed} onclick={(e) => onHunkChip?.(e, d, hunk)} aria-haspopup="menu" title="Move this change to another changelist">
            {chip.label}<svg class="icon" viewBox="0 0 16 16"><path d="M4.5 6.5 8 10l3.5-3.5" /></svg>
          </button>
        {:else}
          <span class="state" class:staged={side === "staged"}>{side === "staged" ? "Staged" : "Unstaged"}</span>
        {/if}
        {#if side === "unstaged"}
          <button class="button mini text" onclick={() => hunkAction("discardHunk", d.file.path, hunk)}>Discard</button>
          <button class="button mini text" onclick={() => stashLines(d.file.path, hunk)} title="Put this hunk aside in a new stash">Stash</button>
          <button class="button mini quiet" onclick={() => hunkAction("stageHunk", d.file.path, hunk)}>Stage Hunk</button>
        {:else}
          <button class="button mini text" onclick={() => hunkAction("unstageHunk", d.file.path, hunk)}>Unstage Hunk</button>
        {/if}
      {/if}
    </div>
  {/if}
{/snippet}
{#snippet unstagedHunk(d: FileDiff, hunk: Hunk)}{@render hunkBar(d, hunk, "unstaged")}{/snippet}
{#snippet stagedHunk(d: FileDiff, hunk: Hunk)}{@render hunkBar(d, hunk, "staged")}{/snippet}

<!-- The whole file's actions on its header bar. -->
{#snippet unstagedFile(d: FileDiff)}
  {#if d.submodule}
    <!-- A submodule's own files are committed in it; here only the commit it is at can be staged. -->
    {#if d.file.submodule?.moved}
      {@const move = d.submodule}
      {#if move.old}<button class="button mini text" onclick={() => act(updateFolderRequest(move))} title="Check out the commit recorded here again">Update to {move.old.slice(0, 7)}</button>{/if}
      <button class="button mini quiet" onclick={() => stage([d.file])} title="Record the commit it is at in the next commit">Stage</button>
    {:else}
      <button class="button mini text" onclick={() => api.openWindow(d.file.path).catch((err) => confirm.fail(err))} title="Its changes are committed in it">Open in New Window</button>
    {/if}
  {:else if d.file.status === "conflicted"}
    {#if onResolve}<button class="button mini text" onclick={onResolve}>Resolve Conflicts…</button>{/if}
  {:else}
    {@const rest = stagedByPath.has(d.file.path)}
    {@const label = rest ? "Stage the Rest" : "Stage"}
    {@const keys = shortcut("stageFile")}
    <!-- A file staged in part has Discard the Rest in the Stage menu: Discard beside Stage the Rest would read as the whole file. -->
    {#if !rest || diffsOf?.kind !== "file"}
      <button class="button mini text" onclick={() => discard([d.file])} title={d.file.status === "untracked" ? "Delete this new file" : rest ? "Throw away the edits not staged; the staged part stays" : "Throw away the edits in this file"}>Discard</button>
    {/if}
    {#if diffsOf?.kind === "file"}
      <span class="split">
        <button class="button mini quiet" onclick={() => stage([d.file])} title="{rest ? 'Add the rest of the file to the next commit' : 'Add the whole file to the next commit'}{keys ? ` (${keys})` : ''}">{label}</button>
        <button class="button mini quiet more" onclick={(e) => stageMenu(e, d.file, label)} aria-label="More ways to stage" aria-haspopup="menu" title="More ways to stage">
          <svg class="icon" viewBox="0 0 16 16"><path d="M4.5 6.5 8 10l3.5-3.5" /></svg>
        </button>
      </span>
    {:else}
      <button class="button mini quiet" onclick={() => stage([d.file])} title={rest ? "Add the rest of the file to the next commit" : "Add the whole file to the next commit"}>{label}</button>
    {/if}
  {/if}
{/snippet}
{#snippet stagedFile(d: FileDiff)}
  <button class="button mini text" onclick={() => unstage([d.file])} title="Take the whole file out of the next commit">Unstage</button>
{/snippet}

{#if lastOpen}
  <div class="dhead">
    <div class="grow">
      <div class="dtitle"><b>From last commit</b></div>
      <div class="caption">Amend keeps these changes in the commit</div>
    </div>
    <DiffTools {whole} onToggleWhole={() => (whole = !whole)} working />
  </div>
{:else if shown}
  <div class="dhead">
    {#if shown.kind === "file"}
      {@const file = shown}
      <div class="segmented" role="radiogroup" aria-label="Which part of the file">
        <button role="radio" aria-checked={file.side === "unstaged"} class:on={file.side === "unstaged"} disabled={!openUnstaged} onclick={() => onShow({ ...file, side: "unstaged" })} title={openUnstaged ? "What stays out of the commit" : "Nothing of this file is left out of the commit"}>Unstaged</button>
        <button role="radio" aria-checked={file.side === "staged"} class:on={file.side === "staged"} disabled={!openStaged} onclick={() => onShow({ ...file, side: "staged" })} title={openStaged ? "What goes into the commit" : "Nothing of this file is staged yet"}>Staged</button>
      </div>
      <span class="caption grow">{file.side === "unstaged" && fileCaption ? fileCaption : caption(file)}</span>
    {:else}
      <div class="grow">
        <div class="dtitle">
          <b>{shown.kind === "files" ? "Selected files" : listTitle ? listTitle : shown.side === "staged" ? "Next commit" : shown.untracked ? "Untracked files" : "Unstaged changes"}</b>
          <span class="n">{sideFiles.length} {sideFiles.length === 1 ? "file" : "files"}</span>
          {#if sideFiles.length && !counted}<span class="mono" title="Counting the changed lines…">…</span>{:else if sideFiles.length}<span class="mono add">+{sideTotals.add}</span><span class="mono del">−{sideTotals.del}</span>{/if}
        </div>
        <div class="caption">{shown.side === "staged" ? "Only staged changes go into the commit" : listTitle ? "A changelist: its changes stay out of the commit until staged, or committed on their own with Commit…" : shown.untracked ? "New files git does not track yet stay out of the commit until staged" : "They stay out of the commit until staged"}</div>
      </div>
      {#if diffs.length > 1}<FoldAll onFold={(fold) => diffView?.setAll(fold)} />{/if}
    {/if}
    <DiffTools {whole} onToggleWhole={() => (whole = !whole)} working />
  </div>
{/if}
<div class="scroll" bind:this={scroller}>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if openFile && openUnstaged && openStaged && !lastOpen}<div class="tip"><TipCard id="partlyStaged" /></div>{/if}
  {#if lastOpen}
    <DiffView diffs={lastDiffs} {color} {whole} onToggleWhole={() => (whole = !whole)} tools={false} />
  {:else if !shown}
    <p class="nothing">No uncommitted changes.</p>
  {:else if shown.kind === "all" && !sideFiles.length && !needle}
    <!-- Nothing left on this side: a short line with the way on, not a big empty screen. -->
    <div class="done-strip">
      <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" /><path d="M5.5 8.2 7.2 10l3.3-3.8" /></svg>
      {#if shown.list}
        <span class="grow">No changes in {listTitle ?? "this changelist"} yet</span>
        {#if tracked.length || conflicted.length}<button class="button mini quiet" onclick={() => onShow({ kind: "all", side: "unstaged" })}>Review Unstaged</button>{/if}
      {:else if shown.untracked}
        <span class="grow">{staged.length ? "All new files staged" : "No untracked files"}</span>
        {#if tracked.length || conflicted.length}<button class="button mini quiet" onclick={() => onShow({ kind: "all", side: "unstaged" })}>Review Unstaged</button>{/if}
        {#if staged.length}<button class="button mini quiet" onclick={() => onShow({ kind: "all", side: "staged" })}>Review Staged</button>{/if}
      {:else if shown.side === "unstaged"}
        <span class="grow">{staged.length ? "All changes staged" : "No edits to tracked files"}</span>
        {#if untracked.length}<button class="button mini" onclick={() => onShow({ kind: "file", path: untracked[0].path, side: "unstaged" })}>Next Unstaged File</button>{/if}
        {#if staged.length}<button class="button mini quiet" onclick={() => onShow({ kind: "all", side: "staged" })}>Review Staged</button>{/if}
      {:else}
        <span class="grow">Nothing staged yet</span>
        {#if tracked.length || conflicted.length}<button class="button mini quiet" onclick={() => onShow({ kind: "all", side: "unstaged" })}>Review Unstaged</button>{/if}
      {/if}
    </div>
  {:else if diffsOf}
    {@const side = diffsOf.side}
    <DiffView
      bind:this={diffView}
      foldable={diffsOf.kind !== "file"}
      {diffs}
      {color}
      {whole}
      onToggleWhole={() => (whole = !whole)}
      hunkBar={side === "staged" ? stagedHunk : unstagedHunk}
      fileActions={side === "staged" ? stagedFile : unstagedFile}
      {picker}
      pending={diffsOf.kind !== "file" ? pending : 0}
      {onMore}
      loadFull={(d) => api.workingDiff(d.file.path, side, whole, true)}
      command={(d) => (side === "staged" ? `git diff --cached -- ${d.file.path}` : `git diff -- ${d.file.path}`)}
      working
      {side}
      tools={false}
    />
  {/if}
</div>

{#if menu}
  <Menu x={menu.x} y={menu.y} label={menu.label} entries={menu.entries} onClose={() => (menu = null)} />
{/if}

<style>
  .segmented {
    flex-shrink: 0;
    display: flex;
    padding: 2px;
    border-radius: var(--radius-m);
    background: var(--field);
  }
  .segmented button {
    height: 22px;
    min-width: 58px;
    padding: 0 10px;
    border-radius: var(--radius-s);
    font-size: var(--text-small);
    font-weight: 500;
    color: var(--text2);
    white-space: nowrap;
  }
  .segmented button.on {
    background: var(--sheet);
    color: var(--text);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
  }
  .segmented button:disabled:not(.on) {
    opacity: 0.45;
    cursor: default;
  }
  /* Stage with its menu on the bar of the open file: one capsule cut in two. */
  .split {
    display: inline-flex;
    gap: 1px;
    flex-shrink: 0;
  }
  .split .button:first-child {
    border-radius: var(--radius-pill) 0 0 var(--radius-pill);
    padding-right: 8px;
  }
  .split .more {
    width: 22px;
    padding: 0;
    border-radius: 0 var(--radius-pill) var(--radius-pill) 0;
  }
  .split .more .icon {
    width: 11px;
    height: 11px;
    stroke-width: 1.8;
  }
  /* Over the diff: which side of the file it is, or which side's files, and how it is drawn. */
  .dhead {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 46px;
    padding: var(--space-2) var(--space-3) var(--space-2) var(--space-4);
    flex-shrink: 0;
  }
  .caption {
    font-size: var(--text-small);
    color: var(--text2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dtitle {
    display: flex;
    align-items: baseline;
    gap: 8px;
    white-space: nowrap;
  }
  .dtitle b {
    font-size: var(--text-body);
    font-weight: 600;
  }
  .dtitle .n {
    font-size: var(--text-small);
    color: var(--text2);
  }
  .dtitle .mono {
    font-size: var(--text-small);
  }
  .tip {
    margin: 0 var(--space-3) var(--space-2);
    display: flex;
    flex-direction: column;
  }
  .scroll {
    flex-grow: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .error {
    margin: 12px 20px 0;
    font-size: var(--text-small);
    color: var(--red);
  }
  .nothing {
    margin: var(--space-5);
    color: var(--text2);
    font-size: var(--text-small);
  }
  /* A side with nothing left: one line with the way on. */
  .done-strip {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 40px;
    margin: 0 var(--space-3);
    padding: var(--space-1) var(--space-2) var(--space-1) var(--space-3);
    border-radius: var(--radius-m);
    background: var(--field);
    font-size: var(--text-small);
    color: var(--text2);
  }
  .done-strip .icon {
    width: 15px;
    height: 15px;
    flex-shrink: 0;
  }
  .done-strip .button {
    flex-shrink: 0;
    white-space: nowrap;
  }
  /* Hunk bar, as in the design: the @@ line, its state and the actions over it. */
  .hunk {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    margin: var(--space-2) 0 var(--space-1);
    padding: 0 var(--space-2) 0 var(--space-3);
    border-radius: var(--radius-m);
    background: var(--hunk-bg);
    color: var(--hunk-text);
    font-family: var(--mono);
    font-size: var(--text-small);
    line-height: normal;
  }
  .hunk .header {
    flex-grow: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .state {
    font-family: var(--font);
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
  }
  .state.staged {
    color: var(--green);
  }
  /* With changelists, the hunk's list, as a chip that moves it to another. */
  .state.chip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    height: 20px;
    padding: 0 6px 0 8px;
    border-radius: var(--radius-pill);
    background: var(--field);
  }
  .state.chip .icon {
    width: 10px;
    height: 10px;
    stroke-width: 1.8;
  }
  .state.chip.listed {
    color: var(--accent-ink);
    background: var(--accent-quiet);
  }
  .state.chip:hover {
    color: var(--text);
  }
  .state.picked {
    color: var(--text);
    padding: 0 4px;
  }
  .grow {
    flex-grow: 1;
    min-width: 0;
  }
  .n {
    font-weight: 400;
  }
  .add {
    color: var(--green);
  }
  .del {
    color: var(--red);
  }
</style>
