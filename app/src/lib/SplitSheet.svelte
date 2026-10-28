<script lang="ts">
  // Split Commit: the commit's files on the left, each with a checkbox; the open file's diff on
  // the right, where single lines or whole hunks can be picked. What is picked becomes a new
  // commit right after this one, with the message typed under it; the rest stays in the commit
  // with its own message. From the commit menu it goes on to the dry run's sheet, in Edit Stack
  // into the plan.

  import { api } from "./api";
  import { modal } from "./modal";
  import { shortId } from "./format";
  import { withKeys } from "./keys";
  import DiffView, { type LinePicker } from "./DiffView.svelte";
  import FileBadge from "./FileBadge.svelte";
  import PathName from "./PathName.svelte";
  import WhyNote from "./WhyNote.svelte";
  import {
    byLine,
    fileState,
    pickedCounts,
    pickedInHunk,
    splitFiles,
    splitProblem,
    toggleFile,
    toggleHunk,
    toggleLines,
    type DraftSplit,
    type Picks,
  } from "./split";
  import type { FileDiff, Hunk } from "./types";

  let {
    commit,
    color,
    picks: start = new Map(),
    message: startMessage = "",
    button,
    onDone,
    onClose,
  }: {
    commit: { id: string; summary: string };
    /** The branch color of the commit, for the diff. */
    color: number;
    /** What is picked when it opens: a split made before, or the file it was opened on. */
    picks?: Picks;
    message?: string;
    /** The primary button: "Split…" when a sheet with the dry run follows, "Split" in Edit Stack. */
    button: string;
    /** The split, and how many files it takes out. */
    onDone: (split: DraftSplit, files: number) => void;
    onClose: () => void;
  } = $props();

  let diffs = $state.raw<FileDiff[] | null>(null);
  let error = $state<string | null>(null);
  // svelte-ignore state_referenced_locally
  let picks = $state.raw<Picks>(new Map(start));
  // svelte-ignore state_referenced_locally
  let message = $state(startMessage);
  let open = $state<string | null>(null);
  /** The line clicked last, for Shift-click to pick the lines from it. */
  let anchor: { hunk: Hunk; index: number } | null = null;
  let messageBox = $state<HTMLTextAreaElement>();

  $effect(() => {
    const id = commit.id;
    api.splitFiles(id).then(
      (found) => {
        diffs = found;
        open = [...start.keys()].find((p) => found.some((d) => d.file.path === p)) ?? found[0]?.file.path ?? null;
      },
      (err) => (error = String(err).replace(/^Error: /, "")),
    );
  });

  const current = $derived(diffs?.find((d) => d.file.path === open) ?? null);
  const counts = $derived(diffs ? pickedCounts(picks, diffs) : { files: 0, additions: 0, deletions: 0 });
  const problem = $derived(diffs ? splitProblem(picks, diffs, message) : "Reading the commit…");

  const picker: LinePicker = {
    enabled: () => !!current && byLine(current),
    picked: (hunk) => (current ? pickedInHunk(picks, current, hunk) : []),
    toggle: (hunk, index, range) => {
      if (!current) return;
      const from = range && anchor?.hunk === hunk ? Math.min(anchor.index, index) : index;
      const to = range && anchor?.hunk === hunk ? Math.max(anchor.index, index) : index;
      // A range goes the way the clicked line goes.
      const lines = [hunk.lines[index], ...hunk.lines.slice(from, to + 1).filter((_, k) => from + k !== index)];
      picks = toggleLines(picks, current, lines);
      anchor = { hunk, index };
    },
  };

  function tickFile(diff: FileDiff) {
    picks = toggleFile(picks, diff);
    open = diff.file.path;
  }

  function finish() {
    if (!diffs || problem) return;
    onDone({ id: commit.id, files: splitFiles(picks, diffs), message: message.trim(), additions: counts.additions, deletions: counts.deletions }, counts.files);
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    } else if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      finish();
    }
  }

  const pickedLine = $derived(
    counts.files ? `${counts.files} ${counts.files === 1 ? "file" : "files"} · +${counts.additions} −${counts.deletions} picked` : "Nothing picked yet",
  );
</script>

<svelte:window onkeydown={onKey} />

{#snippet hunkBar(d: FileDiff, hunk: Hunk)}
  {#if byLine(d)}
    {@const n = pickedInHunk(picks, d, hunk).length}
    {@const all = hunk.lines.filter((l) => l.kind !== "context").length}
    <div class="hunk">
      <span class="header">{hunk.header}</span>
      <span class="state" class:on={n > 0}>{n === 0 ? "Stays in the commit" : n === all ? "Goes into the new commit" : `${n} of ${all} lines go`}</span>
      <button class="button mini text" onclick={() => (picks = toggleHunk(picks, d, hunk))}>{n === all ? "Leave Hunk" : "Pick Hunk"}</button>
    </div>
  {/if}
{/snippet}

<div class="dim">
  <div class="sheet" use:modal role="dialog" aria-modal="true" aria-labelledby="split-title">
    <div class="top">
      <span class="tile"><svg class="icon" viewBox="0 0 16 16"><path d="M8 1.5v4M8 5.5 4 9.5v5M8 5.5l4 4v5" /></svg></span>
      <div class="words">
        <span class="title" id="split-title">Split Commit</span>
        <span class="body">
          Pick the files or lines that go into a new commit right after <span class="chip mono">{shortId(commit.id)}</span> “{commit.summary}”. The rest stays in it, with its message.
        </span>
      </div>
    </div>

    {#if error}
      <p class="error">{error}</p>
    {:else}
      <div class="panes">
        <div class="files" role="list" aria-label="Files of the commit">
          {#each diffs ?? [] as d (d.file.path)}
            {@const state = fileState(picks, d)}
            <div class="file" class:on={open === d.file.path} role="listitem">
              <button
                class="tick"
                class:on={state === "all"}
                class:some={state === "some"}
                role="checkbox"
                aria-checked={state === "all" ? true : state === "some" ? "mixed" : false}
                aria-label="Pick {d.file.path}"
                title={byLine(d) ? "Pick the whole file, or pick lines in its diff" : "This file goes whole: it has no lines to pick"}
                onclick={() => tickFile(d)}
              >
                <svg viewBox="0 0 16 16"><path d={state === "some" ? "M4 8h8" : "M3.5 8.5l3 3 6-7"} /></svg>
              </button>
              <button class="name" onclick={() => (open = d.file.path)}>
                <PathName path={d.file.path} />
                <span class="sub">
                  {#if d.file.status === "added"}New file · {:else if d.file.status === "deleted"}Deleted · {/if}{#if !byLine(d)}whole only{:else}<FileBadge file={d.file} />{/if}
                </span>
              </button>
            </div>
          {:else}
            <p class="muted">Reading the commit…</p>
          {/each}
        </div>
        <div class="diff">
          {#if current}
            {#if byLine(current)}
              <DiffView diffs={[current]} {color} whole={false} onToggleWhole={() => {}} {picker} {hunkBar} openable={false} history={false} tools={false} />
            {:else}
              <div class="whole">
                <span>{current.file.binary ? "A binary file" : current.tooLarge ? "Too many changed lines to pick one by one" : current.file.status === "deleted" ? "The commit deletes this file" : "A link or a submodule"}: it goes into the new commit whole, or stays.</span>
                <button class="button small" onclick={() => (picks = toggleFile(picks, current!))}>{fileState(picks, current) === "all" ? "Leave in This Commit" : "Pick Whole File"}</button>
              </div>
            {/if}
          {/if}
        </div>
      </div>
    {/if}

    <label class="message">
      <span class="label">Message of the new commit</span>
      <textarea rows="3" bind:value={message} bind:this={messageBox} placeholder="What the picked changes do"></textarea>
    </label>

    <div class="foot">
      <span class="picked">{pickedLine}</span>
      <span class="grow"></span>
      {#if problem && diffs}<WhyNote text={problem} />{/if}
      <button class="button" onclick={onClose} title={withKeys("Cancel", "Escape")}>Cancel</button>
      <button class="button primary" disabled={!!problem} title={problem ?? withKeys(button, "Mod+Enter")} onclick={finish}>{button}</button>
    </div>
  </div>
</div>

<style>
  .dim {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: var(--dim);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .sheet {
    width: min(1040px, calc(100vw - 48px));
    height: min(760px, calc(100vh - 48px));
    padding: 22px 22px 18px;
    border-radius: var(--radius-sheet);
    background: var(--sheet);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--shadow-sheet);
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .top {
    display: flex;
    gap: 14px;
    align-items: flex-start;
  }
  .tile {
    width: 44px;
    height: 44px;
    flex-shrink: 0;
    border-radius: var(--radius-l);
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--lane-0) var(--tint-label), transparent);
    color: var(--plate-0);
  }
  .tile .icon {
    width: 22px;
    height: 22px;
    stroke-width: 1.4;
  }
  .words {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .title {
    font-size: var(--text-heading);
    font-weight: 700;
    line-height: 1.3;
  }
  .body {
    line-height: 1.6;
  }
  .chip {
    padding: 1px 6px;
    border-radius: var(--radius-s);
    background: var(--field);
  }
  .mono {
    font-family: var(--mono);
  }
  .error {
    color: var(--red);
  }
  .panes {
    flex-grow: 1;
    min-height: 0;
    display: flex;
    border-radius: var(--radius-l);
    border: 0.5px solid var(--panel-border);
    background: var(--panel);
    overflow: hidden;
  }
  .files {
    width: 260px;
    flex-shrink: 0;
    overflow-y: auto;
    padding: var(--space-2);
    border-right: 0.5px solid var(--sep);
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .file {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 6px;
    border-radius: var(--radius-m);
  }
  .file.on {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }
  .name {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 1px;
    text-align: left;
  }
  .sub {
    display: flex;
    gap: 6px;
    align-items: center;
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .tick {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    padding: 0;
    border-radius: var(--radius-xs);
    border: 1.2px solid var(--text2);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .tick svg {
    width: 11px;
    height: 11px;
    fill: none;
    stroke: transparent;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .tick.on,
  .tick.some {
    background: var(--accent);
    border-color: var(--accent);
  }
  .tick.on svg,
  .tick.some svg {
    stroke: #fff;
  }
  .diff {
    flex-grow: 1;
    min-width: 0;
    overflow: auto;
    display: flex;
    flex-direction: column;
    padding: 0 var(--space-3);
  }
  .whole {
    margin: auto;
    max-width: 420px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    text-align: center;
    color: var(--text2);
  }
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
  .state.on {
    color: var(--accent);
  }
  .message {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .label {
    font-weight: 600;
  }
  textarea {
    resize: vertical;
    font: inherit;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .picked {
    font-size: var(--text-small);
    color: var(--text2);
  }
  .grow {
    flex-grow: 1;
  }
  .muted {
    padding: var(--space-2);
    color: var(--text2);
  }
  .foot .button {
    min-width: 96px;
    padding: 0 16px;
  }
</style>
