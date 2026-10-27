<script lang="ts">
  // The Files tab of a pull request: what it changes since it split from its base, as GitHub
  // shows it, fetched here and read like any diff. Each file has a Viewed box, kept on GitHub:
  // a viewed file folds, and GitHub unticks it once it changes again.
  import { untrack } from "svelte";
  import { api, stopped } from "./api";
  import { wholeByDefault } from "./prefs.svelte";
  import type { Comparison, FileChange, FileDiff, PullRequest } from "./types";
  import { splitPath } from "./format";
  import DiffView from "./DiffView.svelte";
  import FileBadge from "./FileBadge.svelte";

  let {
    pr,
    color,
    version,
  }: {
    pr: PullRequest;
    color: number;
    /** Goes up on every reload, so the files load again. */
    version: number;
  } = $props();

  /** Lines of diff a page brings, about what the diff shows at first. */
  const PAGE_LINES = 4000;
  /** Lanes of this view's reads: a newer read in a lane stops the older one. */
  const COUNTS = "pull-counts";
  const DIFF = "pull-diff";
  const FULL = "pull-full";

  // Raw, so the reads below can tell it is still the one they were for.
  let comparison = $state.raw<Comparison | null>(null);
  let error = $state<string | null>(null);
  /** The fetch failed: what shows is what was fetched before. */
  let stale = $state<string | null>(null);
  let loading = $state(false);
  let whole = $state(wholeByDefault());
  let diffs = $state<FileDiff[]>([]);
  let next = $state<number | null>(null);
  let counts = $state<FileChange[] | null>(null);
  /** Files ticked Viewed on GitHub, and the ones GitHub unticked as they changed since. */
  let viewed = $state.raw(new Set<string>());
  let dismissed = $state.raw(new Set<string>());
  let filter = $state("");
  let picked = $state<string | null>(null);
  let diffView = $state<DiffView>();
  let scroller = $state<HTMLDivElement>();

  const head = $derived(`refs/oxbow/pull/${pr.number}/head`);
  const base = $derived(`refs/oxbow/pull/${pr.number}/base`);

  $effect(() => () => {
    api.stopReads([COUNTS, DIFF, FULL]).catch(() => {});
  });

  // Fetched again for every new head, and on every reload.
  $effect(() => {
    const number = pr.number;
    const sha = pr.headSha;
    const onto = pr.base;
    void version;
    untrack(() => load(number, sha, onto));
  });

  async function load(number: number, sha: string, onto: string) {
    const still = () => pr.number === number && pr.headSha === sha;
    loading = !comparison;
    error = null;
    stale = await api.pullFetch(number, onto).then(
      () => null,
      (err) => String(err),
    );
    if (!still()) return;
    api.githubViewing(number).then(
      (v) => {
        if (!still()) return;
        viewed = new Set(v.viewed);
        dismissed = new Set(v.dismissed);
      },
      () => {},
    );
    try {
      const c = await api.compare(base, head, "split");
      if (!still()) return;
      const same = comparison && comparison.from === c.from && comparison.targetId === c.targetId;
      comparison = c;
      if (!same) readDiffs(c);
    } catch (err) {
      if (still()) error = stale ?? String(err);
    } finally {
      loading = false;
    }
  }

  function readDiffs(c: Comparison) {
    diffs = [];
    next = null;
    counts = null;
    const wholeFile = whole;
    const still = () => comparison === c && whole === wholeFile;
    api.compareDiffPage(c.from, c.targetId, 0, PAGE_LINES, wholeFile, DIFF).then(
      (page) => {
        if (!still()) return;
        diffs = page.diffs;
        next = page.next;
      },
      (err) => still() && !stopped(err) && (error = String(err)),
    );
    api.compareCounts(c.from, c.targetId, 0, COUNTS).then(
      (list) => comparison === c && (counts = list),
      () => {},
    );
  }

  function toggleWhole() {
    whole = !whole;
    if (comparison) readDiffs(comparison);
  }

  let more: Promise<void> | null = null;
  /** The next page of diffs. */
  function loadMore(): Promise<void> {
    const c = comparison;
    if (!c || next === null) return Promise.resolve();
    if (more) return more;
    const start = next;
    const wholeFile = whole;
    more = api.compareDiffPage(c.from, c.targetId, start, PAGE_LINES, wholeFile, DIFF).then(
      (page) => {
        more = null;
        if (comparison !== c || whole !== wholeFile || next !== start) return;
        diffs = [...diffs, ...page.diffs];
        next = page.next;
      },
      (err) => {
        more = null;
        if (!stopped(err)) error = String(err);
      },
    );
    return more;
  }

  /** The files with their line counts once counted, else as far as their diffs say. */
  const files = $derived.by<FileChange[]>(() => {
    if (!comparison) return [];
    if (counts) return counts;
    const loaded = new Map(diffs.map((d) => [d.file.path, d.file]));
    return comparison.files.map((f) => loaded.get(f.path) ?? f);
  });
  const shownFiles = $derived(filter.trim() ? files.filter((f) => f.path.toLowerCase().includes(filter.trim().toLowerCase())) : files);
  const viewedCount = $derived(files.filter((f) => viewed.has(f.path)).length);

  /** Tick or untick a file's Viewed box, here at once and then on GitHub. */
  function setViewed(path: string, on: boolean) {
    const before = { viewed, dismissed };
    const v = new Set(viewed);
    const d = new Set(dismissed);
    if (on) v.add(path);
    else v.delete(path);
    d.delete(path);
    viewed = v;
    dismissed = d;
    diffView?.fold(path, on);
    api.githubSend({ kind: "viewed", id: pr.nodeId, path, viewed: on }).catch((err) => {
      viewed = before.viewed;
      dismissed = before.dismissed;
      error = `Couldn’t mark ${path} ${on ? "viewed" : "not viewed"} on GitHub: ${err}`;
    });
  }

  /** Show a file's diff, loading pages of diffs until it is there. */
  async function reveal(path: string) {
    picked = path;
    while (!diffs.some((d) => d.file.path === path) && next !== null) {
      const before = next;
      await loadMore();
      if (next === before) break;
    }
    diffView?.fold(path, false);
    requestAnimationFrame(() => {
      const section = scroller?.querySelector<HTMLElement>(`section[data-path="${CSS.escape(path)}"]`);
      section?.scrollIntoView({ block: "start" });
    });
  }

  const statusIcon: Record<string, string> = { added: "A", deleted: "D", modified: "M", renamed: "R", copied: "C" };
</script>

{#snippet viewedBox(path: string, label: boolean)}
  {@const on = viewed.has(path)}
  <button
    class="viewed"
    class:labelled={label}
    role="checkbox"
    aria-checked={on}
    aria-label="Viewed"
    title={on ? "Viewed: click when it needs another look" : dismissed.has(path) ? "Changed since you viewed it" : "Mark viewed: it folds, and GitHub keeps the tick"}
    onclick={(e) => {
      e.stopPropagation();
      setViewed(path, !on);
    }}
  >
    <span class="box" class:on><svg viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg></span>
    {#if label}<span>Viewed</span>{/if}
  </button>
{/snippet}

{#snippet fileActions(diff: FileDiff)}
  {@render viewedBox(diff.file.path, true)}
{/snippet}

<div class="files-tab">
  <div class="list">
    <label class="filter">
      <svg class="icon small" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5" /><path d="m10.5 10.5 3 3" /></svg>
      <input type="text" placeholder="Filter files" bind:value={filter} spellcheck="false" />
    </label>
    {#if files.length}
      <div class="progress" title="Files ticked Viewed, here and on GitHub">
        <span>Viewed {viewedCount} of {files.length}</span>
        <span class="bar"><span style:width="{(viewedCount * 100) / files.length}%"></span></span>
      </div>
    {/if}
    <div class="rows">
      {#each shownFiles as file (file.path)}
        {@const p = splitPath(file.path)}
        <div class="file" class:on={picked === file.path} class:done={viewed.has(file.path)}>
          {@render viewedBox(file.path, false)}
          <button class="name" onclick={() => reveal(file.path)} title={file.path}>
            <span class="letter {file.status}">{statusIcon[file.status] ?? "M"}</span>
            <span class="path"><span class="dir">{p.dir}</span>{p.name}</span>
            {#if dismissed.has(file.path)}<span class="changed" title="Changed since you viewed it">changed</span>{/if}
          </button>
          <span class="counts"><FileBadge {file} /></span>
        </div>
      {/each}
    </div>
  </div>
  <div class="diff" bind:this={scroller}>
    {#if stale && comparison}<p class="note">Couldn’t fetch the latest of #{pr.number}, so this is what was fetched before. {stale}</p>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if loading && !comparison}
      <p class="wait">Fetching #{pr.number}…</p>
    {:else if comparison && !comparison.files.length}
      <p class="wait">No changes.</p>
    {/if}
    {#if comparison}
      <DiffView
        bind:this={diffView}
        foldable
        {diffs}
        {color}
        {whole}
        {fileActions}
        commit={comparison.targetId}
        shut={(d) => viewed.has(d.file.path)}
        pending={next !== null ? comparison.files.length - diffs.length : 0}
        onMore={() => void loadMore()}
        onToggleWhole={toggleWhole}
        loadFull={(d) => api.compareDiff(comparison!.from, comparison!.targetId, d.file.path, whole, true, FULL).then((all) => all[0])}
        command={(d) => `git diff ${comparison!.from.slice(0, 7)} ${comparison!.targetId.slice(0, 7)} -- ${d.file.path}`}
      />
    {/if}
  </div>
</div>

<style>
  .files-tab {
    flex-grow: 1;
    min-height: 0;
    display: flex;
    gap: 0;
    margin: 0 -22px -16px;
    border-top: 1px solid var(--sep);
  }
  .list {
    width: 300px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px 10px 10px 16px;
    border-right: 1px solid var(--sep);
    min-height: 0;
  }
  .filter {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    border-radius: var(--radius-m);
    background: var(--field);
    color: var(--text2);
    flex-shrink: 0;
  }
  .filter input {
    flex-grow: 1;
    min-width: 0;
    border: none;
    background: none;
    outline: none;
    box-shadow: none;
    font-size: var(--text-body);
    color: var(--text);
  }
  .icon.small {
    width: 13px;
    height: 13px;
  }
  .progress {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    margin: 6px 6px 2px;
    font-size: var(--text-caption);
    font-weight: 600;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    color: var(--text2);
  }
  .bar {
    width: 90px;
    height: 4px;
    border-radius: 2px;
    background: var(--field);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  .rows {
    flex-grow: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .file {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 8px 0 6px;
    border-radius: var(--radius-m);
  }
  .file:hover {
    background: var(--row-hover);
  }
  .file.on {
    background: var(--side-sel);
  }
  .file.done .path {
    color: var(--text2);
  }
  .name {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 100%;
    text-align: left;
  }
  .path {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dir {
    color: var(--text2);
  }
  .letter {
    width: 16px;
    height: 16px;
    border-radius: var(--radius-xs);
    font-size: 10px;
    font-weight: 700;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    color: var(--orange);
    background: var(--orange-soft);
  }
  .letter.added {
    color: var(--green);
    background: var(--green-soft);
  }
  .letter.deleted {
    color: var(--red);
    background: var(--red-soft);
  }
  .changed {
    font-size: var(--text-caption);
    color: var(--orange);
    flex-shrink: 0;
  }
  .counts {
    display: flex;
    gap: 4px;
    font-size: var(--text-small);
    flex-shrink: 0;
  }
  .viewed {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
    font-size: var(--text-small);
    color: var(--text);
  }
  .viewed.labelled {
    padding: 0 6px;
  }
  .box {
    width: 15px;
    height: 15px;
    border-radius: var(--radius-xs);
    border: 1.3px solid var(--lead-in);
    background: var(--win);
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .box svg {
    width: 11px;
    height: 11px;
    fill: none;
    stroke: #fff;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
    visibility: hidden;
  }
  .box.on {
    background: var(--accent);
    border-color: var(--accent);
  }
  .box.on svg {
    visibility: visible;
  }
  .diff {
    flex-grow: 1;
    min-width: 0;
    overflow-y: auto;
    padding: 10px 16px 16px 14px;
  }
  .note,
  .error,
  .wait {
    margin: 0 0 10px;
    font-size: var(--text-small);
  }
  .note,
  .wait {
    color: var(--text2);
  }
  .error {
    color: var(--red);
  }
</style>
