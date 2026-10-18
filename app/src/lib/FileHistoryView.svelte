<script lang="ts">
  // File History: the commits that changed one file, across renames, with either what each one
  // changed (Changes) or who last changed every line (Blame). A line number shows every commit
  // that changed that line (git log -L).

  import { tick, untrack } from "svelte";
  import { api } from "./api";
  import { nav } from "./nav.svelte";
  import { diffSettings, wholeByDefault } from "./prefs.svelte";
  import type { Blame, FileCommit, FileDiff, FileHistory, History, HistoryRow } from "./types";
  import { initials, lane, person, relativeTime, shortId, splitPath, tint } from "./format";
  import { languageOf, occurrences, paint } from "./syntax";
  import DiffView from "./DiffView.svelte";
  import Lazy, { BLOCK, matchesIn, placeOf, type FindHits } from "./Lazy.svelte";

  let {
    history,
    path,
    start,
    lookup,
    copy,
  }: {
    history: History;
    path: string;
    /** The commit to select first; HEAD's latest change when missing. */
    start: string | null;
    lookup: (id: string) => HistoryRow | undefined;
    copy: (text: string) => void;
  } = $props();

  let file = $state<FileHistory | null>(null);
  let error = $state<string | null>(null);
  let selected = $state<string | null>(null);
  let showElsewhere = $state(false);
  /** Blame's revision: HEAD, or a commit (Blame Before / Blame Here), and the file's path there. */
  let at = $state<{ rev: string; path: string } | null>(null);
  let blame = $state<Blame | null>(null);
  let blameError = $state<string | null>(null);
  /** A line picked for its history, and the commits that changed it. */
  let line = $state<{ no: number; ids: string[] } | null>(null);
  let diffs = $state<FileDiff[]>([]);
  let whole = $state(wholeByDefault());
  let list = $state<HTMLDivElement>();
  let main = $state<HTMLElement>();
  /** Commits that added or removed the text being found (git log -S); null while there is none. */
  let finding = $state<{ text: string; ids: string[] } | null>(null);

  const mode = $derived(nav.fileMode);
  const all = $derived(file ? [...(showElsewhere ? file.elsewhere : []), ...file.commits] : []);
  const byId = $derived(new Map(all.map((c) => [c.id, c])));
  const pick = $derived(selected ? (byId.get(selected) ?? null) : null);
  const headName = $derived(history.head.branch ? `${history.head.branch} (HEAD)` : "HEAD");

  // Every reload hands over a new history object; these change only when what they stand for does.
  const historyKey = $derived(history.key);
  const headCommit = $derived(history.head.commit);

  // A new file, or a reload that moved a branch, a tag or HEAD, reads the history again. A reload
  // that only saw other files change (focus, staging) leaves it: `key` stays the same.
  $effect(() => {
    const p = path;
    historyKey;
    error = null;
    api.fileHistory(p, null).then(
      (h) => {
        if (path !== p) return;
        file = h;
        if (!selected || !h.commits.some((c) => c.id === selected)) {
          const wanted = start && (h.commits.some((c) => c.id === start) || h.elsewhere.some((c) => c.id === start)) ? start : h.commits[0]?.id ?? null;
          if (wanted && h.elsewhere.some((c) => c.id === wanted)) showElsewhere = true;
          selected = wanted;
        }
      },
      (err) => (error = String(err)),
    );
  });

  // Another file starts over at HEAD.
  $effect(() => {
    path;
    at = null;
    line = null;
  });

  const blameAt = $derived(at ?? { rev: "HEAD", path });

  $effect(() => {
    if (mode !== "blame") return;
    const target = blameAt;
    // A commit's blame never changes; HEAD's does once HEAD moves.
    if (target.rev === "HEAD") headCommit;
    blameError = null;
    api.blame(target.path, target.rev).then(
      (b) => {
        if (blameAt.rev === target.rev && blameAt.path === target.path) blame = b;
      },
      (err) => {
        blame = null;
        blameError = String(err);
      },
    );
  });

  $effect(() => {
    if (mode !== "changes" || !pick) return;
    const commit = pick;
    const wholeFile = whole;
    diffSettings();
    diffs = [];
    api.commitDiff(commit.id, commit.path, wholeFile, false, DIFF).then(
      (d) => {
        if (selected === commit.id && whole === wholeFile) diffs = d;
      },
      // A newer pick stopped it.
      () => {},
    );
  });

  /** Lanes of this view's reads: a newer read in a lane stops the older one. */
  const DIFF = "history-diff";
  const FULL = "history-full";
  // Its reads stop when it closes, and Show Diff Anyway's when another commit is picked.
  $effect(() => () => {
    api.stopReads([DIFF, FULL]).catch(() => {});
  });
  $effect(() => {
    selected;
    untrack(() => api.stopReads([FULL]).catch(() => {}));
  });

  // Keep the selected commit in view in the list.
  $effect(() => {
    selected;
    requestAnimationFrame(() => list?.querySelector<HTMLElement>(".commit.on")?.scrollIntoView({ block: "nearest" }));
  });

  // Blame lines in runs of the same commit, with how old each commit is.
  const blocks = $derived.by(() => {
    if (!blame) return [];
    const out: { commit: number; from: number; lines: { no: number; text: string }[] }[] = [];
    blame.lines.forEach((l, i) => {
      const last = out[out.length - 1];
      if (last && last.commit === l.commit) last.lines.push({ no: i + 1, text: l.text });
      else out.push({ commit: l.commit, from: i + 1, lines: [{ no: i + 1, text: l.text }] });
    });
    return out;
  });
  type Run = (typeof blocks)[number];
  /** Runs drawn together, near the visible part only: a few short ones make up a group of about
   *  a block's lines, and a long one is a group of its own, its lines drawn a block at a time. */
  const groups = $derived.by(() => {
    const out: { runs: Run[]; lines: number; long: boolean }[] = [];
    let open: (typeof out)[number] | null = null;
    for (const run of blocks) {
      if (run.lines.length > BLOCK) {
        out.push({ runs: [run], lines: run.lines.length, long: true });
        open = null;
        continue;
      }
      if (!open || open.lines >= BLOCK) {
        open = { runs: [], lines: 0, long: false };
        out.push(open);
      }
      open.runs.push(run);
      open.lines += run.lines.length;
    }
    return out;
  });

  /** Starts of the blocks a run of `count` lines is drawn in. */
  function starts(count: number): number[] {
    const out: number[] = [];
    for (let at = 0; at < count; at += BLOCK) out.push(at);
    return out;
  }

  const ages = $derived.by(() => {
    const times = blame?.commits.map((c) => c.time) ?? [];
    const min = Math.min(...times);
    const max = Math.max(...times);
    return (time: number) => (max > min ? 0.2 + (0.8 * (time - min)) / (max - min) : 1);
  });
  const linesHere = $derived(blame && selected ? blame.lines.filter((l) => blame!.commits[l.commit].id === selected).length : 0);
  const language = $derived(languageOf(path));

  function colorOfCommit(id: string): number {
    return lookup(id)?.graph.color ?? 0;
  }

  function select(id: string) {
    selected = id;
  }

  /** Blame as of the commit before this one: who wrote the lines it changed. */
  function blameBefore(c: FileCommit) {
    const parent = c.parents[0];
    if (!parent) return;
    at = { rev: parent, path: c.oldPath ?? c.path };
    line = null;
    nav.fileMode = "blame";
  }

  function blameHere(c: FileCommit) {
    at = { rev: c.id, path: c.path };
    line = null;
    nav.fileMode = "blame";
  }

  async function lineHistory(no: number) {
    if (line?.no === no) {
      line = null;
      return;
    }
    try {
      const ids = await api.lineHistory(blameAt.path, no, blameAt.rev);
      line = { no, ids };
      if (ids[0]) selected = ids[0];
    } catch (err) {
      blameError = String(err);
    }
  }

  /** Where Blame is: "at main (HEAD)" or "at 1a2b3c4, before ‘Edit config’". */
  const atLabel = $derived.by(() => {
    if (!at) return `at ${headName}`;
    const c = byId.get(at.rev) ?? file?.commits.find((x) => x.parents[0] === at!.rev);
    if (c && c.id !== at.rev) return `at ${shortId(at.rev)}, just before “${c.summary}”`;
    return `at ${shortId(at.rev)}`;
  });

  /** Whether Blame's revision has this commit: otherwise it offers Blame Here. */
  function inRevision(c: FileCommit): boolean {
    if (!file || file.elsewhere.some((x) => x.id === c.id)) return false;
    if (!at) return true;
    const i = file.commits.findIndex((x) => x.id === c.id);
    const here = file.commits.findIndex((x) => x.id === at!.rev);
    if (here >= 0) return i >= here;
    const before = file.commits.findIndex((x) => x.parents[0] === at!.rev);
    return before < 0 || i > before;
  }

  // Find in file: the matches are counted from the lines (the diff's by DiffView), not from what
  // is drawn, and only the block with the current one is drawn wherever it is: drawing every
  // block with a match would undo drawing only what is seen.
  const find = $derived(nav.find.text);
  let diffHits = $state<FindHits>([]);
  const blameHits = $derived.by(() => {
    const out: FindHits = [];
    if (!find || !blame) return out;
    const matchCase = nav.find.matchCase;
    const count = (lines: { text: string }[]) => lines.reduce((n, l) => n + occurrences(l.text || " ", find, matchCase).length, 0);
    for (const group of groups) {
      const first = group.runs[0];
      if (group.long) {
        for (const start of starts(first.lines.length)) {
          const n = count(first.lines.slice(start, start + BLOCK));
          if (n) out.push({ key: `${first.from}:${start}`, count: n });
        }
      } else {
        const n = group.runs.reduce((k, r) => k + count(r.lines), 0);
        if (n) out.push({ key: `${first.from}`, count: n });
      }
    }
    return out;
  });
  const hits = $derived(mode === "changes" ? diffHits : blameHits);
  /** The block with the current match. */
  let focus = $state<string | null>(null);
  let findKey = "";
  let findShown: unknown = null;
  let shownAt = -1;
  /** New code starts at its first match, scrolled to once its matches are counted. */
  let scrollDue = false;

  $effect(() => {
    const key = `${find}\u0000${nav.find.matchCase}\u0000${mode}\u0000${path}`;
    const shown = mode === "changes" ? diffs : blame;
    const count = hits.reduce((n, h) => n + h.count, 0);
    untrack(() => {
      const fresh = key !== findKey || shown !== findShown;
      findKey = key;
      findShown = shown;
      if (fresh) scrollDue = true;
      const at = fresh ? 0 : Math.min(nav.found.at, Math.max(0, count - 1));
      shownAt = at;
      nav.found = { count, at };
      show(at, false);
    });
  });

  // The arrows and Enter move to another match.
  $effect(() => {
    const at = nav.found.at;
    if (at === shownAt) return;
    shownAt = at;
    show(at, true);
  });

  /** Ring the current match, and scroll to it: its block is drawn first if it is far away. */
  let showing = 0;
  function show(at: number, scroll: boolean) {
    const place = placeOf(hits, at);
    focus = place?.key ?? null;
    const call = ++showing;
    tick().then(() => {
      if (call !== showing || !main) return;
      main.querySelectorAll(".found.current").forEach((el) => el.classList.remove("current"));
      const block = place && main.querySelector(`[data-block="${CSS.escape(place.key)}"]`);
      const match = block ? matchesIn(block)[place.nth] : undefined;
      if (!match) return;
      match.forEach((el) => el.classList.add("current"));
      if (scroll || scrollDue) {
        scrollDue = false;
        match[0].scrollIntoView({ block: "center", inline: "nearest" });
      }
    });
  }

  // The commits that added or removed the text, asked of git a moment after typing stops.
  $effect(() => {
    const text = find;
    const matchCase = nav.find.matchCase;
    const p = path;
    headCommit;
    if (!text) {
      finding = null;
      return;
    }
    const timer = setTimeout(() => {
      api.filePickaxe(p, text, matchCase, "HEAD").then(
        (ids) => {
          if (nav.find.text === text && nav.find.matchCase === matchCase && path === p) finding = { text, ids };
        },
        () => (finding = null),
      );
    }, 300);
    return () => clearTimeout(timer);
  });

  const renamedAt = (c: FileCommit) => c.oldPath && (c.status === "renamed" || c.status === "copied");
</script>

{#snippet rows(lines: { no: number; text: string }[])}
  {#each lines as l (l.no)}
    <div class="row">
      <button class="no" class:picked={line?.no === l.no} onclick={() => lineHistory(l.no)} title="Every change to line {l.no}">{l.no}</button>
      <span class="text">{#each paint(l.text || " ", null, language, find || null, nav.find.matchCase) as piece, k (k)}<span class:found={piece.found} data-hit={piece.hit} class={piece.role ? `syn-${piece.role}` : undefined}>{piece.text}</span>{/each}</span>
    </div>
  {/each}
{/snippet}

{#snippet run(block: Run, long: boolean)}
  {@const c = blame!.commits[block.commit]}
  {@const color = colorOfCommit(c.id)}
  {@const on = c.id === selected}
  {@const dim = !!line && !line.ids.includes(c.id)}
  <div class="block" class:on class:dim class:first={block.from === 1} style:background={on ? tint(color) : undefined}>
    <button class="gutter" onclick={() => select(c.id)} title="{c.summary}\n{c.authorName} · {shortId(c.id)}">
      <span class="age" style:background={lane(color)} style:opacity={ages(c.time)}></span>
      <span class="mini" style:background={person(c.authorEmail)}>{initials(c.authorName).slice(0, 1)}</span>
      <span class="msg">{c.summary}</span>
      <span class="when">{relativeTime(c.time)}</span>
    </button>
    <div class="rows">
      {#if long}
        {#each starts(block.lines.length) as start (start)}
          {@const part = block.lines.slice(start, start + BLOCK)}
          <Lazy lines={part.length} of={part[0]} name={`${block.from}:${start}`} force={focus === `${block.from}:${start}`}>{@render rows(part)}</Lazy>
        {/each}
      {:else}
        {@render rows(block.lines)}
      {/if}
    </div>
  </div>
{/snippet}

{#snippet commitRow(c: FileCommit, other: boolean)}
  {@const color = colorOfCommit(c.id)}
  {@const dim = (!!line && !line.ids.includes(c.id)) || (!!finding && !finding.ids.includes(c.id))}
  <button class="commit" class:on={selected === c.id} class:dim class:other style:--lane={lane(color)} style:background={selected === c.id ? tint(color) : undefined} onclick={() => select(c.id)}>
    <span class="rail"><span class="dot" class:hollow={other}></span></span>
    <span class="lines">
      <span class="summary">{c.summary}</span>
      <span class="byline">{c.authorName} · {relativeTime(c.time)}{#if other && lookup(c.id)?.graph.branch} · on {lookup(c.id)?.graph.branch}{/if}</span>
    </span>
    <span class="stats">
      <span class="nums"><span class="add">+{c.additions}</span> <span class="del">−{c.deletions}</span></span>
      <span class="sha">{shortId(c.id)}</span>
    </span>
  </button>
  {#if renamedAt(c)}
    <div class="renamed">
      <svg class="icon" viewBox="0 0 16 16"><path d="M5 13V6.5a2 2 0 0 1 2-2h5.5M10 2l2.5 2.5L10 7" /></svg>
      Renamed from <span class="mono">{c.oldPath}</span>
    </div>
  {/if}
{/snippet}

<div class="screen">
  <section class="side" aria-label="Commits that changed the file">
    {#if error}
      <p class="error" role="alert">{error}</p>
    {:else if file}
      <div class="top">
        <span>{file.commits.length}{file.more ? "+" : ""} {file.commits.length === 1 ? "change" : "changes"} to this file</span>
        <span class="follows" title="git log --follow">
          <svg class="icon" viewBox="0 0 16 16"><path d="M2.5 5.5h9M9 3l2.5 2.5L9 8M13.5 10.5h-9M7 8l-2.5 2.5L7 13" /></svg>Follows renames
        </span>
      </div>
      {#if line}
        <div class="chip">
          <span><b>Line {line.no}</b> · changed in {line.ids.length} {line.ids.length === 1 ? "commit" : "commits"}</span>
          <button onclick={() => (line = null)} aria-label="Show all commits">×</button>
        </div>
      {/if}
      {#if finding}
        <div class="chip">
          <span><b>“{finding.text}”</b> · {finding.ids.length ? `added or removed in ${finding.ids.length} ${finding.ids.length === 1 ? "commit" : "commits"}` : "no commit added or removed it"}</span>
          <button onclick={() => (nav.find = { ...nav.find, text: "" })} aria-label="Stop finding">×</button>
        </div>
      {/if}
      {#if file.elsewhere.length}
        <div class="elsewhere">
          <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="3" stroke-dasharray="2 2" /></svg>
          <span>{file.elsewhere.length} more {file.elsewhere.length === 1 ? "change" : "changes"} on other branches</span>
          <button onclick={() => (showElsewhere = !showElsewhere)}>{showElsewhere ? "Hide" : "Show"}</button>
        </div>
      {/if}
      <div class="list" bind:this={list}>
        {#if showElsewhere}
          {#each file.elsewhere as c (c.id)}{@render commitRow(c, true)}{/each}
          <div class="sep"></div>
        {/if}
        {#each file.commits as c (c.id)}{@render commitRow(c, false)}{/each}
        {#if !file.commits.length}<p class="hint">No commit on {headName} has this file.</p>{/if}
      </div>
      <p class="hint">
        {mode === "blame" ? "Click a commit to highlight its lines. Click a line number to see every change to that line." : "Click a commit to see what it changed in this file."}
      </p>
    {/if}
  </section>

  <section class="main" bind:this={main} aria-label={mode === "blame" ? "Blame" : "Changes"}>
    {#if mode === "changes"}
      {#if pick}
        {@const color = colorOfCommit(pick.id)}
        <div class="head">
          <span class="avatar" style:background={person(pick.authorEmail)}>{initials(pick.authorName)}</span>
          <span class="who">
            <span class="title">{pick.summary}</span>
            <span class="byline">{pick.authorName} · {relativeTime(pick.time)} · <button class="mono link" onclick={() => copy(pick.id)} title="Copy the SHA">{shortId(pick.id)}</button></span>
          </span>
          <button class="button" onclick={() => blameHere(pick)} disabled={pick.status === "deleted"}>Blame Here</button>
        </div>
        {#if pick.status === "added"}<p class="note">This commit created the file.</p>{/if}
        {#if renamedAt(pick)}<p class="note">This commit renamed it from <span class="mono">{pick.oldPath}</span>.</p>{/if}
        {#if !byId.has(pick.id) || file?.elsewhere.some((c) => c.id === pick.id)}<p class="note">This change is on another branch, not on {headName}.</p>{/if}
        <div class="scroll">
          <DiffView
            {diffs}
            {color}
            {whole}
            history={false}
            find={find || null}
            matchCase={nav.find.matchCase}
            {focus}
            bind:hits={diffHits}
            onToggleWhole={() => (whole = !whole)}
            loadFull={(d) => api.commitDiff(pick!.id, d.file.path, whole, true, FULL).then((all) => all[0])}
          />
        </div>
      {:else}
        <p class="hint pad">Pick a commit on the left.</p>
      {/if}
    {:else}
      {#if pick}
        <div class="head">
          <span class="avatar" style:background={person(pick.authorEmail)}>{initials(pick.authorName)}</span>
          <span class="who">
            <span class="title">{pick.summary}</span>
            <span class="byline">{pick.authorName} · {relativeTime(pick.time)} · {shortId(pick.id)} · {linesHere} {linesHere === 1 ? "line" : "lines"} here</span>
          </span>
          <button class="button" onclick={() => (nav.fileMode = "changes")}>Changes</button>
          {#if !inRevision(pick)}
            <button class="button" onclick={() => blameHere(pick)} disabled={pick.status === "deleted"}>Blame Here</button>
          {:else if pick.parents[0] && pick.status !== "added"}
            <button class="button" onclick={() => blameBefore(pick)}>
              <svg class="icon" viewBox="0 0 16 16"><path d="M3 6.5h7a3.5 3.5 0 0 1 0 7H6M5.5 4 3 6.5 5.5 9" /></svg>Blame Before
            </button>
          {/if}
        </div>
      {/if}
      <div class="revision">
        <span class="where"><span class="dir">{splitPath(blameAt.path).dir}</span><b>{splitPath(blameAt.path).name}</b> <span class="at">{atLabel}</span></span>
        {#if at}<button class="back" onclick={() => ((at = null), (line = null))}>Back to HEAD</button>{/if}
        <span class="spacer"></span>
        {#if blame}<button class="mono cmd" onclick={() => copy(blame!.command)} title="Copy the command">$ git {blame.command.replace(/^git /, "")}</button>{/if}
        <span class="legend">Older <span class="heat">{#each [0.25, 0.45, 0.65, 0.85, 1] as o (o)}<i style:opacity={o}></i>{/each}</span> Newer</span>
      </div>
      {#if blameError}
        <p class="error" role="alert">{blameError}</p>
      {:else if blame}
        <div class="code mono selectable" style:--digits={String(blame.lines.length).length}>
          {#each groups as group (group.runs[0].from)}
            {#if group.long}
              {@render run(group.runs[0], true)}
            {:else}
              <Lazy lines={group.lines} of={group.runs[0]} name={`${group.runs[0].from}`} force={focus === `${group.runs[0].from}`}>
                {#each group.runs as block (block.from)}{@render run(block, false)}{/each}
              </Lazy>
            {/if}
          {/each}
        </div>
      {:else}
        <p class="hint pad">Reading who changed each line…</p>
      {/if}
    {/if}
  </section>
</div>

<style>
  .screen {
    flex-grow: 1;
    display: flex;
    min-height: 0;
    gap: 0;
  }
  .side {
    width: 340px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
    margin: 4px 0 12px 12px;
    border: 1px solid var(--sep);
    border-radius: var(--radius-l);
    background: var(--graph-bg);
    overflow: hidden;
  }
  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 12px 14px 6px;
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
  }
  .follows {
    display: flex;
    align-items: center;
    gap: 4px;
    font-weight: 400;
  }
  .follows .icon {
    width: 12px;
    height: 12px;
  }
  .chip,
  .elsewhere {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 4px 8px;
    padding: 0 6px 0 12px;
    height: 32px;
    border-radius: var(--radius-m);
    background: var(--field);
    font-size: var(--text-small);
  }
  .chip span,
  .elsewhere span {
    flex-grow: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .chip button {
    width: 22px;
    height: 22px;
    border-radius: var(--radius-pill);
    font-size: var(--text-heading);
    color: var(--text2);
    text-align: center;
  }
  .elsewhere .icon {
    width: 12px;
    height: 12px;
    color: var(--text2);
  }
  .elsewhere button {
    height: 24px;
    padding: 0 10px;
    border-radius: var(--radius-pill);
    background: var(--win);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.1);
    font-size: var(--text-small);
  }
  .list {
    flex-grow: 1;
    overflow-y: auto;
    padding: 4px 8px 8px;
  }
  .commit {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-height: 52px;
    padding: 6px 10px 6px 4px;
    border-radius: var(--radius-l);
    text-align: left;
  }
  .commit:hover:not(.on) {
    background: var(--field);
  }
  .commit.dim {
    opacity: 0.4;
  }
  .rail {
    position: relative;
    align-self: stretch;
    width: 18px;
    flex-shrink: 0;
  }
  .rail::before {
    content: "";
    position: absolute;
    left: 8px;
    top: -6px;
    bottom: -6px;
    width: 2px;
    background: var(--lane);
    opacity: 0.6;
  }
  .other .rail::before {
    background: none;
    border-left: 2px dashed var(--lane);
  }
  .dot {
    position: absolute;
    left: 4px;
    top: 50%;
    margin-top: -5px;
    width: 10px;
    height: 10px;
    border-radius: var(--radius-pill);
    background: var(--lane);
    box-shadow: 0 0 0 2px var(--graph-bg);
  }
  .on .dot {
    left: 2px;
    margin-top: -7px;
    width: 10px;
    height: 10px;
    border: 2px solid var(--graph-bg);
    box-shadow: 0 0 0 2px var(--lane);
  }
  .dot.hollow {
    background: var(--graph-bg);
    border: 2px solid var(--lane);
    width: 6px;
    height: 6px;
  }
  .lines {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .summary,
  .title {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .on .summary {
    font-weight: 600;
  }
  .byline {
    font-size: var(--text-caption);
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .stats {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 2px;
    font-family: var(--mono);
    font-size: var(--text-caption);
    flex-shrink: 0;
  }
  .sha {
    color: var(--text2);
  }
  .add {
    color: var(--green);
  }
  .del {
    color: var(--red);
  }
  .renamed {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px 6px 34px;
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .renamed .icon {
    width: 12px;
    height: 12px;
  }
  .mono {
    font-family: var(--mono);
  }
  .sep {
    height: 1px;
    margin: 6px 10px;
    background: var(--sep);
  }
  .hint {
    margin: 0;
    padding: 10px 14px 12px;
    font-size: var(--text-caption);
    color: var(--text2);
    line-height: 1.45;
  }
  .hint.pad {
    padding: 24px;
  }
  .main {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 4px 12px 0;
    padding: 10px 12px;
    border: 1px solid var(--sep);
    border-radius: var(--radius-l);
    flex-shrink: 0;
  }
  .avatar,
  .mini {
    color: #fff;
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 600;
    flex-shrink: 0;
  }
  .avatar {
    width: 30px;
    height: 30px;
    border-radius: var(--radius-pill);
    font-size: var(--text-small);
  }
  .who {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .title {
    font-weight: 600;
  }
  .link {
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .link:hover {
    color: var(--text);
  }

  .button .icon {
    width: 13px;
    height: 13px;
  }
  .note {
    margin: 8px 16px 0;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .scroll {
    flex-grow: 1;
    overflow-y: auto;
    min-height: 0;
  }
  .revision {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 16px 6px;
    font-size: var(--text-small);
    flex-shrink: 0;
    min-width: 0;
  }
  .where {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .dir,
  .at {
    color: var(--text2);
  }
  .back {
    height: 24px;
    padding: 0 10px;
    border-radius: var(--radius-pill);
    background: var(--field);
    font-size: var(--text-small);
    flex-shrink: 0;
  }
  .spacer {
    flex-grow: 1;
  }
  .cmd {
    font-size: var(--text-caption);
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
    flex-shrink: 1;
  }
  .cmd:hover {
    color: var(--text);
  }
  .legend {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: var(--text-caption);
    color: var(--text2);
    flex-shrink: 0;
  }
  .heat {
    display: flex;
    gap: 2px;
  }
  .heat i {
    width: 3px;
    height: 11px;
    border-radius: 1.5px;
    background: var(--lane-0);
  }
  .code {
    flex-grow: 1;
    overflow: auto;
    min-height: 0;
    margin: 0 12px 12px;
    font-family: var(--code-font);
    font-size: var(--code-size);
    line-height: var(--code-line);
    tab-size: var(--tab);
    color: var(--code-fg, var(--code));
    background: var(--code-bg, transparent);
    border-radius: var(--radius-l);
  }
  .block {
    display: flex;
    min-width: max-content;
    border-radius: var(--radius-m);
  }
  .block.dim {
    opacity: 0.45;
  }
  .gutter {
    position: sticky;
    left: 0;
    align-self: stretch;
    display: flex;
    align-items: flex-start;
    gap: 6px;
    width: 250px;
    flex-shrink: 0;
    padding: 0 8px 0 10px;
    font-family: var(--font);
    font-size: var(--text-caption);
    line-height: var(--code-line);
    text-align: left;
    color: var(--text2);
    background: inherit;
  }
  .block:not(.first) .gutter {
    border-top: 1px solid var(--sep);
  }
  .gutter:hover .msg {
    color: var(--text);
  }
  .age {
    position: absolute;
    left: 2px;
    top: 2px;
    bottom: 2px;
    width: 3px;
    border-radius: 1.5px;
  }
  .mini {
    width: 15px;
    height: 15px;
    border-radius: var(--radius-pill);
    font-size: var(--text-caption);
    margin-top: 2px;
  }
  .msg {
    flex-grow: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text);
  }
  .on .msg {
    font-weight: 600;
  }
  .when {
    flex-shrink: 0;
  }
  .rows {
    flex-grow: 1;
  }
  .row {
    display: grid;
    /* As wide as the file's longest line number, in the code's font, and its padding. */
    grid-template-columns: max(44px, calc(var(--digits, 4) * 1ch + 20px)) 1fr;
  }
  .no {
    color: var(--code-dim, var(--text2));
    opacity: 0.7;
    text-align: right;
    padding-right: 12px;
    font: inherit;
    user-select: none;
    -webkit-user-select: none;
    border-radius: var(--radius-s);
  }
  .no:hover {
    opacity: 1;
    color: var(--text);
  }
  .no.picked {
    opacity: 1;
    color: var(--text);
    font-weight: 700;
    background: var(--side-sel);
  }
  /* No wrapping: a wrapped line would push the blame of the lines below out of step. */
  .text {
    white-space: pre;
    padding-right: 16px;
  }
  .error {
    margin: 20px;
    color: var(--red);
  }
  /* Found text: an outlined neutral capsule, as in History search; the current one gets a strong ring. */
  .found {
    border-radius: var(--radius-xs);
    box-shadow: 0 0 0 1.5px var(--found-ring);
    background: var(--found-bg);
  }
  .found:global(.current) {
    box-shadow: 0 0 0 2px var(--text);
  }
</style>
