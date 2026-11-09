<script lang="ts">
  // Every change between the two sides of Compare, or the diff of a selected file.

  import { untrack, type Snippet } from "svelte";
  import { quoteGitArg } from "./compareRefs";
  import { api, stopped } from "./api";
  import { diffSettings, prefs, wholeByDefault } from "./prefs.svelte";
  import type { CompareFile, Comparison, FileChange, FileDiff, DiffPage } from "./types";
  import { shortId } from "./format";
  import { CountTurns } from "./counting";
  import DiffView from "./DiffView.svelte";
  import ComparisonDiffPane from "./ComparisonDiffPane.svelte";

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
  /** Lanes of this view's reads: a newer read in a lane stops the older one. */
  const COUNTS = "compare-counts";
  const DIFF = "compare-diff";
  /** Show Diff Anyway: a whole diff past the limits, asked for on purpose. */
  const FULL = "compare-full";

  let tab = $state<string | null>(null);
  let whole = $state(wholeByDefault());
  let diffs = $state<FileDiff[]>([]);
  let shownTab = $state<string | null>(null);
  let summary = $state<DiffPage | null>(null);
  let summaryKey = "";
  let summaryFor: Comparison | undefined;
  let request = 0;
  let loadingMore = $state(false);
  /** The files with their line counts, once counted, and what they were counted for. */
  let counted = $state<{ key: string; files: FileChange[]; from: number } | null>(null);
  /** The files of Summary's first page, counted by their diffs, and what they were read for. */
  let firstFiles = $state<{ key: string; files: Map<string, FileChange> }>({ key: "", files: new Map() });
  let error = $state<string | null>(null);

  /** What the counts are for: the two sides and the settings that change them. */
  const comparisonKey = $derived(`${c.from}\0${c.targetId}`);
  const countKey = $derived(`${comparisonKey}\0${diffSettings()}`);
  /** The files as far as they are known: until the counts come, a loaded diff has them. */
  const files = $derived.by<CompareFile[]>(() => {
    const done = counted?.key === countKey ? counted : null;
    const first = firstFiles.key === done?.key ? firstFiles.files : null;
    if (done) return c.files.map((f, i) => ({ ...f, ...(i < done.from ? (first?.get(f.path) ?? done.files[i]) : done.files[i]) }));
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
  const metadataByPath = $derived(new Map(c.files.map((file) => [file.path, file])));
  const authors = $derived([...new Set(c.ahead.map((a) => a.authorName))]);
  const commits = $derived(c.mode === "split" ? c.aheadCount : c.aheadCount + c.behindCount);

  // The view's reads stop when it closes: their answers would go nowhere.
  $effect(() => () => {
    api.stopReads([COUNTS, DIFF, FULL]).catch(() => {});
  });

  // A new comparison starts on Summary.
  $effect(() => {
    comparisonKey;
    untrack(() => {
      tab = null;
      shownTab = null;
      diffs = [];
      // A whole diff of the comparison before is no longer wanted.
      api.stopReads([FULL]).catch(() => {});
    });
  });

  /** Which read counts the lines: a new tab or diff mode does not ask again. */
  const turns = new CountTurns();

  /** Counts the lines of the files after Summary's first page, whose diffs counted theirs. */
  function count(key: string, run: number, from: string, to: string, first: FileChange[]) {
    if (!turns.start(key, run, countKey)) return;
    // A diff that ignores whitespace counts fewer lines than the list does: count them all.
    const skip = prefs.get("oxbow.diff.ignoreWhitespace") ? 0 : first.length;
    firstFiles = { key, files: new Map(first.map((f) => [f.path, f])) };
    api.compareCounts(from, to, skip, COUNTS).then(
      (list) => {
        turns.end(run);
        if (countKey === key) counted = { key, files: list, from: skip };
      },
      // Counts are a nicety: the list stays without them, and the next read asks again.
      () => turns.end(run),
    );
  }

  $effect(() => {
    const comparison = c;
    const from = comparison.from;
    const to = comparison.targetId;
    const path = tab !== null && metadataByPath.has(tab) ? tab : null;
    const wholeFile = whole;
    const key = countKey;
    untrack(() => {
      tab = path;
      const turn = ++request;
      const pageKey = JSON.stringify([key, wholeFile]);
      if (summaryFor !== comparison || summaryKey !== pageKey) {
        summaryFor = comparison;
        summary = null;
        summaryKey = pageKey;
      }
      loadingMore = false;
      error = null;
      const still = () => request === turn && c === comparison && countKey === key && tab === path && whole === wholeFile;
      const show = (loaded: FileDiff[]) => {
        shownTab = path;
        diffs = loaded;
      };
      const failed = (err: unknown) => {
        if (still() && !stopped(err)) error = String(err);
      };
      const run = turns.read(key, counted?.key === key);
      if (path === null) {
        if (summary) {
          count(key, run, from, to, summary.diffs.map((d) => d.file));
          show(summary.diffs);
          return;
        }
        api.compareDiffPage(from, to, 0, PAGE_LINES, wholeFile, DIFF).then((page) => {
          if (!still()) return;
          count(key, run, from, to, page.diffs.map((d) => d.file));
          summary = page;
          show(page.diffs);
        }, (err) => {
          if (still() && !stopped(err)) count(key, run, from, to, []);
          failed(err);
        });
      } else {
        count(key, run, from, to, []);
        const cached = summary?.diffs.find((d) => d.file.path === path);
        if (cached) {
          show([cached]);
          return;
        }
        api.compareDiff(from, to, path, wholeFile, false, DIFF).then((d) => {
          if (still()) show(d);
        }, failed);
      }
    });
  });

  function loadMore() {
    const page = summary;
    if (!page || page.next === null || loadingMore || tab !== null || shownTab !== null) return;
    const turn = request;
    const comparison = c;
    const key = countKey;
    const wholeFile = whole;
    const still = () => request === turn && c === comparison && countKey === key && tab === null && whole === wholeFile;
    loadingMore = true;
    api.compareDiffPage(c.from, c.targetId, page.next, PAGE_LINES, wholeFile, DIFF).then(
      (more) => {
        if (!still()) return;
        loadingMore = false;
        diffs = [...page.diffs, ...more.diffs];
        summary = { ...more, diffs };
      },
      (err) => {
        if (!still()) return;
        loadingMore = false;
        if (!stopped(err)) error = String(err);
      },
    );
  }
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

  {#snippet metadata(path: string)}
    {@const file = metadataByPath.get(path)}
    {#if file?.onlyBase}
      <span class="meta mono" title="Only {base}'s newer commits changed it">only on {base}</span>
    {:else if file?.last}
      {@const id = file.last}
      <button class="sha mono" onclick={() => onPickCommit(id)} title="The latest commit on {target} that changed it">{shortId(id)}</button>
    {/if}
  {/snippet}

  {#key comparisonKey}
    <ComparisonDiffPane {files} {color} path={tab} shownPath={shownTab} onOpen={(path) => (tab = path)}
      {whole} onToggleWhole={() => (whole = !whole)} {totals} {metadata}
      counted={(file) => !!totals || known.has(file.path)}
      onFold={shownTab === null && diffs.length > 1 ? (fold) => diffView?.setAll(fold) : undefined}>
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      {#if !c.files.length}<p class="empty">No differences: both sides have the same files.</p>{/if}
      <DiffView
        bind:this={diffView}
        foldable={shownTab === null}
        tools={false}
        {diffs}
        {color}
        {whole}
        pending={tab === null && shownTab === null && summary?.next != null ? c.files.length - diffs.length : 0}
        onMore={loadMore}
        onToggleWhole={() => (whole = !whole)}
        loadFull={(d) => api.compareDiff(c.from, c.targetId, d.file.path, whole, true, FULL).then((all) => all[0])}
        command={(d) => `git diff ${c.from} ${c.targetId} -- ${quoteGitArg(`:(literal)${d.file.path}`)}`}
      />
    </ComparisonDiffPane>
  {/key}
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
  .meta {
    font-size: var(--text-caption);
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 35%;
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
