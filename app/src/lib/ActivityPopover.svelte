<script lang="ts">
  // The Activity popover under Fetch / Pull / Push, with Reload at the top: the fetch that runs now with its progress,
  // Output and Stop; a Push or Pull waiting for it; and under Done what ran lately with its git
  // commands, with Fix… and Try Again on a fetch that failed.

  import { activity, type ActivityItem } from "./activity.svelte";
  import { relativeTime } from "./format";
  import { actionIcons } from "./icons";
  import { prefs } from "./prefs.svelte";
  import CommandWords from "./CommandWords.svelte";
  import { keys } from "./keys";

  /** Commands a row shows before Show All. */
  const FEW = 2;

  /** Rows whose commands are all shown. */
  let unfolded = $state<Set<number>>(new Set());
  let copiedId = $state<number | null>(null);

  async function copy(item: ActivityItem) {
    await navigator.clipboard.writeText((item.commands ?? []).join("\n")).catch(() => {});
    copiedId = item.id;
    setTimeout(() => copiedId === item.id && (copiedId = null), 1500);
  }

  const OUTPUT: Record<string, string> = {
    out: "var(--term-out)",
    err: "var(--term-err)",
    hint: "var(--term-hint)",
    ok: "var(--term-ok)",
  };

  let {
    fix,
    retry,
    settings,
    reload,
  }: {
    /** Open the way out of a failed fetch. */
    fix: (item: ActivityItem) => void;
    retry: (item: ActivityItem) => void;
    settings: () => void;
    /** Read the repository again, without asking the remote. */
    reload: () => void;
  } = $props();

  const auto = $derived(prefs.get("oxbow.fetch.auto"));
  const every = $derived(prefs.get("oxbow.fetch.interval"));

  const ICONS: Record<string, string> = {
    fetch: "M13 8a5 5 0 0 1-8.6 3.5M3 8a5 5 0 0 1 8.6-3.5M11.8 1.8v2.9H8.9M4.2 14.2v-2.9h2.9",
    pull: "M8 2v9M4.5 7.5 8 11l3.5-3.5M3 14h10",
    push: "M8 12V3M4.5 6.5 8 3l3.5 3.5M3 14h10",
    done: "M3.5 8.5 6.5 11.5 12.5 4.5",
    key: "M10.5 3a2.5 2.5 0 1 1 0 5 2.5 2.5 0 0 1 0-5zM8.5 7.5 3 13M5 11l1.5 1.5M4 12l1.5 1.5",
    stopped: "M5 5h6v6H5z",
  };

  function iconOf(item: ActivityItem): string {
    if (item.state === "failed" || item.fixed) return item.failure?.ssh || item.failure?.kind === "auth" ? ICONS.key : ICONS.fetch;
    if (item.state === "stopped") return ICONS.stopped;
    if (item.state === "done" && item.kind === "fetch") return ICONS.done;
    if (item.kind === "action") return item.icon ? actionIcons[item.icon] : ICONS.done;
    return ICONS[item.kind];
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape" && activity.open) {
      event.preventDefault();
      activity.open = false;
    }
  }

  const fixable = (item: ActivityItem) => item.failure?.kind === "auth" || item.failure?.kind === "network" || !!item.failure?.ssh;
</script>

<svelte:window onkeydown={onKey} />

{#if activity.open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="catcher" onclick={() => (activity.open = false)}></div>
  <div class="pop" role="dialog" aria-label="Activity">
    <div class="head">
      <span class="h">Activity</span>
      <span class="every">{auto ? `Auto-fetch every ${every} min` : "Auto-fetch is off"}</span>
      <!-- Reload lives here and in the repository menu: the toolbar keeps only Fetch. -->
      <button class="reload" onclick={() => ((activity.open = false), reload())} title="Read the repository again, without asking the remote">
        <svg class="icon" viewBox="0 0 16 16"><path d="M13 8a5 5 0 1 1-1.5-3.5M13 2.5V5h-2.5" /></svg>
        Reload <span class="keys">{keys("Mod+R")}</span>
      </button>
    </div>
    <div class="list">
      {#if activity.running}
        {@const item = activity.running}
        <div class="row live">
          <span class="tile"><svg class="icon spin" viewBox="0 0 16 16"><path d={ICONS.fetch} /></svg></span>
          <span class="words">
            <span class="line"><span class="title">{item.title}</span>{#if activity.percent !== null}<span class="pct">{activity.percent}%</span>{/if}</span>
            <span class="detail">{item.sub}</span>
            <span class="track"><span class="fill" class:busy={activity.percent === null} style:width="{activity.percent ?? 30}%"></span></span>
            {#if activity.showOutput}
              <div class="term mono selectable" aria-live="polite">
                {#each activity.lines as line, i (i)}
                  <div style:color={line.kind === "cmd" ? undefined : OUTPUT[line.kind]}>{line.kind === "cmd" ? `$ ${line.text}` : line.text}</div>
                {/each}
              </div>
            {/if}
            <span class="buttons">
              <button class="button small" onclick={() => (activity.showOutput = !activity.showOutput)}>{activity.showOutput ? "Hide Output" : "Show Output"}</button>
              <button class="button small" onclick={() => activity.stop()}>Stop</button>
            </span>
          </span>
        </div>
      {/if}
      {#each activity.queued as item (item.id)}
        <div class="row">
          <span class="tile"><svg class="icon" viewBox="0 0 16 16"><path d={ICONS[item.kind]} /></svg></span>
          <span class="words">
            <span class="line"><span class="title">{item.title}</span><span class="time">next</span></span>
            <span class="detail">{item.sub}</span>
            <span class="buttons"><button class="button small" onclick={() => activity.unqueue(item.id)}>Cancel</button></span>
          </span>
        </div>
      {/each}
      {#if activity.recent.length}<div class="section">Done</div>{/if}
      {#each activity.recent as item (item.id)}
        {@const open = item.state === "failed" && !item.fixed && activity.problem?.id === item.id}
        {@const commands = item.commands ?? []}
        {@const shown = unfolded.has(item.id) ? commands : commands.slice(0, FEW)}
        <div class="row" class:live={open}>
          <span class="tile" class:warn={open}><svg class="icon" viewBox="0 0 16 16"><path d={iconOf(item)} /></svg></span>
          <span class="words">
            <span class="line"><span class="title">{item.title}</span><span class="time">{relativeTime(Math.floor(item.time / 1000))}</span></span>
            {#if item.sub}<span class="detail" class:bad={open}>{item.sub}</span>{/if}
            {#if commands.length && !open}
              <div class="cmds mono selectable" aria-label="Git commands">
                <button class="copy" title="Copy the commands" onclick={() => copy(item)}>{copiedId === item.id ? "Copied" : "Copy"}</button>
                {#each shown as command, i (i)}
                  <div class="cmd"><span class="dollar">$</span> <CommandWords text={command} /></div>
                {/each}
                {#if commands.length > shown.length}
                  <button class="more" onclick={() => (unfolded = new Set([...unfolded, item.id]))}>and {commands.length - shown.length} more</button>
                {/if}
              </div>
            {/if}
            {#if open}
              <span class="buttons">
                <button class="button small primary" onclick={() => fix(item)}>{fixable(item) ? "Fix…" : "Details…"}</button>
                <button class="button small" disabled={!!activity.running} onclick={() => retry(item)}>Try Again</button>
              </span>
            {/if}
          </span>
        </div>
      {/each}
      {#if !activity.running && !activity.queued.length && !activity.recent.length}
        <div class="empty">Nothing ran yet. Every action shows up here with the git commands it ran.</div>
      {/if}
    </div>
    <div class="foot">
      <span>{activity.running ? "Keep working while it runs." : "Fetching never changes your branches."}</span>
      <button class="link" onclick={settings}>Fetch Settings…</button>
    </div>
  </div>
{/if}

<style>
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
    width: 360px;
    max-width: calc(100vw - 32px);
    padding: 14px 8px 8px;
    border-radius: var(--radius-sheet);
    background: var(--sheet);
    -webkit-backdrop-filter: blur(30px) saturate(1.8);
    backdrop-filter: blur(30px) saturate(1.8);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--shadow-pop);
    display: flex;
    flex-direction: column;
    text-align: left;
    color: var(--text);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 6px 6px 10px;
  }
  .h {
    font-size: var(--text-heading);
    font-weight: 700;
  }
  .every {
    flex-grow: 1;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .reload {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 10px;
    border-radius: var(--radius-pill);
    font-size: var(--text-small);
    font-weight: 500;
    white-space: nowrap;
    flex-shrink: 0;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    color: var(--text);
  }
  .reload .icon {
    width: 13px;
    height: 13px;
  }
  .keys {
    color: var(--text2);
    font-weight: 400;
  }
  .list {
    max-height: 460px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .row {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 10px;
    border-radius: var(--radius-l);
  }
  .row.live {
    background: var(--field);
  }
  .tile {
    width: 30px;
    height: 30px;
    flex-shrink: 0;
    border-radius: var(--radius-pill);
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
  .spin {
    animation: spin 1.4s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .words {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .line {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  .title {
    flex-grow: 1;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pct,
  .time {
    flex-shrink: 0;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .detail {
    font-size: var(--text-small);
    color: var(--text2);
  }
  .detail.bad {
    color: var(--orange);
  }
  .section {
    padding: 8px 10px 2px;
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
  }
  .cmds {
    position: relative;
    margin-top: 6px;
    padding: 6px 9px;
    border-radius: var(--radius-m);
    background: var(--term);
    border: 0.5px solid var(--term-border);
    color: var(--term-text);
    font-size: var(--text-caption);
    line-height: 1.45;
  }
  .cmd {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
  }
  .dollar {
    color: var(--term-dim);
  }
  .copy {
    position: absolute;
    top: 4px;
    right: 6px;
    padding: 1px 7px;
    border-radius: var(--radius-s);
    font-family: -apple-system, BlinkMacSystemFont, sans-serif;
    font-size: var(--text-caption);
    color: var(--term-text);
    background: var(--term-button);
    opacity: 0;
    transition: opacity 0.12s;
  }
  .cmds:hover .copy,
  .copy:focus-visible {
    opacity: 1;
  }
  .more {
    color: var(--term-comment);
    font-size: var(--text-caption);
    font-family: inherit;
  }
  .track {
    margin-top: 6px;
    height: 4px;
    border-radius: var(--radius-pill);
    background: var(--sep);
    overflow: hidden;
  }
  .fill {
    display: block;
    height: 100%;
    border-radius: var(--radius-xs);
    background: var(--accent);
    transition: width 0.2s;
  }
  .fill.busy {
    animation: busy 1.2s ease-in-out infinite alternate;
  }
  @keyframes busy {
    from {
      margin-left: 0;
    }
    to {
      margin-left: 70%;
    }
  }
  .term {
    margin-top: 8px;
    max-height: 120px;
    overflow-y: auto;
    padding: 8px 10px;
    border-radius: var(--radius-l);
    background: var(--term);
    border: 0.5px solid var(--term-border);
    color: var(--term-text);
    font-size: var(--text-caption);
    line-height: 1.45;
    white-space: pre-wrap;
    word-break: break-all;
  }
  .buttons {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }

  button:disabled {
    opacity: 0.45;
  }
  .empty {
    padding: 14px 10px;
    font-size: var(--text-small);
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
    font-size: var(--text-small);
    color: var(--text2);
  }
  .link {
    color: var(--accent-text);
    font-size: var(--text-small);
  }
</style>
