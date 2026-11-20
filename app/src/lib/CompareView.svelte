<script lang="ts">
  // Compare: two branches, tags or commits side by side. The list shows the commits only one side
  // has and where they split; the panel shows every change at once, or one commit.

  import { compareLabel, isCompareSha, quoteGitArg } from "./compareRefs";
  import { api } from "./api";
  import type { CompareCommit, CompareMode, Comparison, History, HistoryRow } from "./types";
  import { lane, relativeTime, shortId, tint } from "./format";
  import RefPicker from "./RefPicker.svelte";
  import CompareChanges from "./CompareChanges.svelte";
  import { ancestors } from "./commits";
  import CommitPanel from "./CommitPanel.svelte";

  let {
    history,
    base,
    target,
    mode,
    version,
    colorOf,
    lookup,
    childrenOf,
    onChange,
    copy,
    panelWidth,
    onGrip,
  }: {
    history: History;
    base: string;
    target: string;
    mode: CompareMode;
    /** Goes up on every reload of the history, so the comparison reloads too. */
    version: number;
    colorOf: (name: string) => number;
    lookup: (id: string) => HistoryRow | undefined;
    childrenOf: (id: string) => string[];
    onChange: (next: { base: string; target: string; mode: CompareMode }) => void;
    copy: (text: string) => void;
    panelWidth: number;
    onGrip: (event: PointerEvent) => void;
  } = $props();

  let loaded = $state<{
    query: { base: string; target: string; mode: CompareMode };
    comparison: Comparison;
    base: string;
    target: string;
  } | null>(null);
  const result = $derived(loaded?.query.base === base && loaded.query.target === target && loaded.query.mode === mode ? loaded : null);
  const comparison = $derived(result?.comparison);
  const baseLabel = $derived(compareLabel(result?.base ?? base));
  const targetLabel = $derived(compareLabel(result?.target ?? target));
  let error = $state<string | null>(null);
  /** A commit picked in the list; none shows all the changes. */
  let picked = $state<string | null>(null);
  let list = $state<HTMLDivElement>();
  let previousRefs: { base: string; target: string } | undefined;

  $effect(() => {
    const current = list?.querySelector<HTMLButtonElement>("[aria-current='true']");
    const active = document.activeElement;
    if (current && (!active || active === document.body)) {
      current.focus({ preventScroll: true });
      current.scrollIntoView({ block: "nearest" });
    }
  });

  $effect(() => {
    const query = { base, target, mode };
    if (previousRefs?.base !== query.base || previousRefs.target !== query.target) picked = null;
    previousRefs = query;
    version;
    error = null;
    // Only the latest request counts: one for the same refs from before a reload may answer
    // after it, with what the branches were then.
    let latest = true;
    api.compare(query.base, query.target, query.mode).then(
      (c) => {
        if (!latest) return;
        loaded = {
          query,
          comparison: c,
          base: isCompareSha(query.base) ? c.baseId : query.base,
          target: isCompareSha(query.target) ? c.targetId : query.target,
        };
        // A commit that is no longer on either side goes back to all the changes.
        if (picked && ![...c.ahead, ...c.behind].some((x) => x.id === picked) && c.mergeBase?.id !== picked) picked = null;
      },
      (err) => {
        if (!latest) return;
        loaded = null;
        error = String(err);
      },
    );
    return () => {
      latest = false;
    };
  });

  const color = $derived(colorOf(targetLabel));
  const pickedRow = $derived(picked ? lookup(picked) : undefined);

  function headline(c: Comparison): string {
    if (!c.mergeBase) return `${targetLabel} and ${baseLabel} share no history`;
    if (!c.aheadCount && !c.behindCount) return `${targetLabel} and ${baseLabel} are the same commit`;
    if (!c.behindCount) return `${targetLabel} is ${n(c.aheadCount)} ahead of ${baseLabel}`;
    if (!c.aheadCount) return `${targetLabel} is ${n(c.behindCount)} behind ${baseLabel}`;
    return `${targetLabel} is ${n(c.aheadCount)} ahead and ${n(c.behindCount)} behind ${baseLabel}`;
  }

  const n = (count: number) => `${count.toLocaleString()} ${count === 1 ? "commit" : "commits"}`;

  function subline(c: Comparison): string {
    if (!c.mergeBase) return "Their histories never met, so the tips are compared directly.";
    return `Common ancestor · ${shortId(c.mergeBase.id)} ${c.mergeBase.summary} · ${relativeTime(c.mergeBase.time)}`;
  }

  function dotColor(id: string): string {
    const row = lookup(id);
    return row ? lane(row.graph.color) : "var(--text2)";
  }

  function pickRow(row: HTMLButtonElement, id: string | null) {
    picked = id;
    row.focus({ preventScroll: true });
  }

  function onListKey(event: KeyboardEvent & { currentTarget: HTMLDivElement }) {
    if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const rows = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>("button[data-compare-row]"));
    const focused = rows.findIndex((row) => row === event.target);
    if (focused < 0) return;
    const current = rows.findIndex((row) => row.dataset.compareRow === (picked ?? ""));

    event.preventDefault();
    const index = (current < 0 ? focused : current) + (event.key === "ArrowDown" ? 1 : -1);
    const next = rows[Math.max(0, Math.min(rows.length - 1, index))];
    pickRow(next, next.dataset.compareRow || null);
    next.scrollIntoView({ block: "nearest" });
  }

  let copied = $state(false);
  function copyCommand(command: string) {
    copy(command);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }
</script>

{#snippet commitRow(commit: CompareCommit)}
  {@const rowColor = lookup(commit.id)?.graph.color ?? color}
  <button class="commit" class:on={picked === commit.id} data-compare-row={commit.id} aria-current={picked === commit.id ? "true" : undefined} style:--pick-lane={lane(rowColor)} style:background={picked === commit.id ? tint(rowColor) : undefined} onclick={(event) => pickRow(event.currentTarget, commit.id)}>
    <span class="dot" style:background={dotColor(commit.id)}></span>
    <span class="lines">
      <span class="summary">{commit.summary}</span>
      <span class="byline"><span class="mono">{shortId(commit.id)}</span> {commit.authorName} · {relativeTime(commit.time)}</span>
    </span>
  </button>
{/snippet}

<div class="compare">
  <div class="bar">
    <RefPicker label="Base" value={base} other={target} {history} {colorOf} onPick={(name) => onChange({ base: name, target, mode })} />
    <button class="swap" onclick={() => onChange({ base: target, target: base, mode })} aria-label="Swap Base and Compare" title="Swap">
      <svg class="icon" viewBox="0 0 16 16"><path d="M2.5 5.5h9M9 3l2.5 2.5L9 8M13.5 10.5h-9M7 8l-2.5 2.5L7 13" /></svg>
    </button>
    <RefPicker label="Compare" value={target} other={base} {history} {colorOf} onPick={(name) => onChange({ base, target: name, mode })} />
    <div class="segmented" role="radiogroup" aria-label="File comparison mode">
      <button role="radio" aria-checked={(comparison?.mode ?? mode) === "split"} class:on={(comparison?.mode ?? mode) === "split"} disabled={!!comparison && !comparison.mergeBase} onclick={() => onChange({ base, target, mode: "split" })} title="Compare the common ancestor with the Compare side (git diff A...B)">From Common Ancestor <span class="dots">...</span></button>
      <button role="radio" aria-checked={(comparison?.mode ?? mode) === "tips"} class:on={(comparison?.mode ?? mode) === "tips"} onclick={() => onChange({ base, target, mode: "tips" })} title="Compare the Base and Compare snapshots directly (git diff A..B)">Tip to Tip <span class="dots">..</span></button>
    </div>
    <div class="summary-text" aria-live="polite">
      {#if comparison}
        <span class="headline">{headline(comparison)}</span>
        <span class="sub">{subline(comparison)}</span>
      {:else if !error}
        <span class="sub">Comparing…</span>
      {/if}
    </div>
  </div>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {:else if result}
    {@const { comparison: c, base: shownBase, target: shownTarget } = result}
    {@const aheadCommand = `git log ${quoteGitArg(`${shownBase}..${shownTarget}`)} --`}
    {@const behindCommand = `git log ${quoteGitArg(`${shownTarget}..${shownBase}`)} --`}
    <div class="content">
      <section class="list" aria-label="Commits">
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions (Delegate arrow keys from the row buttons.) -->
        <div class="scroll" role="group" bind:this={list} onkeydown={onListKey}>
          <button class="all" class:on={picked === null} data-compare-row="" aria-current={picked === null ? "true" : undefined} style:--pick-lane={lane(color)} style:background={picked === null ? tint(color) : undefined} onclick={(event) => pickRow(event.currentTarget, null)}>
            <svg class="icon" viewBox="0 0 16 16"><path d="M4 2.5h5.5L12 5v8.5H4z" /><path d="M6 8h4M6 10.5h4" /></svg>
            <span class="lines">
              <span class="summary">File changes</span>
              <span class="byline">{c.files.length} {c.files.length === 1 ? "file" : "files"}</span>
            </span>
          </button>

          <div class="section">
            <span><b>Only on {targetLabel}</b> {c.aheadCount}</span>
            <button class="hint mono" onclick={() => copyCommand(aheadCommand)} title="Copy">{aheadCommand}</button>
          </div>
          {#each c.ahead as commit (commit.id)}{@render commitRow(commit)}{/each}
          {#if c.aheadCount > c.ahead.length}<p class="more">and {(c.aheadCount - c.ahead.length).toLocaleString()} more</p>{/if}

          <div class="section">
            <span><b>Only on {baseLabel}</b> {c.behindCount}</span>
            <button class="hint mono" onclick={() => copyCommand(behindCommand)} title="Copy">{behindCommand}</button>
          </div>
          {#each c.behind as commit (commit.id)}{@render commitRow(commit)}{/each}
          {#if c.behindCount > c.behind.length}<p class="more">and {(c.behindCount - c.behind.length).toLocaleString()} more</p>{/if}

          {#if c.mergeBase}
            {@const m = c.mergeBase}
            <button class="ancestor" class:on={picked === m.id} data-compare-row={m.id} aria-current={picked === m.id ? "true" : undefined} style:--pick-lane={lane(lookup(m.id)?.graph.color ?? color)} onclick={(event) => pickRow(event.currentTarget, m.id)}>
              <span class="ring"></span>
              <span class="lines">
                <span class="byline">Common ancestor · git merge-base</span>
                <span class="summary"><span class="mono">{shortId(m.id)}</span> {m.summary}</span>
              </span>
            </button>
          {/if}
        </div>
      </section>
      <aside class="panel" style:width="{panelWidth}px" aria-label="Changes">
        <button class="vertical-splitter" title="Drag to resize" tabindex="-1" onpointerdown={onGrip} aria-label="Resize the changes"></button>
        {#if picked && pickedRow}
          <CommitPanel row={pickedRow} childIds={childrenOf(pickedRow.id)} {lookup} onSelect={(id) => (picked = id)} trunk={history.trunk} localTags={history.localTags} {version} inHead={(id) => ancestors(history, history.head.commit).has(id)} />
        {:else if picked}
          <p class="missing">This commit is older than the loaded history, so its details aren’t shown here.</p>
        {:else}
          <div class="command">
            <button class="term" onclick={() => copyCommand(c.command)} title="Copy the command" aria-label="Copy {c.command}">
              <span class="prompt">$</span>
              <span class="words"><b style:color="var(--term-git)">git</b> <span style:color="var(--term-sub)">diff</span> <span style:color="var(--term-text)">{c.command.replace(/^git diff /, "")}</span></span>
              {#if copied}
                <svg class="icon copy" viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg>
              {:else}
                <svg class="icon copy" viewBox="0 0 16 16"><path d="M5.5 5.5h7v8h-7zM3.5 10.5v-8h7" /></svg>
              {/if}
            </button>
          </div>
          <div class="changes">
            <CompareChanges comparison={c} base={baseLabel} target={targetLabel} {color} onPickCommit={(id) => (picked = id)} />
          </div>
        {/if}
      </aside>
    </div>
  {/if}
</div>

<style>
  .compare {
    flex-grow: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  /* The two refs and the mode: a strip between hairlines, no box of its own. */
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-top: 1px solid var(--sep);
    border-bottom: 1px solid var(--sep);
  }
  .swap {
    width: 32px;
    height: 32px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-pill);
    background: var(--win);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    color: var(--icon);
  }
  .summary-text {
    flex-basis: 100%;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
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
  .segmented {
    display: flex;
    flex-shrink: 0;
    padding: 2px;
    border-radius: var(--radius-pill);
    background: var(--field);
  }
  .segmented button {
    height: 28px;
    padding: 0 12px;
    border-radius: var(--radius-pill);
    font-size: var(--text-small);
    color: var(--text2);
    white-space: nowrap;
  }
  .segmented button:disabled {
    opacity: 0.4;
  }
  .segmented button.on {
    background: var(--win);
    color: var(--text);
    font-weight: 600;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
  }
  .dots {
    font-family: var(--mono);
    font-weight: 700;
    margin-left: 2px;
  }
  .content {
    flex-grow: 1;
    display: flex;
    min-height: 0;
  }
  .list {
    flex-grow: 1;
    min-width: 0;
    overflow: hidden;
    background: var(--surface-graph);
    display: flex;
    flex-direction: column;
  }
  .scroll {
    flex-grow: 1;
    overflow-y: auto;
    padding: 8px;
  }
  .all,
  .commit,
  .ancestor {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 6px 10px;
    border-radius: var(--radius-m);
    text-align: left;
  }
  .all {
    padding: 9px 10px;
    color: var(--icon);
  }
  .commit:hover:not(.on),
  .ancestor:hover:not(.on) {
    background: var(--row-hover);
  }
  .ancestor.on {
    background: var(--side-sel);
  }
  :is(.all, .commit, .ancestor).on {
    box-shadow: inset 0 0 0 1.5px color-mix(in srgb, var(--pick-lane) 40%, transparent);
    outline: none;
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
  .dot {
    width: 10px;
    height: 10px;
    border-radius: var(--radius-pill);
    flex-shrink: 0;
    margin-left: 2px;
  }
  .ring {
    width: 10px;
    height: 10px;
    border-radius: var(--radius-pill);
    flex-shrink: 0;
    margin-left: 2px;
    border: 1.5px dashed var(--text2);
  }
  .section {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 14px 10px 4px;
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .section > span {
    flex-grow: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .section b {
    color: var(--text);
    font-weight: 600;
  }
  .hint {
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 50%;
  }
  .hint:hover {
    color: var(--text);
  }
  .more {
    margin: 2px 10px 4px 34px;
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .ancestor {
    margin-top: 12px;
    border-top: 1px dashed var(--sep);
    border-radius: 0 0 var(--radius-m) var(--radius-m);
    padding-top: 10px;
  }
  .panel {
    position: relative;
    flex-shrink: 0;
    border-left: 1px solid var(--sep);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .command {
    display: flex;
    padding: var(--space-2) var(--space-3);
    flex-shrink: 0;
  }
  .term {
    display: flex;
    align-items: center;
    gap: 7px;
    height: 28px;
    min-width: 0;
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
  .changes {
    position: relative;
    flex-grow: 1;
    min-height: 0;
  }
  .missing,
  .error {
    margin: 20px;
    color: var(--text2);
  }
  .error {
    color: var(--red);
  }
</style>
