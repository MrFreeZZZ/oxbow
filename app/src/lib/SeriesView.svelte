<script lang="ts">
  // Compare Series: the commits of a branch before a rebase, an amend or a force push beside the
  // ones after it, paired by `git range-diff`. Each pair says whether it is the same change, a
  // changed one (message, code, author), a new commit or a dropped one; a changed pair shows how
  // its two versions differ, apart from the base under them.

  import { api } from "./api";
  import type { HistoryRow, Series, SeriesCommit, SeriesPair } from "./types";
  import { relativeTime, shortId, tint, plate } from "./format";
  import { baseLine, firstPick, headline, pairLabel, seriesRow, tally, type SeriesQuery } from "./series";
  import CommitPanel from "./CommitPanel.svelte";
  import InterDiff from "./InterDiff.svelte";

  let {
    query,
    version,
    colorOf,
    lookup,
    onBranch,
    copy,
  }: {
    query: SeriesQuery;
    /** Goes up on every reload of the history. */
    version: number;
    colorOf: (name: string) => number;
    lookup: (id: string) => HistoryRow | undefined;
    /** Another branch of the same step. */
    onBranch: (branch: string) => void;
    copy: (text: string) => void;
  } = $props();

  let series = $state<Series | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let picked = $state(0);
  /** A changed pair: how its versions differ, or one version as a commit. */
  let side = $state<"between" | "old" | "new">("between");
  let list = $state<HTMLElement>();

  $effect(() => {
    const { old, new: next, rewritten } = query;
    version;
    loading = true;
    error = null;
    let latest = true;
    api.series(old, next, rewritten).then(
      (s) => {
        if (!latest) return;
        series = s;
        loading = false;
        picked = firstPick(s);
        side = "between";
      },
      (err) => {
        if (!latest) return;
        series = null;
        loading = false;
        error = String(err);
      },
    );
    return () => {
      latest = false;
    };
  });

  const color = $derived(colorOf(query.branch));
  const pair = $derived(series?.pairs[picked] ?? null);

  const tone: Record<SeriesPair["kind"], () => [string, string]> = {
    same: () => ["var(--field)", "var(--text2)"],
    changed: () => ["var(--orange-soft)", "var(--orange)"],
    added: () => [tint(0, "label"), plate(0)],
    dropped: () => ["var(--field)", "var(--text2)"],
  };
  const ICONS: Record<SeriesPair["kind"], string> = {
    same: "M3.5 6h9M3.5 10h9",
    changed: "M10.5 2.8l2.7 2.7-7 7H3.5V9.8zM9 4.3l2.7 2.7",
    added: "M8 3.5v9M3.5 8h9",
    dropped: "M4.5 4.5l7 7M11.5 4.5l-7 7",
  };

  function pick(i: number) {
    picked = i;
    side = "between";
  }

  function onKey(event: KeyboardEvent) {
    if (!series || (event.key !== "ArrowDown" && event.key !== "ArrowUp")) return;
    event.preventDefault();
    const next = Math.min(series.pairs.length - 1, Math.max(0, picked + (event.key === "ArrowDown" ? 1 : -1)));
    pick(next);
    list?.querySelectorAll<HTMLElement>(".pair")[next]?.scrollIntoView({ block: "nearest" });
  }

  let copied = $state(false);
  function copyCommand(command: string) {
    copy(command);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  const when = (c: SeriesCommit) => relativeTime(c.time);

  function changedText(p: SeriesPair): string {
    const parts = [p.message && "message", p.code && "code", p.author && "author"].filter(Boolean) as string[];
    const what = parts.length > 1 ? `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}` : (parts[0] ?? "change");
    return `The ${what} ${parts.length > 1 ? "differ" : "differs"} between the two versions; what they share shows once on both sides. The base under them doesn’t count.`;
  }
</script>

{#snippet version_(label: string, commit: SeriesCommit | null)}
  <div class="version" class:none={!commit}>
    <span class="label">{label}</span>
    {#if commit}
      <span class="mono">{shortId(commit.id)}</span>
      <span class="ellipsis">{commit.authorName} · {when(commit)}</span>
    {:else}
      <span class="ellipsis">No commit</span>
    {/if}
  </div>
{/snippet}

{#snippet head(p: SeriesPair)}
  {@const [bg, fg] = tone[p.kind]()}
  <div class="pair-head">
    <div class="versions">
      {@render version_("Before", p.old)}
      <svg class="icon arrow" viewBox="0 0 16 16"><path d="M3 8h9.5M9.5 5l3 3-3 3" /></svg>
      {@render version_("After", p.new)}
      <span class="grow"></span>
      <span class="capsule-label" style:background={bg} style:color={fg}>{pairLabel(p)}</span>
    </div>
    <p class="explain">
      {#if p.kind === "same"}
        The same change, message and author: only the base under it is new, so its id changed.
      {:else if p.kind === "added"}
        Only the new series has this commit: it was added, or changed too much to match one before.
      {:else if p.kind === "dropped"}
        The new series has no commit like this one: it was dropped, squashed into another, or changed too much to match. Undo in the Operation Log brings it back.
      {:else}
        {changedText(p)}
      {/if}
    </p>
    {#if p.kind === "changed"}
      <div class="segmented" role="radiogroup" aria-label="Show">
        <button role="radio" aria-checked={side === "between"} class:on={side === "between"} onclick={() => (side = "between")}>Between Versions</button>
        <button role="radio" aria-checked={side === "old"} class:on={side === "old"} onclick={() => (side = "old")}>Before</button>
        <button role="radio" aria-checked={side === "new"} class:on={side === "new"} onclick={() => (side = "new")}>After</button>
      </div>
    {/if}
  </div>
{/snippet}

<div class="series">
  <div class="bar">
    {#if query.branches.length > 1}
      <div class="segmented" role="radiogroup" aria-label="Branch">
        {#each query.branches as b (b.branch)}
          <button role="radio" aria-checked={b.branch === query.branch} class:on={b.branch === query.branch} onclick={() => onBranch(b.branch)}>{b.branch}</button>
        {/each}
      </div>
    {/if}
    <div class="summary-text">
      {#if series}
        <span class="headline">{headline(series)}</span>
        <span class="sub">{baseLine(series)}</span>
      {:else if loading}
        <span class="sub">Comparing…</span>
      {/if}
    </div>
    {#if series}
      <div class="tally">
        {#each tally(series) as t (t.kind)}
          {@const [bg, fg] = tone[t.kind]()}
          <span class="capsule-label" style:background={bg} style:color={fg}>{t.text}</span>
        {/each}
      </div>
      <button class="term" onclick={() => copyCommand(series!.command)} title="Copy the command" aria-label="Copy {series.command}">
        <span class="prompt">$</span>
        <span class="words"><b style:color="var(--term-git)">git</b> <span style:color="var(--term-sub)">range-diff</span> <span style:color="var(--term-text)">{series.command.replace(/^git range-diff /, "")}</span></span>
        {#if copied}
          <svg class="icon copy" viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg>
        {:else}
          <svg class="icon copy" viewBox="0 0 16 16"><path d="M5.5 5.5h7v8h-7zM3.5 10.5v-8h7" /></svg>
        {/if}
      </button>
    {/if}
  </div>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {:else if series}
    <div class="content">
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
      <section class="list" aria-label="Commits, before and after" tabindex="0" onkeydown={onKey} bind:this={list}>
        <div class="list-head"><span>Newest first</span><span>{series.oldCount} before · {series.newCount} after</span></div>
        {#each series.pairs as p, i (i)}
          {@const [bg, fg] = tone[p.kind]()}
          {@const c = p.new ?? p.old!}
          <button class="pair" class:on={picked === i} class:gone={p.kind === "dropped"} style:background={picked === i ? tint(color) : undefined} onclick={() => pick(i)}>
            <span class="tile" style:background={bg} style:color={fg}><svg class="icon" viewBox="0 0 16 16"><path d={ICONS[p.kind]} /></svg></span>
            <span class="lines">
              <span class="summary">{c.summary}</span>
              <span class="byline">
                {#if p.old && p.new}
                  <span class="mono">{shortId(p.old.id)}</span> → <span class="mono">{shortId(p.new.id)}</span>
                  {#if p.message}<span class="was" title="Before: {p.old.summary}">· was “{p.old.summary}”</span>{/if}
                {:else if p.new}
                  <span class="mono">{shortId(p.new.id)}</span> · {when(p.new)}
                {:else if p.old}
                  <span class="mono">{shortId(p.old.id)}</span> · {when(p.old)}
                {/if}
              </span>
            </span>
            <span class="capsule-label" style:background={bg} style:color={fg}>{pairLabel(p)}</span>
          </button>
        {/each}
        {#if series.baseCommits > 0}
          <div class="base">
            <span class="ring"></span>
            <span class="lines">
              <span class="byline">New base{series.baseName ? ` · ${series.baseName}` : ""}</span>
              <span class="summary">{series.baseCommits} {series.baseCommits === 1 ? "commit" : "commits"} under the series that weren’t there before</span>
            </span>
          </div>
        {/if}
      </section>
      <aside class="panel" aria-label="The pair">
        {#if pair}
          {@const commit = pair.kind === "changed" ? (side === "old" ? pair.old : side === "new" ? pair.new : null) : (pair.new ?? pair.old)}
          {#if commit}
            {#snippet banner()}{@render head(pair!)}{/snippet}
            <div class="holder">
              {#key commit.id}
                <CommitPanel row={seriesRow(commit, color)} childIds={[]} {lookup} onSelect={() => {}} {banner} />
              {/key}
            </div>
          {:else}
            {@render head(pair)}
            <div class="scroll">
              <InterDiff sections={pair.sections} before={shortId(pair.old!.id)} after={shortId(pair.new!.id)} />
            </div>
          {/if}
        {/if}
      </aside>
    </div>
  {/if}
</div>

<style>
  .series {
    flex-grow: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-top: 1px solid var(--sep);
    border-bottom: 1px solid var(--sep);
    min-height: 52px;
  }
  .summary-text {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-left: 4px;
  }
  .headline {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub {
    font-size: var(--text-caption);
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tally {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
  }
  .capsule-label {
    flex-shrink: 0;
    height: 20px;
    padding: 0 8px;
    border-radius: var(--radius-pill);
    display: inline-flex;
    align-items: center;
    font-size: var(--text-caption);
    font-weight: 500;
    white-space: nowrap;
  }
  .segmented {
    display: flex;
    flex-shrink: 0;
    padding: 2px;
    border-radius: var(--radius-pill);
    background: var(--field);
    align-self: flex-start;
  }
  .bar .segmented {
    align-self: center;
  }
  .segmented button {
    height: 26px;
    padding: 0 12px;
    border-radius: var(--radius-pill);
    font-size: var(--text-small);
    color: var(--text2);
    white-space: nowrap;
  }
  .segmented button.on {
    background: var(--win);
    color: var(--text);
    font-weight: 600;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
  }
  .term {
    display: flex;
    align-items: center;
    gap: 7px;
    height: 28px;
    min-width: 0;
    max-width: 340px;
    flex-shrink: 1;
    padding: 0 10px;
    border-radius: var(--radius-m);
    background: var(--term);
    border: 1px solid var(--term-border);
    font-family: var(--mono);
    font-size: var(--text-caption);
  }
  .prompt {
    color: var(--term-dim);
  }
  .words {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .copy {
    width: 13px;
    height: 13px;
    color: var(--term-dim);
    flex-shrink: 0;
  }
  .content {
    flex-grow: 1;
    display: flex;
    min-height: 0;
  }
  .list {
    width: 400px;
    flex-shrink: 0;
    overflow-y: auto;
    background: var(--graph-bg);
    padding: 8px;
    outline: none;
  }
  .list-head {
    display: flex;
    justify-content: space-between;
    padding: 4px 10px 8px;
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .pair,
  .base {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-height: 44px;
    padding: 5px 10px;
    border-radius: var(--radius-m);
    text-align: left;
  }
  .pair:hover:not(.on) {
    background: var(--row-hover);
  }
  .tile {
    width: 24px;
    height: 24px;
    flex-shrink: 0;
    border-radius: var(--radius-pill);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .tile .icon {
    width: 13px;
    height: 13px;
  }
  .lines {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex-grow: 1;
  }
  .summary {
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .on .summary {
    font-weight: 600;
  }
  .gone .summary {
    text-decoration: line-through;
    text-decoration-color: color-mix(in srgb, var(--text2) 60%, transparent);
    color: var(--text2);
  }
  .byline {
    font-size: var(--text-caption);
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .mono {
    font-family: var(--mono);
    font-size: var(--text-caption);
  }
  .base {
    margin-top: 8px;
    border-top: 1px dashed var(--sep);
    border-radius: 0 0 var(--radius-m) var(--radius-m);
    padding-top: 10px;
  }
  .ring {
    width: 10px;
    height: 10px;
    margin: 0 7px;
    border-radius: var(--radius-pill);
    flex-shrink: 0;
    border: 1.5px dashed var(--text2);
  }
  .panel {
    flex-grow: 1;
    min-width: 0;
    border-left: 1px solid var(--sep);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .holder {
    position: relative;
    flex-grow: 1;
    min-height: 0;
  }
  .scroll {
    flex-grow: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .pair-head {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: var(--space-3) var(--space-4) var(--space-2);
    flex-shrink: 0;
  }
  .versions {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .version {
    display: flex;
    align-items: baseline;
    gap: 6px;
    min-width: 0;
    padding: 5px 10px;
    border-radius: var(--radius-m);
    background: var(--field);
    font-size: var(--text-small);
  }
  .version.none {
    color: var(--text2);
  }
  .version .label {
    font-weight: 600;
  }
  .ellipsis {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text2);
  }
  .arrow {
    width: 14px;
    height: 14px;
    color: var(--text2);
    flex-shrink: 0;
  }
  .grow {
    flex-grow: 1;
  }
  .explain {
    margin: 0;
    font-size: var(--text-small);
    line-height: 1.45;
    color: var(--text2);
  }
  .was {
    margin-left: 2px;
  }
  .error {
    margin: 20px;
    color: var(--red);
  }
</style>
