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
  import { untrack } from "svelte";
  import { foldReason, startsFolded } from "./diffFold";
  import { mac } from "./keys";
  import ImageDiff from "./ImageDiff.svelte";
  import UnusualCard from "./UnusualCard.svelte";
  import LfsCard from "./LfsCard.svelte";
  import { BIG_FILE } from "./lfs.svelte";
  import { bytes, imageType, isSvg } from "./unusual";

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
    foldable = false,
    loadFull,
    command,
    working = false,
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
    /** Each file folds to its header; Smart, Expanded or Collapsed in Settings › Diff & Text says which start folded. */
    foldable?: boolean;
    /** The file's diff past the line limit in Settings, for Show Diff Anyway. */
    loadFull?: (diff: FileDiff) => Promise<FileDiff>;
    /** The `git diff` that shows a file's change, for the binary card. */
    command?: (diff: FileDiff) => string | null;
    /** Uncommitted changes: a big file offers Git LFS, an LFS file Stop Tracking. */
    working?: boolean;
  } = $props();

  /** Diffs loaded in full with Show Diff Anyway, by the very diff they stand in for: a diff
   *  loaded again (the file changed, the other side) is a new one and shows as it is. */
  let full = $state.raw(new Map<FileDiff, FileDiff>());
  /** Files whose line endings are shown, by path. */
  let showEol = $state<Record<string, boolean>>({});
  /** SVG files shown as code instead of as a picture, by path. */
  let showCode = $state<Record<string, boolean>>({});
  $effect(() => {
    fileSet;
    untrack(() => {
      full = new Map();
      showEol = {};
      showCode = {};
    });
  });
  const items = $derived(diffs.map((d) => full.get(d) ?? d));

  /** `map[key]`, only when `key` is its own: a file may be called `constructor` or `__proto__`. */
  const own = <T,>(map: Record<string, T>, key: string): T | undefined => (Object.hasOwn(map, key) ? map[key] : undefined);

  function showAnyway(diff: FileDiff) {
    loadFull?.(diff).then(
      // Only while that diff is still shown: a late answer doesn't bring back an old one.
      (d) => diffs.includes(diff) && (full = new Map(full).set(diff, d)),
      (err) => confirm.say(String(err)),
    );
  }

  /** What stands in for the code: a picture, or a card about a change a line diff can't show. */
  function special(diff: FileDiff): "lfs" | "big" | "image" | "binary" | "limited" | "tooLarge" | "mode" | "eol" | null {
    const f = diff.file;
    // What git diffs for an LFS file is its pointer, not the picture or the video.
    if (f.lfs) return "lfs";
    if (working && (f.newSize ?? 0) >= BIG_FILE && f.status !== "deleted") return "big";
    if (f.binary && imageType(f.path) && (diff.old || diff.new)) return "image";
    if (isSvg(f.path) && !own(showCode, f.path) && (diff.old || diff.new)) return "image";
    if (f.binary) return "binary";
    if (diff.limited) return "limited";
    if (diff.tooLarge) return "tooLarge";
    if (f.mode && diff.hunks.length === 0) return "mode";
    if (f.eol && !own(showEol, f.path)) return "eol";
    return null;
  }

  /** The header's path: a rename within one folder reads `dir/old.rs → new.rs`. */
  function renamedName(diff: FileDiff): string | null {
    const old = diff.file.oldPath;
    if (!old) return null;
    const o = splitPath(old);
    return o.dir === splitPath(diff.file.path).dir ? o.name : null;
  }

  /** Folded files, by path. */
  let folded = $state<Record<string, boolean>>({});
  // Another commit starts from the setting; the same files loaded again (the whole-file toggle)
  // keep what was folded by hand.
  const fileSet = $derived(diffs.map((d) => d.file.path).join("\n"));
  $effect(() => {
    const key = fileSet;
    prefs.get("oxbow.diff.files");
    prefs.get("oxbow.diff.foldOver");
    if (!foldable || !key) return;
    untrack(() => {
      // A file shown on its own tab is what was asked for, so it starts open.
      folded = Object.fromEntries(diffs.map((d) => [d.file.path, diffs.length > 1 && startsFolded(d)]));
    });
  });

  const isFolded = (diff: FileDiff) => foldable && !!own(folded, diff.file.path);

  /** Fold or unfold every file, from the file list's buttons or ⌥-click on a chevron. */
  export function setAll(fold: boolean) {
    folded = Object.fromEntries(diffs.map((d) => [d.file.path, fold]));
    if (!fold) showAll = true;
  }

  function toggle(diff: FileDiff, event: MouseEvent) {
    const fold = !isFolded(diff);
    if (event.altKey) setAll(fold);
    else folded = { ...folded, [diff.file.path]: fold };
  }

  /** Open the file in the editor, at its first change. */
  function openInEditor(diff: FileDiff) {
    const first = diff.hunks[0]?.lines.find((l) => l.kind !== "context" && l.newLine !== null) ?? diff.hunks[0]?.lines[0];
    api.openInEditor(diff.file.path, first?.newLine ?? null).catch((err) => confirm.say(String(err)));
  }

  let showAll = $state(false);

  /** Files to render, cut off once the line budget is spent so huge commits stay responsive. */
  const shown = $derived.by(() => {
    if (showAll) return { files: items, hidden: 0 };
    let budget = lineBudget;
    const files: FileDiff[] = [];
    for (const diff of items) {
      if (budget <= 0) break;
      files.push(diff);
      // A folded file draws no lines.
      if (!isFolded(diff)) budget -= diff.hunks.reduce((n, h) => n + h.lines.length, 0);
    }
    return { files, hidden: items.length - files.length };
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

{#snippet name(diff: FileDiff)}
  {@const path = splitPath(diff.file.path)}
  {@const old = renamedName(diff)}
  {#if old}
    <span class="path"><span class="dir">{path.dir}</span>{old} <span class="dir">→</span> {path.name}</span>
  {:else}
    <span class="path"><span class="dir">{path.dir}</span>{path.name}</span>
    {#if diff.file.oldPath}<span class="from">from {diff.file.oldPath}</span>{/if}
  {/if}
{/snippet}

{#each shown.files as diff (diff.file.path)}
  {@const path = splitPath(diff.file.path)}
  {@const isShut = isFolded(diff)}
  {@const why = isShut ? foldReason(diff) : null}
  {@const kind = isShut ? null : special(diff)}
  <section class="file" class:shut={isShut} data-path={diff.file.path}>
    <header>
      <div class="bar" style:background={tint(color, "bar")} style:--name={plate(color)}>
        {#if foldable}
          <button
            class="head"
            onclick={(e) => toggle(diff, e)}
            aria-expanded={!isShut}
            title={`${isShut ? "Show" : "Hide"} the diff. ${mac ? "⌥-click" : "Alt+click"}: every file`}
          >
            <svg class="icon chevron" class:shut={isShut} viewBox="0 0 16 16"><path d="M4.5 6 8 9.5 11.5 6" /></svg>
            {@render name(diff)}
            <span class="status">{statusLabel[diff.file.status]}</span>
            {#if why && why !== statusLabel[diff.file.status]}<span class="why">· {why}</span>{/if}
            <span class="spacer"></span>
          </button>
        {:else}
          {@render name(diff)}
          <span class="status">{statusLabel[diff.file.status]}</span>
          <span class="spacer"></span>
        {/if}
        {#if diff.file.lfs}
          {@const f = diff.file}
          <span class="mono size" title="Kept in Git LFS">LFS · {f.oldSize !== undefined && f.newSize !== undefined && f.oldSize !== f.newSize ? `${bytes(f.oldSize)} → ${bytes(f.newSize)}` : bytes(f.newSize ?? f.oldSize ?? 0)}</span>
        {:else if diff.file.binary}
          {@const f = diff.file}
          <span class="mono size">{f.oldSize !== undefined && f.newSize !== undefined ? `${bytes(f.oldSize)} → ${bytes(f.newSize)}` : bytes(f.newSize ?? f.oldSize ?? 0)}</span>
        {:else if diff.file.mode && diff.file.additions + diff.file.deletions === 0}
          <span class="mono size">mode {diff.file.mode.old} → {diff.file.mode.new}</span>
        {:else}
          <span class="mono add">+{diff.file.additions}</span>
          <span class="mono del">−{diff.file.deletions}</span>
        {/if}
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

    {#if !isShut}
      {#if isSvg(diff.file.path) && (diff.old || diff.new)}
        <div class="banner">
          <svg class="icon" viewBox="0 0 16 16"><rect x="2" y="2.5" width="12" height="11" rx="2" /><circle cx="6" cy="6.5" r="1.3" /><path d="M2.5 12l3.5-3.5 3 3 2-2 2.5 2.5" /></svg>
          <span class="grow">{own(showCode, diff.file.path) ? "SVG code. The picture it draws is one click away." : "SVG picture, drawn from the file. Its code is one click away."}</span>
          <button
            class="chip"
            aria-pressed={!!own(showCode, diff.file.path)}
            onclick={() => (showCode = { ...showCode, [diff.file.path]: !own(showCode, diff.file.path) })}>{own(showCode, diff.file.path) ? "Show Picture" : "Show Code"}</button
          >
        </div>
      {/if}
      {#if diff.file.eol}
        <div class="banner">
          <svg class="icon" viewBox="0 0 16 16"><path d="M12.5 3.5v5a2 2 0 0 1-2 2h-7M6 8l-2.5 2.5L6 13" /></svg>
          <span class="grow">Only line endings changed: {diff.file.eol.from} → {diff.file.eol.to} in {diff.file.eol.lines} {diff.file.eol.lines === 1 ? "line" : "lines"}.</span>
          <button
            class="chip"
            class:on={own(showEol, diff.file.path)}
            aria-pressed={!!own(showEol, diff.file.path)}
            onclick={() => (showEol = { ...showEol, [diff.file.path]: !own(showEol, diff.file.path) })}>{own(showEol, diff.file.path) ? "Hide Line Endings" : "Show Line Endings"}</button
          >
        </div>
      {:else if diff.file.oldPath && diff.file.similarity !== undefined && diff.hunks.length > 0}
        <div class="banner">
          <svg class="icon" viewBox="0 0 16 16"><path d="M2.5 8h9M8.5 5l3 3-3 3M13.5 3.5v9" /></svg>
          <span class="grow"
            >{diff.file.status === "copied" ? "Copied" : "Renamed"} from {diff.file.oldPath} · {diff.file.similarity}% similar, so git shows it as a
            {diff.file.status === "copied" ? "copy" : "rename"} with a small diff.</span
          >
        </div>
      {/if}
      {#if diff.file.mode && diff.hunks.length > 0}
        <div class="banner">
          <svg class="icon" viewBox="0 0 16 16"><rect x="1.5" y="2.5" width="13" height="11" rx="2" /><path d="M4.5 6l2 2-2 2M8 10.5h3" /></svg>
          <span class="grow">Mode {diff.file.mode.old} → {diff.file.mode.new} too{diff.file.mode.new === "100755" ? ": the file is executable now" : ""}.</span>
        </div>
      {/if}
    {/if}
    {#if isShut}
      <!-- Folded: the header alone. -->
    {:else if kind === "lfs" || kind === "big"}
      <LfsCard {diff} {kind} {working} />
    {:else if kind === "image"}
      <ImageDiff {diff} />
    {:else if kind}
      <UnusualCard {diff} {kind} {openable} command={command?.(diff) ?? null} onShowAnyway={loadFull ? () => showAnyway(diff) : undefined} />
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
              <span class="text">{#each paint(line.text || " ", prefs.get("oxbow.diff.wordHighlight") ? line.words : null, language, find, matchCase) as piece, w (w)}<span class:word={piece.changed} class:found={piece.found} data-hit={piece.hit} class={piece.role ? `syn-${piece.role}` : undefined}>{piece.text}</span>{/each}{#if line.cr && own(showEol, diff.file.path)}<span class="cr" class:word={line.kind !== "context"} title="Carriage return: this line ends with CRLF">CR</span>{/if}</span>
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
  .file.shut {
    padding-bottom: 0;
  }
  .head {
    flex-grow: 1;
    min-width: 0;
    align-self: stretch;
    display: flex;
    align-items: center;
    gap: 8px;
    margin-left: -6px;
    padding-left: 4px;
    border-radius: 8px;
  }
  .chevron {
    width: 14px;
    height: 14px;
    color: var(--name);
    transition: transform 0.15s ease;
  }
  .chevron.shut {
    transform: rotate(-90deg);
  }
  .why {
    color: var(--text2);
    font-size: 11px;
    white-space: nowrap;
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
  .size {
    color: var(--text2);
    font-size: 11px;
    white-space: nowrap;
  }
  .banner {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 30px;
    margin: 2px 0 8px;
    padding: 4px 4px 4px 10px;
    border-radius: 9px;
    background: var(--field);
    font-size: 12px;
  }
  .banner .icon {
    width: 13px;
    height: 13px;
    flex-shrink: 0;
    color: var(--text2);
  }
  .banner .grow {
    flex-grow: 1;
    min-width: 0;
  }
  .chip {
    height: 22px;
    padding: 0 10px;
    border-radius: 11px;
    background: var(--win);
    font-size: 12px;
    white-space: nowrap;
    flex-shrink: 0;
  }
  .chip.on {
    font-weight: 600;
  }
  /* A carriage return, shown on request where only line endings changed. */
  .cr {
    margin-left: 4px;
    padding: 0 4px;
    border-radius: 5px;
    font-size: 0.85em;
    color: var(--text2);
    background: var(--field);
    user-select: none;
    -webkit-user-select: none;
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
