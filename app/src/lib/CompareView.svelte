<script lang="ts">
  // Compare: two branches, tags or commits side by side. The list shows the commits only one side
  // has and where they split; the panel shows every change at once, or one commit.

  import { api } from "./api";
  import type { CompareCommit, CompareMode, Comparison, History, HistoryRow } from "./types";
  import { lane, relativeTime, shortId, tint } from "./format";
  import RefPicker from "./RefPicker.svelte";
  import CompareChanges from "./CompareChanges.svelte";
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

  let comparison = $state<Comparison | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  /** A commit picked in the list; none shows all the changes. */
  let picked = $state<string | null>(null);

  $effect(() => {
    const query = { base, target, mode };
    version;
    loading = true;
    error = null;
    api.compare(query.base, query.target, query.mode).then(
      (c) => {
        if (base !== query.base || target !== query.target || mode !== query.mode) return;
        comparison = c;
        loading = false;
        // A commit that is no longer on either side goes back to all the changes.
        if (picked && ![...c.ahead, ...c.behind].some((x) => x.id === picked) && c.mergeBase?.id !== picked) picked = null;
      },
      (err) => {
        if (base !== query.base || target !== query.target || mode !== query.mode) return;
        comparison = null;
        loading = false;
        error = String(err);
      },
    );
  });

  // Other refs start over on all the changes.
  $effect(() => {
    base;
    target;
    picked = null;
  });

  const color = $derived(colorOf(target));
  const pickedRow = $derived(picked ? lookup(picked) : undefined);

  function headline(c: Comparison): string {
    if (!c.mergeBase) return `${target} and ${base} share no history`;
    if (!c.aheadCount && !c.behindCount) return `${target} and ${base} are the same commit`;
    if (!c.behindCount) return `${target} is ${n(c.aheadCount)} ahead of ${base}`;
    if (!c.aheadCount) return `${target} is ${n(c.behindCount)} behind ${base}`;
    return `${target} is ${n(c.aheadCount)} ahead and ${n(c.behindCount)} behind ${base}`;
  }

  const n = (count: number) => `${count.toLocaleString()} ${count === 1 ? "commit" : "commits"}`;

  function subline(c: Comparison): string {
    if (!c.mergeBase) return "Their histories never met, so the tips are compared directly.";
    const split = `They split at ${shortId(c.mergeBase.id)} ${c.mergeBase.summary} · ${relativeTime(c.mergeBase.time)}`;
    if (c.aheadCount && !c.behindCount) return `${base} can fast-forward to ${target}. ${split}`;
    return split;
  }

  function dotColor(id: string): string {
    const row = lookup(id);
    return row ? lane(row.graph.color) : "var(--text2)";
  }

  let copied = $state(false);
  function copyCommand(command: string) {
    copy(command);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }
</script>

{#snippet commitRow(commit: CompareCommit)}
  <button class="commit" class:on={picked === commit.id} style:background={picked === commit.id ? tint(lookup(commit.id)?.graph.color ?? color) : undefined} onclick={() => (picked = commit.id)}>
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
    <div class="summary-text">
      {#if comparison}
        <span class="headline">{headline(comparison)}</span>
        <span class="sub">{subline(comparison)}</span>
      {:else if loading}
        <span class="sub">Comparing…</span>
      {/if}
    </div>
    <div class="segmented" role="radiogroup" aria-label="Compare from">
      <button role="radio" aria-checked={mode === "split"} class:on={mode === "split"} disabled={!!comparison && !comparison.mergeBase} onclick={() => onChange({ base, target, mode: "split" })} title="What {target} changed after it split from {base} (git diff A...B)">Since Split <span class="dots">...</span></button>
      <button role="radio" aria-checked={mode === "tips"} class:on={mode === "tips"} onclick={() => onChange({ base, target, mode: "tips" })} title="The two latest states, directly (git diff A..B)">Tip to Tip <span class="dots">..</span></button>
    </div>
  </div>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {:else if comparison}
    {@const c = comparison}
    <div class="content">
      <section class="list" aria-label="Commits">
        <div class="scroll">
          <button class="all" class:on={picked === null} style:background={picked === null ? tint(color) : undefined} onclick={() => (picked = null)}>
            <svg class="icon" viewBox="0 0 16 16"><path d="M4 2.5h5.5L12 5v8.5H4z" /><path d="M6 8h4M6 10.5h4" /></svg>
            <span class="lines">
              <span class="summary">{c.mode === "split" ? `All changes on ${target}` : "Every difference, tip to tip"}</span>
              <span class="byline">{c.files.length} {c.files.length === 1 ? "file" : "files"} · {n(c.mode === "split" ? c.aheadCount : c.aheadCount + c.behindCount)}</span>
            </span>
          </button>

          <div class="section">
            <span><b>Only on {target}</b> {c.aheadCount}</span>
            <button class="hint mono" onclick={() => copyCommand(`git log ${base}..${target}`)} title="Copy">git log {base}..{target}</button>
          </div>
          {#each c.ahead as commit (commit.id)}{@render commitRow(commit)}{/each}
          {#if c.aheadCount > c.ahead.length}<p class="more">and {(c.aheadCount - c.ahead.length).toLocaleString()} more</p>{/if}
          {#if !c.aheadCount}<p class="more">Nothing: every commit of {target} is on {base} too.</p>{/if}

          <div class="section">
            <span><b>Only on {base}</b> {c.behindCount} <span class="aside">· {c.mode === "split" ? "not in the diff" : "in the diff, reversed"}</span></span>
            <button class="hint mono" onclick={() => copyCommand(`git log ${target}..${base}`)} title="Copy">git log {target}..{base}</button>
          </div>
          {#each c.behind as commit (commit.id)}{@render commitRow(commit)}{/each}
          {#if c.behindCount > c.behind.length}<p class="more">and {(c.behindCount - c.behind.length).toLocaleString()} more</p>{/if}
          {#if !c.behindCount}<p class="more">Nothing new on {base} since they split.</p>{/if}

          {#if c.mergeBase}
            {@const m = c.mergeBase}
            <button class="ancestor" class:on={picked === m.id} onclick={() => (picked = m.id)}>
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
        <button class="grip" onpointerdown={onGrip} aria-label="Resize the changes" title="Drag to resize"></button>
        {#if picked && pickedRow}
          <CommitPanel row={pickedRow} childIds={childrenOf(pickedRow.id)} {lookup} onSelect={(id) => (picked = id)} localTags={history.localTags} />
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
            <CompareChanges comparison={c} {base} {target} {color} onPickCommit={(id) => (picked = id)} />
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
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 4px 12px 8px;
    padding: 8px;
    border-radius: 16px;
    background: var(--field);
  }
  .swap {
    width: 32px;
    height: 32px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 16px;
    background: var(--win);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    color: var(--icon);
  }
  .summary-text {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-left: 8px;
  }
  .headline {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub {
    font-size: 11px;
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .segmented {
    display: flex;
    flex-shrink: 0;
    padding: 2px;
    border-radius: 15px;
    background: var(--field);
  }
  .segmented button {
    height: 28px;
    padding: 0 12px;
    border-radius: 14px;
    font-size: 12px;
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
    margin: 0 12px 12px;
    border: 1px solid var(--sep);
    border-radius: 12px;
    overflow: hidden;
    background: var(--graph-bg);
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
    border-radius: 9px;
    text-align: left;
  }
  .all {
    padding: 9px 10px;
    color: var(--icon);
  }
  .commit:hover:not(.on),
  .ancestor:hover:not(.on) {
    background: var(--field);
  }
  .ancestor.on {
    background: var(--side-sel);
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
    font-size: 11px;
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .mono {
    font-family: var(--mono);
    font-size: 11px;
  }
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 5px;
    flex-shrink: 0;
    margin-left: 2px;
  }
  .ring {
    width: 10px;
    height: 10px;
    border-radius: 5px;
    flex-shrink: 0;
    margin-left: 2px;
    border: 1.5px dashed var(--text2);
  }
  .section {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 14px 10px 4px;
    font-size: 11px;
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
    font-size: 11px;
    color: var(--text2);
  }
  .ancestor {
    margin-top: 12px;
    border-top: 1px dashed var(--sep);
    border-radius: 0 0 9px 9px;
    padding-top: 10px;
  }
  .panel {
    position: relative;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .grip {
    position: absolute;
    left: -4px;
    top: 50%;
    width: 7px;
    height: 36px;
    margin-top: -18px;
    border-radius: 4px;
    background: var(--win);
    border: 1px solid var(--glass-border);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.08);
    z-index: 2;
    cursor: col-resize;
  }
  .command {
    display: flex;
    padding: 0 12px 8px;
    flex-shrink: 0;
  }
  .term {
    display: flex;
    align-items: center;
    gap: 7px;
    height: 28px;
    min-width: 0;
    padding: 0 10px;
    border-radius: 8px;
    background: var(--term);
    border: 1px solid var(--term-border);
    font-family: var(--mono);
    font-size: 11px;
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
