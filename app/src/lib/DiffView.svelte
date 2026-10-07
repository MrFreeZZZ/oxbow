<script lang="ts" module>
  import type { Hunk as HunkOf } from "./types";

  /** Which changed lines of a hunk are picked; the others dim once any is. */
  export interface LinePicker {
    enabled: (hunk: HunkOf) => boolean;
    picked: (hunk: HunkOf) => number[];
    toggle: (hunk: HunkOf, index: number, range: boolean) => void;
  }
</script>

<script lang="ts">
  import type { Snippet } from "svelte";
  import type { DiffLine, FileDiff, Hunk } from "./types";
  import { plate, splitPath, tint } from "./format";
  import { prefs } from "./prefs.svelte";
  import { api } from "./api";
  import { confirm } from "./confirm.svelte";
  import { languageOf, paint } from "./syntax";
  import { nav } from "./nav.svelte";

  let {
    diffs,
    color,
    whole,
    onToggleWhole,
    lineBudget = 4000,
    hunkBar,
    openable = true,
    find = null,
    matchCase = true,
    commit = null,
    history = true,
    picker = null,
  }: {
    diffs: FileDiff[];
    color: number;
    whole: boolean;
    onToggleWhole: () => void;
    lineBudget?: number;
    /** Actions over each hunk, for the working copy. */
    hunkBar?: Snippet<[FileDiff, Hunk]>;
    /** Offer to open the file in the editor from Settings › Integrations. */
    openable?: boolean;
    /** Code being searched for in History: marked wherever it appears. */
    find?: string | null;
    /** Whether `find` must match letter case. */
    matchCase?: boolean;
    /** The commit shown, so File History starts on it. */
    commit?: string | null;
    /** Offer File History; not on File History itself. */
    history?: boolean;
    /** Checkboxes on the changed lines of a hunk, to stage or discard single lines. */
    picker?: LinePicker | null;
  } = $props();

  /** Open the file in the editor, at its first change. */
  function openInEditor(diff: FileDiff) {
    const first = diff.hunks[0]?.lines.find((l) => l.kind !== "context" && l.newLine !== null) ?? diff.hunks[0]?.lines[0];
    api.openInEditor(diff.file.path, first?.newLine ?? null).catch((err) => confirm.say(String(err)));
  }

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
        {#if openable && history && diff.file.status !== "untracked"}
          <button class="toggle" onclick={() => nav.openFile(diff.file.path, commit)} aria-label="File history" title="File History and Blame">
            <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="5.5" /><path d="M8 5v3.2l2 1.3" /></svg>
          </button>
        {/if}
        {#if openable && diff.file.status !== "deleted"}
          <button class="toggle" onclick={() => openInEditor(diff)} aria-label="Open in editor" title="Open in Editor">
            <svg class="icon" viewBox="0 0 16 16"><path d="M9.5 2.5h4v4M13.5 2.5 7.5 8.5M12 9.5v3a1 1 0 0 1-1 1H3.5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h3" /></svg>
          </button>
        {/if}
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
      <p class="note">
        {#if diff.file.status === "renamed"}
          Renamed without changes.
        {:else if prefs.get("oxbow.diff.ignoreWhitespace")}
          No changes besides whitespace, which Settings › Diff & Text is set to ignore.
        {:else}
          No changes in content.
        {/if}
      </p>
    {:else}
      {@const language = languageOf(diff.file.path)}
      <div class="code mono selectable">
        {#each diff.hunks as hunk, h (h)}
          {@const before = h === 0 ? hunk.oldStart - 1 : hunk.oldStart - (diff.hunks[h - 1].oldStart + diff.hunks[h - 1].oldLines)}
          {#if before > 0 && !whole}
            <button class="fold" onclick={onToggleWhole} title="Show whole file">⋯ {before} unchanged {before === 1 ? "line" : "lines"}</button>
          {/if}
          {#if hunkBar}{@render hunkBar(diff, hunk)}{/if}
          {@const picking = !!picker?.enabled(hunk)}
          {@const picked = picking ? picker!.picked(hunk) : []}
          {#each hunk.lines as line, i (i)}
            {@const on = picked.includes(i)}
            <div
              class="line {line.kind}"
              class:picking
              class:faded={picked.length > 0 && line.kind !== "context" && !on}
              style:border-radius={radius(hunk.lines, i)}
              style:margin-top="{gap(hunk.lines, i)}px"
            >
              {#if picking}
                {#if line.kind !== "context"}
                  <button
                    class="pick"
                    class:on
                    role="checkbox"
                    aria-checked={on}
                    aria-label="{on ? 'Unselect' : 'Select'} line {line.newLine ?? line.oldLine}"
                    title="Pick this line; Shift-click picks a range"
                    onclick={(e) => picker!.toggle(hunk, i, e.shiftKey)}
                  ><svg viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg></button>
                {:else}<span></span>{/if}
              {/if}
              <span class="no">{line.oldLine ?? ""}</span>
              <span class="no">{line.newLine ?? ""}</span>
              <span class="text">{#each paint(line.text || " ", prefs.get("oxbow.diff.wordHighlight") ? line.words : null, language, find, matchCase) as piece, w (w)}<span class:word={piece.changed} class:found={piece.found} data-hit={piece.hit} class={piece.role ? `syn-${piece.role}` : undefined}>{piece.text}</span>{/each}</span>
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
    font-family: var(--code-font);
    font-size: var(--code-size);
    line-height: var(--code-line);
    tab-size: var(--tab);
    color: var(--code-fg, var(--code));
    background: var(--code-bg, transparent);
    border-radius: 10px;
  }
  .line {
    display: grid;
    grid-template-columns: 40px 40px 1fr;
  }
  .line.picking {
    grid-template-columns: 24px 40px 40px 1fr;
  }
  .line.faded > :not(.pick) {
    opacity: 0.45;
  }
  .line.added {
    background: var(--add);
  }
  .pick {
    align-self: center;
    justify-self: center;
    width: 14px;
    height: 14px;
    border-radius: 4px;
    border: 1.2px solid var(--text2);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
  }
  .pick svg {
    width: 11px;
    height: 11px;
    fill: none;
    stroke: transparent;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .pick.on {
    background: var(--accent);
    border-color: var(--accent);
  }
  .pick.on svg {
    stroke: #fff;
  }
  .line.removed {
    background: var(--del);
  }
  .no {
    color: var(--code-dim, var(--text2));
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
  /* Searched code: an outlined neutral capsule, since yellow is for tags. */
  .found {
    border-radius: 4px;
    box-shadow: 0 0 0 1.5px var(--found-ring);
    background: var(--found-bg);
  }
  /* The match Find in file is on. */
  .found:global(.current) {
    box-shadow: 0 0 0 2px var(--text);
  }
  .fold {
    display: block;
    width: 100%;
    margin: 4px 0;
    padding: 0 12px;
    height: 22px;
    border-radius: 9px;
    background: var(--fold-bg, var(--field));
    color: var(--code-dim, var(--text2));
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
