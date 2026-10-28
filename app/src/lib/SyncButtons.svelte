<script lang="ts">
  // Fetch, Pull and Push in the toolbar, in one capsule: Fetch shows Activity's progress, and a
  // grey Pull or Push still answers a click, with why it can't run, under it.

  import { api } from "./api";
  import { activity, type ActivityItem } from "./activity.svelte";
  import ActivityPopover from "./ActivityPopover.svelte";
  import WhyPopover from "./WhyPopover.svelte";
  import type { RemoteContext, SyncBlock, SyncFix } from "./remote";
  import type { History } from "./types";

  let {
    history,
    remote,
    pullBlock,
    pushBlock,
    why = $bindable(),
    onSync,
    onFixFetch,
    onRetryFetch,
    onReload,
    onFix,
  }: {
    history: History;
    remote: RemoteContext;
    /** Why Pull or Push can't run now, if it can't. */
    pullBlock: SyncBlock | null;
    pushBlock: SyncBlock | null;
    /** The grey Pull or Push button clicked last: its reason shows under it. */
    why: "pull" | "push" | null;
    onSync: (kind: "fetch" | "pull" | "push") => void;
    onFixFetch: (item: ActivityItem) => void;
    onRetryFetch: () => void;
    onReload: () => void;
    /** The way out a reason offers, e.g. Publish… when the branch has no upstream to pull from. */
    onFix: (fix: SyncFix) => void;
  } = $props();

  const t = $derived(history.tracking);
  const whyBlock = $derived(why === "pull" ? pullBlock : why === "push" ? pushBlock : null);
  // Once the button can run again, its reason is gone for good.
  $effect(() => {
    if (why && !whyBlock) why = null;
  });
</script>

<div class="sync">
  <div class="group" role="group" aria-label="Sync with remote">
    <button
      class="fetch"
      onclick={() => onSync("fetch")}
      oncontextmenu={(event) => {
        event.preventDefault();
        activity.open = !activity.open;
      }}
      aria-label={activity.running ? "Fetching, show Activity" : activity.problem ? "Fetch failed, show Activity" : "Fetch"}
      aria-haspopup="dialog"
      aria-expanded={activity.open}
      title={activity.running
        ? `${activity.running.title} · click for Activity`
        : activity.problem
          ? `${activity.problem.sub} · click for Activity`
          : `Fetch from ${history.remotes.length > 1 ? "all remotes" : remote.remote} · right-click, or the repository menu, for Activity`}
    >
      {#if activity.running}
        <svg class="icon ring" class:spin={activity.percent === null} viewBox="0 0 16 16">
          <circle cx="8" cy="8" r="5.5" class="ring-track" />
          <circle cx="8" cy="8" r="5.5" class="ring-fill" pathLength="100" stroke-dasharray="{activity.percent ?? 25} 100" />
        </svg>
      {:else}
        <svg class="icon" viewBox="0 0 16 16"><path d="M13 8a5 5 0 0 1-8.6 3.5M3 8a5 5 0 0 1 8.6-3.5" /><path d="M11.8 1.8v2.9H8.9M4.2 14.2v-2.9h2.9" /></svg>
        {#if activity.problem}<span class="dot"></span>{/if}
      {/if}
    </button>
    <!-- Grey when it can't run, but it still answers a click: with why, under it. -->
    <button
      class:off={!!pullBlock}
      class:asking={why === "pull"}
      onclick={() => (pullBlock ? (why = why === "pull" ? null : "pull") : onSync("pull"))}
      aria-disabled={!!pullBlock}
      aria-haspopup={pullBlock ? "dialog" : undefined}
      aria-label={t?.behind && !t.rewritten ? `Pull ${t.behind} commits` : "Pull"}
      title={why === "pull" ? undefined : pullBlock ? `${pullBlock.title} · click for more` : `Pull from ${t?.remote}/${t?.branch}`}
    >
      <svg class="icon" viewBox="0 0 16 16"><path d="M8 2v9M4.5 7.5 8 11l3.5-3.5M3 14h10" /></svg>
      <!-- Old versions of commits rewritten here are nothing to pull. -->
      {#if t?.behind && !t.rewritten}<span class="count">{t.behind}</span>{/if}
    </button>
    <button
      class:off={!!pushBlock}
      class:asking={why === "push"}
      onclick={() => (pushBlock ? (why = why === "push" ? null : "push") : onSync("push"))}
      aria-disabled={!!pushBlock}
      aria-haspopup={pushBlock ? "dialog" : undefined}
      aria-label={t ? (t.ahead ? `Push ${t.ahead} commits` : "Push") : "Publish branch"}
      title={why === "push" ? undefined : pushBlock ? `${pushBlock.title} · click for more` : t?.rewritten ? `Force push to ${t.remote}/${t.branch}: it has the old versions of commits you rewrote` : t && !t.gone ? `Push to ${t.remote}/${t.branch}` : `Publish ${history.head.branch} to ${remote.remote}`}
    >
      <svg class="icon" viewBox="0 0 16 16"><path d="M8 12V3M4.5 6.5 8 3l3.5 3.5M3 14h10" /></svg>
      {#if t?.ahead}<span class="count">{t.ahead}</span>{:else if !t && history.head.branch && !remote.unread}<span class="count">Publish</span>{/if}
    </button>
  </div>
  <ActivityPopover fix={onFixFetch} retry={onRetryFetch} settings={() => ((activity.open = false), api.openSettings())} reload={onReload} />
  {#if whyBlock}
    {@const block = whyBlock}
    <WhyPopover title={block.title} body={block.body} fixes={block.fixes} onFix={(i) => onFix(block.fixes[i].fix)} onClose={() => (why = null)} />
  {/if}
</div>

<style>
  .group {
    display: flex;
    align-items: center;
    height: 34px;
    padding: 0 4px;
    border-radius: var(--radius-pill);
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    color: var(--icon);
  }
  .group button {
    height: 34px;
    min-width: 36px;
    padding: 0 9px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 3px;
    border-radius: var(--radius-pill);
  }
  .group button:disabled,
  .group button.off {
    opacity: 0.4;
  }
  .group button.off.asking {
    opacity: 0.55;
    background: var(--hover-wash);
  }
  .sync {
    position: relative;
    display: flex;
  }
  .fetch {
    position: relative;
  }
  .ring {
    transform: rotate(-90deg);
  }
  .ring circle {
    fill: none;
    stroke-width: 1.8;
  }
  .ring-track {
    stroke: var(--sep);
  }
  .ring-fill {
    stroke: var(--accent);
    stroke-linecap: round;
    transition: stroke-dasharray 0.2s;
  }
  .ring.spin {
    animation: ring-spin 1.2s linear infinite;
  }
  @keyframes ring-spin {
    from {
      transform: rotate(-90deg);
    }
    to {
      transform: rotate(270deg);
    }
  }
  .dot {
    position: absolute;
    top: 7px;
    right: 7px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--orange);
  }
  .count {
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text);
  }
</style>
