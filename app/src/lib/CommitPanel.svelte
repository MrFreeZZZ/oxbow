<script lang="ts">
  import { api } from "./api";
  import { diffSettings, wholeByDefault } from "./prefs.svelte";
  import type { CommitDetail, FileDiff, HistoryRow } from "./types";
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
  } = $props();

  let diffView = $state<DiffView>();

  let detail = $state<CommitDetail | null>(null);
  let diffs = $state<FileDiff[]>([]);
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
  const totals = $derived(
    detail?.files.reduce((t, f) => ({ add: t.add + f.additions, del: t.del + f.deletions }), { add: 0, del: 0 }) ?? { add: 0, del: 0 },
  );

  // A new commit resets the tab; the diff mode is kept.
  $effect(() => {
    const current = id;
    tab = null;
    detail = null;
    error = null;
    api.commitDetail(current).then(
      (d) => {
        if (id === current) detail = d;
      },
      // An answer for a commit that is no longer shown must not cover the current one.
      (err) => {
        if (id === current) error = String(err);
      },
    );
  });

  $effect(() => {
    const current = id;
    const path = tab;
    const wholeFile = whole;
    diffSettings();
    diffs = [];
    api.commitDiff(current, path, wholeFile).then(
      (d) => {
        if (id === current && tab === path && whole === wholeFile) diffs = d;
      },
      (err) => {
        if (id === current) error = String(err);
      },
    );
  });

  function openTab(path: string | null) {
    tab = path;
    if (scroller) scroller.scrollTop = 0;
  }

  const statusIcon: Record<string, string> = { added: "A", deleted: "D", modified: "M", renamed: "R", copied: "C" };
</script>

<div class="panel">
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
    {#if tab === null && detail}
      <div class="files">
        <div class="count">
          <span class="grow">{detail.files.length} {detail.files.length === 1 ? "file" : "files"} changed</span>
          <span class="add">+{totals.add}</span>
          <span class="del">−{totals.del}</span>
          <FoldAll onFold={(fold) => diffView?.setAll(fold)} />
        </div>
        {#each detail.files as file (file.path)}
          {@const p = splitPath(file.path)}
          <button class="file" onclick={() => openTab(file.path)}>
            <span class="badge {file.status}">{statusIcon[file.status]}</span>
            <span class="grow ellipsis"><span class="dir">{p.dir}</span>{p.name}</span>
            {#if foundFiles.includes(file.path)}
              <span class="found" title={find ? `Adds or removes “${find}”` : "The path matches the search"}>
                <svg class="icon" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5" /><path d="M10.3 10.3 14 14" /></svg>{foundText ?? "match"}
              </span>
            {/if}
            <FileBadge {file} />
          </button>
        {/each}
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
      onToggleWhole={() => (whole = !whole)}
      loadFull={(d) => api.commitDiff(row.id, d.file.path, whole, true).then((all) => all[0])}
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
  /* The commit's details are a card, like the graph and the file list in Summary. */
  .head {
    margin: 4px 12px 0;
    padding: 14px 16px 16px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    border: 1px solid var(--sep);
    border-radius: 12px;
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
    border-radius: 10px;
    font-size: 12px;
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
    font-size: 17px;
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
    border-radius: 15px;
    color: #fff;
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 600;
    font-size: 12px;
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
    font-size: 11px;
    color: var(--text2);
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
    border-radius: 13px;
    border: 1px solid var(--sep);
    font-size: 12px;
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
  /* The file list is a card of its own, so it reads apart from the diffs below it. */
  .files {
    margin: 12px 12px 6px;
    padding: 8px 4px 6px;
    border-radius: 12px;
    border: 1px solid var(--sep);
  }
  .count {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px 6px;
    font-size: 11px;
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
    border-radius: 9px;
    background: var(--found-bg);
    box-shadow: inset 0 0 0 1px var(--found-ring);
    font-size: 11px;
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
    border-radius: 7px;
  }
  .file:hover {
    background: var(--field);
  }
  .badge {
    width: 16px;
    height: 16px;
    border-radius: 5px;
    font-size: 10px;
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
    font-size: 11px;
  }
  .del {
    color: var(--red);
    font-size: 11px;
  }
  .num {
    min-width: 22px;
    text-align: right;
  }
  .meta {
    font-size: 11px;
    color: var(--text2);
  }
  .error {
    color: var(--red);
    margin: 12px 20px;
  }
</style>
