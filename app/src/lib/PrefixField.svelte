<script lang="ts">
  // A branch's Name box with its prefix as a pop-up at the start, whose menu says what each prefix
  // is for. The name stays one string: a known prefix typed or pasted in the box takes the pop-up's
  // place, and Backspace at the start of the box takes the prefix off.

  import { tick } from "svelte";
  import { BRANCH_PREFIXES, splitPrefix, typedAfter, withPrefix } from "./branchPrefixes";

  let {
    value,
    placeholder,
    label,
    focus = false,
    onedit,
  }: { value: string; placeholder?: string; label: string; focus?: boolean; onedit: (value: string) => void } = $props();

  const parts = $derived(splitPrefix(value));
  let open = $state(false);
  let at = $state({ left: 0, top: 0 });
  let box = $state<HTMLDivElement>();
  let input = $state<HTMLInputElement>();
  let menu = $state<HTMLDivElement>();

  /** Puts the cursor in the box when it appears with `focus`. */
  function focusIf(node: HTMLInputElement, on: boolean) {
    if (on) requestAnimationFrame(() => node.focus());
  }

  const items = () => [...(menu?.querySelectorAll<HTMLButtonElement>("button") ?? [])];

  async function show() {
    if (!box) return;
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
    onedit(withPrefix(value, prefix));
    hide();
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
    if (event.key === "ArrowDown" && !event.metaKey && !event.ctrlKey) {
      event.preventDefault();
      void show();
    } else if (event.key === "Backspace" && parts.prefix && el.selectionStart === 0 && el.selectionEnd === 0) {
      event.preventDefault();
      onedit(parts.rest);
    }
  }

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

<div class="prefixed" bind:this={box}>
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
  <input
    class="text"
    bind:this={input}
    value={parts.rest}
    placeholder={placeholder === undefined ? undefined : splitPrefix(placeholder).rest}
    aria-label={label}
    spellcheck="false"
    autocomplete="off"
    use:focusIf={focus}
    onkeydown={onInputKey}
    oninput={(e) => onedit(typedAfter(parts.prefix, e.currentTarget.value))}
  />
</div>

{#if open}
  <div class="menu" role="menu" tabindex="-1" aria-label="Prefix" bind:this={menu} style:left="{at.left}px" style:top="{at.top}px" onkeydown={onMenuKey}>
    {#each BRANCH_PREFIXES as p (p.prefix)}
      <button role="menuitemradio" aria-checked={parts.prefix === p.prefix} onclick={() => pick(p.prefix)}>
        <span class="check">{#if parts.prefix === p.prefix}<svg viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg>{/if}</span>
        <span class="prefix">{p.prefix}</span>
        <span class="words">{p.words}</span>
      </button>
    {/each}
    <span class="sep"></span>
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
  .sep {
    height: 1px;
    margin: 5px 10px;
    background: var(--sep);
  }
</style>
