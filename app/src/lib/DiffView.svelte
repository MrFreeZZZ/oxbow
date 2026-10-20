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
  import type { DiffLine, FileDiff, Hunk, Side } from "./types";
  import { plate, splitPath, tint } from "./format";
  import { prefs } from "./prefs.svelte";
  import { api, stopped } from "./api";
  import { confirm } from "./confirm.svelte";
  import { languageOf, occurrences, paint } from "./syntax";
  import Lazy, { BLOCK, type FindHits } from "./Lazy.svelte";
  import { nav } from "./nav.svelte";
  import { untrack } from "svelte";
  import { foldReason, startsFolded } from "./diffFold";
  import { changedRow, changeStarts, rowsOf, sideKind, type Row } from "./diffRows";
  import { mac } from "./keys";
  import ImageDiff from "./ImageDiff.svelte";
  import UnusualCard from "./UnusualCard.svelte";
  import LfsCard from "./LfsCard.svelte";
  import SubmoduleCard from "./SubmoduleCard.svelte";
  import { BIG_FILE } from "./lfs.svelte";
  import { bytes, imageType, isSvg } from "./unusual";

  let {
    diffs,
    color,
    whole,
    onToggleWhole,
    lineBudget = 4000,
    hunkBar,
    fileActions,
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
    side = null,
    pending = 0,
    onMore,
    focus = null,
    hits = $bindable([]),
  }: {
    diffs: FileDiff[];
    color: number;
    whole: boolean;
    onToggleWhole: () => void;
    lineBudget?: number;
    /** Actions over each hunk, for the working copy. */
    hunkBar?: Snippet<[FileDiff, Hunk]>;
    /** Actions for the whole file on its header bar, for the working copy. */
    fileActions?: Snippet<[FileDiff]>;
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
    /** Which side of the uncommitted changes these are, for what a submodule's card says. */
    side?: Side | null;
    /** Files after `diffs` whose diffs are not loaded yet. */
    pending?: number;
    /** Load the diffs of more of the pending files. */
    onMore?: () => void;
    /** The block with the current match of `find`, drawn wherever it is so it can be scrolled to. */
    focus?: string | null;
    /** The blocks of lines drawn with matches of `find`, in order, and how many each has. */
    hits?: FindHits;
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
      // Stopped by a newer one, or as another commit was picked.
      (err) => stopped(err) || confirm.fail(err),
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
  let foldedFor = "";
  $effect(() => {
    const key = fileSet;
    prefs.get("oxbow.diff.files");
    prefs.get("oxbow.diff.foldOver");
    if (!foldable || !key) return;
    untrack(() => {
      // More files of the same commit keep how the ones already shown are folded.
      const kept = foldedFor && key.startsWith(foldedFor + "\n") ? folded : {};
      foldedFor = key;
      // A file shown on its own tab is what was asked for, so it starts open.
      const many = diffs.length + pending > 1;
      folded = Object.fromEntries(
        diffs.map((d) => [d.file.path, own(kept, d.file.path) ?? (many && startsFolded(d))]),
      );
    });
  });

  const isFolded = (diff: FileDiff) => foldable && !!own(folded, diff.file.path);

  /** Fold or unfold every file, from the file list's buttons or ⌥-click on a chevron. */
  export function setAll(fold: boolean) {
    folded = Object.fromEntries(diffs.map((d) => [d.file.path, fold]));
    if (!fold) extra += PORTION;
  }

  function toggle(diff: FileDiff, event: MouseEvent) {
    const fold = !isFolded(diff);
    if (event.altKey) setAll(fold);
    else folded = { ...folded, [diff.file.path]: fold };
  }

  /** Open the file in the editor, at its first change. */
  function openInEditor(diff: FileDiff) {
    const first = diff.hunks[0]?.lines.find((l) => l.kind !== "context" && l.newLine !== null) ?? diff.hunks[0]?.lines[0];
    api.openInEditor(diff.file.path, first?.newLine ?? null).catch((err) => confirm.fail(err));
  }

  /** Lines shown past `lineBudget`: each Show More adds a portion, never everything at once. */
  let extra = $state(0);
  const PORTION = 20_000;
  /** What a folded file's header, or a card in place of the code, costs of the budget, in lines. */
  const FOLDED_LINES = 8;
  /** What a picture costs of the budget, in lines: about the room it takes. */
  const PICTURE_LINES = 24;
  // Other diffs start from the budget again; more files loaded after the first keep it.
  const first = $derived(diffs[0]);
  $effect(() => {
    first;
    untrack(() => (extra = 0));
  });

  /** Files to render, cut off once the line budget is spent so huge commits stay responsive. A
   *  file much longer than what is left of the budget is cut too: `cut` is how many lines of
   *  each of its hunks to draw, and `more` how many are left out. */
  const shown = $derived.by(() => {
    type Cut = { diff: FileDiff; lines: number[]; more: number };
    let budget = lineBudget + extra;
    const files: FileDiff[] = [];
    let cut: Cut | null = null;
    for (const diff of items) {
      if (budget <= 0) break;
      files.push(diff);
      // A folded file draws only its header.
      if (isFolded(diff)) {
        budget -= FOLDED_LINES;
        continue;
      }
      // A card or a picture takes room too: without a cost, a run of them would never spend
      // the budget, and pages would load on and on.
      const drawn = diff.hunks.reduce((n, h) => n + h.lines.length, 0);
      const total = Math.max(drawn, special(diff) === "image" ? PICTURE_LINES : FOLDED_LINES);
      if (drawn > budget + CUT_SLACK) {
        let left = budget;
        const lines = diff.hunks.map((h) => {
          const take = Math.min(left, h.lines.length);
          left -= take;
          return take;
        });
        cut = { diff, lines, more: drawn - budget };
        budget = 0;
        break;
      }
      budget -= total;
    }
    return { files, hidden: items.length - files.length, cut, left: budget };
  });

  // Files still to load while the budget isn't spent, e.g. as the first ones start folded.
  $effect(() => {
    if (pending > 0 && shown.hidden === 0 && !shown.cut && shown.left > 0) untrack(() => onMore?.());
  });

  /** More files: the ones loaded but not shown, then more of the ones still to load. */
  function moreFiles() {
    extra += PORTION;
    if (shown.hidden === 0) onMore?.();
  }

  /** Starts of the blocks a run of `count` lines is drawn in. */
  function blocks(count: number): number[] {
    const out: number[] = [];
    for (let at = 0; at < count; at += BLOCK) out.push(at);
    return out;
  }

  /** Old and new lines next to each other, from Settings › Diff & Text or this view's button. */
  const split = $derived(prefs.get("oxbow.diff.layout") === "split");
  /** As VS Code's Toggle Inline View, the button changes the setting, so every diff follows. */
  const toggleLayout = () => prefs.set("oxbow.diff.layout", split ? "unified" : "split");

  /** Where changes start among a hunk's lines `start` to `end`, for Next Change. */
  const lineMarks = (hunk: Hunk, start: number, end: number) => changeStarts((i) => hunk.lines[i].kind !== "context", start, end);
  /** Where changes start among Side by Side rows `start` to `end`. */
  const rowMarks = (rows: Row[], start: number, end: number) => changeStarts((i) => changedRow(rows[i]), start, end);

  /** Name of the block of a hunk's lines from `start`, for Find in file to step to it. */
  const blockKey = (diff: FileDiff, h: number, start: number) => `${diff.file.path}:${h}:${start}`;

  // The matches counted from the lines, not from what is drawn: a block far from the screen
  // isn't drawn, and drawing every block with a match would undo drawing only what is seen.
  const found = $derived.by(() => {
    const out: FindHits = [];
    if (!find) return out;
    for (const diff of shown.files) {
      if (isFolded(diff) || diff.submodule || special(diff) || diff.hunks.length === 0) continue;
      const cut = shown.cut?.diff === diff ? shown.cut : null;
      diff.hunks.forEach((hunk, h) => {
        const count = cut ? cut.lines[h] : hunk.lines.length;
        if (split) {
          // Side by Side draws an unchanged line twice: its matches count once, on the new side.
          const rows = rowsOf(hunk, count);
          const hitsOf = (at: number | null) => (at === null ? 0 : occurrences(hunk.lines[at].text || " ", find, matchCase).length);
          for (const start of blocks(rows.length)) {
            let n = 0;
            for (let r = start; r < Math.min(rows.length, start + BLOCK); r++) {
              const row = rows[r];
              n += (changedRow(row) ? hitsOf(row.left) : 0) + hitsOf(row.right);
            }
            if (n) out.push({ key: blockKey(diff, h, start), count: n });
          }
          return;
        }
        for (const start of blocks(count)) {
          let n = 0;
          for (let i = start; i < Math.min(count, start + BLOCK); i++) n += occurrences(hunk.lines[i].text || " ", find, matchCase).length;
          if (n) out.push({ key: blockKey(diff, h, start), count: n });
        }
      });
    }
    return out;
  });
  $effect(() => {
    hits = found;
  });
  /** A file is cut only when this many lines more than the budget allows would be left out. */
  const CUT_SLACK = 500;

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

  /** Digits of the longest line number, so the number columns fit it. */
  function digits(diff: FileDiff): number {
    const last = diff.hunks.reduce((n, h) => Math.max(n, h.oldStart + h.oldLines, h.newStart + h.newLines), 0);
    return Math.max(3, String(last).length);
  }

  /** Rounded corners of one side of a Side by Side row, at the ends of a run of its kind. */
  function sideRadius(lines: DiffLine[], rows: Row[], r: number, side: "left" | "right"): string {
    const kind = sideKind(lines, rows[r], side);
    if (kind === "context") return "0";
    const top = sideKind(lines, rows[r - 1], side) !== kind ? 9 : 0;
    const bottom = sideKind(lines, rows[r + 1], side) !== kind ? 9 : 0;
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

<!-- A line's code: syntax colors, changed words, matches of Find. `counted` is false for the
     old side of an unchanged line in Side by Side, whose matches are the new side's again. -->
{#snippet code(diff: FileDiff, line: DiffLine, language: ReturnType<typeof languageOf>, counted: boolean)}<span class="text">{#each paint(line.text || " ", prefs.get("oxbow.diff.wordHighlight") ? line.words : null, language, find, matchCase) as piece, w (w)}<span class:word={piece.changed} class:found={piece.found && counted} class:twin={piece.found && !counted} data-hit={counted ? piece.hit : undefined} class={piece.role ? `syn-${piece.role}` : undefined}>{piece.text}</span>{/each}{#if line.cr && own(showEol, diff.file.path)}<span class="cr" class:word={line.kind !== "context"} title="Carriage return: this line ends with CRLF">CR</span>{/if}{#if line.secret}<span class="secret" title="Looks like a secret: Oxbow stops the commit until it is unstaged, or you commit anyway. Add oxbow:allow to the line if it is not one."><svg viewBox="0 0 16 16"><path d="M4.5 7.5h7v6h-7zM6 7.5V5.5a2 2 0 0 1 4 0v2" /></svg>{line.secret}</span>{/if}</span>{/snippet}

{#snippet pickBox(hunk: Hunk, line: DiffLine, i: number, on: boolean)}
  <button
    class="pick"
    class:on
    role="checkbox"
    aria-checked={on}
    aria-label="{on ? 'Unselect' : 'Select'} line {line.newLine ?? line.oldLine}"
    title="Pick this line; Shift-click picks a range"
    onclick={(e) => picker!.toggle(hunk, i, e.shiftKey)}
  ><svg viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg></button>
{/snippet}

<!-- One side of a Side by Side row: the old line on the left, the new one on the right. -->
{#snippet half(diff: FileDiff, hunk: Hunk, rows: Row[], r: number, side: "left" | "right", language: ReturnType<typeof languageOf>, picking: boolean, picked: number[])}
  {@const at = side === "left" ? rows[r].left : rows[r].right}
  {@const line = at === null ? null : hunk.lines[at]}
  {@const on = at !== null && picked.includes(at)}
  <div
    class="half {line ? line.kind : 'empty'}"
    class:picking
    class:faded={picked.length > 0 && !!line && line.kind !== "context" && !on}
    style:border-radius={sideRadius(hunk.lines, rows, r, side)}
  >
    {#if picking}
      {#if line && at !== null && line.kind !== "context"}{@render pickBox(hunk, line, at, on)}{:else}<span></span>{/if}
    {/if}
    <span class="no">{line ? ((side === "left" ? line.oldLine : line.newLine) ?? "") : ""}</span>
    {#if line}{@render code(diff, line, language, side === "right" || line.kind !== "context")}{/if}
  </div>
{/snippet}

{#each shown.files as diff (diff.file.path)}
  {@const path = splitPath(diff.file.path)}
  {@const isShut = isFolded(diff)}
  {@const why = isShut ? foldReason(diff) : null}
  {@const kind = isShut ? null : special(diff)}
  <section class="file" class:shut={isShut} data-path={diff.file.path}>
    <header>
      <div class="bar" style:background={tint(color, "head")} style:--name={plate(color)}>
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
        {#if diff.file.submodule}
          <span class="size">Submodule</span>
        {:else if diff.file.lfs}
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
        {#if fileActions}{@render fileActions(diff)}{/if}
        {#if openable && history && diff.file.status !== "untracked" && !diff.file.submodule}
          <button class="toggle" onclick={() => nav.openFile(diff.file.path, commit)} aria-label="File history" title="File History and Blame">
            <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="5.5" /><path d="M8 5v3.2l2 1.3" /></svg>
          </button>
        {/if}
        {#if openable && diff.file.status !== "deleted" && !diff.file.submodule}
          <button class="toggle" onclick={() => openInEditor(diff)} aria-label="Open in editor" title="Open in Editor">
            <svg class="icon" viewBox="0 0 16 16"><path d="M9.5 2.5h4v4M13.5 2.5 7.5 8.5M12 9.5v3a1 1 0 0 1-1 1H3.5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h3" /></svg>
          </button>
        {/if}
        <button
          class="toggle"
          class:on={split}
          onclick={toggleLayout}
          aria-pressed={split}
          aria-label={split ? "Show unified" : "Show side by side"}
          title={split ? "Show old and new lines one under the other" : "Show old and new lines side by side"}
        >
          <svg class="icon" viewBox="0 0 16 16">
            {#if split}<rect x="2" y="2.5" width="12" height="11" rx="2" /><path d="M8 2.5v11M4 6h2M10 6h2M4 9h2M10 9h2" />{:else}<rect x="2" y="2.5" width="12" height="11" rx="2" /><path d="M5 6h6M5 9h6" />{/if}
          </svg>
        </button>
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
    {:else if diff.submodule}
      <SubmoduleCard move={diff.submodule} {side} />
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
      {@const cut = shown.cut?.diff === diff ? shown.cut : null}
      <div class="code mono selectable" style:--digits={digits(diff)}>
        {#each cut ? diff.hunks.filter((_, h) => cut.lines[h] > 0) : diff.hunks as hunk, h (h)}
          {@const before = h === 0 ? hunk.oldStart - 1 : hunk.oldStart - (diff.hunks[h - 1].oldStart + diff.hunks[h - 1].oldLines)}
          {#if before > 0 && !whole}
            <button class="fold" onclick={onToggleWhole} title="Show whole file">⋯ {before} unchanged {before === 1 ? "line" : "lines"}</button>
          {/if}
          {#if hunkBar}{@render hunkBar(diff, hunk)}{/if}
          {@const picking = !!picker?.enabled(hunk)}
          {@const picked = picking ? picker!.picked(hunk) : []}
          {@const count = cut ? cut.lines[h] : hunk.lines.length}
          {#if split}
            {@const rows = rowsOf(hunk, count)}
            {#each blocks(rows.length) as start (start)}
              {@const end = Math.min(rows.length, start + BLOCK)}
              <Lazy
                lines={end - start}
                of={hunk.lines[rows[start].left ?? rows[start].right ?? 0]}
                name={blockKey(diff, h, start)}
                force={focus === blockKey(diff, h, start)}
                marks={rowMarks(rows, start, end)}
              >
                {#each rows.slice(start, end) as _, j (j)}
                  <div class="row">
                    {@render half(diff, hunk, rows, start + j, "left", language, picking, picked)}
                    {@render half(diff, hunk, rows, start + j, "right", language, picking, picked)}
                  </div>
                {/each}
              </Lazy>
            {/each}
          {:else}
            {#each blocks(count) as start (start)}
              {@const end = Math.min(count, start + BLOCK)}
              <Lazy lines={end - start} of={hunk.lines[start]} name={blockKey(diff, h, start)} force={focus === blockKey(diff, h, start)} marks={lineMarks(hunk, start, end)}>
                {#each hunk.lines.slice(start, end) as line, j (j)}
                  {@const i = start + j}
                  {@const on = picked.includes(i)}
                  <div
                    class="line {line.kind}"
                    class:picking
                    class:faded={picked.length > 0 && line.kind !== "context" && !on}
                    style:border-radius={radius(hunk.lines, i)}
                    style:margin-top="{gap(hunk.lines, i)}px"
                  >
                    {#if picking}
                      {#if line.kind !== "context"}{@render pickBox(hunk, line, i, on)}{:else}<span></span>{/if}
                    {/if}
                    <span class="no">{line.oldLine ?? ""}</span>
                    <span class="no">{line.newLine ?? ""}</span>
                    {@render code(diff, line, language, true)}
                  </div>
                {/each}
              </Lazy>
            {/each}
          {/if}
        {/each}
      </div>
      {#if cut}
        <button class="more lines" onclick={() => (extra += PORTION)}
          >Show {Math.min(cut.more, PORTION).toLocaleString()} more {Math.min(cut.more, PORTION) === 1 ? "line" : "lines"}{#if cut.more > PORTION}<span class="of"> of {cut.more.toLocaleString()}</span>{/if}</button
        >
      {/if}
    {/if}
  </section>
{/each}

{#if shown.hidden + pending > 0}
  <button class="more" onclick={moreFiles}>Show {(shown.hidden + pending).toLocaleString()} more {shown.hidden + pending === 1 ? "file" : "files"}</button>
{/if}

<style>
  .file {
    padding: 0 var(--space-3) var(--space-4);
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
    border-radius: var(--radius-m);
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
    font-size: var(--text-caption);
    white-space: nowrap;
  }
  /* Each file starts with a bar in the branch color, so files don't run together. */
  header {
    position: sticky;
    top: 0;
    z-index: 1;
    padding: var(--space-2) 0;
    background: var(--win);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 var(--space-1) 0 var(--space-3);
    border-radius: var(--radius-m);
    font-size: var(--text-small);
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
    font-size: var(--text-caption);
    white-space: nowrap;
  }
  .spacer {
    flex-grow: 1;
  }
  .add {
    color: var(--green);
    font-size: var(--text-caption);
  }
  .del {
    color: var(--red);
    font-size: var(--text-caption);
  }
  .toggle {
    width: 26px;
    height: 26px;
    border-radius: var(--radius-pill);
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
    font-size: var(--text-caption);
    white-space: nowrap;
  }
  .banner {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 30px;
    margin: 2px 0 8px;
    padding: var(--space-1) var(--space-1) var(--space-1) var(--space-3);
    border-radius: var(--radius-m);
    background: var(--field);
    font-size: var(--text-small);
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
    border-radius: var(--radius-pill);
    background: var(--win);
    font-size: var(--text-small);
    white-space: nowrap;
    flex-shrink: 0;
  }
  .chip.on {
    font-weight: 600;
  }
  /* A carriage return, shown on request where only line endings changed. */
  .secret {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    margin-left: 8px;
    padding: 0 7px 0 5px;
    border-radius: var(--radius-s);
    font-family: var(--font);
    font-size: var(--text-caption);
    font-weight: 500;
    color: var(--orange);
    background: var(--orange-soft);
    vertical-align: 1px;
    user-select: none;
    -webkit-user-select: none;
  }
  .secret svg {
    width: 11px;
    height: 11px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linejoin: round;
  }
  .cr {
    margin-left: 4px;
    padding: 0 4px;
    border-radius: var(--radius-s);
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
    border-radius: var(--radius-l);
  }
  /* The number columns fit the longest line number at the code's size. */
  .code {
    --empty-side: color-mix(in srgb, var(--text2) 10%, transparent);
  }
  .line {
    --no: calc(var(--digits, 3) * 1ch + 18px);
    display: grid;
    grid-template-columns: var(--no) var(--no) 1fr;
  }
  .line.picking {
    grid-template-columns: 24px var(--no) var(--no) 1fr;
  }
  .line.faded > :not(.pick),
  .half.faded > :not(.pick) {
    opacity: 0.45;
  }
  /* Side by Side: the old side and the new side as two equal columns, a row as tall as the
     taller of its lines, so the sides stay level as they wrap. */
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    column-gap: 4px;
  }
  .half {
    --no: calc(var(--digits, 3) * 1ch + 18px);
    display: grid;
    grid-template-columns: var(--no) minmax(0, 1fr);
    align-content: start;
  }
  .half.picking {
    grid-template-columns: 24px var(--no) minmax(0, 1fr);
  }
  .half.added {
    background: var(--add);
  }
  .half.removed {
    background: var(--del);
  }
  /* No line on this side: soft hatching, as VS Code marks it, so the eye keeps the rows level. */
  .half.empty {
    background: repeating-linear-gradient(-45deg, transparent 0 6px, var(--empty-side) 6px 7px) var(--code-filler, transparent);
  }
  .line.added {
    background: var(--add);
  }
  .pick {
    align-self: center;
    justify-self: center;
    width: 14px;
    height: 14px;
    border-radius: var(--radius-xs);
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
    /* A theme's own line number color, or the comment color softened. */
    color: var(--code-ln, color-mix(in srgb, var(--code-dim, var(--text2)) 70%, transparent));
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
    border-radius: var(--radius-s);
    padding: 1px 0;
  }
  .removed .word {
    background: var(--del-word);
    border-radius: var(--radius-s);
    padding: 1px 0;
  }
  /* Searched code: an outlined neutral capsule, since yellow is for tags. */
  .found {
    border-radius: var(--radius-xs);
    box-shadow: 0 0 0 1.5px var(--found-ring);
    background: var(--found-bg);
  }
  .twin {
    border-radius: var(--radius-xs);
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
    line-height: 22px;
    border-radius: var(--radius-m);
    background: var(--fold-bg, var(--field));
    color: var(--code-dim, var(--text2));
    font-family: var(--font);
    font-size: var(--text-caption);
  }
  .more.lines {
    margin: 10px 8px 0;
  }
  .of {
    color: var(--text2);
  }
  .more {
    margin: 0 20px 20px;
    height: 28px;
    padding: 0 12px;
    border-radius: var(--radius-pill);
    background: var(--field);
  }
</style>
