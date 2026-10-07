<script lang="ts">
  // The toolbar's Undo button and its popover: every step Oxbow ran here, newest first. The top
  // one has Undo; any other goes back to right after it with Restore to Here.

  import type { Request } from "./confirm.svelte";
  import { cannotRestore, clearRequest, entryIcon, isRedo, oplog, restoreRequest, undoRequest } from "./oplog.svelte";
  import { relativeTime } from "./format";
  import { prefs } from "./prefs.svelte";

  let { repo, run }: { repo: string; run: (request: Request) => void } = $props();

  const mac = navigator.platform.startsWith("Mac");
  const latest = $derived(oplog.entries[0] ?? null);
  /** Steps that throw work away get the warning tint, as the design has for a reset. */
  const warn = new Set(["reset", "discard", "drop"]);

  function toggle() {
    oplog.open = !oplog.open;
  }

  $effect(() => {
    if (oplog.open) oplog.load();
  });

  function go(request: Request) {
    oplog.open = false;
    run(request);
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape" && oplog.open) {
      event.preventDefault();
      oplog.open = false;
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="anchor">
  <button
    class="capsule"
    onclick={toggle}
    aria-haspopup="dialog"
    aria-expanded={oplog.open}
    aria-label={latest ? `Undo ${latest.title}, open operation log` : "Open operation log"}
    title={latest ? `Operation Log. ${mac ? "⌘Z" : "Ctrl+Z"} ${isRedo(latest) ? "redoes what the undo took back" : `undoes “${latest.title}”`}` : "Operation Log"}
  >
    <svg class="icon" viewBox="0 0 16 16"><path d="M5.5 4 2.5 7l3 3M3 7h6.5a3.5 3.5 0 0 1 0 7H7" /></svg>
    <svg class="icon small" viewBox="0 0 16 16"><path d="M4 6l4 4 4-4" /></svg>
  </button>
  {#if oplog.open}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="catcher" onclick={() => (oplog.open = false)}></div>
    <div class="pop" role="dialog" aria-label="Operation Log">
      <div class="head">
        <span class="h">Operation Log</span>
        <span class="repo">{repo}</span>
      </div>
      <p class="intro">Every change to the repository is recorded. Restore any point, including after reset or rebase.</p>
      <div class="list">
        {#each oplog.entries as entry, i (entry.id)}
          {@const blocked = cannotRestore(entry, i > 0)}
          <div class="row" class:top={i === 0}>
            <span class="tile" class:warn={warn.has(entry.kind)}>
              <svg class="icon" viewBox="0 0 16 16"><path d={entryIcon(entry)} /></svg>
            </span>
            <span class="words">
              <span class="title">{entry.title}</span>
              <span class="detail">{entry.failed ? "Stopped half way · " : ""}{entry.detail}{i === 0 ? ` · ${relativeTime(entry.time)}` : ""}</span>
            </span>
            {#if i === 0}
              <button class="primary" disabled={!!blocked} title={blocked ?? undefined} onclick={() => go(undoRequest(entry))}>
                {isRedo(entry) ? "Redo" : "Undo"}<span class="key">{mac ? "⌘Z" : "Ctrl+Z"}</span>
              </button>
            {:else}
              <span class="time">{relativeTime(entry.time)}</span>
              <button class="restore" disabled={!!blocked} title={blocked ?? "Back to right after this step"} onclick={() => go(restoreRequest(entry, i))}>Restore to Here</button>
            {/if}
          </div>
        {:else}
          <div class="empty">Nothing to undo yet. Every change Oxbow makes to this repository shows up here.</div>
        {/each}
      </div>
      <div class="foot">
        <span>Snapshots are kept for {prefs.get("oxbow.undo.keepDays")} days</span>
        {#if oplog.entries.length}
          <button class="link" onclick={() => go(clearRequest(oplog.entries.length))}>Clear Log…</button>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .anchor {
    position: relative;
    display: flex;
  }
  .capsule {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 34px;
    padding: 0 10px 0 12px;
    border-radius: 17px;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    color: var(--icon);
  }
  .small {
    width: 12px;
    height: 12px;
    color: var(--text2);
  }
  .catcher {
    position: fixed;
    inset: 0;
    z-index: 19;
  }
  .pop {
    position: absolute;
    right: 0;
    top: 40px;
    z-index: 20;
    width: 440px;
    max-width: calc(100vw - 32px);
    padding: 14px 8px 8px;
    border-radius: 22px;
    background: var(--sheet);
    -webkit-backdrop-filter: blur(30px) saturate(1.8);
    backdrop-filter: blur(30px) saturate(1.8);
    border: 0.5px solid var(--glass-border);
    box-shadow: 0 16px 44px rgba(0, 0, 0, 0.22);
    display: flex;
    flex-direction: column;
  }
  .head {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 0 10px;
  }
  .h {
    font-size: 14px;
    font-weight: 700;
  }
  .repo {
    font-size: 12px;
    color: var(--text2);
  }
  .intro {
    margin: 4px 10px 8px;
    font-size: 11.5px;
    line-height: 1.4;
    color: var(--text2);
  }
  .list {
    max-height: 400px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 46px;
    padding: 6px 10px;
    border-radius: 12px;
  }
  .row:hover,
  .row.top {
    background: var(--field);
  }
  .tile {
    width: 30px;
    height: 30px;
    flex-shrink: 0;
    border-radius: 15px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--side-sel);
    color: var(--icon);
  }
  .tile.warn {
    background: var(--orange-soft);
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
    gap: 1px;
  }
  .words span {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .title {
    font-weight: 600;
  }
  .detail {
    font-size: 11.5px;
    color: var(--text2);
  }
  .time {
    flex-shrink: 0;
    font-size: 11.5px;
    color: var(--text2);
  }
  .primary,
  .restore {
    flex-shrink: 0;
    height: 26px;
    padding: 0 12px;
    border-radius: 13px;
    font-weight: 500;
    font-size: 12px;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .primary {
    background: var(--accent);
    color: #fff;
  }
  .key {
    opacity: 0.75;
    font-size: 11px;
  }
  .restore {
    display: none;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    color: var(--text);
  }
  .row:hover .restore {
    display: flex;
  }
  .row:hover .time {
    display: none;
  }
  button:disabled {
    opacity: 0.45;
  }
  .empty {
    padding: 14px 10px;
    font-size: 12px;
    line-height: 1.45;
    color: var(--text2);
  }
  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: 6px;
    padding: 8px 10px 4px;
    border-top: 1px solid var(--sep);
    font-size: 11.5px;
    color: var(--text2);
  }
  .link {
    color: var(--accent-text);
    font-size: 11.5px;
  }
</style>
