<script lang="ts">
  import type { Snippet } from "svelte";
  import type { DiffLine, FileDiff, Hunk } from "./types";
  import { plate, splitPath, tint } from "./format";

  let {
    diffs,
    color,
    whole,
    onToggleWhole,
    lineBudget = 4000,
    hunkBar,
  }: {
    diffs: FileDiff[];
    color: number;
    whole: boolean;
    onToggleWhole: () => void;
    lineBudget?: number;
    /** Actions over each hunk, for the working copy. */
    hunkBar?: Snippet<[FileDiff, Hunk]>;
  } = $props();

  let showAll = $state(false);

  /** Files to render, cut off once the line budget is spent so huge commits stay responsive. */
  const shown = $derived.by(() => {
    if (showAll) return { files: diffs, hidden: 0 };
    let budget = lineBudget;
    const files: FileDiff[] = [];
    for (const diff of diffs) {
      if (budget <= 0) break;
      files.push(diff);
      budget -= diff.hunks.reduce((n, h) => n + h.lines.length, 0);
    }
    return { files, hidden: diffs.length - files.length };
  });

  const statusLabel: Record<string, string> = {
    added: "Added",
    deleted: "Deleted",
    modified: "Modified",
    renamed: "Renamed",
    copied: "Copied",
    untracked: "New",
    conflicted: "Conflicted",
  };

  /** Rounded corners for the first and last line of a run of added or removed lines. */
  function radius(lines: DiffLine[], i: number): string {
    const line = lines[i];
    if (line.kind === "context") return "0";
    const top = i === 0 || lines[i - 1].kind !== line.kind ? 9 : 0;
    const bottom = i === lines.length - 1 || lines[i + 1].kind !== line.kind ? 9 : 0;
    return `${top}px ${top}px ${bottom}px ${bottom}px`;
  }

  function gap(lines: DiffLine[], i: number): number {
    if (i === 0) return 0;
    const line = lines[i];
    const prev = lines[i - 1];
    return line.kind !== "context" && prev.kind !== line.kind ? 3 : 0;
  }
</script>

{#each shown.files as diff (diff.file.path)}
  {@const path = splitPath(diff.file.path)}
  <section class="file">
    <header>
      <div class="bar" style:background={tint(color, "bar")} style:--name={plate(color)}>
        <span class="path"><span class="dir">{path.dir}</span>{path.name}</span>
        {#if diff.file.oldPath}<span class="from">from {diff.file.oldPath}</span>{/if}
        <span class="status">{statusLabel[diff.file.status]}</span>
        <span class="spacer"></span>
        <span class="mono add">+{diff.file.additions}</span>
        <span class="mono del">−{diff.file.deletions}</span>
        <button
          class="toggle"
          class:on={whole}
          onclick={onToggleWhole}
          aria-pressed={whole}
          aria-label={whole ? "Show changes only" : "Show whole file"}
          title={whole ? "Show changes only" : "Show whole file"}
        >
          <svg class="icon" viewBox="0 0 16 16">
            {#if whole}<path d="M3 5.5h10M3 10.5h10M8 1.5v2.5M8 12v2.5M6 3l2 1.5L10 3M6 13l2-1.5 2 1.5" />{:else}<path d="M3 3h10M3 13h10M3 8h10M8 4.5v-3M8 11.5v3" />{/if}
          </svg>
        </button>
      </div>
    </header>

    {#if diff.file.binary}
      <p class="note">Binary file, no text diff.</p>
    {:else if diff.tooLarge}
      <p class="note">This file is too large to show.</p>
    {:else if diff.hunks.length === 0}
      <p class="note">{diff.file.status === "renamed" ? "Renamed without changes." : "No changes in content."}</p>
    {:else}
      <div class="code mono selectable">
        {#each diff.hunks as hunk, h (h)}
          {@const before = h === 0 ? hunk.oldStart - 1 : hunk.oldStart - (diff.hunks[h - 1].oldStart + diff.hunks[h - 1].oldLines)}
          {#if before > 0 && !whole}
            <button class="fold" onclick={onToggleWhole} title="Show whole file">⋯ {before} unchanged {before === 1 ? "line" : "lines"}</button>
          {/if}
          {#if hunkBar}{@render hunkBar(diff, hunk)}{/if}
          {#each hunk.lines as line, i (i)}
            <div class="line {line.kind}" style:border-radius={radius(hunk.lines, i)} style:margin-top="{gap(hunk.lines, i)}px">
              <span class="no">{line.oldLine ?? ""}</span>
              <span class="no">{line.newLine ?? ""}</span>
              <span class="text">{#if line.words}{#each line.words as part, w (w)}{#if part.changed}<span class="word">{part.text}</span>{:else}{part.text}{/if}{/each}{:else}{line.text || " "}{/if}</span>
            </div>
          {/each}
        {/each}
      </div>
    {/if}
  </section>
{/each}

{#if shown.hidden > 0}
  <button class="more" onclick={() => (showAll = true)}>Show {shown.hidden} more {shown.hidden === 1 ? "file" : "files"}</button>
{/if}

<style>
  .file {
    padding: 0 12px 14px;
  }
  /* Each file starts with a bar in the branch color, so files don't run together. */
  header {
    position: sticky;
    top: 0;
    z-index: 1;
    padding: 6px 0;
    background: var(--win);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 4px 0 10px;
    border-radius: 10px;
    font-size: 12px;
  }
  .path {
    font-weight: 600;
    color: var(--name);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dir {
    color: var(--text2);
    font-weight: 400;
  }
  .from,
  .status {
    color: var(--text2);
    font-size: 11px;
    white-space: nowrap;
  }
  .spacer {
    flex-grow: 1;
  }
  .add {
    color: var(--green);
    font-size: 11px;
  }
  .del {
    color: var(--red);
    font-size: 11px;
  }
  .toggle {
    width: 26px;
    height: 26px;
    border-radius: 13px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--icon);
  }
  .toggle.on {
    background: var(--win);
    color: var(--text);
  }
  .note {
    margin: 4px 8px;
    color: var(--text2);
  }
  .code {
    font-size: 12px;
    line-height: 20px;
    color: var(--code);
  }
  .line {
    display: grid;
    grid-template-columns: 40px 40px 1fr;
  }
  .line.added {
    background: var(--add);
  }
  .line.removed {
    background: var(--del);
  }
  .no {
    color: var(--text2);
    opacity: 0.7;
    text-align: right;
    padding-right: 10px;
    user-select: none;
    -webkit-user-select: none;
  }
  .text {
    white-space: pre-wrap;
    word-break: break-all;
    padding-right: 10px;
  }
  .added .word {
    background: var(--add-word);
    border-radius: 5px;
    padding: 1px 0;
  }
  .removed .word {
    background: var(--del-word);
    border-radius: 5px;
    padding: 1px 0;
  }
  .fold {
    display: block;
    width: 100%;
    margin: 4px 0;
    padding: 0 12px;
    height: 22px;
    border-radius: 9px;
    background: var(--field);
    color: var(--text2);
    font-family: var(--font);
    font-size: 11px;
  }
  .more {
    margin: 0 20px 20px;
    height: 28px;
    padding: 0 12px;
    border-radius: 14px;
    background: var(--field);
  }
</style>
