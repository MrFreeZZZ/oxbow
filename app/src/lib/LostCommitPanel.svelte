<script lang="ts">
  // A commit from git's reflog that no branch has, in History's details panel: where it came
  // from, until when git keeps it, and Create Branch to keep it, without a reset.

  import type { HistoryRow, ReflogEntry } from "./types";
  import { relativeTime } from "./format";
  import { lostRow, lostUntil, moveText, oplog } from "./oplog.svelte";
  import CommitPanel from "./CommitPanel.svelte";

  let {
    entry,
    color,
    lookup,
    onSelect,
    onBranch,
    onClose,
  }: {
    entry: ReflogEntry;
    color: number;
    lookup: (id: string) => HistoryRow | undefined;
    /** A parent History has, or another commit of the reflog. */
    onSelect: (id: string) => void;
    onBranch: (entry: ReflogEntry) => void;
    onClose: () => void;
  } = $props();

  const row = $derived(lostRow(entry, color));
  /** The newer lost commit whose history has this one: a branch there keeps both. */
  const tip = $derived(entry.within ? oplog.reflog.find((e) => e.id === entry.within) : undefined);
  const until = $derived(lostUntil(entry));
</script>

{#snippet banner()}
  <div class="card">
    <div class="top">
      <span class="tile">
        <svg class="icon" viewBox="0 0 16 16"><path d="M8 4.5v4M8 11h.01M7.1 1.9 1.4 12a1 1 0 0 0 .9 1.5h11.4a1 1 0 0 0 .9-1.5L8.9 1.9a1 1 0 0 0-1.8 0z" /></svg>
      </span>
      <span class="words">
        <span class="title">
          {#if !entry.lost}From git's reflog{:else if entry.lostCount > 1}No branch has this commit or the {entry.lostCount - 1} before it{:else}No branch has this commit{/if}
        </span>
        <span class="detail">
          <span class="mono">{entry.selector}</span> · {moveText(entry)} · {relativeTime(entry.time)}{#if entry.lost && until}. Git keeps it {until}{/if}
        </span>
      </span>
      <button class="close" onclick={onClose} aria-label="Close" title="Close (Esc)">
        <svg class="icon" viewBox="0 0 16 16"><path d="M4 4l8 8M12 4l-8 8" /></svg>
      </button>
    </div>
    {#if entry.lost}
      <div class="actions">
        {#if tip}
          <span class="note">A newer lost commit, <button class="link" onclick={() => onSelect(tip.id)}>{tip.summary}</button>, has it in its history.</span>
        {:else}
          <span class="note">A branch here keeps {entry.lostCount > 1 ? `all ${entry.lostCount}` : "it"}. Nothing else changes: no reset, your files stay as they are.</span>
        {/if}
        <button class="button" onclick={() => onBranch(entry)}>Create Branch…</button>
      </div>
    {/if}
  </div>
{/snippet}

<CommitPanel {row} childIds={[]} {lookup} {onSelect} {banner} />

<style>
  .card {
    margin: 4px 12px 8px;
    padding: 10px 12px 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    border-radius: 12px;
    background: var(--orange-soft);
    flex-shrink: 0;
  }
  .top {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }
  .tile {
    width: 26px;
    height: 26px;
    flex-shrink: 0;
    border-radius: 13px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--orange);
  }
  .tile .icon {
    width: 15px;
    height: 15px;
  }
  .words {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-top: 3px;
  }
  .title {
    font-weight: 600;
  }
  .detail {
    font-size: 11.5px;
    color: var(--text2);
    line-height: 1.4;
  }
  .mono {
    font-family: var(--mono);
    font-size: 10.5px;
  }
  .close {
    width: 22px;
    height: 22px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 11px;
    color: var(--icon);
  }
  .close:hover {
    background: var(--field);
  }
  .close .icon {
    width: 12px;
    height: 12px;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 12px;
    padding-left: 36px;
  }
  .note {
    flex-grow: 1;
    min-width: 0;
    font-size: 11.5px;
    line-height: 1.4;
    color: var(--text2);
  }
  .link {
    color: var(--accent-text);
    font-size: inherit;
  }
  .button {
    flex-shrink: 0;
    height: 28px;
    padding: 0 12px;
    border-radius: 14px;
    background: var(--accent);
    color: #fff;
    font-weight: 600;
    font-size: 12px;
    white-space: nowrap;
  }
</style>
