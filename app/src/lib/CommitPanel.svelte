<script lang="ts">
  import { untrack, type Snippet } from "svelte";
  import { api, stopped } from "./api";
  import { diffSettings, wholeByDefault } from "./prefs.svelte";
  import type { CommitDetail, FileChange, FileDiff, HistoryRow } from "./types";
  import { fullDate, initials, shortTime, lane, person, plate, shortId, splitPath, tint } from "./format";
  import { fileTabs, tabStepper } from "./tabs";
  import { confirm } from "./confirm.svelte";
  import DiffView from "./DiffView.svelte";
  import DiffTools from "./DiffTools.svelte";
  import FoldAll from "./FoldAll.svelte";
  import CommitChain from "./CommitChain.svelte";
  import FileBadge from "./FileBadge.svelte";
  import { byFolder, filesWidth, listView, resizeFiles, setFolders } from "./listView.svelte";

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
  let tab = $state<string | null>(null); // null = All Changes, otherwise a file path
  let whole = $state(wholeByDefault());
  // Diff & Text's default view applies again when it changes.
  $effect(() => {
    whole = wholeByDefault();
  });
  let error = $state<string | null>(null);
  let scroller = $state<HTMLDivElement>();
  let filesBox = $state<HTMLDivElement>();

  // Stepped to with the keys, the open file may be scrolled out of its list.
  $effect(() => {
    tab;
    detail;
    requestAnimationFrame(() => filesBox?.querySelector<HTMLElement>('[aria-current="true"]')?.scrollIntoView({ block: "nearest" }));
  });

  // Effects follow the id, not the row object, so reloading the history keeps the panel as it is.
  const id = $derived(row.id);
  const color = $derived(row.graph.color);
  // A stash has no branch line of its own; its capsule names the stash entry.
  const branchName = $derived(row.graph.branch ?? row.labels.find((l) => l.kind === "stash")?.name ?? null);
  const tags = $derived(row.labels.filter((l) => l.kind === "tag").map((l) => l.name));
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

  /** Only files whose path has this text are listed. */
  let filter = $state("");
  const needle = $derived(filter.trim().toLowerCase());
  const shownFiles = $derived(needle ? files.filter((f) => f.path.toLowerCase().includes(needle) || !!f.oldPath?.toLowerCase().includes(needle)) : files);
  /** Folders closed in the list. */
  let folded = $state(new Set<string>());
  function fold(dir: string) {
    const next = new Set(folded);
    if (!next.delete(dir)) next.add(dir);
    folded = next;
  }
  /** The files listed, in the order they are shown: the steps of Next Tab after All Changes. */
  const paths = $derived(
    listView.folders ? byFolder(shownFiles).flatMap((g) => (folded.has(g.dir) ? [] : g.files.map((f) => f.path))) : shownFiles.map((f) => f.path),
  );

  /** Two columns, the files beside their diff, from this width of the panel; under it the files
   *  sit over the diff. */
  const TWO_COLUMNS = 600;
  let bodyWidth = $state(0);
  const stacked = $derived(bodyWidth > 0 && bodyWidth < TWO_COLUMNS);

  /** Commit details folded to their heading; kept for every window. */
  const DETAILS_KEY = "oxbow.commit.detailsShut";
  let detailsShut = $state(readShut());
  function readShut(): boolean {
    try {
      return localStorage.getItem(DETAILS_KEY) === "true";
    } catch {
      return false;
    }
  }
  function toggleDetails() {
    detailsShut = !detailsShut;
    try {
      localStorage.setItem(DETAILS_KEY, String(detailsShut));
    } catch {
      // Lasts until the app quits.
    }
  }
  function copy(text: string, what: string) {
    navigator.clipboard.writeText(text).then(() => confirm.say(`Copied ${what}.`, "done"), () => {});
  }
  const message = $derived(detail ? [detail.summary, detail.body].filter(Boolean).join("\n\n") : row.summary);

  const statusIcon: Record<string, string> = { added: "A", deleted: "D", modified: "M", renamed: "R", copied: "C" };
  const tone: Record<string, string> = { added: "green", deleted: "red", modified: "orange", renamed: "orange", copied: "orange" };
</script>

<div class="panel">
  {@render banner?.()}
  <div class="head">
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
    <div class="people">
      <span class="avatar" style:background={person(row.authorEmail)}>{initials(row.authorName)}</span>
      <span class="who">
        <span class="author" title={row.authorEmail}>{row.authorName}</span>
        <span class="date">{detail ? fullDate(detail.author.time, detail.author.offset) : ""}</span>
      </span>
    </div>
    <CommitChain {row} {childIds} {lookup} {onSelect} />
  </div>

  <div class="body" class:stacked bind:clientWidth={bodyWidth}>
    <div class="files" bind:this={filesBox} aria-label="Changed files" data-steps use:tabStepper={fileTabs(paths, tab, openTab)} style:width={stacked ? null : `${filesWidth(bodyWidth)}px`}>
      <div class="tools">
        <label class="filter">
          <svg class="icon" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5" /><path d="m10.5 10.5 3 3" /></svg>
          <input
            bind:value={filter}
            placeholder="Filter files"
            aria-label="Filter files"
            spellcheck="false"
            autocomplete="off"
            onkeydown={(e) => {
              if (e.key === "Escape" && filter) {
                e.stopPropagation();
                filter = "";
              }
            }}
          />
          {#if filter}<button class="clear" onclick={() => (filter = "")} aria-label="Clear the filter" title="Clear the filter">×</button>{/if}
        </label>
        <div class="segmented" role="radiogroup" aria-label="Show files">
          <button role="radio" aria-checked={!listView.folders} class:on={!listView.folders} onclick={() => setFolders(false)} aria-label="List" title="One list of paths">
            <svg class="icon" viewBox="0 0 16 16"><path d="M5.5 4h8M5.5 8h8M5.5 12h8M2.5 4h.01M2.5 8h.01M2.5 12h.01" /></svg>
          </button>
          <button role="radio" aria-checked={listView.folders} class:on={listView.folders} onclick={() => setFolders(true)} aria-label="Folders" title="Files under their folders">
            <svg class="icon" viewBox="0 0 16 16"><path d="M1.5 4.5a1 1 0 0 1 1-1H6l1.5 1.5h6a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" /></svg>
          </button>
        </div>
      </div>

      <div class="list">
        {#if detail && !detail.shallowEnd}
          <!-- The heading shows every file's diff, as Summary did. -->
          <button
            class="all"
            aria-current={tab === null ? "true" : undefined}
            style:background={tab === null ? tint(color) : undefined}
            onclick={() => openTab(null)}
            title="Show the changes of every file"
          >
            <span class="gtitle" style:color={plate(color)}>All Changes</span>
            <span class="n">{needle ? `${shownFiles.length} of ${detail.files.length}` : detail.files.length.toLocaleString()}</span>
            <span class="grow"></span>
            {#if totals}
              <span class="mono add">+{totals.add}</span>
              <span class="mono del">−{totals.del}</span>
            {:else}
              <span class="counting" title="Counting the changed lines">Counting…</span>
            {/if}
          </button>
          {#snippet fileRow(file: FileChange, nested: boolean)}
            {@const p = splitPath(file.path)}
            {@const open = tab === file.path}
            <button class="file" class:nested aria-current={open ? "true" : undefined} style:background={open ? tint(color) : undefined} onclick={() => openTab(file.path)} title={file.oldPath ? `${file.oldPath} → ${file.path}` : file.path}>
              <span class="badge {tone[file.status]}">{statusIcon[file.status]}</span>
              <span class="grow ellipsis">{#if !nested}<span class="dir">{p.dir}</span>{/if}{p.name}</span>
              {#if foundFiles.includes(file.path)}
                <span class="found" title={find ? `Adds or removes “${find}”` : "The path matches the search"}>
                  <svg class="icon" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5" /><path d="M10.3 10.3 14 14" /></svg>{foundText ?? "match"}
                </span>
              {/if}
              {#if detail?.counted || known.has(file.path)}
                <FileBadge {file} />
              {/if}
            </button>
          {/snippet}
          {#if listView.folders}
            {#each byFolder(shownFiles.slice(0, listed)) as group (group.dir)}
              {@const open = !folded.has(group.dir)}
              <button class="folder" aria-expanded={open} onclick={() => fold(group.dir)} title={group.dir || "The repository's top folder"}>
                <svg class="chevron" class:open viewBox="0 0 16 16"><path d="M6 4l4 4-4 4" /></svg>
                <svg class="icon folder-icon" viewBox="0 0 16 16"><path d="M1.5 4.5a1 1 0 0 1 1-1H6l1.5 1.5h6a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" /></svg>
                <span class="dirname">{group.dir ? group.dir.slice(0, -1) : "Top folder"}</span>
                <span class="n">{group.files.length}</span>
              </button>
              {#if open}{#each group.files as file (file.path)}{@render fileRow(file, true)}{/each}{/if}
            {/each}
          {:else}
            {#each shownFiles.slice(0, listed) as file (file.path)}{@render fileRow(file, false)}{/each}
          {/if}
          {#if shownFiles.length > listed}
            <button class="file more" onclick={() => (listed += LIST_STEP)}>Show {Math.min(shownFiles.length - listed, LIST_STEP).toLocaleString()} more {shownFiles.length - listed === 1 ? "file" : "files"}</button>
          {/if}
          {#if needle && !shownFiles.length}<p class="empty">No files match the filter.</p>{/if}
        {/if}
      </div>

      <!-- The whole message and who made the commit, as a card in the branch color like «this commit» in the chain. -->
      <div class="details" style:background={tint(color)} style:border-color={`color-mix(in srgb, ${lane(color)} 40%, transparent)`}>
        <button class="details-head" aria-expanded={!detailsShut} onclick={toggleDetails} style:color={plate(color)}>
          <svg class="chevron" class:open={!detailsShut} viewBox="0 0 16 16"><path d="M6 4l4 4-4 4" /></svg>
          Commit Details
        </button>
        {#if !detailsShut}
          {#if detail?.body}<p class="message selectable">{detail.body}</p>{/if}
          <dl>
            <dt>Commit</dt>
            <dd>
              <span class="mono sha selectable" title={row.id}>{row.id}</span>
              <button class="copy" onclick={() => copy(row.id, shortId(row.id))} aria-label="Copy the commit's SHA" title="Copy SHA">
                <svg class="icon" viewBox="0 0 16 16"><rect x="5.5" y="5.5" width="8" height="8" rx="1.5" /><path d="M10.5 5.5V3.5a1 1 0 0 0-1-1h-6a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2" /></svg>
              </button>
            </dd>
            {#if detail}
              <dt>Committed</dt>
              <dd class="ellipsis" title="{detail.committer.email} · {fullDate(detail.committer.time, detail.committer.offset)}">{detail.committer.name}, {shortTime(detail.committer.time)}</dd>
            {/if}
          </dl>
          <div><button class="button mini" onclick={() => copy(message, "the message")}>Copy Message</button></div>
        {/if}
      </div>
    </div>

    {#if !stacked}<button class="edge" tabindex="-1" style:left={`${filesWidth(bodyWidth) - 4}px`} onpointerdown={(e) => resizeFiles(e, bodyWidth)} aria-label="Resize the files" title="Drag to resize"></button>{/if}
    <div class="diff" aria-label="Diff">
      <div class="dhead">
        <span class="caption grow">
          {#if tab === null}
            {detail ? `${detail.files.length.toLocaleString()} ${detail.files.length === 1 ? "file" : "files"}` : ""}
          {:else}
            {@const at = detail?.files.findIndex((f) => f.path === tab) ?? -1}
            {#if at >= 0 && detail}File {at + 1} of {detail.files.length.toLocaleString()}{/if}
          {/if}
        </span>
        {#if tab === null && diffs.length > 1}<FoldAll onFold={(fold) => diffView?.setAll(fold)} />{/if}
        <DiffTools {whole} onToggleWhole={() => (whole = !whole)} />
      </div>
      <div class="scroll" bind:this={scroller}>
        {#if error}
          <p class="error" role="alert">{error}</p>
        {/if}
        {#if detail?.shallowEnd}
          <div class="end">
            <svg class="icon" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="6.3" /><path d="M8 7.2v4M8 4.9v.1" /></svg>
            <span class="end-text">
              History ends here
              <span class="sub">This is a shallow clone, and the commit before this one isn’t downloaded, so its changes can’t be shown yet.</span>
            </span>
            {#if onUnshallow}<button class="button small" onclick={() => onUnshallow?.()}>Fetch Full History…</button>{/if}
          </div>
        {/if}
        <DiffView
          bind:this={diffView}
          foldable={tab === null}
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
          tools={false}
        />
      </div>
    </div>
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
  /* The commit's subject, author and chain sit on the panel itself, with no frame; a hairline parts them from the files. */
  .head {
    padding: var(--space-4) var(--space-5) var(--space-3);
    display: flex;
    flex-direction: column;
    gap: 12px;
    border-bottom: 1px solid var(--sep);
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

  /* The files beside their diff; in a narrow panel, over it, scrolling as one with the details. */
  .body {
    position: relative;
    flex-grow: 1;
    min-height: 0;
    display: flex;
  }
  .body.stacked {
    flex-direction: column;
  }
  .files {
    flex-shrink: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--sep);
  }
  .stacked .files {
    width: auto;
    max-height: 50%;
    border-right: 0;
    border-bottom: 1px solid var(--sep);
    overflow-y: auto;
  }
  .stacked .list {
    flex: none;
    overflow: visible;
  }
  /* The border to drag between the files and the diff, like the graph's. */
  .edge {
    position: absolute;
    top: 50%;
    width: 7px;
    height: 36px;
    margin-top: -18px;
    border-radius: var(--radius-xs);
    background: var(--win);
    border: 1px solid var(--glass-border);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.08);
    z-index: 2;
    cursor: col-resize;
  }
  .diff {
    flex-grow: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .tools {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: var(--space-3) var(--space-3) var(--space-1);
    flex-shrink: 0;
  }
  .filter {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-grow: 1;
    min-width: 0;
    height: 26px;
    padding: 0 6px 0 10px;
    border-radius: var(--radius-pill);
    background: var(--field);
    color: var(--text2);
  }
  .filter:focus-within {
    box-shadow: var(--focus-ring);
  }
  .filter .icon {
    width: 13px;
    height: 13px;
    flex-shrink: 0;
  }
  .filter input {
    flex-grow: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    font: inherit;
    font-size: var(--text-small);
    color: var(--text);
    user-select: text;
    -webkit-user-select: text;
    box-shadow: none;
  }
  .clear {
    width: 18px;
    height: 18px;
    border-radius: var(--radius-pill);
    color: var(--text2);
    line-height: 1;
    flex-shrink: 0;
  }
  .clear:hover {
    background: var(--side-sel);
    color: var(--text);
  }
  .segmented {
    flex-shrink: 0;
    display: flex;
    padding: 2px;
    border-radius: var(--radius-m);
    background: var(--field);
  }
  .segmented button {
    width: 28px;
    height: 22px;
    border-radius: var(--radius-s);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text2);
  }
  .segmented button.on {
    background: var(--sheet);
    color: var(--text);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
  }
  .segmented .icon {
    width: 14px;
    height: 14px;
  }
  .list {
    flex-grow: 1;
    min-height: 60px;
    overflow-y: auto;
    padding: 0 var(--space-2) var(--space-2);
  }
  /* The heading of the list, in small capitals in the branch color, as in Changes. */
  .all {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    height: 30px;
    padding: 0 var(--space-2);
    border-radius: var(--radius-s);
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
  }
  .gtitle {
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }
  .n {
    font-weight: 400;
  }
  .counting {
    font-weight: 400;
  }
  .file {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 28px;
    padding: 0 var(--space-2) 0 22px;
    border-radius: var(--radius-s);
    text-align: left;
  }
  .file:hover,
  .folder:hover {
    background: var(--field);
  }
  .file.nested {
    padding-left: 44px;
  }
  .file.more {
    justify-content: center;
    padding: 0;
    color: var(--text2);
    font-size: var(--text-small);
  }
  .folder {
    display: flex;
    align-items: center;
    gap: 5px;
    width: 100%;
    height: 26px;
    padding: 0 var(--space-2) 0 22px;
    border-radius: var(--radius-s);
    font-size: var(--text-small);
    color: var(--text2);
  }
  .chevron {
    width: 10px;
    height: 10px;
    flex-shrink: 0;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
    transition: transform 0.15s;
  }
  .chevron.open {
    transform: rotate(90deg);
  }
  .folder-icon {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
  }
  .dirname {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
    color: var(--text);
  }
  .empty {
    margin: 0;
    padding: var(--space-1) var(--space-3) var(--space-1) 22px;
    font-size: var(--text-small);
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
  /* The same letters as in Changes. */
  .badge {
    width: 16px;
    height: 16px;
    border-radius: var(--radius-xs);
    font-size: var(--text-caption);
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }
  .badge.orange {
    background: var(--orange-soft);
    color: var(--orange);
  }
  .badge.green {
    background: var(--green-soft);
    color: var(--green);
  }
  .badge.red {
    background: var(--red-soft);
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

  /* Commit Details, at the bottom of the files. */
  .details {
    flex-shrink: 0;
    margin: var(--space-2) var(--space-3) var(--space-3);
    padding: 10px 12px 12px;
    border: 1px solid transparent;
    border-radius: var(--radius-xl);
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: var(--text-small);
  }
  .details-head {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-caption);
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    text-align: left;
  }
  .message {
    margin: 0;
    line-height: 1.45;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 160px;
    overflow-y: auto;
  }
  dl {
    margin: 0;
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    column-gap: 10px;
    row-gap: 4px;
    align-items: center;
  }
  dt {
    color: var(--text2);
  }
  dd {
    margin: 0;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  dd.ellipsis {
    display: block;
  }
  .sha {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-caption);
  }
  .copy {
    width: 22px;
    height: 22px;
    flex-shrink: 0;
    border-radius: var(--radius-pill);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--icon);
  }
  .copy:hover {
    background: var(--field);
  }
  .copy .icon {
    width: 13px;
    height: 13px;
  }

  /* Over the diff: how many files, and how the diff is drawn. */
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
  .scroll {
    flex-grow: 1;
    overflow-y: auto;
    min-height: 0;
  }
  /* The same card as at the end of a shallow clone's history. */
  .end {
    margin: 0 var(--space-3) var(--space-2);
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
  .error {
    color: var(--red);
    margin: 12px 20px;
  }
</style>
