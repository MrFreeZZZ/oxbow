<script lang="ts">
  // A branch's Name box with its prefix as a pop-up at the start, whose menu says what each prefix
  // is for. The name stays one string: a known prefix typed or pasted in the box takes the pop-up's
  // place, and Backspace at the start of the box takes the prefix off. What is typed is cleaned
  // up as Settings › Git › Branches says.

  import { onDestroy, tick } from "svelte";
  import { onlyPrefix, splitPrefix, typedAfter, withPrefix, type BranchPrefix } from "./branchPrefixes";
  import { cleanTyped, rememberPrefix } from "./branchPrefs";

  import type { RefInfo } from "./types";

  const id = $props.id();

  let {
    value,
    list = [],
    branchRefs,
    placeholder,
    label,
    focus = false,
    onedit,
  }: { value: string; list?: BranchPrefix[]; branchRefs?: RefInfo[]; placeholder?: string; label: string; focus?: boolean; onedit: (value: string) => void } = $props();

  const parts = $derived(splitPrefix(value, list));
  const own = $derived(list.filter((p) => !p.repo));
  const here = $derived(list.filter((p) => p.repo));
  let open = $state(false);
  let at = $state({ left: 0, top: 0 });
  let box = $state<HTMLDivElement>();
  let input = $state<HTMLInputElement>();
  let menu = $state<HTMLDivElement>();

  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let matches = $state.raw<RefInfo[]>([]);
  let active = $state(-1);
  let results = $state<HTMLDivElement>();
  let placement = $state({ left: 0, top: 0, width: 0, maxHeight: 0 });

  function dismissMatches() {
    clearTimeout(searchTimer);
    matches = [];
    active = -1;
  }

  onDestroy(() => clearTimeout(searchTimer));

  function placeMatches() {
    if (!box) return;
    const rect = box.getBoundingClientRect();
    const top = rect.bottom + 6;
    placement = { left: rect.left, top, width: rect.width, maxHeight: Math.max(0, window.innerHeight - top - 8) };
  }

  function search(name: string, rest: string) {
    dismissMatches();
    if (!branchRefs || onlyPrefix(name.trim()) || !rest.trim()) return;
    const query = rest.trim().toLowerCase();
    searchTimer = setTimeout(() => {
      if (document.activeElement !== input) return;
      placeMatches();
      matches = (branchRefs ?? [])
        .filter((ref) => (ref.kind === "local" || ref.kind === "remote") && ref.name.toLowerCase().includes(query))
        .sort((a, b) => a.name.localeCompare(b.name));
    }, 150);
  }

  async function pickBranch(ref: RefInfo) {
    dismissMatches();
    const name = ref.kind === "remote" && ref.remote ? ref.name.slice(ref.remote.length + 1) : ref.name;
    onedit(name);
    await tick();
    if (!input) return;
    input.focus();
    const end = input.value.length;
    input.setSelectionRange(end, end);
  }

  /** Puts the cursor in the box when it appears with `focus`. */
  function focusIf(node: HTMLInputElement, on: boolean) {
    if (on) requestAnimationFrame(() => node.focus());
  }

  const items = () => [...(menu?.querySelectorAll<HTMLButtonElement>("button") ?? [])];

  async function show() {
    if (!box) return;
    dismissMatches();
    open = true;
    await tick();
    const r = box.getBoundingClientRect();
    const height = menu?.offsetHeight ?? 0;
    // Under the box, or over it when the window has no room below.
    const below = r.bottom + 6 + height <= window.innerHeight - 8;
    at = { left: r.left, top: below ? r.bottom + 6 : Math.max(8, r.top - 6 - height) };
    const all = items();
    (all.find((b) => b.getAttribute("aria-checked") === "true") ?? all[0])?.focus();
  }

  function hide() {
    open = false;
    input?.focus();
  }

  function pick(prefix: string | null) {
    rememberPrefix(prefix);
    onedit(withPrefix(value, prefix, list));
    hide();
  }

  function onInput(event: Event) {
    const el = event.currentTarget as HTMLInputElement;
    const name = list.length ? typedAfter(parts.prefix, cleanTyped(el.value), list) : el.value;
    const split = splitPrefix(name, list);
    if (split.prefix && split.prefix !== parts.prefix) rememberPrefix(split.prefix);
    // The box shows the cleaned name at once, even when the name itself stays the same.
    if (el.value !== split.rest) {
      const caret = Math.max(0, (el.selectionStart ?? el.value.length) - (el.value.length - split.rest.length));
      el.value = split.rest;
      el.setSelectionRange(caret, caret);
    }
    onedit(name);
    if (branchRefs) search(name, split.rest);
  }

  // Escape and Tab close only the menu, not the sheet; Return and Space pick the focused item.
  function onMenuKey(event: KeyboardEvent) {
    const all = items();
    const here = all.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === "ArrowDown") all[(here + 1) % all.length]?.focus();
    else if (event.key === "ArrowUp") all[(here - 1 + all.length) % all.length]?.focus();
    else if (event.key === "Home") all[0]?.focus();
    else if (event.key === "End") all[all.length - 1]?.focus();
    else if (event.key === "Escape" || event.key === "Tab" || event.code === "Tab") hide();
    else return;
    event.preventDefault();
    event.stopPropagation();
  }

  function onInputKey(event: KeyboardEvent) {
    const el = event.currentTarget as HTMLInputElement;
    if (branchRefs && (event.key === "Tab" || event.code === "Tab")) dismissMatches();
    if (matches.length && event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      dismissMatches();
    } else if (matches.length && (event.key === "ArrowDown" || event.key === "ArrowUp") && !event.altKey && !event.metaKey && !event.ctrlKey) {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      active = active < 0 && step < 0 ? matches.length - 1 : (active + step + matches.length) % matches.length;
      results?.children[active]?.scrollIntoView({ block: "nearest" });
    } else if (active >= 0 && event.key === "Enter" && !event.metaKey && !event.ctrlKey) {
      event.preventDefault();
      event.stopPropagation();
      void pickBranch(matches[active]);
    } else if (list.length && event.key === "ArrowDown" && !event.metaKey && !event.ctrlKey) {
      event.preventDefault();
      void show();
    } else if (event.key === "Backspace" && parts.prefix && el.selectionStart === 0 && el.selectionEnd === 0) {
      event.preventDefault();
      onedit(parts.rest);
      if (branchRefs) search(parts.rest, parts.rest);
    }
  }

  $effect(() => {
    if (!branchRefs) return;
    const away = (event: Event) => {
      if (event.target instanceof Node && results?.parentElement?.contains(event.target)) return;
      if (event.type === "pointerdown" && event.target instanceof Node && box?.contains(event.target)) return;
      dismissMatches();
    };
    window.addEventListener("pointerdown", away, true);
    window.addEventListener("scroll", away, true);
    window.addEventListener("resize", placeMatches);
    return () => {
      window.removeEventListener("pointerdown", away, true);
      window.removeEventListener("scroll", away, true);
      window.removeEventListener("resize", placeMatches);
    };
  });

  // The command preview can resize and recenter the sheet after the results open.
  $effect(() => {
    if (!matches.length || !box) return;
    const observer = new ResizeObserver(placeMatches);
    observer.observe(box);
    const sheet = box.closest('[role="alertdialog"]');
    if (sheet) observer.observe(sheet);
    return () => observer.disconnect();
  });

  // A click anywhere else, or the sheet scrolling under it, closes the menu.
  $effect(() => {
    if (!open) return;
    const away = (event: Event) => {
      const target = event.target as Node;
      if (!menu?.contains(target) && !box?.contains(target)) open = false;
    };
    window.addEventListener("pointerdown", away, true);
    window.addEventListener("scroll", away, true);
    return () => {
      window.removeEventListener("pointerdown", away, true);
      window.removeEventListener("scroll", away, true);
    };
  });
</script>

<div class="prefixed" class:plain={!list.length} bind:this={box}>
  {#if list.length}
    <button
      class="type"
      class:none={!parts.prefix}
      class:open
      aria-haspopup="menu"
      aria-expanded={open}
      title="What the branch is for"
      onclick={() => (open ? hide() : show())}
    >
      {parts.prefix ?? "No prefix"}
      <svg viewBox="0 0 10 10"><path d="M2.5 4l2.5 2.5L7.5 4" /></svg>
    </button>
  {/if}
  <input
    class="text"
    bind:this={input}
    value={parts.rest}
    placeholder={placeholder === undefined ? undefined : splitPrefix(placeholder, list).rest}
    aria-label={label}
    role={branchRefs ? "combobox" : undefined}
    aria-autocomplete={branchRefs ? "list" : undefined}
    aria-expanded={branchRefs ? matches.length > 0 : undefined}
    aria-controls={matches.length ? `${id}-branches` : undefined}
    aria-activedescendant={active >= 0 ? `${id}-branch-${active}` : undefined}
    spellcheck="false"
    autocomplete="off"
    use:focusIf={focus}
    onkeydown={onInputKey}
    oninput={onInput}
  />
</div>

{#if matches.length}
  <div
    class="menu suggestions"
    style:left="{placement.left}px"
    style:top="{placement.top}px"
    style:width="{placement.width}px"
    style:max-height="{placement.maxHeight}px"
  >
    <span class="head" id="{id}-branches-title" role="status">Existing branches ({matches.length})</span>
    <div class="matches" role="listbox" id="{id}-branches" aria-labelledby="{id}-branches-title" bind:this={results}>
      {#each matches as ref, i}
        <button
          role="option"
          tabindex="-1"
          id="{id}-branch-{i}"
          aria-selected={i === active}
          title={ref.name}
          onmousedown={(event) => event.preventDefault()}
          onclick={() => pickBranch(ref)}
        >
          <span class="branch-name">{ref.name}</span>
          <span class="words">{ref.kind === "local" ? "Local" : "Remote"}</span>
        </button>
      {/each}
    </div>
  </div>
{/if}

{#if open}
  <div class="menu" role="menu" tabindex="-1" aria-label="Prefix" bind:this={menu} style:left="{at.left}px" style:top="{at.top}px" onkeydown={onMenuKey}>
    {#snippet item(p: BranchPrefix)}
      <button role="menuitemradio" aria-checked={parts.prefix === p.prefix} onclick={() => pick(p.prefix)}>
        <span class="check">{#if parts.prefix === p.prefix}<svg viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg>{/if}</span>
        <span class="prefix">{p.prefix}</span>
        <span class="words">{p.words}</span>
      </button>
    {/snippet}
    {#each own as p (p.prefix)}{@render item(p)}{/each}
    {#if here.length}
      {#if own.length}<span class="sep"></span>{/if}
      <span class="head">In this repository</span>
      {#each here as p (p.prefix)}{@render item(p)}{/each}
    {/if}
    {#if list.length}<span class="sep"></span>{/if}
    <button role="menuitemradio" aria-checked={!parts.prefix} onclick={() => pick(null)}>
      <span class="check">{#if !parts.prefix}<svg viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg>{/if}</span>
      <span>No prefix</span>
    </button>
  </div>
{/if}

<style>
  .prefixed {
    display: flex;
    align-items: center;
    height: 28px;
    border-radius: var(--radius-m);
    background: var(--win);
    border: 1px solid var(--sep);
  }
  .prefixed:focus-within {
    border-color: var(--accent);
    box-shadow: var(--focus-ring);
  }
  .plain .text {
    padding-inline: 10px;
  }
  .type {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
    height: 22px;
    margin-left: 2px;
    padding: 0 6px 0 8px;
    border-radius: var(--radius-s);
    background: var(--field);
    font-family: var(--mono);
    font-size: var(--text-small);
    white-space: nowrap;
  }
  .type.none {
    font-family: var(--font);
    color: var(--text2);
  }
  .type.open {
    background: var(--accent-quiet);
    color: var(--accent-ink);
  }
  .type svg {
    width: 10px;
    height: 10px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
    stroke-linecap: round;
    stroke-linejoin: round;
    color: var(--text2);
  }
  .text {
    flex-grow: 1;
    min-width: 0;
    height: 100%;
    padding: 0 8px;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--text-body);
    outline: none;
    user-select: text;
    -webkit-user-select: text;
  }
  /* The ring goes round the whole box, prefix and all. */
  .text:focus-visible {
    box-shadow: none;
  }
  .menu {
    position: fixed;
    z-index: 50;
    width: max-content;
    min-width: 300px;
    padding: 5px;
    border-radius: var(--radius-xl);
    background: var(--sheet);
    -webkit-backdrop-filter: blur(30px) saturate(1.8);
    backdrop-filter: blur(30px) saturate(1.8);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--shadow-pop);
    display: flex;
    flex-direction: column;
    outline: none;
  }
  .menu button {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 26px;
    padding: 0 10px 0 6px;
    border-radius: var(--radius-m);
    color: var(--text);
    text-align: left;
    white-space: nowrap;
    outline: none;
  }
  .menu button:hover,
  .menu button:focus-visible,
  .menu button:focus {
    background: var(--side-sel);
  }
  .suggestions {
    min-width: 0;
  }
  .suggestions .head {
    flex-shrink: 0;
    padding-inline: 6px;
  }
  .matches {
    max-height: calc(5 * 26px);
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  .matches button {
    width: 100%;
    justify-content: space-between;
  }
  .matches .words {
    flex-shrink: 0;
  }
  .matches button[aria-selected="true"] {
    background: var(--side-sel);
  }
  .branch-name {
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: var(--mono);
    font-size: var(--text-small);
  }
  .check {
    width: 14px;
    flex-shrink: 0;
    display: flex;
    color: var(--accent);
  }
  .check svg {
    width: 13px;
    height: 13px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .prefix {
    width: 80px;
    flex-shrink: 0;
    font-family: var(--mono);
    font-size: var(--text-small);
  }
  .menu button[aria-checked="true"] .prefix {
    font-weight: 600;
  }
  .words {
    font-size: var(--text-small);
    color: var(--text2);
  }
  .head {
    height: 24px;
    padding: 0 10px 0 28px;
    font-size: var(--text-caption);
    font-weight: 600;
    line-height: 24px;
    color: var(--text2);
  }
  .sep {
    height: 1px;
    margin: 5px 10px;
    background: var(--sep);
  }
</style>
