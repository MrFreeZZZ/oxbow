<script lang="ts">
  // Two commits picked in History (click one, ⌘-click another): what changed from the older one to
  // the newer one, as one diff. Swap reads it the other way; Open in Compare goes further.

  import { api } from "./api";
  import type { Comparison, HistoryRow } from "./types";
  import { lane, plate, relativeTime, shortId, tint } from "./format";
  import CompareChanges from "./CompareChanges.svelte";

  let {
    older,
    newer,
    version,
    copy,
    onSelect,
    onClear,
    onOpenCompare,
  }: {
    /** The pair in graph order; From starts as the older one. */
    older: HistoryRow;
    newer: HistoryRow;
    /** Goes up on every reload of the history, so the comparison reloads too. */
    version: number;
    copy: (text: string) => void;
    /** Show one commit alone, leaving the pair. */
    onSelect: (id: string) => void;
    onClear: () => void;
    onOpenCompare: (from: string, to: string) => void;
  } = $props();

  let swapped = $state(false);
  let comparison = $state<Comparison | null>(null);
  let error = $state<string | null>(null);
  let copied = $state(false);

  const from = $derived(swapped ? newer : older);
  const to = $derived(swapped ? older : newer);
  const mac = navigator.platform.startsWith("Mac");

  // Another pair reads from its older commit again.
  $effect(() => {
    older.id;
    newer.id;
    swapped = false;
  });

  $effect(() => {
    const a = from.id;
    const b = to.id;
    version;
    error = null;
    api.compare(a, b, "tips").then(
      (c) => {
        if (from.id === a && to.id === b) comparison = c;
      },
      (err) => {
        if (from.id !== a || to.id !== b) return;
        comparison = null;
        error = String(err);
      },
    );
  });

  // The answer for this pair, not the one before it.
  const c = $derived(comparison && comparison.baseId === from.id && comparison.targetId === to.id ? comparison : null);
  const command = $derived(`git diff ${shortId(from.id)} ${shortId(to.id)}`);
  const n = (count: number) => `${count.toLocaleString()} ${count === 1 ? "commit" : "commits"}`;

  /** How the two commits relate, and what the diff therefore shows. */
  const relation = $derived.by(() => {
    if (!c) return null;
    const a = shortId(from.id);
    const b = shortId(to.id);
    if (!c.mergeBase) return { line: `${a} and ${b} share no history`, sub: "The diff shows every difference between the two commits." };
    if (!c.behindCount) return { line: `${b} comes ${n(c.aheadCount)} after ${a}`, sub: "The diff is what those commits changed." };
    if (!c.aheadCount) return { line: `${b} is ${n(c.behindCount)} before ${a}`, sub: "The diff shows those commits undone. Swap to read it forward." };
    return { line: `Not on one line: they split at ${shortId(c.mergeBase.id)}`, sub: "The diff shows every difference between the two commits." };
  });

  function copyCommand(command: string) {
    copy(command);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }
</script>

{#snippet card(row: HistoryRow, label: string)}
  <button
    class="card"
    style:background={tint(row.graph.color)}
    style:border-color="color-mix(in srgb, {lane(row.graph.color)} 30%, transparent)"
    onclick={() => onSelect(row.id)}
    title="Show only this commit"
  >
    <span class="label">{label} <span class="mono">{shortId(row.id)}</span></span>
    <span class="summary">{row.summary}</span>
    <span class="byline">
      {#if row.graph.branch}
        <span class="pill" style:color={plate(row.graph.color)} style:background={tint(row.graph.color, "label")}>{row.graph.branch}</span>
      {/if}
      <span class="who">{row.authorName} · {relativeTime(row.time)}</span>
    </span>
  </button>
{/snippet}

{#snippet header()}
  <div class="head">
    <div class="top">
      <span class="count">2 commits selected</span>
      <span class="hint">· {mac ? "⌘" : "Ctrl"}-click another commit to change the second one</span>
      <span class="grow"></span>
      <button class="close" onclick={onClear} aria-label="Clear the second commit" title="Clear (Esc)">
        <svg class="icon" viewBox="0 0 16 16"><path d="M4 4l8 8M12 4l-8 8" /></svg>
      </button>
    </div>
    <div class="cards">
      {@render card(from, "From")}
      <svg class="icon arrow" viewBox="0 0 16 16" aria-hidden="true"><path d="M3 8h10M9 4l4 4-4 4" /></svg>
      {@render card(to, "To")}
    </div>
    {#if relation}
      <h1>{relation.line}</h1>
      <p class="sub">{relation.sub}</p>
    {/if}
    {#if c}
      <div class="actions">
        <button class="term" onclick={() => copyCommand(command)} title="Copy the command" aria-label="Copy {command}">
          <span class="prompt">$</span>
          <span class="words"><b style:color="var(--term-git)">git</b> <span style:color="var(--term-sub)">diff</span> <span style:color="var(--term-text)">{shortId(from.id)} {shortId(to.id)}</span></span>
          {#if copied}
            <svg class="icon copy" viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg>
          {:else}
            <svg class="icon copy" viewBox="0 0 16 16"><path d="M5.5 5.5h7v8h-7zM3.5 10.5v-8h7" /></svg>
          {/if}
        </button>
        <span class="grow"></span>
        <button class="button" onclick={() => (swapped = !swapped)} title="Read the diff the other way">
          <svg class="icon" viewBox="0 0 16 16"><path d="M2.5 5.5h11M10.5 2.5l3 3-3 3M13.5 10.5h-11M5.5 7.5l-3 3 3 3" /></svg>
          Swap
        </button>
        <button class="button primary" onclick={() => onOpenCompare(from.id, to.id)} title="See the commits in between, and more">Open in Compare</button>
      </div>
    {/if}
  </div>
{/snippet}

{#if error}
  <p class="error" role="alert">{error}</p>
{:else if c}
  <div class="changes">
    <CompareChanges comparison={c} base={shortId(from.id)} target={shortId(to.id)} color={to.graph.color} onPickCommit={onSelect} {header} />
  </div>
{:else}
  {@render header()}
{/if}

<style>
  .changes {
    position: relative;
    flex-grow: 1;
    min-height: 0;
  }
  .head {
    margin: 4px 12px 0;
    padding: 12px 14px 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    border: 1px solid var(--sep);
    border-radius: 12px;
    flex-shrink: 0;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
    color: var(--text2);
  }
  .count {
    font-weight: 600;
  }
  .hint {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .grow {
    flex-grow: 1;
  }
  .close {
    width: 22px;
    height: 22px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 11px;
    color: var(--icon);
    flex-shrink: 0;
  }
  .close:hover {
    background: var(--field);
  }
  .close .icon {
    width: 12px;
    height: 12px;
  }
  .cards {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .arrow {
    width: 14px;
    height: 14px;
    color: var(--text2);
    flex-shrink: 0;
  }
  .card {
    flex: 1 1 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 3px;
    padding: 9px 12px;
    border-radius: 12px;
    border: 1px solid transparent;
    text-align: left;
  }
  .label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
  }
  .mono {
    font-family: var(--mono);
    font-weight: 400;
  }
  .summary {
    max-width: 100%;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .byline {
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 100%;
    font-size: 11px;
  }
  .pill {
    height: 16px;
    padding: 0 6px;
    border-radius: 8px;
    font-weight: 500;
    line-height: 16px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex-shrink: 0;
    max-width: 60%;
  }
  .who {
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  h1 {
    margin: 4px 0 0;
    font-size: 15px;
    font-weight: 600;
    line-height: 1.3;
  }
  .sub {
    margin: 0;
    color: var(--text2);
    font-size: 12px;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 4px;
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
  .button {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 12px;
    border-radius: 14px;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    font-size: 12px;
    font-weight: 500;
    color: var(--text);
    white-space: nowrap;
    flex-shrink: 0;
  }
  .button .icon {
    width: 13px;
    height: 13px;
  }
  .button.primary {
    background: var(--accent);
    border-color: transparent;
    color: #fff;
    font-weight: 600;
  }
  .error {
    margin: 20px;
    color: var(--red);
  }
</style>
