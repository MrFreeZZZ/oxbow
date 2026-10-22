<script lang="ts">
  // Go to Branch… (⌘T, as VS Code's Go to Symbol in Workspace): branches, tags, stashes,
  // worktrees and submodules, one type at a time, Tab for the next. Each row says where the
  // object is in full and how it stands; what can be done with the one picked is spelled out
  // under the list, Return for the first, ⌘Return for the second.

  import { keys, shortcut } from "./keys";
  import { lane } from "./format";
  import { menuIcons } from "./Menu.svelte";
  import { actionIcons } from "./icons";
  import { KINDS, actionsOf, countOf, emptyText, objectsOf, search, type ObjectAction, type ObjectItem, type ObjectKind } from "./objects";
  import type { History } from "./types";

  let {
    history,
    tilde,
    first,
    initial = "",
    colorOf,
    onAct,
    onClose,
  }: {
    history: History;
    /** A folder the way people read it, with ~ for the home folder. */
    tilde: (path: string) => string;
    /** The type shown first. */
    first: ObjectKind;
    /** What the search starts with, from History's search. */
    initial?: string;
    colorOf: (branch: string) => number;
    /** Do `action` with `item`; the picker has closed. */
    onAct: (item: ObjectItem, action: ObjectAction) => void;
    onClose: () => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let kind = $state<ObjectKind>(first);
  // svelte-ignore state_referenced_locally
  let query = $state(initial);
  let active = $state(0);
  let input = $state<HTMLInputElement>();
  let listEl = $state<HTMLDivElement>();

  const all = $derived(objectsOf(history, tilde));
  const found = $derived(Object.fromEntries(KINDS.map((k) => [k.kind, search(all[k.kind], query)])) as Record<ObjectKind, ReturnType<typeof search>>);
  const shown = $derived(found[kind]);
  const current = $derived(shown[active]?.item ?? null);
  const actions = $derived(current ? actionsOf(current, history) : []);
  /** With nothing of this type matching, the next type that has some. */
  const elsewhere = $derived(query.trim() && !shown.length ? (KINDS.find((k) => found[k.kind].length) ?? null) : null);

  $effect(() => {
    input?.focus();
  });

  // A new query or type starts at the best match.
  $effect(() => {
    query;
    kind;
    active = 0;
  });

  $effect(() => {
    const i = active;
    requestAnimationFrame(() => listEl?.querySelector<HTMLElement>(`[data-index="${i}"]`)?.scrollIntoView({ block: "nearest" }));
  });

  function go(item: ObjectItem, action: ObjectAction | undefined) {
    if (!action) return;
    onClose();
    // After the picker is gone, so a confirmation the action asks for gets the keyboard.
    requestAnimationFrame(() => onAct(item, action));
  }

  function step(by: number) {
    const at = KINDS.findIndex((k) => k.kind === kind);
    kind = KINDS[(at + by + KINDS.length) % KINDS.length].kind;
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      active = Math.min(shown.length - 1, active + 1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      active = Math.max(0, active - 1);
    } else if (event.key === "PageDown" || event.key === "PageUp") {
      event.preventDefault();
      active = Math.max(0, Math.min(shown.length - 1, active + (event.key === "PageDown" ? 10 : -10)));
    } else if (event.key === "Tab") {
      event.preventDefault();
      step(event.shiftKey ? -1 : 1);
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (current) go(current, actions[event.metaKey || event.ctrlKey ? 1 : 0]);
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
    }
  }

  /** `text` in pieces, with the letters at `hits` marked. */
  function pieces(text: string, hits: number[]): { text: string; hit: boolean }[] {
    const set = new Set(hits);
    const out: { text: string; hit: boolean }[] = [];
    for (let i = 0; i < text.length; i++) {
      const last = out[out.length - 1];
      if (last && last.hit === set.has(i)) last.text += text[i];
      else out.push({ text: text[i], hit: set.has(i) });
    }
    return out;
  }

  const ICONS: Record<ObjectKind, string> = { branches: menuIcons.branch, tags: menuIcons.tag, stashes: menuIcons.stash, worktrees: menuIcons.folder, submodules: actionIcons.box };
  const ACTION_ICONS: Record<ObjectAction["id"], string> = { show: menuIcons.history, open: menuIcons.open, checkout: menuIcons.checkout, stashes: menuIcons.stash, details: actionIcons.box };
  const ENTER = ["Enter", "Mod+Enter"];
</script>

{#snippet marked(text: string, hits: number[])}{#each pieces(text, hits) as piece, k (k)}<span class:hit={piece.hit}>{piece.text}</span>{/each}{/snippet}

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="catcher" onclick={onClose}></div>
<div class="pop" role="dialog" aria-label="Go to">
  <div class="field">
    <svg class="icon" viewBox="0 0 16 16"><path d="M7 2.5a4.5 4.5 0 1 0 0 9a4.5 4.5 0 1 0 0-9M10.3 10.3 14 14" /></svg>
    <input bind:this={input} bind:value={query} onkeydown={onKey} placeholder="Search {KINDS.find((k) => k.kind === kind)?.title.toLowerCase()} by name or path" spellcheck="false" autocomplete="off" aria-label="Search" />
    <span class="keys">{shortcut("goToBranch")}</span>
  </div>
  <div class="tabs" role="tablist" aria-label="Types">
    {#each KINDS as k (k.kind)}
      {@const n = found[k.kind].length}
      <button role="tab" class="tab" class:on={kind === k.kind} class:none={!n} aria-selected={kind === k.kind} title={query.trim() ? `${countOf(k.kind, n)} match` : countOf(k.kind, n)} onclick={() => ((kind = k.kind), input?.focus())}>
        <svg class="icon" viewBox="0 0 16 16"><path d={ICONS[k.kind]} /></svg>
        {k.title}
        <span class="count">{n}</span>
      </button>
    {/each}
  </div>
  <div class="list" bind:this={listEl} role="listbox" aria-label={KINDS.find((k) => k.kind === kind)?.title}>
    {#if !shown.length}
      <p class="note">
        {emptyText(kind, query)}
        {#if elsewhere}<button class="link" onclick={() => ((kind = elsewhere.kind), input?.focus())}>{countOf(elsewhere.kind, found[elsewhere.kind].length)} {found[elsewhere.kind].length === 1 ? "matches" : "match"}</button>{/if}
      </p>
    {:else}
      {#each shown as row, i (row.item.key)}
        {@const item = row.item}
        <button class="item" class:on={i === active} data-index={i} role="option" aria-selected={i === active} onmousemove={() => (active = i)} onclick={() => go(item, actionsOf(item, history)[0])}>
          {#if item.kind === "branches"}
            <span class="dot" style:background={lane(colorOf(item.branch!))}></span>
          {:else}
            <svg class="icon" class:tag={item.kind === "tags"} class:here={item.worktree?.current} class:dim={item.worktree?.missing || (item.submodule && !item.submodule.cloned)} viewBox="0 0 16 16"><path d={ICONS[item.kind]} /></svg>
          {/if}
          <span class="text">
            <span class="line">
              <span class="name">{@render marked(item.name, row.name)}</span>
              {#each item.notes as note, k (k)}<span class="state {note.tone ?? ''}">{note.text}</span>{/each}
            </span>
            <span class="path">{@render marked(item.path, row.path)}{#if item.about}<span class="about">{" · "}{item.about}</span>{/if}</span>
          </span>
        </button>
      {/each}
    {/if}
  </div>
  <div class="foot">
    <span class="hint"><kbd>{keys("Tab")}</kbd> next type</span>
    <span class="grow"></span>
    {#each actions as action, k (action.id)}
      <button class="button small act" class:primary={k === 0} onclick={() => current && go(current, action)} title="{action.label} ({keys(ENTER[k])})">
        <svg class="icon" viewBox="0 0 16 16"><path d={ACTION_ICONS[action.id]} /></svg>
        {action.label}
        <kbd>{keys(ENTER[k])}</kbd>
      </button>
    {/each}
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
    width: min(680px, calc(100vw - 48px));
    max-height: min(560px, calc(100vh - 120px));
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-xl);
    background: color-mix(in srgb, var(--win) 97%, transparent);
    -webkit-backdrop-filter: blur(30px) saturate(1.8);
    backdrop-filter: blur(30px) saturate(1.8);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--shadow-sheet);
    color: var(--text);
    overflow: hidden;
  }
  .field {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 48px;
    padding: 0 14px 0 16px;
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
    font-size: var(--text-heading);
    user-select: text;
    -webkit-user-select: text;
    box-shadow: none;
  }
  .keys {
    font-size: var(--text-caption);
    color: var(--text2);
    background: var(--field);
    border-radius: var(--radius-s);
    padding: 2px 7px;
    flex-shrink: 0;
  }
  .tabs {
    display: flex;
    gap: 2px;
    padding: 0 10px 8px;
    border-bottom: 1px solid var(--sep);
    flex-shrink: 0;
    overflow-x: auto;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 10px;
    border-radius: var(--radius-m);
    color: var(--text2);
    white-space: nowrap;
    flex-shrink: 0;
  }
  .tab:hover {
    color: var(--text);
  }
  .tab.on {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--text);
  }
  .tab.none .count {
    opacity: 0.5;
  }
  .count {
    font-size: var(--text-caption);
    color: var(--text2);
    font-variant-numeric: tabular-nums;
  }
  .list {
    overflow-y: auto;
    padding: 6px;
    min-height: 0;
    flex-grow: 1;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-height: 44px;
    padding: 4px 10px;
    border-radius: var(--radius-m);
    text-align: left;
  }
  .item.on {
    background: var(--side-sel);
  }
  .item > .icon {
    flex-shrink: 0;
    color: var(--text2);
  }
  .item > .icon.tag {
    color: var(--tag-border);
  }
  .item > .icon.here {
    color: var(--accent);
  }
  .item > .icon.dim {
    opacity: 0.5;
  }
  .dot {
    width: 8px;
    height: 8px;
    margin: 0 4px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    flex-grow: 1;
    min-width: 0;
  }
  .line {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
  }
  .name {
    flex-grow: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .state {
    flex-shrink: 0;
    font-size: var(--text-small);
    color: var(--text2);
    white-space: nowrap;
  }
  .state.accent {
    color: var(--accent);
  }
  .state.warn {
    color: var(--orange);
  }
  .state.dim {
    opacity: 0.75;
  }
  .path {
    font-size: var(--text-small);
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .about {
    opacity: 0.85;
  }
  .hit {
    font-weight: 700;
    color: var(--text);
  }
  .note {
    margin: 0;
    padding: 10px 12px;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .link {
    color: var(--accent);
    padding: 0;
  }
  .link:hover {
    text-decoration: underline;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 10px 0 16px;
    border-top: 1px solid var(--sep);
    flex-shrink: 0;
  }
  .hint {
    font-size: var(--text-small);
    color: var(--text2);
    white-space: nowrap;
  }
  .grow {
    flex-grow: 1;
  }
  .act kbd {
    color: inherit;
    background: color-mix(in srgb, currentColor 14%, transparent);
    padding: 0 5px;
  }
  kbd {
    font: inherit;
    font-size: var(--text-caption);
    color: var(--text2);
    background: var(--field);
    border-radius: var(--radius-s);
    padding: 1px 6px;
    min-width: 20px;
    text-align: center;
  }
</style>
