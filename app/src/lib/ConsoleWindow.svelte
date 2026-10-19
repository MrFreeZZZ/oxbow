<script lang="ts">
  // The Console window (⌥⌘C): every process Oxbow ran, oldest at the top and the newest at the
  // bottom as in a terminal, with its output, exit code and how long it took. The commands Oxbow
  // runs for an action (checks, the secrets check, the snapshot for Undo) fold under it; reads of
  // the repository and the timer's fetches are quieter. Read-only: nothing here runs git.

  import { listen } from "@tauri-apps/api/event";
  import { tick } from "svelte";
  import { api } from "./api";
  import CommandWords from "./CommandWords.svelte";
  import { lineKind } from "./confirm.svelte";
  import {
    CONSOLE_KEEP,
    clock,
    dayOf,
    duration,
    exitOf,
    failed,
    helperCommands,
    outputOpen,
    ownCommands,
    shows,
    upsert,
    type ConsoleEntry,
    type ConsoleFilter,
    type ConsoleLine,
    type ConsoleRan,
    type ConsoleRepos,
  } from "./console";
  import { mac } from "./keys";
  import Menu, { menuIcons, type MenuEntry } from "./Menu.svelte";

  /** An entry with the repository it is of, for All Repositories. */
  type Shown = ConsoleEntry & { repo: string | null };
  /** This Repository follows the window used last; a picked one stays. */
  type Source = { kind: "current" } | { kind: "repo"; path: string | null } | { kind: "all" };

  const MASK = "••••••";
  const CLOCK = "M8 2.5a5.5 5.5 0 1 0 0 11a5.5 5.5 0 1 0 0-11M8 5v3l2 1.5";
  const CHECK = "M3.5 8.5 6.5 11.5 12.5 4.5";
  const TRASH = "M2.5 4.5h11M6 4.5V3a.8.8 0 0 1 .8-.8h2.4A.8.8 0 0 1 10 3v1.5M4 4.5l.7 8.6a1 1 0 0 0 1 .9h4.6a1 1 0 0 0 1-.9l.7-8.6";
  const INFO = "M8 1.8a6.2 6.2 0 1 0 0 12.4a6.2 6.2 0 1 0 0-12.4M8 7.2v4M8 4.8v.1";

  let repos = $state<ConsoleRepos | null>(null);
  let source = $state<Source>({ kind: "current" });
  let entries = $state<Shown[]>([]);
  let loaded = $state(false);
  let filter = $state<ConsoleFilter>("all");
  let query = $state("");
  /** Outputs and folds opened or closed by a click, against how they start. */
  let flipped = $state(new Set<string>());
  /** Scrolled to the bottom: new entries keep it there. */
  let following = $state(true);
  let picker = $state<{ x: number; y: number } | null>(null);
  let clearing = $state<{ all: boolean } | null>(null);
  let log = $state<HTMLDivElement>();
  let search = $state<HTMLInputElement>();

  /** The path the source stands for; undefined for All Repositories. */
  const path = $derived(source.kind === "current" ? (repos?.current ?? null) : source.kind === "repo" ? source.path : undefined);
  const nameOf = (p: string | null) => repos?.repos.find((r) => r.path === p)?.name ?? (p ? (p.split(/[\\/]/).filter(Boolean).pop() ?? p) : "Outside Repositories");
  const sourceName = $derived(path === undefined ? "All Repositories" : nameOf(path));
  const pickerLabel = $derived(source.kind === "current" ? "This Repository" : sourceName);
  const shown = $derived(entries.filter((e) => shows(e, filter, query)));
  const commandTotal = $derived(entries.reduce((n, e) => n + e.commands.length, 0));
  const currentCount = $derived(repos?.repos.find((r) => r.path === (repos?.current ?? null))?.count ?? 0);
  const allCount = $derived(repos?.repos.reduce((n, r) => n + r.count, 0) ?? 0);

  /** The days of the shown entries, each with its entries in time order. */
  const days = $derived.by(() => {
    const now = Date.now();
    const out: { day: string; entries: Shown[] }[] = [];
    for (const entry of shown) {
      const day = dayOf(entry.time, now);
      if (out[out.length - 1]?.day !== day) out.push({ day, entries: [] });
      out[out.length - 1].entries.push(entry);
    }
    return out;
  });

  let generation = 0;
  async function load() {
    const mine = ++generation;
    const next = await api.consoleRepos().catch(() => null);
    if (mine !== generation) return;
    repos = next;
    const paths: (string | null)[] = path === undefined ? (next?.repos.map((r) => r.path) ?? []) : [path];
    const lists = await Promise.all(paths.map((p) => api.consoleEntries(p).then((list) => list.map((e): Shown => ({ ...e, repo: p }))).catch(() => [] as Shown[])));
    if (mine !== generation) return;
    entries = lists.flat().sort((a, b) => a.time - b.time);
    loaded = true;
    following = true;
    await toBottom();
  }

  async function toBottom() {
    await tick();
    if (log) log.scrollTop = log.scrollHeight;
  }

  function onScroll() {
    if (!log) return;
    following = log.scrollHeight - log.scrollTop - log.clientHeight < 24;
  }

  $effect(() => {
    // The first load, and again when This Repository changes.
    void source;
    void load();
  });

  $effect(() => {
    const entry = listen<{ repo: string | null; entry: ConsoleEntry }>("console-entry", ({ payload }) => {
      if (!loaded) return;
      const known = path === undefined ? (repos?.repos.some((r) => r.path === payload.repo) ?? false) : payload.repo === path;
      // A repository new to All Repositories, or This Repository before its first entry.
      if (!known && path === undefined) {
        void load();
        return;
      }
      if (!known) return;
      entries = upsert(entries, { ...payload.entry, repo: payload.repo });
      if (following) void toBottom();
    });
    const changed = listen("repo-changed", () => {
      if (source.kind === "current") void load();
    });
    return () => {
      entry.then((off) => off()).catch(() => {});
      changed.then((off) => off()).catch(() => {});
    };
  });

  const isOpen = (key: string, initially: boolean) => initially !== flipped.has(key);
  function flip(key: string) {
    const next = new Set(flipped);
    if (!next.delete(key)) next.add(key);
    flipped = next;
  }

  function pickerEntries(): MenuEntry[] {
    const list: MenuEntry[] = [];
    const current = repos?.current;
    list.push({
      kind: "item",
      label: "This Repository",
      icon: menuIcons.folder,
      detail: current ? nameOf(current) : "No repository is open",
      run: () => (source = { kind: "current" }),
    });
    const others = (repos?.repos ?? []).filter((r) => r.path !== current);
    if (others.length) list.push({ kind: "sep" });
    for (const repo of others) {
      // Two folders of one name tell apart by where they are.
      const twin = (repos?.repos ?? []).some((r) => r !== repo && r.name === repo.name);
      const where = twin && repo.path ? ` · ${repo.path.split(/[\\/]/).slice(-2, -1)[0] ?? repo.path}` : "";
      list.push({ kind: "item", label: repo.name, icon: repo.path ? menuIcons.folder : menuIcons.open, detail: entryCount(repo.count) + where, run: () => (source = { kind: "repo", path: repo.path }) });
    }
    list.push({ kind: "sep" }, { kind: "item", label: "All Repositories", icon: menuIcons.history, run: () => (source = { kind: "all" }) });
    return list;
  }

  async function clearHistory(all: boolean) {
    const paths = all ? (repos?.repos.map((r) => r.path) ?? []) : [path === undefined ? (repos?.current ?? null) : path];
    clearing = null;
    await api.consoleClear(paths).catch(() => {});
    await load();
  }

  /** Parts of an output line: masked secrets and commit ids get their own color. */
  function parts(text: string): { text: string; kind: "text" | "mask" | "sha" }[] {
    const out: { text: string; kind: "text" | "mask" | "sha" }[] = [];
    // A commit id stands alone, as in `7a6ce6d..d7855a5`; hex in a path or a word stays text.
    for (const piece of text.split(/(••••••|(?<![\w/-])[0-9a-f]{7,40}(?![\w/-]))/)) {
      if (!piece) continue;
      out.push({ text: piece, kind: piece === MASK ? "mask" : /^[0-9a-f]{7,40}$/.test(piece) && /\d/.test(piece) ? "sha" : "text" });
    }
    return out;
  }

  const plural = (n: number, word: string) => `${n.toLocaleString("en-US")} ${word}${n === 1 ? "" : "s"}`;
  const entryCount = (n: number) => `${n.toLocaleString("en-US")} ${n === 1 ? "entry" : "entries"}`;
  const keyed = (event: KeyboardEvent) => event.metaKey || event.ctrlKey;
</script>

<svelte:window
  onkeydown={(event) => {
    if (keyed(event) && !event.shiftKey && !event.altKey && event.key.toLowerCase() === "f") {
      event.preventDefault();
      search?.focus();
      search?.select();
    } else if (event.key === "Escape" && clearing) {
      clearing = null;
    }
  }}
/>

{#snippet output(lines: ConsoleLine[])}
  <div class="out mono selectable">
    {#each lines as line, i (i)}
      {@const kind = lineKind(line.t, !!line.e)}
      <div class="line" class:hint={kind === "hint"} class:cut={/^⋯ \d+ more lines ⋯$/.test(line.t)}>
        {#each parts(line.t) as part, k (k)}<span class={part.kind}>{part.text}</span>{/each}
      </div>
    {/each}
  </div>
{/snippet}

{#snippet command(ran: ConsoleRan)}
  <div class="cmd mono selectable"><span class="prompt">%</span> <CommandWords text={ran.display} /></div>
{/snippet}

<!-- Amber for the action's own commands; a check or a read often asks git what may not be there, so
     its exit code stays grey. -->
{#snippet status(ran: ConsoleRan, own: boolean)}
  {#if ran.running}
    <span class="spinner" aria-label="Running"></span>
  {:else if ran.stopped}
    <span class="badge">stopped</span>
  {:else if failed(ran)}
    <span class="badge" class:exit={own}>{ran.code === null ? "did not start" : `exit ${ran.code}`}</span>
  {/if}
{/snippet}

{#snippet ownCommand(entry: Shown, ran: ConsoleRan, i: number)}
  {@const lines = ran.output?.length ?? 0}
  {@const key = `${entry.id}:o${i}`}
  {@const open = isOpen(key, outputOpen(entry, ran))}
  <div class="own">
    {@render command(ran)}
    {#if entry.kind === "read"}{@render status(ran, false)}{/if}
  </div>
  {#if lines && open}
    {@render output(ran.output ?? [])}
  {/if}
  {#if lines && (!open || !outputOpen(entry, ran))}
    <button class="more" class:open aria-expanded={open} onclick={() => flip(key)}>
      <svg class="icon chev" viewBox="0 0 16 16"><path d="M6 4l4 4-4 4" /></svg>{open ? "Hide output" : plural(lines, "line") + " of output"}
    </button>
  {:else if !lines && entry.kind === "background" && !ran.running}
    <div class="more still">no output</div>
  {/if}
{/snippet}

{#snippet entryRow(entry: Shown)}
  {@const own = entry.kind === "read" && entry.commands.length > 1 ? [] : entry.kind === "read" ? entry.commands : ownCommands(entry)}
  {@const helpers = entry.kind === "read" && entry.commands.length > 1 ? entry.commands : helperCommands(entry)}
  {@const exit = exitOf(entry)}
  {@const foldKey = `${entry.id}:h`}
  {@const folded = !isOpen(foldKey, false)}
  <div class="entry" class:quiet={entry.kind !== "action"} class:failed={entry.failed}>
    <span class="time mono">{clock(entry.time)}</span>
    <div class="body">
      <div class="head">
        <span class="title">{entry.title}</span>
        {#if source.kind === "all"}<span class="repo">{nameOf(entry.repo)}</span>{/if}
        {#if entry.kind === "background"}
          <span class="tag"><svg class="icon" viewBox="0 0 16 16"><path d={CLOCK} /></svg>background</span>
        {/if}
        {#if entry.gitoxide}<span class="gix">gitoxide{entry.commands.length ? "" : " · no git process"}</span>{/if}
        <span class="ms">{entry.running ? "running…" : duration(entry.millis)}</span>
        {#if entry.running}
          <span class="spinner" aria-label="Running"></span>
        {:else if entry.stopped}
          <span class="badge">stopped</span>
        {:else if entry.failed}
          <span class="badge exit">{exit === null ? "failed" : `exit ${exit}`}</span>
        {:else}
          <svg class="icon ok" viewBox="0 0 16 16" aria-label="Done"><path d={CHECK} /></svg>
        {/if}
      </div>
      {#if entry.note}<div class="note mono">{entry.note}</div>{/if}
      {#each own as ran, i (i)}
        {@render ownCommand(entry, ran, i)}
      {/each}
      {#if helpers.length}
        <button class="more" class:open={!folded} aria-expanded={!folded} onclick={() => flip(foldKey)}>
          <svg class="icon chev" viewBox="0 0 16 16"><path d="M6 4l4 4-4 4" /></svg>
          {#if entry.kind === "read"}{plural(helpers.length, "git command")}{:else}{plural(helpers.length, own.length ? "more command" : "command")} Oxbow ran for it{/if}
        </button>
        {#if !folded}
          <div class="helpers">
            {#each helpers as ran, i (i)}
              {@const key = `${entry.id}:h${i}`}
              {@const lines = ran.output?.length ?? 0}
              <button class="helper" class:roled={entry.kind !== "read"} disabled={!lines} aria-expanded={lines ? isOpen(key, false) : undefined} onclick={() => flip(key)} title={lines ? (isOpen(key, false) ? "Hide its output" : `Show its output: ${plural(lines, "line")}`) : "No output"}>
                {#if entry.kind !== "read"}<span class="role">{ran.role}</span>{/if}
                <span class="mono"><span class="prompt">%</span> <CommandWords text={ran.display} /></span>
                <span class="ms">{ran.running ? "running…" : duration(ran.millis)}</span>
                {@render status(ran, false)}
              </button>
              {#if lines && isOpen(key, false)}{@render output(ran.output ?? [])}{/if}
            {/each}
          </div>
        {/if}
      {/if}
    </div>
  </div>
{/snippet}

<div class="console" class:mac>
  <header data-tauri-drag-region>
    <div class="title" data-tauri-drag-region>Console<small>{sourceName}</small></div>
    <span class="spacer" data-tauri-drag-region></span>
    <button
      class="button small"
      aria-haspopup="menu"
      aria-expanded={!!picker}
      onclick={(event) => {
        const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
        picker = picker ? null : { x: box.left, y: box.bottom + 6 };
      }}
    >
      <svg class="icon" viewBox="0 0 16 16"><path d={menuIcons.folder} /></svg>
      {pickerLabel}
      <svg class="icon chevron" viewBox="0 0 16 16"><path d="M4.5 6.5 8 10l3.5-3.5" /></svg>
    </button>
    <div class="segmented" role="radiogroup" aria-label="Show">
      {#each [["all", "All"], ["mine", "My Actions"], ["background", "Background"]] as [value, label] (value)}
        <button role="radio" aria-checked={filter === value} class:on={filter === value} onclick={() => (filter = value as ConsoleFilter)}>{label}</button>
      {/each}
    </div>
    <label class="search">
      <svg class="icon" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5" /><path d="M10.5 10.5 14 14" /></svg>
      <input bind:this={search} type="search" aria-label="Filter commands and output" placeholder="Filter commands and output" bind:value={query} spellcheck="false" />
    </label>
    <button class="button icon-only trash" aria-label="Clear History…" title="Clear History…" disabled={!allCount} onclick={() => (clearing = { all: source.kind === "all" })}>
      <svg class="icon" viewBox="0 0 16 16"><path d={TRASH} /></svg>
    </button>
  </header>

  <div class="log" bind:this={log} onscroll={onScroll}>
    {#if loaded && !shown.length}
      <div class="empty">
        {#if entries.length}
          Nothing here matches. Try another filter.
        {:else if path === undefined || path === null || repos?.current || source.kind !== "current"}
          Nothing ran yet in {sourceName}. Every git command Oxbow runs shows up here, with what it printed.
        {:else}
          Open a repository to see the git commands Oxbow runs in it.
        {/if}
      </div>
    {/if}
    {#each days as day (day.day)}
      <div class="day">{day.day}</div>
      {#each day.entries as entry (entry.id)}
        {@render entryRow(entry)}
      {/each}
    {/each}
  </div>

  <footer>
    <span>{entryCount(shown.length)} · {plural(commandTotal, "command")}</span>
    <span class="dot-sep">·</span>
    <span>Keeps the last {CONSOLE_KEEP.toLocaleString("en-US")} for {path === undefined ? "each repository" : "this repository"}</span>
    <span class="spacer"></span>
    {#if following}
      <span class="live"><i></i>Follows new commands</span>
    {:else}
      <button class="link" onclick={() => ((following = true), toBottom())}>Jump to Latest</button>
    {/if}
  </footer>

  {#if picker}
    <Menu x={picker.x} y={picker.y} label="Repository" entries={pickerEntries()} onClose={() => (picker = null)} />
  {/if}

  {#if clearing}
    {@const only = path === undefined ? (repos?.current ?? null) : path}
    {@const onlyCount = path === undefined ? currentCount : (repos?.repos.find((r) => r.path === path)?.count ?? entries.length)}
    {@const withLogs = repos?.repos.length ?? 0}
    <div class="dim" role="presentation" onclick={() => (clearing = null)}></div>
    <div class="sheet" role="dialog" aria-modal="true" aria-labelledby="clear-title">
      <h2 id="clear-title">Do you really want to clear the console history?</h2>
      <p>The commands and their output go away for good. Your commits, branches and the Undo history stay as they are.</p>
      <div class="choices" role="radiogroup" aria-label="Clear">
        <button role="radio" aria-checked={!clearing.all} disabled={!onlyCount} onclick={() => clearing && (clearing.all = false)}>
          <span class="radio" class:on={!clearing.all}></span>Only {nameOf(only)}<small>{entryCount(onlyCount)}</small>
        </button>
        <button role="radio" aria-checked={clearing.all} onclick={() => clearing && (clearing.all = true)}>
          <span class="radio" class:on={clearing.all}></span>All repositories<small>{entryCount(allCount)} in {withLogs}</small>
        </button>
      </div>
      <div class="info">
        <svg class="icon" viewBox="0 0 16 16"><path d={INFO} /></svg>
        <span>Oxbow keeps the last {CONSOLE_KEEP.toLocaleString("en-US")} entries per repository on its own, so clearing is only needed when you want a clean log.</span>
      </div>
      <div class="buttons">
        <button class="button" onclick={() => (clearing = null)}>Cancel</button>
        <button class="button primary danger" onclick={() => clearing && clearHistory(clearing.all || !onlyCount)}>Clear History</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .console {
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--win);
    position: relative;
  }
  header {
    height: 56px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 14px 0 18px;
    border-bottom: 0.5px solid var(--sep);
  }
  /* Room for the traffic lights, which macOS draws over the bar. */
  .mac header {
    padding-left: 92px;
  }
  .title {
    font-weight: 600;
    font-size: var(--text-body);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .title small {
    font-weight: 400;
    color: var(--text2);
    font-size: var(--text-small);
    margin-left: 6px;
  }
  .spacer {
    flex: 1;
  }
  header .button .chevron {
    width: 10px;
    height: 10px;
    margin-left: -2px;
  }
  header .button .icon {
    width: 14px;
    height: 14px;
  }
  .segmented {
    display: flex;
    flex-shrink: 0;
    padding: 2px;
    border-radius: var(--radius-pill);
    background: var(--field);
  }
  .segmented button {
    height: 24px;
    padding: 0 11px;
    border-radius: var(--radius-pill);
    font-size: var(--text-small);
    color: var(--text2);
  }
  .segmented button.on {
    background: var(--seg-on);
    color: var(--text);
    font-weight: 600;
    box-shadow: var(--seg-shadow);
  }
  .search {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 220px;
    min-width: 120px;
    flex-shrink: 1;
    height: 28px;
    padding: 0 10px;
    border-radius: var(--radius-pill);
    background: var(--field);
    color: var(--text2);
  }
  .search .icon {
    width: 13px;
    height: 13px;
  }
  .search:focus-within {
    box-shadow: var(--focus-ring);
  }
  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    font: inherit;
    font-size: var(--text-small);
    color: var(--text);
    box-shadow: none;
  }
  .trash {
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
  }
  .trash .icon {
    width: 15px;
    height: 15px;
  }

  .log {
    flex: 1;
    min-height: 0;
    overflow: auto;
    background: var(--term);
    padding-bottom: 12px;
  }
  .empty {
    margin: 60px auto;
    max-width: 380px;
    text-align: center;
    color: var(--text2);
    line-height: 1.5;
  }
  .day {
    position: sticky;
    top: 0;
    z-index: 1;
    padding: 10px 18px 6px;
    font-size: var(--text-caption);
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
    color: var(--term-dim);
    background: var(--term);
  }
  .entry {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    column-gap: 18px;
    padding: 8px 18px;
    border-top: 0.5px solid var(--term-border);
  }
  /* Commands, their output and the times follow Diff & Text › Console & Activity; the time column
     grows with them, so it never runs into the titles. */
  .mono {
    font-family: var(--term-font);
  }
  .time {
    color: var(--term-dim);
    font-size: calc(var(--term-size) - 1px);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    padding-top: 2px;
  }
  .head {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .head .title {
    font-size: var(--text-small);
    color: var(--text);
    font-weight: 600;
  }
  .quiet .head .title {
    color: var(--text2);
    font-weight: 500;
  }
  .ms {
    color: var(--term-dim);
    font-size: var(--text-caption);
  }
  .ok {
    width: 13px;
    height: 13px;
    color: var(--term-dim);
  }
  .tag {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: var(--text-caption);
    color: var(--term-dim);
  }
  .tag .icon {
    width: 11px;
    height: 11px;
  }
  .gix {
    font-size: var(--text-caption);
    color: var(--term-dim);
    border: 0.5px solid var(--term-border);
    padding: 0 7px;
    border-radius: var(--radius-pill);
  }
  .repo {
    font-size: var(--text-caption);
    color: var(--accent-ink);
    background: var(--accent-quiet);
    padding: 1px 7px;
    border-radius: var(--radius-pill);
  }
  .badge {
    font-size: var(--text-caption);
    font-weight: 500;
    color: var(--text2);
    background: var(--field);
    padding: 1px 7px;
    border-radius: var(--radius-pill);
    white-space: nowrap;
  }
  /* A failure is amber, never red: red is for what can't be undone. */
  .badge.exit {
    color: var(--orange);
    background: var(--orange-soft);
  }
  .spinner {
    width: 11px;
    height: 11px;
    border-radius: 50%;
    border: 1.5px solid var(--term-border);
    border-top-color: var(--accent);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .note {
    margin-top: 3px;
    color: var(--term-dim);
    font-style: italic;
    font-size: var(--term-size);
  }
  .own {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  .cmd {
    margin-top: 3px;
    color: var(--term-git);
    font-size: var(--term-size);
    line-height: var(--term-line);
    white-space: pre-wrap;
    word-break: break-word;
    text-indent: -1.3em;
    padding-left: 1.3em;
  }
  .prompt {
    color: var(--term-dim);
  }
  .quiet .cmd,
  .quiet .cmd :global(span),
  .quiet .cmd :global(button) {
    color: var(--term-dim) !important;
  }
  .out {
    margin: 4px 0 2px;
    padding-left: 14px;
    border-left: 2px solid var(--term-border);
    color: var(--term-out);
    font-size: var(--term-size);
    line-height: var(--term-line);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .out .hint {
    color: var(--term-hint);
  }
  .out .cut {
    color: var(--term-dim);
    font-style: italic;
  }
  .out .mask {
    color: var(--term-dim);
  }
  .out .sha {
    color: var(--term-sha);
  }
  .more {
    margin-top: 3px;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: var(--text-caption);
    color: var(--term-dim);
    border-radius: var(--radius-xs);
  }
  button.more:hover,
  .more.open {
    color: var(--text2);
  }
  .more.still {
    display: block;
  }
  .more .chev {
    width: 10px;
    height: 10px;
    transition: transform 0.15s ease;
  }
  .more.open .chev {
    transform: rotate(90deg);
  }
  .helpers {
    margin-top: 3px;
    padding-left: 14px;
    border-left: 2px solid var(--term-border);
    display: flex;
    flex-direction: column;
  }
  .helper {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 2px 6px 2px 0;
    border-radius: var(--radius-xs);
    white-space: normal;
    color: var(--term-dim);
  }
  .helper:not(:disabled):hover {
    background: var(--term-button);
  }
  .helper .role {
    flex: 0 0 130px;
    font-size: var(--text-caption);
  }
  .helper .mono {
    font-size: var(--term-size);
    line-height: var(--term-line);
    word-break: break-word;
  }
  /* Quieter than the action's own commands: there to check, not to read first. */
  .helper .mono {
    opacity: 0.72;
  }
  .helper .mono :global(.bold) {
    font-weight: 400;
  }

  footer {
    height: 34px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 16px;
    border-top: 0.5px solid var(--sep);
    font-size: var(--text-caption);
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
  }
  .dot-sep {
    color: var(--term-dim);
  }
  .live {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .live i {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
  }
  .link {
    color: var(--accent-text);
    font-size: var(--text-caption);
  }

  .dim {
    position: absolute;
    inset: 56px 0 0 0;
    background: var(--dim);
    z-index: 5;
  }
  .sheet {
    position: absolute;
    left: 50%;
    top: 96px;
    transform: translateX(-50%);
    z-index: 6;
    width: min(440px, calc(100% - 32px));
    padding: 22px 22px 18px;
    border-radius: var(--radius-sheet);
    background: var(--sheet);
    -webkit-backdrop-filter: blur(30px) saturate(1.8);
    backdrop-filter: blur(30px) saturate(1.8);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--shadow-sheet);
  }
  .sheet h2 {
    margin: 0 0 6px;
    font-size: var(--text-heading);
    font-weight: 600;
  }
  .sheet p {
    margin: 0 0 14px;
    color: var(--text2);
    line-height: 1.45;
  }
  .choices {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 4px;
    margin-bottom: 12px;
    border-radius: var(--radius-l);
    background: var(--panel);
    border: 0.5px solid var(--panel-border);
  }
  .choices button {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 10px;
    border-radius: var(--radius-m);
  }
  .choices button:disabled {
    opacity: 0.45;
  }
  .choices small {
    margin-left: auto;
    color: var(--text2);
    font-size: var(--text-small);
  }
  .radio {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    border-radius: 50%;
    border: 1.5px solid var(--text2);
    opacity: 0.6;
  }
  .radio.on {
    border: 5px solid var(--accent);
    opacity: 1;
  }
  .info {
    display: flex;
    gap: 8px;
    margin-bottom: 16px;
    color: var(--text2);
    font-size: var(--text-small);
    line-height: 1.4;
  }
  .info .icon {
    width: 14px;
    height: 14px;
    margin-top: 1px;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
