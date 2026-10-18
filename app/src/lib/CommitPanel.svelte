<script lang="ts">
  import { untrack, type Snippet } from "svelte";
  import { api, stopped } from "./api";
  import { diffSettings, wholeByDefault } from "./prefs.svelte";
  import type { CommitDetail, FileChange, FileDiff, HistoryRow } from "./types";
  import { fullDate, initials, lane, person, plate, splitPath, tabLabels, tint } from "./format";
  import DiffView from "./DiffView.svelte";
  import FoldAll from "./FoldAll.svelte";
  import CommitChain from "./CommitChain.svelte";
  import FileBadge from "./FileBadge.svelte";

  let {
    row,
    childIds,
    lookup,
    onSelect,
    localTags = [],
    find = null,
    foundFiles = [],
    foundText = null,
    banner,
    version = 0,
    onUnshallow = null,
  }: {
    row: HistoryRow;
    childIds: string[];
    lookup: (id: string) => HistoryRow | undefined;
    onSelect: (id: string) => void;
    /** Tags the remote doesn't have yet: drawn dashed. */
    localTags?: string[];
    /** Text a code search looked for: marked in the diff. */
    find?: string | null;
    /** Files a code or file search matched in this commit. */
    foundFiles?: string[];
    /** What that search looked for, shown on those files. */
    foundText?: string | null;
    /** A card above the commit's details, e.g. where a commit from the reflog came from. */
    banner?: Snippet;
    /** Goes up with each reload of the repository. */
    version?: number;
    /** Fetch Full History, for a commit where a shallow clone stops. */
    onUnshallow?: (() => void) | null;
  } = $props();

  let diffView = $state<DiffView>();

  /** Lines of diff a page of the Summary brings, about what the diff shows at first. */
  const PAGE_LINES = 4000;
  /** Rows of the Summary's file list drawn at first, and more on each request. */
  const LIST_STEP = 300;

  let detail = $state<CommitDetail | null>(null);
  let diffs = $state<FileDiff[]>([]);
  /** In Summary, where the next page of diffs starts; null once every file is loaded. */
  let next = $state<number | null>(null);
  /** Changed files in all, as the pages count them. */
  let fileCount = $state(0);
  /** Summary's first page of diffs for a commit, whose files the counted list then skips. */
  let firstPage: { id: string; files: Promise<FileChange[]> } | null = null;
  /** The files of that first page, with their counts. */
  let firstFiles = $state<Map<string, FileChange>>(new Map());
  let loadingMore = $state(false);
  /** Rows of the Summary's file list drawn: a commit of thousands of files draws the rest on request. */
  let listed = $state(LIST_STEP);
  let tab = $state<string | null>(null); // null = Summary, otherwise a file path
  let whole = $state(wholeByDefault());
  // Diff & Text's default view applies again when it changes.
  $effect(() => {
    whole = wholeByDefault();
  });
  let error = $state<string | null>(null);
  let scroller = $state<HTMLDivElement>();
  let tabBar = $state<HTMLDivElement>();

  // The selected tab may be scrolled out of the tab bar, for example when a file is picked in Summary.
  $effect(() => {
    tab;
    detail;
    requestAnimationFrame(() =>
      tabBar?.querySelector<HTMLElement>('[aria-selected="true"]')?.scrollIntoView({ inline: "nearest", block: "nearest" }),
    );
  });

  // Effects follow the id, not the row object, so reloading the history keeps the panel as it is.
  const id = $derived(row.id);
  const color = $derived(row.graph.color);
  // A stash has no branch line of its own; its capsule names the stash entry.
  const branchName = $derived(row.graph.branch ?? row.labels.find((l) => l.kind === "stash")?.name ?? null);
  const tags = $derived(row.labels.filter((l) => l.kind === "tag").map((l) => l.name));
  const labels = $derived(tabLabels(detail?.files.map((f) => f.path) ?? []));
  /** The files as far as they are known: until the counted list comes, a loaded diff has counts. */
  const files = $derived.by(() => {
    if (!detail) return [];
    const from = detail.countedFrom;
    if (detail.counted) return from ? detail.files.map((f, i) => (i < from ? (firstFiles.get(f.path) ?? f) : f)) : detail.files;
    const loaded = new Map(diffs.map((d) => [d.file.path, d.file]));
    return detail.files.map((f) => loaded.get(f.path) ?? f);
  });
  const totals = $derived(
    detail?.counted ? files.reduce((t, f) => ({ add: t.add + f.additions, del: t.del + f.deletions }), { add: 0, del: 0 }) : null,
  );
  const known = $derived(new Set(detail?.counted ? [] : diffs.map((d) => d.file.path)));

  /** Lanes of this panel's reads: a newer read in a lane stops the older one. */
  const COUNTS = "panel-counts";
  const DIFF = "panel-diff";
  const FULL = "panel-full";

  // The panel's reads stop when it closes: their answers would go nowhere.
  $effect(() => () => {
    api.stopReads([COUNTS, DIFF, FULL]).catch(() => {});
  });
  // Show Diff Anyway's read is for the commit it was asked on.
  $effect(() => {
    id;
    untrack(() => api.stopReads([FULL]).catch(() => {}));
  });

  /** The commit's details: the list of files at once, then their line counts. */
  function loadDetail(current: string) {
    api.commitDetail(current, true).then(
      (d) => {
        if (id !== current) return;
        if (!detail?.counted) detail = d;
        error = null;
        if (d.shallowEnd) return;
        // The files of Summary's first page are counted by their diffs already.
        const first = firstPage?.id === current ? firstPage.files : Promise.resolve([]);
        first
          .then((known) => {
            if (id !== current) return null;
            firstFiles = new Map(known.map((f) => [f.path, f]));
            return api.commitDetail(current, false, COUNTS, known.length);
          })
          .then(
            (counted) => {
              if (counted && id === current) detail = counted;
            },
            // Counts are a nicety: the list stays without them.
            () => {},
          );
      },
      // An answer for a commit that is no longer shown must not cover the current one.
      (err) => {
        if (id === current && !stopped(err)) error = String(err);
      },
    );
  }

  /** The diffs to show: Summary's first page, or the file of the tab. */
  function loadDiffs(current: string, path: string | null, wholeFile: boolean, keep = false) {
    const still = () => id === current && tab === path && whole === wholeFile;
    const failed = (err: unknown) => {
      if (id === current && !stopped(err) && !keep) error = String(err);
    };
    if (path === null) {
      const page = api.commitDiffPage(current, 0, PAGE_LINES, wholeFile, DIFF);
      if (!keep) firstPage = { id: current, files: page.then((p) => p.diffs.map((d) => d.file), () => []) };
      page.then((page) => {
        if (!still()) return;
        diffs = page.diffs;
        next = page.next;
        fileCount = page.files;
      }, failed);
    } else {
      api.commitDiff(current, path, wholeFile, false, DIFF).then((d) => {
        if (still()) diffs = d;
      }, failed);
    }
  }

  /** The next page of Summary's diffs. */
  function loadMore() {
    if (next === null || loadingMore || tab !== null) return;
    const current = id;
    const wholeFile = whole;
    const from = next;
    loadingMore = true;
    api
      .commitDiffPage(current, from, PAGE_LINES, wholeFile, DIFF)
      .then(
        (page) => {
          // Cleared with the new files, so the view can ask for the next page right away.
          if (id === current) loadingMore = false;
          if (id !== current || tab !== null || whole !== wholeFile || next !== from) return;
          diffs = [...diffs, ...page.diffs];
          next = page.next;
        },
        (err) => {
          if (id === current) loadingMore = false;
          if (id === current && !stopped(err)) error = String(err);
        },
      );
  }

  // A new commit resets the tab; the diff mode is kept.
  $effect(() => {
    const current = id;
    untrack(() => {
      tab = null;
      detail = null;
      error = null;
      listed = LIST_STEP;
      loadDetail(current);
    });
  });

  $effect(() => {
    const current = id;
    const path = tab;
    const wholeFile = whole;
    diffSettings();
    untrack(() => {
      diffs = [];
      next = null;
      loadingMore = false;
      loadDiffs(current, path, wholeFile);
    });
  });

  // A commit never changes, but what a submodule card says about it does: whether the submodule
  // is cloned here, which of its commits this clone has. A commit where a shallow clone stops
  // shows its changes once a fetch brings its parent, and one that failed to load may load now.
  // A reload asks again for these, keeping the panel on screen meanwhile.
  $effect(() => {
    version;
    untrack(() => {
      const again = !!detail?.shallowEnd || error !== null;
      if (!again && !diffs.some((d) => d.submodule)) return;
      if (again) loadDetail(id);
      loadDiffs(id, tab, whole, true);
    });
  });

  function openTab(path: string | null) {
    tab = path;
    if (scroller) scroller.scrollTop = 0;
  }

  const statusIcon: Record<string, string> = { added: "A", deleted: "D", modified: "M", renamed: "R", copied: "C" };
</script>

<div class="panel">
  {@render banner?.()}
  <div class="head">
    <div class="message">
      <h1 class="selectable">
        {detail?.summary ?? row.summary}
        {#if branchName}
          <span class="branch" style:color={plate(color)} style:background={tint(color, "label")} title="Branch of this commit">
            <svg class="icon tiny" viewBox="0 0 16 16"><circle cx="4.5" cy="3.5" r="1.5" /><circle cx="4.5" cy="12.5" r="1.5" /><circle cx="11.5" cy="5.5" r="1.5" /><path d="M4.5 5v6M11.5 7c0 3-7 2-7 4" /></svg>
            {branchName}
          </span>
        {/if}
        {#each tags as name (name)}
          <span class="branch tag" class:local={localTags.includes(name)} title={localTags.includes(name) ? "Tag on this commit, not pushed yet" : "Tag on this commit"}>
            <svg class="icon tiny" viewBox="0 0 16 16"><path d="M2.5 2.5h5l6 6-5 5-6-6z" /><circle cx="5.5" cy="5.5" r="0.8" /></svg>
            {name}
          </span>
        {/each}
      </h1>
      {#if detail?.body}<p class="body selectable">{detail.body}</p>{/if}
    </div>
    <div class="people">
      <span class="avatar" style:background={person(row.authorEmail)}>{initials(row.authorName)}</span>
      <span class="who">
        <span class="author" title={row.authorEmail}>{row.authorName}</span>
        <span class="date">{detail ? fullDate(detail.author.time, detail.author.offset) : ""}</span>
      </span>
    </div>
    <CommitChain {row} {childIds} {lookup} {onSelect} />
  </div>

  <div class="tabs" role="tablist" aria-label="Changes in this commit">
    <!-- Summary stays pinned first; only the file tabs scroll. -->
    <button
      role="tab"
      aria-selected={tab === null}
      class="tab"
      style:background={tab === null ? tint(color) : undefined}
      style:border-color={tab === null ? `color-mix(in srgb, ${lane(color)} 40%, transparent)` : undefined}
      style:color={tab === null ? plate(color) : undefined}
      class:on={tab === null}
      onclick={() => openTab(null)}>Summary</button
    >
    {#if detail?.files.length}
      <span class="divider" aria-hidden="true"></span>
      <div class="file-tabs" bind:this={tabBar}>
        {#each detail.files as file (file.path)}
          <button role="tab" aria-selected={tab === file.path} class="tab neutral" class:on={tab === file.path} onclick={() => openTab(file.path)} title={file.path}
            >{labels.get(file.path)}</button
          >
        {/each}
      </div>
    {/if}
  </div>

  <div class="scroll" bind:this={scroller}>
    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}
    {#if tab === null && detail?.shallowEnd}
      <div class="end">
        <svg class="icon" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="6.3" /><path d="M8 7.2v4M8 4.9v.1" /></svg>
        <span class="end-text">
          History ends here
          <span class="sub">This is a shallow clone, and the commit before this one isn’t downloaded, so its changes can’t be shown yet.</span>
        </span>
        {#if onUnshallow}<button class="button small" onclick={() => onUnshallow?.()}>Fetch Full History…</button>{/if}
      </div>
    {:else if tab === null && detail}
      <div class="files">
        <div class="count">
          <span class="grow">{detail.files.length.toLocaleString()} {detail.files.length === 1 ? "file" : "files"} changed</span>
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
          <button class="file" onclick={() => openTab(file.path)}>
            <span class="badge {file.status}">{statusIcon[file.status]}</span>
            <span class="grow ellipsis"><span class="dir">{p.dir}</span>{p.name}</span>
            {#if foundFiles.includes(file.path)}
              <span class="found" title={find ? `Adds or removes “${find}”` : "The path matches the search"}>
                <svg class="icon" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5" /><path d="M10.3 10.3 14 14" /></svg>{foundText ?? "match"}
              </span>
            {/if}
            {#if detail.counted || known.has(file.path)}
              <FileBadge {file} />
            {/if}
          </button>
        {/each}
        {#if files.length > listed}
          <button class="file more" onclick={() => (listed += LIST_STEP)}>Show {Math.min(files.length - listed, LIST_STEP).toLocaleString()} more {files.length - listed === 1 ? "file" : "files"}</button>
        {/if}
      </div>
    {/if}
    <DiffView
      bind:this={diffView}
      foldable
      {diffs}
      {color}
      {whole}
      {find}
      commit={row.id}
      pending={tab === null && next !== null ? fileCount - diffs.length : 0}
      onMore={loadMore}
      onToggleWhole={() => (whole = !whole)}
      loadFull={(d) => api.commitDiff(row.id, d.file.path, whole, true, FULL).then((all) => all[0])}
      command={(d) =>
        detail?.parents.length ? `git diff ${row.id.slice(0, 7)}~1 ${row.id.slice(0, 7)} -- ${d.file.path}` : `git show ${row.id.slice(0, 7)} -- ${d.file.path}`}
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
  /* The commit's details sit on the panel itself, with no frame; a hairline parts them from the tabs. */
  .head {
    padding: var(--space-4) var(--space-5) var(--space-2);
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .message {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .branch {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 20px;
    margin-left: 6px;
    padding: 0 8px;
    border-radius: var(--radius-pill);
    font-size: var(--text-small);
    font-weight: 500;
    line-height: 1;
    white-space: nowrap;
    vertical-align: 2px;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    user-select: none;
  }
  /* Muted yellow with a brighter outline, as in the history rows: yellow is for tags only. */
  .branch.tag {
    background: var(--tag-bg);
    box-shadow: inset 0 0 0 1px var(--tag-border);
    color: var(--tag-fg);
  }
  .branch.tag.local {
    box-shadow: none;
    outline: 1px dashed var(--tag-border);
    outline-offset: -1px;
  }
  .tiny {
    width: 12px;
    height: 12px;
  }
  h1 {
    margin: 0;
    font-size: var(--text-title);
    font-weight: 600;
    line-height: 1.3;
  }
  .body {
    margin: 0;
    color: var(--text2);
    line-height: 1.45;
    white-space: pre-wrap;
    max-height: 160px;
    overflow-y: auto;
  }
  .people {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .avatar {
    width: 30px;
    height: 30px;
    border-radius: var(--radius-pill);
    color: #fff;
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 600;
    font-size: var(--text-small);
    flex-shrink: 0;
  }
  .who {
    flex-grow: 1;
    display: flex;
    flex-direction: column;
    line-height: 1.25;
    min-width: 140px;
  }
  .author {
    font-weight: 600;
  }
  .date {
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .tabs {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: var(--space-2) 0 var(--space-2) var(--space-5);
    border-bottom: 1px solid var(--sep);
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
    padding: 0 var(--space-3);
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
  /* The same card as at the end of a shallow clone's history. */
  .end {
    margin: var(--space-3) var(--space-3) var(--space-2);
    display: flex;
    align-items: center;
    gap: 10px;
    padding: var(--space-3);
    border-radius: var(--radius-l);
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    font-size: var(--text-body);
  }
  .end .icon {
    width: 15px;
    height: 15px;
    flex-shrink: 0;
    color: var(--text2);
  }
  .end-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex-grow: 1;
    font-weight: 500;
  }
  .end-text .sub {
    font-weight: 400;
    font-size: var(--text-small);
    color: var(--text2);
  }

  .files {
    margin: 8px 12px 4px;
    padding: 4px 0 6px;
  }
  .counting {
    font-weight: 400;
  }
  .file.more {
    justify-content: center;
    color: var(--text2);
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
  .found {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    max-width: 40%;
    height: 18px;
    padding: 0 7px 0 5px;
    border-radius: var(--radius-pill);
    background: var(--found-bg);
    box-shadow: inset 0 0 0 1px var(--found-ring);
    font-size: var(--text-caption);
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex-shrink: 1;
  }
  .found .icon {
    width: 10px;
    height: 10px;
    flex-shrink: 0;
  }
  .file {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 26px;
    padding: 0 8px;
    border-radius: var(--radius-s);
  }
  .file:hover {
    background: var(--field);
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
  }
  .error {
    color: var(--red);
    margin: 12px 20px;
  }
</style>
