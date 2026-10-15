<script lang="ts">
  // The toolbar's Undo button and its popover: every step Oxbow ran here, newest first. The top
  // one has Undo; any other goes back to right after it with Restore to Here. Its second tab
  // reads git's reflog, which also sees what a terminal or another app did, and finds commits
  // no branch has any more.

  import type { Request } from "./confirm.svelte";
  import { cannotRestore, clearRequest, entryIcon, isRedo, keptUntil, lostUntil, moveIcon, moveText, oplog, reflogRows, restoreRequest, undoRequest } from "./oplog.svelte";
  import { relativeTime } from "./format";
  import { prefs } from "./prefs.svelte";
  import { keys, withKeys } from "./keys";
  import WhyNote from "./WhyNote.svelte";
  import type { OpEntry, ReflogEntry } from "./types";

  let {
    repo,
    run,
    onCompare,
    onShow,
    onBranch,
  }: {
    repo: string;
    run: (request: Request) => void;
    /** Show the repository now beside how it was at a step, in History's details panel. */
    onCompare: (entry: OpEntry, after: boolean) => void;
    /** Show a commit from the reflog: selected in History, or in its details panel when History
     *  doesn't have it. */
    onShow: (entry: ReflogEntry) => void;
    /** Keep a commit from the reflog on a new branch. */
    onBranch: (entry: ReflogEntry) => void;
  } = $props();

  const latest = $derived(oplog.entries[0] ?? null);
  /** Steps that throw work away get the warning tint, as the design has for a reset. */
  const warn = new Set(["reset", "discard", "drop"]);

  function toggle() {
    oplog.open = !oplog.open;
  }

  $effect(() => {
    if (oplog.open) oplog.load();
  });

  const moves = $derived(reflogRows(oplog.reflog, oplog.allMoves));

  function setTab(tab: "steps" | "reflog") {
    oplog.tab = tab;
    if (tab === "reflog") oplog.loadReflog();
  }

  function show(entry: ReflogEntry) {
    oplog.open = false;
    onShow(entry);
  }

  function branch(entry: ReflogEntry) {
    oplog.open = false;
    onBranch(entry);
  }

  function compare(entry: OpEntry, after: boolean) {
    oplog.open = false;
    onCompare(entry, after);
  }

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
    title={latest ? withKeys(isRedo(latest) ? "Redo, from the Operation Log" : `Undo “${latest.title}”`, "Mod+Z") : "Operation Log"}
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
        <span class="grow"></span>
        <div class="segmented" role="tablist" aria-label="Source">
          <button role="tab" aria-selected={oplog.tab === "steps"} class:on={oplog.tab === "steps"} onclick={() => setTab("steps")} title="Every step Oxbow ran here, with Undo">Oxbow</button>
          <button role="tab" aria-selected={oplog.tab === "reflog"} class:on={oplog.tab === "reflog"} onclick={() => setTab("reflog")} title="Where HEAD and your branches have been, from git's reflog">Git Reflog</button>
        </div>
      </div>
      {#if oplog.tab === "reflog"}
        <p class="intro">
          Where HEAD and your branches have been, from git's own log, including what a terminal or another app did.
          {#if !oplog.allMoves}Commits no branch has any more are listed here, so you can keep them on a branch.{/if}
        </p>
        <div class="list">
          {#each moves as entry (entry.id)}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <div class="row move" onclick={() => show(entry)}>
              <span class="tile" class:warn={entry.lost}>
                <svg class="icon" viewBox="0 0 16 16"><path d={moveIcon(entry)} /></svg>
              </span>
              <span class="words">
                <span class="line">
                  <span class="title">{entry.summary}</span>
                  {#if entry.lost}
                    <span class="lost" title={entry.lostCount === 1 ? "No branch, tag or remote branch has this commit" : `No branch, tag or remote branch has this commit or the ${entry.lostCount - 1} before it`}>{entry.lostCount > 1 ? `Lost · ${entry.lostCount}` : "Lost"}</span>
                  {/if}
                </span>
                <span class="detail"><span class="mono">{entry.selector}</span> · {moveText(entry)}</span>
              </span>
              <span class="when" title={entry.lost ? "Git keeps this commit at least until then" : undefined}>
                <span>{relativeTime(entry.time)}</span>
                {#if entry.lost && entry.expires !== null}<span class="until">{lostUntil(entry)}</span>{/if}
              </span>
              <button class="compare" title="Show the commit and its changes" onclick={(e) => (e.stopPropagation(), show(entry))}>Show</button>
              {#if entry.lost}
                <button class="restore" title="Keep {entry.lostCount === 1 ? 'this commit' : `these ${entry.lostCount} commits`} on a new branch, without a reset" onclick={(e) => (e.stopPropagation(), branch(entry))}>Create Branch…</button>
              {/if}
            </div>
          {:else}
            <div class="empty">
              {#if oplog.allMoves || !oplog.reflog.length}Git's reflog is empty here.{:else}Nothing is lost: every commit in git's reflog is on a branch.{/if}
            </div>
          {/each}
        </div>
        <div class="foot">
          <span>Git may delete a lost commit after the date beside it.</span>
          <button class="link" onclick={() => (oplog.allMoves = !oplog.allMoves)}>{oplog.allMoves ? "Only Lost Commits" : "Show All Moves"}</button>
        </div>
      {:else}
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
              <span class="detail">{entry.failed ? "Stopped half way · " : ""}{entry.detail}</span>
              {#if blocked}<WhyNote text={blocked} />{/if}
            </span>
            <span class="when" title="Oxbow can go back to this step until then">
              <span>{relativeTime(entry.time)}</span>
              <span class="until">{keptUntil(entry)}</span>
            </span>
            <button
              class="compare"
              title={i === 0 ? "Compare the files before this step with now" : "Compare the files right after this step with now"}
              onclick={() => compare(entry, i > 0)}>Compare</button
            >
            {#if i === 0}
              <button class="primary" disabled={!!blocked} title={blocked ?? undefined} onclick={() => go(undoRequest(entry))}>
                {isRedo(entry) ? "Redo" : "Undo"}<span class="key">{keys("Mod+Z")}</span>
              </button>
            {:else}
              <button class="restore" disabled={!!blocked} title={blocked ?? "Back to right after this step"} onclick={() => go(restoreRequest(entry, i))}>Restore to Here</button>
            {/if}
          </div>
        {:else}
          <div class="empty">Nothing to undo yet. Every change Oxbow makes to this repository shows up here. What a terminal or another app did is in Git Reflog.</div>
        {/each}
      </div>
      <div class="foot">
        <span>Snapshots are kept for {prefs.get("oxbow.undo.keepDays")} days. Untracked files over 50 MB and files git ignores are not in them.</span>
        {#if oplog.entries.length}
          <button class="link" onclick={() => go(clearRequest(oplog.entries.length))}>Clear Log…</button>
        {/if}
      </div>
      {/if}
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
    min-width: 0;
    font-size: 12px;
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .grow {
    flex-grow: 1;
  }
  .head {
    align-items: center;
  }
  .segmented {
    flex-shrink: 0;
    display: flex;
    padding: 2px;
    border-radius: 9px;
    background: var(--field);
  }
  .segmented button {
    height: 22px;
    min-width: 64px;
    padding: 0 10px;
    border-radius: 7px;
    font-size: 11.5px;
    font-weight: 500;
    color: var(--text2);
    white-space: nowrap;
  }
  .segmented button.on {
    background: var(--sheet);
    color: var(--text);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
  }
  .move {
    cursor: default;
  }
  .line {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .line .title {
    min-width: 0;
  }
  .lost {
    flex-shrink: 0;
    height: 17px;
    padding: 0 7px;
    border-radius: 9px;
    display: inline-flex;
    align-items: center;
    font-size: 10.5px;
    font-weight: 500;
    background: var(--orange-soft);
    color: var(--orange);
  }
  .mono {
    font-family: var(--mono);
    font-size: 10.5px;
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
  .when {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 1px;
    font-size: 11.5px;
    color: var(--text2);
  }
  .until {
    font-size: 10.5px;
    opacity: 0.8;
  }
  .compare {
    display: none;
    flex-shrink: 0;
    height: 26px;
    padding: 0 12px;
    border-radius: 13px;
    align-items: center;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    font-weight: 500;
    font-size: 12px;
    color: var(--text);
  }
  .row:hover .compare {
    display: flex;
  }
  .move:hover .lost {
    display: none;
  }
  .move .compare,
  .move .restore {
    min-width: 58px;
    justify-content: center;
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
  .row:hover .when {
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
    gap: 12px;
    margin-top: 6px;
    padding: 8px 10px 4px;
    border-top: 1px solid var(--sep);
    font-size: 11.5px;
    color: var(--text2);
  }
  .link {
    flex-shrink: 0;
    color: var(--accent-text);
    font-size: 11.5px;
  }
</style>
