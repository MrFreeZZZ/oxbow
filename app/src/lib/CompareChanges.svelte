<script lang="ts">
  // Every change between the two sides of Compare, as one diff: Summary with the files, then a
  // tab per file, like the commit details.

  import { untrack, type Snippet } from "svelte";
  import { api, stopped } from "./api";
  import { diffSettings, prefs, wholeByDefault } from "./prefs.svelte";
  import type { CompareFile, Comparison, FileChange, FileDiff } from "./types";
  import { lane, plate, shortId, splitPath, tabLabels, tabWindow, tint } from "./format";
  import DiffView from "./DiffView.svelte";
  import FileBadge from "./FileBadge.svelte";
  import FoldAll from "./FoldAll.svelte";

  let {
    comparison: c,
    base,
    target,
    color,
    onPickCommit,
    header,
  }: {
    comparison: Comparison;
    base: string;
    target: string;
    color: number;
    /** Show one commit of the compared side. */
    onPickCommit: (id: string) => void;
    /** Shown in place of the headline, as when two commits are compared in History. */
    header?: Snippet;
  } = $props();

  let diffView = $state<DiffView>();

  /** Lines of diff a page of the Summary brings, about what the diff shows at first. */
  const PAGE_LINES = 4000;
  /** Rows of the Summary's file list drawn at first, and more on each request. */
  const LIST_STEP = 300;
  /** Lanes of this view's reads: a newer read in a lane stops the older one. */
  const COUNTS = "compare-counts";
  const DIFF = "compare-diff";

  let tab = $state<string | null>(null);
  let whole = $state(wholeByDefault());
  let diffs = $state<FileDiff[]>([]);
  /** In Summary, where the next page of diffs starts; null once every file is loaded. */
  let next = $state<number | null>(null);
  let loadingMore = $state(false);
  /** The files with their line counts, once counted, and what they were counted for. */
  let counted = $state<{ key: string; files: FileChange[]; from: number } | null>(null);
  /** The files of Summary's first page, counted by their diffs. */
  let firstFiles = $state<Map<string, FileChange>>(new Map());
  /** Rows of the Summary's file list drawn: thousands of files draw the rest on request. */
  let listed = $state(LIST_STEP);
  let error = $state<string | null>(null);
  let scroller = $state<HTMLDivElement>();
  let tabBar = $state<HTMLDivElement>();

  const paths = $derived(c.files.map((f) => f.path));
  const labels = $derived(tabLabels(paths));
  const shownTabs = $derived(tabWindow(paths.length, tab === null ? -1 : paths.indexOf(tab)));
  /** What the counts are for: the two sides and the settings that change them. */
  const countKey = $derived(`${c.from}\0${c.targetId}\0${diffSettings()}`);
  /** The files as far as they are known: until the counts come, a loaded diff has them. */
  const files = $derived.by<CompareFile[]>(() => {
    const done = counted?.key === countKey ? counted : null;
    if (done) return c.files.map((f, i) => ({ ...f, ...(i < done.from ? (firstFiles.get(f.path) ?? done.files[i]) : done.files[i]) }));
    const loaded = new Map(diffs.map((d) => [d.file.path, d.file]));
    return c.files.map((f) => {
      const file = loaded.get(f.path);
      return file ? { ...f, ...file } : f;
    });
  });
  const totals = $derived(
    counted?.key === countKey ? files.reduce((t, f) => ({ add: t.add + f.additions, del: t.del + f.deletions }), { add: 0, del: 0 }) : null,
  );
  const known = $derived(new Set(counted?.key === countKey ? [] : diffs.map((d) => d.file.path)));
  const authors = $derived([...new Set(c.ahead.map((a) => a.authorName))]);
  const commits = $derived(c.mode === "split" ? c.aheadCount : c.aheadCount + c.behindCount);

  // The view's reads stop when it closes: their answers would go nowhere.
  $effect(() => () => {
    api.stopReads([COUNTS, DIFF]).catch(() => {});
  });

  // A new comparison starts on Summary.
  $effect(() => {
    c.from;
    c.targetId;
    untrack(() => {
      tab = null;
      listed = LIST_STEP;
    });
  });

  // The selected tab may be scrolled out of the tab bar.
  $effect(() => {
    tab;
    requestAnimationFrame(() => tabBar?.querySelector<HTMLElement>('[aria-selected="true"]')?.scrollIntoView({ inline: "nearest", block: "nearest" }));
  });

  /** The counts asked for last, so a new tab or diff mode does not ask again. */
  let countingFor: string | null = null;

  /** Counts the lines of the files after Summary's first page, whose diffs counted theirs. */
  function count(key: string, from: string, to: string, first: FileChange[]) {
    // A diff that ignores whitespace counts fewer lines than the list does: count them all.
    const skip = prefs.get("oxbow.diff.ignoreWhitespace") ? 0 : first.length;
    firstFiles = new Map(first.map((f) => [f.path, f]));
    api.compareCounts(from, to, skip, COUNTS).then(
      (list) => {
        if (countKey === key) counted = { key, files: list, from: skip };
      },
      // Counts are a nicety: the list stays without them.
      () => {},
    );
  }

  $effect(() => {
    const from = c.from;
    const to = c.targetId;
    const path = tab;
    const wholeFile = whole;
    diffSettings();
    const key = countKey;
    untrack(() => {
      diffs = [];
      next = null;
      loadingMore = false;
      error = null;
      const still = () => c.from === from && c.targetId === to && tab === path && whole === wholeFile;
      const failed = (err: unknown) => {
        if (still() && !stopped(err)) error = String(err);
      };
      const counting = counted?.key !== key && countingFor !== key;
      if (counting) countingFor = key;
      if (path === null) {
        api.compareDiffPage(from, to, 0, PAGE_LINES, wholeFile, DIFF).then((page) => {
          if (counting) count(key, from, to, page.diffs.map((d) => d.file));
          if (!still()) return;
          diffs = page.diffs;
          next = page.next;
        }, (err) => {
          if (counting) count(key, from, to, []);
          failed(err);
        });
      } else {
        if (counting) count(key, from, to, []);
        api.compareDiff(from, to, path, wholeFile).then((d) => {
          if (still()) diffs = d;
        }, failed);
      }
    });
  });

  /** The next page of Summary's diffs. */
  function loadMore() {
    if (next === null || loadingMore || tab !== null) return;
    const from = c.from;
    const to = c.targetId;
    const wholeFile = whole;
    const start = next;
    loadingMore = true;
    api.compareDiffPage(from, to, start, PAGE_LINES, wholeFile, DIFF).then(
      (page) => {
        loadingMore = false;
        if (c.from !== from || c.targetId !== to || tab !== null || whole !== wholeFile || next !== start) return;
        diffs = [...diffs, ...page.diffs];
        next = page.next;
      },
      (err) => {
        loadingMore = false;
        if (!stopped(err)) error = String(err);
      },
    );
  }

  function openTab(path: string | null) {
    tab = path;
    if (scroller) scroller.scrollTop = 0;
  }

  const statusIcon: Record<string, string> = { added: "A", deleted: "D", modified: "M", renamed: "R", copied: "C" };
</script>

<div class="panel">
  {#if header}
    {@render header()}
  {:else}
    <div class="head">
      <h1>
        {#if c.mode === "split"}
          What {target} changed since it split from {base}
        {:else}
          How {target} differs from {base}
        {/if}
      </h1>
      <p class="sub">
        {[
          authors.length ? `By ${authors.slice(0, 3).join(", ")}${authors.length > 3 ? ` and ${authors.length - 3} more` : ""}` : null,
          `${c.files.length} ${c.files.length === 1 ? "file" : "files"}`,
          `${commits} ${commits === 1 ? "commit" : "commits"}`,
        ]
          .filter(Boolean)
          .join(" · ")}
      </p>
      {#if c.mode === "tips" && c.behindCount}
        <p class="warn">
          Also shows the {c.behindCount} newer {c.behindCount === 1 ? "commit" : "commits"} on {base} undone, because the tips are compared directly.
          {#if c.mergeBase}Since Split leaves them out.{/if}
        </p>
      {/if}
    </div>
  {/if}

  <div class="tabs" role="tablist" aria-label="Changes">
    <button
      role="tab"
      aria-selected={tab === null}
      class="tab"
      class:on={tab === null}
      style:background={tab === null ? tint(color) : undefined}
      style:border-color={tab === null ? `color-mix(in srgb, ${lane(color)} 40%, transparent)` : undefined}
      style:color={tab === null ? plate(color) : undefined}
      onclick={() => openTab(null)}>Summary</button
    >
    {#if c.files.length}
      <span class="divider" aria-hidden="true"></span>
      <div class="file-tabs" bind:this={tabBar}>
        <!-- Thousands of files draw only the tabs around the open one. -->
        {#if shownTabs.start}<button class="tab neutral more" onclick={() => openTab(paths[shownTabs.start - 1])} title="Open the file before these">+{shownTabs.start.toLocaleString()}</button>{/if}
        {#each paths.slice(shownTabs.start, shownTabs.end) as path (path)}
          <button role="tab" aria-selected={tab === path} class="tab neutral" class:on={tab === path} onclick={() => openTab(path)} title={path}>{labels.get(path)}</button>
        {/each}
        {#if shownTabs.end < paths.length}<button class="tab neutral more" onclick={() => openTab(paths[shownTabs.end])} title="Open the file after these">+{(paths.length - shownTabs.end).toLocaleString()}</button>{/if}
      </div>
    {/if}
  </div>

  <div class="scroll" bind:this={scroller}>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if tab === null}
      {#if c.files.length}
        <div class="files">
          <div class="count">
            <span class="grow">{c.files.length} {c.files.length === 1 ? "file" : "files"} changed</span>
            {#if totals}
              <span class="add">+{totals.add}</span>
              <span class="del">−{totals.del}</span>
            {:else}
              <span class="counting" title="Counting the changed lines">Counting…</span>
            {/if}
            <FoldAll onFold={(fold) => diffView?.setAll(fold)} />
          </div>
          {#each files.slice(0, listed) as file (file.path)}
            {@const p = splitPath(file.path)}
            <div class="file">
              <button class="open" onclick={() => openTab(file.path)}>
                <span class="badge {file.status}">{statusIcon[file.status]}</span>
                <span class="grow ellipsis"><span class="dir">{p.dir}</span>{p.name}</span>
              </button>
              {#if file.onlyBase}
                <span class="meta mono" title="Only {base}'s newer commits changed it">only on {base}</span>
              {:else if file.last}
                <button class="sha mono" onclick={() => onPickCommit(file.last!)} title="The latest commit on {target} that changed it">{shortId(file.last)}</button>
              {/if}
              {#if totals || known.has(file.path)}<FileBadge {file} />{/if}
            </div>
          {/each}
          {#if files.length > listed}
            <button class="file more" onclick={() => (listed += LIST_STEP)}>Show {Math.min(files.length - listed, LIST_STEP).toLocaleString()} more {files.length - listed === 1 ? "file" : "files"}</button>
          {/if}
        </div>
      {:else}
        <p class="empty">No differences: both sides have the same files.</p>
      {/if}
    {/if}
    <DiffView
      bind:this={diffView}
      foldable
      {diffs}
      {color}
      {whole}
      pending={tab === null && next !== null ? c.files.length - diffs.length : 0}
      onMore={loadMore}
      onToggleWhole={() => (whole = !whole)}
      loadFull={(d) => api.compareDiff(c.from, c.targetId, d.file.path, whole, true).then((all) => all[0])}
      command={(d) => `git diff ${c.from.slice(0, 7)} ${c.targetId.slice(0, 7)} -- ${d.file.path}`}
    />
  </div>
</div>

<style>
  .panel {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  /* Headed like a commit's details: no box, the files follow under it. */
  .head {
    padding: var(--space-1) var(--space-5) var(--space-2);
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  h1 {
    margin: 0;
    font-size: var(--text-title);
    font-weight: 600;
    line-height: 1.3;
  }
  .sub {
    margin: 0;
    color: var(--text2);
    font-size: var(--text-small);
  }
  .warn {
    margin: 4px 0 0;
    padding: 8px 12px;
    border-radius: var(--radius-m);
    background: var(--orange-soft);
    color: var(--orange);
    font-size: var(--text-small);
    line-height: 1.45;
  }
  .tabs {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 10px 0 10px 12px;
    flex-shrink: 0;
  }
  .divider {
    width: 1px;
    height: 16px;
    background: var(--sep);
    flex-shrink: 0;
  }
  .file-tabs {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    padding-right: 12px;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .file-tabs::-webkit-scrollbar {
    display: none;
  }
  .tab {
    display: flex;
    align-items: center;
    height: 26px;
    padding: 0 11px;
    border-radius: var(--radius-pill);
    border: 1px solid var(--sep);
    font-size: var(--text-small);
    font-weight: 500;
    color: var(--text2);
    flex-shrink: 0;
  }
  .tab.on {
    font-weight: 600;
  }
  .tab.neutral.on {
    background: var(--side-sel);
    border-color: transparent;
    color: var(--text);
  }
  .scroll {
    flex-grow: 1;
    overflow-y: auto;
    min-height: 0;
  }
  .files {
    margin: var(--space-2) var(--space-3) var(--space-1);
    padding: var(--space-1) 0 6px;
  }
  .count {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px 6px;
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
  }
  .file {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 26px;
    padding: 0 8px 0 0;
    border-radius: var(--radius-s);
  }
  .file:hover {
    background: var(--field);
  }
  .file.more {
    width: 100%;
    justify-content: center;
    color: var(--text2);
  }
  .tab.more {
    color: var(--text2);
    font-weight: 400;
  }
  .counting {
    font-weight: 400;
  }
  .open {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-grow: 1;
    min-width: 0;
    height: 26px;
    padding-left: 8px;
    text-align: left;
  }
  .badge {
    width: 16px;
    height: 16px;
    border-radius: var(--radius-s);
    font-size: var(--text-caption);
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--field);
    color: var(--text2);
    flex-shrink: 0;
  }
  .badge.added {
    color: var(--green);
  }
  .badge.deleted {
    color: var(--red);
  }
  .grow {
    flex-grow: 1;
    min-width: 0;
  }
  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dir {
    color: var(--text2);
  }
  .mono {
    font-family: var(--mono);
  }
  .sha {
    font-size: var(--text-caption);
    color: var(--text2);
    padding: 1px 5px;
    border-radius: var(--radius-s);
  }
  .sha:hover {
    background: var(--side-sel);
    color: var(--text);
  }
  .add {
    color: var(--green);
    font-size: var(--text-caption);
  }
  .del {
    color: var(--red);
    font-size: var(--text-caption);
  }
  .num {
    min-width: 22px;
    text-align: right;
  }
  .meta {
    font-size: var(--text-caption);
    color: var(--text2);
    white-space: nowrap;
  }
  .empty,
  .error {
    margin: 16px 20px;
    color: var(--text2);
  }
  .error {
    color: var(--red);
  }
</style>
