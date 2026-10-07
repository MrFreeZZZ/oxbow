<script lang="ts">
  // The bar under the toolbar while searching: how many commits matched, filters for author,
  // branch and date, the same search as a `git log` command, and In Graph / Only Matches.

  import type { History } from "./types";
  import Menu, { type MenuEntry } from "./Menu.svelte";
  import { SINCE, search } from "./search.svelte";

  let {
    history,
    count,
    copy,
  }: {
    history: History;
    /** Matching commits among the loaded ones. */
    count: number;
    copy: (text: string) => void;
  } = $props();

  const CHECK = "M3.5 8.5l3 3 6-7";
  let menu = $state<{ x: number; y: number; label: string; entries: MenuEntry[] } | null>(null);

  const mode = $derived(search.parsed.mode);
  const result = $derived(search.result);

  /** Authors of the loaded commits, the busiest first. */
  const authors = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const r of history.rows) if (!r.worktree) counts.set(r.authorName, (counts.get(r.authorName) ?? 0) + 1);
    return [...counts.entries()].sort((a, b) => b[1] - a[1]).slice(0, 12).map(([name]) => name);
  });

  const branches = $derived.by(() => {
    const names = history.refs.filter((r) => r.kind === "local").map((r) => r.name);
    const head = history.head.branch;
    return head && names.includes(head) ? [head, ...names.filter((n) => n !== head)] : names;
  });

  function open(event: MouseEvent, label: string, entries: MenuEntry[]) {
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    menu = { x: box.left, y: box.bottom + 6, label, entries };
  }

  const item = (label: string, on: boolean, run: () => void): MenuEntry => ({ kind: "item", label, icon: on ? CHECK : "", run });

  function authorMenu(event: MouseEvent) {
    open(event, "Author", [
      item("Anyone", !search.author, () => (search.author = null)),
      { kind: "sep" },
      ...authors.map((name) => item(name, search.author === name, () => (search.author = name))),
    ]);
  }

  function branchMenu(event: MouseEvent) {
    open(event, "Branches", [
      item("All branches", !search.branch, () => (search.branch = null)),
      { kind: "sep" },
      ...branches.map((name) => item(name === history.head.branch ? `${name} (HEAD)` : name, search.branch === name, () => (search.branch = name))),
    ]);
  }

  function dateMenu(event: MouseEvent) {
    open(
      event,
      "Date",
      SINCE.map((s) => item(s.label, search.since === s.since, () => (search.since = s.since))),
    );
  }

  /** The command, split into words colored like the terminal block of confirmations. */
  const words = $derived(
    (result?.command ?? "").match(/'(?:[^']|'\\'')*'|"[^"]*"|\S+/g)?.map((word, i) => ({
      text: word,
      color:
        i === 0 ? "var(--term-git)" : i === 1 ? "var(--term-sub)" : word.startsWith("-") || word.startsWith("'-") ? "var(--term-flag)" : /^['"]/.test(word) ? "var(--term-str)" : "var(--term-text)",
      bold: i === 0,
    })) ?? [],
  );

  let copied = $state(false);
  function copyCommand() {
    if (!result) return;
    copy(result.command);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }
</script>

<div class="bar" role="region" aria-label="Search results">
  <span class="count">
    {#if search.error}
      <span class="error" role="alert">{search.error}</span>
    {:else}
      <b>{count.toLocaleString()}{result?.more ? "+" : ""} {count === 1 ? "commit" : "commits"}</b>
      <span class="sub">{search.only ? "newest first" : count ? "others faded" : "match"}</span>
    {/if}
  </span>
  {#if mode !== "author" && mode !== "sha"}
    <button class="pill" class:set={!!search.author} onclick={authorMenu} aria-haspopup="menu">
      Author: <b>{search.author ?? "Anyone"}</b>
      <svg class="icon chevron" viewBox="0 0 16 16"><path d="M4.5 6.5 8 10l3.5-3.5" /></svg>
    </button>
  {/if}
  {#if mode !== "sha"}
    <button class="pill" class:set={!!search.branch} onclick={branchMenu} aria-haspopup="menu">
      Branches: <b>{search.branch ?? "All"}</b>
      <svg class="icon chevron" viewBox="0 0 16 16"><path d="M4.5 6.5 8 10l3.5-3.5" /></svg>
    </button>
    <button class="pill" class:set={search.since !== "any"} onclick={dateMenu} aria-haspopup="menu">
      Date: <b>{SINCE.find((s) => s.since === search.since)?.label}</b>
      <svg class="icon chevron" viewBox="0 0 16 16"><path d="M4.5 6.5 8 10l3.5-3.5" /></svg>
    </button>
  {/if}
  <span class="spacer"></span>
  {#if result}
    <button class="term" onclick={copyCommand} title="Copy the command" aria-label="Copy {result.command}">
      <span class="prompt">$</span>
      <span class="words">
        {#each words as w, i (i)}<span class:bold={w.bold} style:color={w.color}>{w.text}</span>{" "}{/each}
      </span>
      {#if copied}
        <svg class="icon copy" viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg>
      {:else}
        <svg class="icon copy" viewBox="0 0 16 16"><path d="M5.5 5.5h7v8h-7zM3.5 10.5v-8h7" /></svg>
      {/if}
    </button>
  {/if}
  <div class="segmented" role="radiogroup" aria-label="Show matches">
    <button role="radio" aria-checked={!search.only} class:on={!search.only} onclick={() => (search.only = false)}>In Graph</button>
    <button role="radio" aria-checked={search.only} class:on={search.only} onclick={() => (search.only = true)}>Only Matches</button>
  </div>
</div>

{#if menu}
  <Menu x={menu.x} y={menu.y} label={menu.label} entries={menu.entries} onClose={() => (menu = null)} />
{/if}

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 40px;
    flex-shrink: 0;
    padding: 0 12px 0 16px;
    min-width: 0;
  }
  .count {
    display: flex;
    align-items: baseline;
    gap: 6px;
    white-space: nowrap;
    margin-right: 4px;
    flex-shrink: 0;
  }
  .count b {
    font-weight: 700;
  }
  .sub {
    font-size: 11px;
    color: var(--text2);
  }
  .error {
    color: var(--red);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 360px;
  }
  .pill {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 28px;
    padding: 0 8px 0 11px;
    border-radius: 14px;
    border: 1px solid var(--sep);
    color: var(--text2);
    font-size: 12px;
    white-space: nowrap;
    flex-shrink: 0;
  }
  .pill b {
    font-weight: 500;
    color: var(--text);
  }
  .pill.set {
    background: var(--side-sel);
    border-color: transparent;
  }
  .pill.set b {
    font-weight: 600;
  }
  .chevron {
    width: 11px;
    height: 11px;
  }
  .spacer {
    flex-grow: 1;
  }
  .term {
    display: flex;
    align-items: center;
    gap: 7px;
    height: 28px;
    min-width: 0;
    flex-shrink: 1;
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
    min-width: 0;
  }
  .bold {
    font-weight: 700;
  }
  .copy {
    width: 13px;
    height: 13px;
    color: var(--term-dim);
    flex-shrink: 0;
  }
  .segmented {
    display: flex;
    flex-shrink: 0;
    padding: 2px;
    border-radius: 15px;
    background: var(--field);
  }
  .segmented button {
    height: 26px;
    padding: 0 12px;
    border-radius: 13px;
    font-size: 12px;
    color: var(--text2);
    white-space: nowrap;
  }
  .segmented button.on {
    background: var(--win);
    color: var(--text);
    font-weight: 600;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
  }
</style>
