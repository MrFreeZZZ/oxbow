<script lang="ts">
  // Local History: every version of a file saved in the working copy lately, of every file,
  // newest first under the day it was saved, so even a file deleted since, or never added to git,
  // can be found and restored. A version shows what it changed, as in File History.

  import { untrack } from "svelte";
  import { api } from "./api";
  import { splitPath } from "./format";
  import { prefs } from "./prefs.svelte";
  import { byDay, keyOf, MAX_LISTED_ALL } from "./localHistory";
  import { localVersionEntries } from "./localHistoryMenu";
  import type { LocalVersion } from "./types";
  import LocalVersionView from "./LocalVersionView.svelte";
  import Menu, { type MenuEntry } from "./Menu.svelte";

  let {
    color,
    version = 0,
  }: {
    /** The checked-out branch's color, for the diffs. */
    color: number;
    /** Goes up on every reload: new saves come in. */
    version?: number;
  } = $props();

  let versions = $state<LocalVersion[] | null>(null);
  let error = $state<string | null>(null);
  let picked = $state<string | null>(null);
  let comparing = $state(false);
  let filter = $state("");
  let menu = $state<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  let list = $state<HTMLDivElement>();

  $effect(() => {
    version;
    untrack(() =>
      api.localHistoryAll().then(
        (all) => {
          versions = all;
          error = null;
          if (!picked || !all.some((v) => keyOf(v) === picked)) picked = all[0] ? keyOf(all[0]) : null;
        },
        (err) => (error = String(err)),
      ),
    );
  });

  const shown = $derived.by(() => {
    const words = filter.trim().toLowerCase();
    return (versions ?? []).filter((v) => !words || v.path.toLowerCase().includes(words));
  });
  const days = $derived(byDay(shown));
  const pick = $derived(shown.find((v) => keyOf(v) === picked) ?? null);
  const off = $derived(prefs.get("oxbow.localHistory.keepDays") === 0);

  function choose(v: LocalVersion) {
    picked = keyOf(v);
    comparing = false;
  }

  function rowMenu(event: MouseEvent, v: LocalVersion) {
    event.preventDefault();
    const entries: MenuEntry[] = [{ kind: "header", label: v.path }];
    entries.push(...localVersionEntries(v, { compare: () => ((picked = keyOf(v)), (comparing = true)), history: true }));
    menu = { x: event.clientX, y: event.clientY, entries };
  }

  /** ↑ and ↓ move through the versions shown. A click focuses its row itself: WebKit on macOS
   *  doesn't focus a button it clicks. */
  function keys(event: KeyboardEvent) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const at = shown.findIndex((v) => keyOf(v) === picked);
    const next = shown[Math.max(0, Math.min(shown.length - 1, at + (event.key === "ArrowDown" ? 1 : -1)))];
    if (!next) return;
    event.preventDefault();
    choose(next);
    requestAnimationFrame(() => list?.querySelector<HTMLElement>(".row.on")?.focus());
  }

  const time = (at: number) => new Date(at).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
</script>

<div class="screen">
  <section class="side" aria-label="Saved versions">
    <div class="top">
      <span>{versions ? `${versions.length}${versions.length >= MAX_LISTED_ALL ? "+" : ""} saved ${versions.length === 1 ? "version" : "versions"}` : "Saved versions"}</span>
      <span class="days">{off ? "Off in Settings" : `Kept ${prefs.get("oxbow.localHistory.keepDays")} days`}</span>
    </div>
    <input class="filter" type="search" placeholder="Filter by file" bind:value={filter} aria-label="Filter by file" spellcheck="false" />
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="list" bind:this={list} onkeydown={keys}>
      {#if error}
        <p class="hint error" role="alert">{error}</p>
      {:else if versions && !versions.length}
        <p class="hint">
          {off
            ? "Local History is off: turn it on in Settings › General › Safety to keep each version of a file you save."
            : "No file was saved here yet. Each time an editor saves a file of this repository while Oxbow has it open, the version is kept here for a few days."}
        </p>
      {:else if versions && !shown.length}
        <p class="hint">No saved file matches “{filter}”.</p>
      {/if}
      {#each days as day (day.day)}
        <div class="day">{day.day}</div>
        {#each day.versions as v (keyOf(v))}
          {@const name = splitPath(v.path)}
          <button class="row" class:on={keyOf(v) === picked} class:dim={!v.kept} onclick={(e) => (choose(v), (e.currentTarget as HTMLElement).focus())} oncontextmenu={(e) => rowMenu(e, v)} title={v.path}>
            <span class="time">{time(v.at)}</span>
            <span class="name"><b>{name.name}</b>{#if name.dir}<span class="dir">{name.dir.slice(0, -1)}</span>{/if}</span>
            {#if v.gone}<span class="tag">deleted</span>{/if}
          </button>
        {/each}
      {/each}
    </div>
    <p class="hint">Every version saved by any editor while Oxbow has the repository open. Right-click one to restore it.</p>
  </section>
  <section class="main" aria-label="Saved version">
    {#if pick}
      <LocalVersionView version={pick} {color} reload={version} history bind:comparing />
    {:else if versions?.length}
      <p class="hint pad">Pick a version on the left.</p>
    {/if}
  </section>
</div>

{#if menu}
  <Menu x={menu.x} y={menu.y} label="Version menu" entries={menu.entries} onClose={() => (menu = null)} />
{/if}

<style>
  .screen {
    flex-grow: 1;
    display: flex;
    min-height: 0;
    border-top: 1px solid var(--sep);
  }
  .side {
    width: 340px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--sep);
    background: var(--graph-bg);
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
  .days {
    font-weight: 400;
  }
  .filter {
    margin: 4px 12px 6px;
    height: 26px;
    padding: 0 8px;
    border-radius: var(--radius-m);
    border: none;
    background: var(--field);
    color: var(--text);
    font: inherit;
    font-size: var(--text-small);
  }
  .filter:focus {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }
  .list {
    flex-grow: 1;
    overflow-y: auto;
    padding: 0 8px 8px;
  }
  .day {
    position: sticky;
    top: 0;
    padding: 8px 8px 4px;
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
    background: var(--graph-bg);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 30px;
    padding: 0 8px;
    border-radius: var(--radius-m);
    text-align: left;
    font-size: var(--text-small);
  }
  .row:hover:not(.on) {
    background: var(--row-hover);
  }
  .row.on {
    background: var(--accent-quiet);
  }
  .row.dim {
    opacity: 0.45;
  }
  .time {
    flex-shrink: 0;
    min-width: 42px;
    white-space: nowrap;
    font-family: var(--mono);
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .name {
    flex-grow: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .name b {
    font-weight: 500;
  }
  .dir {
    margin-left: 6px;
    color: var(--text2);
  }
  .tag {
    flex-shrink: 0;
    padding: 1px 6px;
    border-radius: var(--radius-pill);
    background: var(--field);
    font-size: var(--text-caption);
    color: var(--text2);
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
  .error {
    color: var(--red);
  }
  .main {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
</style>
