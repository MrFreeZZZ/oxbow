<script lang="ts">
  // The Command Palette (⇧⌘P, F1) as in VS Code: every command Oxbow has here and now, with its
  // shortcut, the ones used lately first. The field starts with ">"; taking it away turns the
  // palette into Quick Open, as in VS Code. The same list picks a recent repository (⌃R).

  import { untrack } from "svelte";
  import { keys, shortcut } from "./keys";
  import { labelOf, rank, recentCommands, rememberCommand, type PaletteItem } from "./palette";
  import { combo } from "./shortcuts";

  let {
    items,
    commands = true,
    initial = "",
    placeholder,
    empty,
    onClose,
    onFiles,
  }: {
    items: PaletteItem[];
    /** A list of commands, with ">" in the field and the ones used lately first; else a plain list. */
    commands?: boolean;
    initial?: string;
    placeholder: string;
    /** What the list says when nothing matches the query. */
    empty: (query: string) => string;
    onClose: () => void;
    /** The ">" was taken away: Quick Open, with what is typed. */
    onFiles?: (query: string) => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let text = $state(commands ? `>${initial}` : initial);
  let active = $state(0);
  let input = $state<HTMLInputElement>();
  let listEl = $state<HTMLDivElement>();
  // svelte-ignore state_referenced_locally
  const recent = commands ? recentCommands() : [];

  const query = $derived(commands ? text.replace(/^>\s*/, "") : text);
  const shown = $derived(
    commands ? rank(items, query, recent) : query.trim() ? rank(items, query, []) : items.map((item) => ({ item, hits: [], recent: false })),
  );

  // Once, when the field is there: typing in the middle of the query keeps the cursor there.
  $effect(() => {
    if (!input) return;
    input.focus();
    // The cursor after the ">", as in VS Code.
    const end = untrack(() => text.length);
    input.setSelectionRange(end, end);
  });

  $effect(() => {
    if (commands && onFiles && !text.startsWith(">")) onFiles(text);
  });

  // A new query starts at the best match.
  $effect(() => {
    query;
    active = 0;
  });

  $effect(() => {
    const i = active;
    requestAnimationFrame(() => listEl?.querySelector<HTMLElement>(`[data-index="${i}"]`)?.scrollIntoView({ block: "nearest" }));
  });

  function go(item: PaletteItem) {
    if (commands) rememberCommand(item.id);
    onClose();
    // After the palette is gone, so a sheet the command opens gets the keyboard.
    requestAnimationFrame(() => item.run());
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
    } else if (event.key === "Enter") {
      event.preventDefault();
      const row = shown[active];
      if (row) go(row.item);
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
    }
  }

  /** The label in pieces, with the matched letters marked and the category grey. */
  function pieces(item: PaletteItem, hits: number[]): { text: string; hit: boolean; dim: boolean }[] {
    const label = labelOf(item);
    const dimTo = item.category ? item.category.length + 1 : 0;
    const set = new Set(hits);
    const out: { text: string; hit: boolean; dim: boolean }[] = [];
    for (let i = 0; i < label.length; i++) {
      const piece = { hit: set.has(i), dim: i < dimTo };
      const last = out[out.length - 1];
      if (last && last.hit === piece.hit && last.dim === piece.dim) last.text += label[i];
      else out.push({ text: label[i], ...piece });
    }
    return out;
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="catcher" onclick={onClose}></div>
<div class="pop" role="dialog" aria-label={commands ? "Command Palette" : placeholder}>
  <div class="field">
    <svg class="icon" viewBox="0 0 16 16">
      {#if commands}<path d="M3.5 4.5 7 8l-3.5 3.5M8.5 11.5h4" />{:else}<path d="M8 3.5V8l3 2M8 1.8a6.2 6.2 0 1 0 0 12.4a6.2 6.2 0 1 0 0-12.4" />{/if}
    </svg>
    <input bind:this={input} bind:value={text} onkeydown={onKey} {placeholder} spellcheck="false" autocomplete="off" aria-label={placeholder} />
    {#if commands}<span class="keys">{shortcut("palette")}</span>{/if}
  </div>
  <div class="list" bind:this={listEl} role="listbox" aria-label={commands ? "Commands" : placeholder}>
    {#if !shown.length}
      <p class="note">{empty(query)}</p>
    {:else}
      {#each shown as row, i (row.item.id)}
        {#if commands && !query.trim() && recent.length && (i === 0 || row.recent !== shown[i - 1].recent)}
          <div class="heading">{row.recent ? "Recently used" : "Other commands"}</div>
        {/if}
        <button class="item" class:on={i === active} data-index={i} role="option" aria-selected={i === active} onmousemove={() => (active = i)} onclick={() => go(row.item)}>
          <span class="label">{#each pieces(row.item, row.hits) as piece, k (k)}<span class:dim={piece.dim} class:hit={piece.hit}>{piece.text}</span>{/each}{#if row.item.detail}<span class="detail">{row.item.detail}</span>{/if}</span>
          {#if row.item.keys}
            <span class="combo" title={keys(combo(row.item.keys))}>
              {#each keys(combo(row.item.keys)).split(" ") as part, k (k)}<kbd>{part}</kbd>{/each}
            </span>
          {/if}
        </button>
      {/each}
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
  .list {
    overflow-y: auto;
    padding: 6px;
    min-height: 0;
  }
  .heading {
    padding: 8px 10px 4px;
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    height: 30px;
    padding: 0 10px;
    border-radius: var(--radius-m);
    text-align: left;
  }
  .item.on {
    background: var(--side-sel);
  }
  .label {
    flex-grow: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dim {
    color: var(--text2);
  }
  .hit {
    font-weight: 700;
    color: var(--text);
  }
  .detail {
    margin-left: 8px;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .combo {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
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
  .note {
    margin: 0;
    padding: 10px 12px;
    font-size: var(--text-small);
    color: var(--text2);
  }
</style>
