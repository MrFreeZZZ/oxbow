<script lang="ts">
  import { keys } from "./keys";
  // Quick Open (⌘P): pick any file in HEAD to open its File History, by typing a few letters of
  // its path. Files opened lately and the ones the selected commit changed come first.

  import { api } from "./api";
  import { nav } from "./nav.svelte";
  import { shortId, splitPath } from "./format";

  let {
    commit,
    onClose,
  }: {
    /** The commit selected in History, whose files are offered first; none for the working copy. */
    commit: { id: string; summary: string } | null;
    onClose: () => void;
  } = $props();

  type Item = { path: string; hits: number[]; from: "recent" | "commit" | "all" };

  let files = $state<string[] | null>(null);
  let changed = $state<string[]>([]);
  let error = $state<string | null>(null);
  let query = $state("");
  let active = $state(0);
  let input = $state<HTMLInputElement>();
  let listEl = $state<HTMLDivElement>();
  const LIMIT = 200;

  $effect(() => {
    api.files().then(
      (f) => (files = f),
      (err) => (error = String(err)),
    );
    input?.focus();
  });

  $effect(() => {
    const id = commit?.id;
    changed = [];
    if (!id) return;
    api.commitDetail(id).then(
      (d) => {
        if (commit?.id === id) changed = d.files.filter((f) => f.status !== "deleted").map((f) => f.path);
      },
      () => {},
    );
  });

  const present = $derived(new Set(files ?? []));
  const recent = $derived((nav.recentFiles[nav.repo] ?? []).filter((p) => present.has(p)));

  /** Letters of the query in order inside the path; a run inside the file name ranks first. */
  function fuzzy(path: string, q: string): { score: number; hits: number[] } | null {
    const p = path.toLowerCase();
    const name = path.lastIndexOf("/") + 1;
    const run = (at: number) => Array.from({ length: q.length }, (_, k) => at + k);
    let at = p.indexOf(q, name);
    if (at >= 0) return { score: 3000 + (at === name ? 500 : 0) - path.length, hits: run(at) };
    at = p.indexOf(q);
    if (at >= 0) return { score: 2000 - path.length, hits: run(at) };
    // Letters spread out: inside the file name alone if they fit there, else across the path.
    return subsequence(p, q, name, 1300 - path.length) ?? subsequence(p, q, 0, 1000 - path.length);
  }

  function subsequence(p: string, q: string, start: number, base: number): { score: number; hits: number[] } | null {
    const name = p.lastIndexOf("/") + 1;
    const hits: number[] = [];
    let score = base;
    let from = start;
    for (const ch of q) {
      const i = p.indexOf(ch, from);
      if (i < 0) return null;
      const prev = hits[hits.length - 1];
      if (prev !== undefined && i === prev + 1) score += 15;
      else if (prev !== undefined) score -= Math.min(i - prev, 20);
      if (i === 0 || "/._- ".includes(p[i - 1])) score += 10;
      if (i >= name) score += 5;
      hits.push(i);
      from = i + 1;
    }
    return { score, hits };
  }

  /** What is listed, and how many files match in all. */
  const found = $derived.by<{ items: Item[]; total: number }>(() => {
    if (!files) return { items: [], total: 0 };
    const q = query.toLowerCase().replace(/\s+/g, "");
    if (!q) {
      const seen = new Set<string>();
      const take = (paths: string[], from: Item["from"]) =>
        paths.filter((p) => !seen.has(p) && seen.add(p)).map((path) => ({ path, hits: [], from }));
      const first = take(recent.slice(0, 5), "recent");
      const second = take(changed.filter((p) => present.has(p)).slice(0, 8), "commit");
      return { items: [...first, ...second, ...take(files.slice(0, LIMIT), "all")], total: files.length };
    }
    const ranked: (Item & { score: number })[] = [];
    for (const path of files) {
      const m = fuzzy(path, q);
      if (!m) continue;
      // Files opened lately or in the selected commit are the likelier ones.
      const bonus = recent.includes(path) ? 40 : changed.includes(path) ? 20 : 0;
      ranked.push({ path, hits: m.hits, from: "all", score: m.score + bonus });
    }
    ranked.sort((a, b) => b.score - a.score || a.path.localeCompare(b.path));
    return { items: ranked.slice(0, LIMIT), total: ranked.length };
  });
  const items = $derived(found.items);
  const total = $derived(found.total);

  // A new query starts at the best match.
  $effect(() => {
    query;
    active = 0;
  });

  $effect(() => {
    const i = active;
    requestAnimationFrame(() => listEl?.querySelector<HTMLElement>(`[data-index="${i}"]`)?.scrollIntoView({ block: "nearest" }));
  });

  function open(item: Item) {
    onClose();
    nav.focusFind = true;
    nav.openFile(item.path, item.from === "commit" && commit ? commit.id : null);
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      active = Math.min(items.length - 1, active + 1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      active = Math.max(0, active - 1);
    } else if (event.key === "Enter") {
      event.preventDefault();
      const item = items[active];
      if (item) open(item);
    } else if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }

  /** The path in pieces, with the matched letters marked. */
  function pieces(path: string, hits: number[]): { text: string; hit: boolean; dir: boolean }[] {
    const name = path.lastIndexOf("/") + 1;
    const set = new Set(hits);
    const out: { text: string; hit: boolean; dir: boolean }[] = [];
    for (let i = 0; i < path.length; i++) {
      const piece = { hit: set.has(i), dir: i < name };
      const last = out[out.length - 1];
      if (last && last.hit === piece.hit && last.dir === piece.dir) last.text += path[i];
      else out.push({ text: path[i], ...piece });
    }
    return out;
  }

  const heading: Record<Item["from"], string> = { recent: "Recently opened", commit: "", all: "All files" };
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="catcher" onclick={onClose}></div>
<div class="pop" role="dialog" aria-label="Open file history">
  <div class="field">
    <svg class="icon" viewBox="0 0 16 16"><path d="M4 1.5h5.5L13 5v9.5H4z" /><path d="M9.5 1.5V5H13" /></svg>
    <input bind:this={input} bind:value={query} onkeydown={onKey} placeholder="Find a file to see its history" spellcheck="false" autocomplete="off" aria-label="File name" />
    <span class="keys">{keys("Mod+P")}</span>
  </div>
  <div class="list" bind:this={listEl} role="listbox" aria-label="Files">
    {#if error}
      <p class="note error">{error}</p>
    {:else if !files}
      <p class="note">Reading the files…</p>
    {:else if !items.length}
      <p class="note">{files.length ? `No file matches “${query}”.` : "This repository has no commits yet."}</p>
    {:else}
      {#each items as item, i (item.from + item.path)}
        {#if i === 0 || items[i - 1].from !== item.from}
          {#if !query.trim()}
            <div class="heading">
              {#if item.from === "commit" && commit}
                In the selected commit <span class="mono">{shortId(commit.id)}</span> · {commit.summary}
              {:else}
                {heading[item.from]}
              {/if}
            </div>
          {/if}
        {/if}
        {@const p = splitPath(item.path)}
        <button class="item" class:on={i === active} data-index={i} role="option" aria-selected={i === active} onmousemove={() => (active = i)} onclick={() => open(item)}>
          <svg class="icon file" viewBox="0 0 16 16"><path d="M4 1.5h5.5L13 5v9.5H4z" /><path d="M9.5 1.5V5H13" /></svg>
          {#if item.hits.length}
            <span class="path">{#each pieces(item.path, item.hits) as piece, k (k)}<span class:dir={piece.dir} class:hit={piece.hit}>{piece.text}</span>{/each}</span>
          {:else}
            <span class="path"><span class="dir">{p.dir}</span>{p.name}</span>
          {/if}
          {#if i === active}<span class="enter">History ↵</span>{/if}
        </button>
      {/each}
      {#if total > items.length}
        <p class="note">{(total - items.length).toLocaleString()} more {query.trim() ? "matches" : "files"}. Type more of the name to narrow it down.</p>
      {/if}
    {/if}
  </div>
</div>

<style>
  .catcher {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: rgba(0, 0, 0, 0.08);
  }
  .pop {
    position: fixed;
    top: 64px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 41;
    width: min(600px, calc(100vw - 48px));
    max-height: min(520px, calc(100vh - 120px));
    display: flex;
    flex-direction: column;
    border-radius: 16px;
    background: color-mix(in srgb, var(--win) 97%, transparent);
    -webkit-backdrop-filter: blur(30px) saturate(1.8);
    backdrop-filter: blur(30px) saturate(1.8);
    border: 0.5px solid var(--glass-border);
    box-shadow: 0 18px 50px rgba(0, 0, 0, 0.25);
    color: var(--text);
    overflow: hidden;
  }
  .field {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 48px;
    padding: 0 14px 0 16px;
    border-bottom: 1px solid var(--sep);
    flex-shrink: 0;
  }
  .field .icon {
    color: var(--text2);
    flex-shrink: 0;
  }
  input {
    flex-grow: 1;
    min-width: 0;
    border: 0;
    outline: none;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 15px;
    user-select: text;
    -webkit-user-select: text;
  }
  .keys {
    font-size: 11px;
    color: var(--text2);
    background: var(--field);
    border-radius: 6px;
    padding: 2px 7px;
    flex-shrink: 0;
  }
  .list {
    overflow-y: auto;
    padding: 6px;
    min-height: 0;
  }
  .heading {
    padding: 8px 10px 4px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .heading .mono {
    font-family: var(--mono);
    font-weight: 400;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    height: 30px;
    padding: 0 10px;
    border-radius: 8px;
    text-align: left;
  }
  .item.on {
    background: var(--side-sel);
  }
  .file {
    width: 14px;
    height: 14px;
    color: var(--text2);
    flex-shrink: 0;
  }
  .path {
    flex-grow: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dir {
    color: var(--text2);
  }
  .hit {
    font-weight: 700;
    color: var(--text);
  }
  .enter {
    font-size: 11px;
    color: var(--text2);
    flex-shrink: 0;
  }
  .note {
    margin: 0;
    padding: 10px 12px;
    font-size: 12px;
    color: var(--text2);
  }
  .error {
    color: var(--red);
  }
</style>
