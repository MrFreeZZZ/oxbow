<script lang="ts">
  // The Files tab of a pull request: what it changes since it split from its base, as GitHub
  // shows it, fetched here and read like any diff. Each file has a Viewed box, kept on GitHub:
  // a viewed file folds, and GitHub unticks it once it changes again. Lines near a change take
  // comments, into the review or sent at once; GitHub's threads show under their lines. After
  // the author pushed again, the changes since the review started can be read on their own.
  import { untrack } from "svelte";
  import { api, stopped } from "./api";
  import { wholeByDefault } from "./prefs.svelte";
  import type { Comparison, DiffLine, DraftComment, FileChange, FileDiff, Hunk } from "./types";
  import { shortId, splitPath } from "./format";
  import DiffView, { type LineNotes } from "./DiffView.svelte";
  import FileBadge from "./FileBadge.svelte";
  import Avatar from "./Avatar.svelte";
  import CommentEditor from "./CommentEditor.svelte";
  import DraftNote from "./DraftNote.svelte";
  import ReviewThread from "./ReviewThread.svelte";
  import type { ReviewSession } from "./review.svelte";
  import { commentable, covers, liveThreads, lineKey, placeOf, pushedText, spotLabel, spotOf, type Thread } from "./reviewDraft";

  let {
    session,
    me,
    color,
    version,
  }: {
    session: ReviewSession;
    /** The account signed in, which comments here. */
    me: string | null;
    color: number;
    /** Goes up on every reload, so the files load again. */
    version: number;
  } = $props();

  const pr = $derived(session.pr);

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
  let filter = $state("");
  let picked = $state<string | null>(null);
  let diffView = $state<DiffView>();
  let scroller = $state<HTMLDivElement>();
  /** All the pull request's changes, or only the ones pushed since the review started. */
  let since = $state(false);
  /** Files the author changed since the review started; null while unknown. */
  let changed = $state.raw<Set<string> | null>(null);

  const head = $derived(`refs/oxbow/pull/${pr.number}/head`);
  const base = $derived(`refs/oxbow/pull/${pr.number}/base`);
  /** The commit the draft's comments are on, when the pull request moved on from it. */
  const reviewed = $derived(session.draft && session.draft.commit !== pr.headSha ? session.draft.commit : null);

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

  // Back to all the changes once there's nothing since the review.
  $effect(() => {
    if (!reviewed && since) untrack(() => ((since = false), read()));
  });

  // The files changed since the review started, so comments stay off their new lines.
  $effect(() => {
    const from = reviewed;
    const sha = pr.headSha;
    changed = null;
    if (!from) return;
    api.compare(from, sha, "tips").then(
      (c) => reviewed === from && (changed = new Set(c.files.map((f) => f.path))),
      () => reviewed === from && (changed = new Set(comparison?.files.map((f) => f.path) ?? [])),
    );
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
    session.loadViewing().catch(() => {});
    await read();
  }

  /** The diff of all the changes, or of the ones since the review started. */
  async function read() {
    const sha = pr.headSha;
    try {
      const c = since && reviewed ? await api.compare(reviewed, head, "tips") : await api.compare(base, head, "split");
      if (pr.headSha !== sha) return;
      const same = comparison && comparison.from === c.from && comparison.targetId === c.targetId;
      comparison = c;
      session.shown = c.targetId;
      if (!since) session.files = c.files.length;
      if (!same) readDiffs(c);
    } catch (err) {
      error = stale ?? String(err);
    } finally {
      loading = false;
    }
  }

  function showSince(on: boolean) {
    if (since === on) return;
    since = on;
    read();
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
  const viewedCount = $derived(files.filter((f) => session.viewed.has(f.path)).length);

  /** Tick or untick a file's Viewed box, here at once and then on GitHub. */
  function setViewed(path: string, on: boolean) {
    diffView?.fold(path, on);
    session.setViewed(path, on, (err) => (error = `Couldn’t mark ${path} ${on ? "viewed" : "not viewed"} on GitHub: ${err}`));
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

  // --- Comments on lines ---

  /** GitHub's threads on the latest lines, by line. */
  const threads = $derived.by(() => {
    const out = new Map<string, Thread[]>();
    for (const t of liveThreads(pr)) {
      const key = lineKey(t.path, t.side, t.line!);
      out.set(key, [...(out.get(key) ?? []), t]);
    }
    return out;
  });
  /** Whether the draft's comments on a file are on the lines shown: not after the file changed. */
  const sameLines = (path: string) => !reviewed || (changed !== null && !changed.has(path));
  /** The draft's comments on lines shown, by line; the others go over their file's code. */
  const drafts = $derived.by(() => {
    const out = new Map<string, DraftComment[]>();
    for (const c of session.draft?.comments ?? []) {
      if (!sameLines(c.spot.path)) continue;
      const key = lineKey(c.spot.path, c.spot.side, c.spot.line);
      out.set(key, [...(out.get(key) ?? []), c]);
    }
    return out;
  });
  const moved = (path: string) => (session.draft?.comments ?? []).filter((c) => c.spot.path === path && !sameLines(path));
  /** Threads and draft comments on each file, for the list. */
  const talk = $derived.by(() => {
    const out = new Map<string, number>();
    for (const t of liveThreads(pr)) out.set(t.path, (out.get(t.path) ?? 0) + 1);
    for (const c of session.draft?.comments ?? []) out.set(c.spot.path, (out.get(c.spot.path) ?? 0) + 1);
    return out;
  });

  /** The line picked first, for Shift-click to make a range of. */
  let anchor: { path: string; hunk: Hunk; index: number } | null = null;
  const writing = $derived(session.editing?.kind === "comment" ? session.editing : null);
  const keyOf = (path: string, line: DiffLine) => {
    const at = placeOf(line);
    return at ? lineKey(path, at.side, at.line) : "";
  };

  const notes: LineNotes = {
    can: (diff, hunk, index) => !since && !!me && pr.state === "open" && commentable(hunk, index),
    start: (diff, hunk, index, range) => {
      const path = diff.file.path;
      const from = range && anchor && anchor.path === path && anchor.hunk === hunk ? anchor.index : index;
      const spot = spotOf(path, hunk, from, index);
      if (!spot) return;
      if (!range) anchor = { path, hunk, index };
      const body = writing?.body ?? "";
      session.editing = { kind: "comment", spot, body };
    },
    lit: (diff, line) => {
      const at = placeOf(line);
      return !!at && !!writing && writing.spot.path === diff.file.path && covers(writing.spot, at.side, at.line);
    },
    has: (diff, line) => {
      if (since) return false;
      const key = keyOf(diff.file.path, line);
      if (!key) return false;
      return threads.has(key) || drafts.has(key) || (!!writing && writing.spot.path === diff.file.path && key === lineKey(writing.spot.path, writing.spot.side, writing.spot.line));
    },
    under: lineNotes,
    top: movedNotes,
  };

  /** Why a comment on this file can't go into the review. */
  const addOff = (path: string) =>
    !reviewed ? null : changed === null ? "Working out what changed since your review started…" : changed.has(path) ? `${path} changed since your review started on ${shortId(reviewed)}: comment now, or send the review first.` : null;

  const statusIcon: Record<string, string> = { added: "A", deleted: "D", modified: "M", renamed: "R", copied: "C" };
</script>

{#snippet lineNotes(diff: FileDiff, line: DiffLine)}
  {@const key = keyOf(diff.file.path, line)}
  {#each threads.get(key) ?? [] as t (t.id)}
    <ReviewThread thread={t} {session} {me} />
  {/each}
  {#each drafts.get(key) ?? [] as c (c.id)}
    <DraftNote {session} id={c.id} body={c.body} time={c.time} me={me ?? ""} label={c.spot.startLine ? spotLabel(c.spot) : ""} />
  {/each}
  {#if writing && writing.spot.path === diff.file.path && key === lineKey(writing.spot.path, writing.spot.side, writing.spot.line)}
    <div class="write">
      <div class="write-head"><Avatar login={me ?? "?"} size={22} /><b>Comment on {spotLabel(writing.spot)}</b></div>
      <CommentEditor
        bind:value={writing.body}
        now="Comment Now"
        addOff={addOff(diff.file.path)}
        onCancel={() => (session.editing = null)}
        onNow={() => session.commentNow(writing.spot, writing.body)}
        onAdd={() => session.addComment(writing.spot, writing.body)}
      />
    </div>
  {/if}
{/snippet}

{#snippet movedNotes(diff: FileDiff)}
  {@const list = since ? [] : moved(diff.file.path)}
  {#if list.length}
    <div class="moved">
      <span class="moved-head">On {shortId(reviewed ?? "")}, the version you read: the file changed since</span>
      {#each list as c (c.id)}
        <DraftNote {session} id={c.id} body={c.body} time={c.time} me={me ?? ""} label="{c.spot.path.split('/').pop()} {spotLabel(c.spot)}" />
      {/each}
    </div>
  {/if}
{/snippet}

{#snippet viewedBox(path: string, label: boolean)}
  {@const on = session.viewed.has(path)}
  <button
    class="viewed"
    class:labelled={label}
    role="checkbox"
    aria-checked={on}
    aria-label="Viewed"
    title={on ? "Viewed: click when it needs another look" : session.dismissed.has(path) ? "Changed since you viewed it" : "Mark viewed: it folds, and GitHub keeps the tick"}
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
        <div class="file" class:on={picked === file.path} class:done={session.viewed.has(file.path)}>
          {@render viewedBox(file.path, false)}
          <button class="name" onclick={() => reveal(file.path)} title={file.path}>
            <span class="letter {file.status}">{statusIcon[file.status] ?? "M"}</span>
            <span class="path"><span class="dir">{p.dir}</span>{p.name}</span>
            {#if session.dismissed.has(file.path)}<span class="changed" title="Changed since you viewed it">changed</span>{:else if reviewed && changed?.has(file.path)}<span class="changed" title="Changed since your review started">changed</span>{/if}
          </button>
          {#if talk.get(file.path)}
            <span class="talk" title="Comments on it"><svg viewBox="0 0 16 16"><path d="M3 4a1.5 1.5 0 0 1 1.5-1.5h7A1.5 1.5 0 0 1 13 4v5a1.5 1.5 0 0 1-1.5 1.5H7L4.5 13v-2.5A1.5 1.5 0 0 1 3 9z" /></svg>{talk.get(file.path)}</span>
          {/if}
          <span class="counts"><FileBadge {file} /></span>
        </div>
      {/each}
    </div>
  </div>
  <div class="diff" bind:this={scroller}>
    {#if reviewed}
      <div class="pushed">
        <svg class="icon" viewBox="0 0 16 16"><path d="M8 13V3.5M4.5 7 8 3.5 11.5 7" /></svg>
        <div class="pushed-text">
          <span><b>{pushedText(pr, reviewed)}</b> Your comments stay on {shortId(reviewed)}, the version you read.</span>
          <div class="segmented" role="radiogroup" aria-label="Changes shown">
            <button role="radio" aria-checked={!since} class:on={!since} onclick={() => showSince(false)}>All Changes</button>
            <button role="radio" aria-checked={since} class:on={since} onclick={() => showSince(true)}>Since Your Review</button>
          </div>
        </div>
      </div>
    {/if}
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
        shut={(d) => session.viewed.has(d.file.path)}
        notes={me ? notes : null}
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
  .talk {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-size: var(--text-caption);
    color: var(--text2);
    flex-shrink: 0;
  }
  .talk svg {
    width: 12px;
    height: 12px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.3;
    stroke-linejoin: round;
  }
  .write {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    border-top: 1px solid var(--sep);
  }
  .write:first-child {
    border-top: none;
  }
  .write-head {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--text-small);
  }
  .moved {
    margin: 4px 0 10px;
    border: 1px dashed var(--sep);
    border-radius: var(--radius-l);
  }
  .moved-head {
    display: block;
    padding: 8px 12px 0;
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .pushed {
    display: flex;
    gap: 10px;
    margin: 0 var(--space-3) 10px;
    padding: 12px 14px;
    border-radius: var(--radius-l);
    background: var(--accent-quiet);
    font-size: var(--text-small);
    line-height: 1.4;
  }
  .pushed .icon {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    margin-top: 1px;
    color: var(--accent-ink);
  }
  .pushed-text {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
  }
  .segmented {
    display: inline-flex;
    padding: 2px;
    border-radius: var(--radius-m);
    background: var(--field);
  }
  .segmented button {
    height: 22px;
    padding: 0 10px;
    border-radius: var(--radius-s);
    font-size: var(--text-small);
    color: var(--text2);
  }
  .segmented button.on {
    background: var(--win);
    color: var(--text);
    font-weight: 600;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.08);
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
