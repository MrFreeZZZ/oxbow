<script lang="ts">
  // A Base or Compare picker: branches, remote branches and tags, or any SHA typed in.

  import type { History } from "./types";
  import { lane } from "./format";

  let {
    label,
    value,
    other,
    history,
    colorOf,
    onPick,
  }: {
    label: string;
    value: string;
    /** The other side's ref: shown, but it can't be picked here too. */
    other: string;
    history: History;
    colorOf: (name: string) => number;
    onPick: (name: string) => void;
  } = $props();

  let open = $state(false);
  let filter = $state("");
  let field = $state<HTMLInputElement>();

  const isTag = $derived(history.refs.some((r) => r.kind === "tag" && r.name === value));
  const groups = $derived.by(() => {
    const f = filter.trim().toLowerCase();
    const keep = (name: string) => !f || name.toLowerCase().includes(f);
    const local = history.refs.filter((r) => r.kind === "local").map((r) => r.name);
    const head = history.head.branch;
    const ordered = head && local.includes(head) ? [head, ...local.filter((n) => n !== head)] : local;
    return [
      { title: "Branches", kind: "local", names: ordered.filter(keep) },
      { title: "Remote", kind: "remote", names: history.refs.filter((r) => r.kind === "remote" && !r.name.endsWith("/HEAD")).map((r) => r.name).filter(keep) },
      { title: "Tags", kind: "tag", names: history.refs.filter((r) => r.kind === "tag").map((r) => r.name).filter(keep) },
    ].filter((g) => g.names.length);
  });
  /** What the field holds looks like a commit id, so Return compares that commit. */
  const typedSha = $derived(/^[0-9a-f]{4,40}$/i.test(filter.trim()) ? filter.trim() : null);

  function toggle() {
    open = !open;
    filter = "";
    if (open) requestAnimationFrame(() => field?.focus());
  }

  function pick(name: string) {
    open = false;
    if (name !== value) onPick(name);
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      open = false;
    } else if (event.key === "Enter") {
      event.preventDefault();
      const first = groups.flatMap((g) => g.names).find((n) => n !== other);
      if (typedSha) pick(typedSha);
      else if (first) pick(first);
    }
  }

  const note = (name: string) => (name === other ? "compared" : name === history.head.branch ? "HEAD" : name === history.trunk ? "default branch" : "");
</script>

<div class="picker">
  <button class="field" onclick={toggle} aria-haspopup="listbox" aria-expanded={open}>
    {#if isTag}
      <svg class="icon kind tag" viewBox="0 0 16 16"><path d="M2.5 2.5h5l6 6-5 5-6-6z" /><circle cx="5.5" cy="5.5" r="0.8" /></svg>
    {:else}
      <svg class="icon kind" viewBox="0 0 16 16" style:color={lane(colorOf(value))}><circle cx="4.5" cy="3.5" r="1.5" /><circle cx="4.5" cy="12.5" r="1.5" /><circle cx="11.5" cy="5.5" r="1.5" /><path d="M4.5 5v6M11.5 7c0 3-7 2-7 4" /></svg>
    {/if}
    <span class="text">
      <span class="label">{label}</span>
      <span class="value">{value}</span>
    </span>
    <svg class="icon chevron" viewBox="0 0 16 16"><path d="M4.5 6.5 8 10l3.5-3.5" /></svg>
  </button>
  {#if open}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="catcher" onclick={() => (open = false)}></div>
    <div class="pop">
      <div class="search">
        <svg class="icon" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5" /><path d="M10.3 10.3 14 14" /></svg>
        <input bind:this={field} bind:value={filter} onkeydown={onKey} placeholder="Branch, tag or commit SHA" spellcheck="false" autocomplete="off" aria-label="Branch, tag or commit SHA" />
      </div>
      <div class="list" role="listbox" aria-label={label}>
        {#if typedSha}
          <button class="row" role="option" aria-selected="false" onclick={() => pick(typedSha!)}>
            <span class="dot commit"></span>
            <span class="name mono">{typedSha}</span>
            <span class="note">commit</span>
          </button>
        {/if}
        {#each groups as g (g.title)}
          <div class="heading">{g.title}</div>
          {#each g.names as name (name)}
            <button class="row" class:on={name === value} role="option" aria-selected={name === value} disabled={name === other} onclick={() => pick(name)}>
              {#if g.kind === "tag"}
                <span class="dot diamond"></span>
              {:else}
                <span class="dot" style:background={lane(colorOf(name))}></span>
              {/if}
              <span class="name">{name}</span>
              <span class="note">{note(name)}</span>
              <span class="check">{name === value ? "✓" : ""}</span>
            </button>
          {/each}
        {/each}
        {#if !groups.length && !typedSha}
          <div class="empty">No branch or tag matches. Type a commit SHA to compare a commit.</div>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .picker {
    position: relative;
    display: flex;
    min-width: 0;
  }
  .field {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 210px;
    height: 40px;
    padding: 0 10px 0 12px;
    border-radius: 12px;
    background: var(--win);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    text-align: left;
  }
  .kind {
    flex-shrink: 0;
  }
  .kind.tag {
    color: var(--tag-border);
  }
  .text {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.2;
  }
  .label {
    font-size: 10px;
    color: var(--text2);
  }
  .value {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .chevron {
    width: 13px;
    height: 13px;
    color: var(--text2);
    flex-shrink: 0;
  }
  .catcher {
    position: fixed;
    inset: 0;
    z-index: 19;
  }
  .pop {
    position: absolute;
    left: 0;
    top: 46px;
    z-index: 20;
    width: 300px;
    padding: 6px;
    border-radius: 16px;
    background: var(--sheet);
    -webkit-backdrop-filter: blur(30px) saturate(1.8);
    backdrop-filter: blur(30px) saturate(1.8);
    border: 0.5px solid var(--glass-border);
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.18);
    display: flex;
    flex-direction: column;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    margin: 0 2px 4px;
    padding: 0 10px;
    border-radius: 9px;
    background: var(--field);
    color: var(--text2);
  }
  .search input {
    flex-grow: 1;
    min-width: 0;
    border: 0;
    outline: none;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 12px;
    user-select: text;
    -webkit-user-select: text;
  }
  .list {
    max-height: 360px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }
  .heading {
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
    padding: 6px 10px 3px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 26px;
    flex-shrink: 0;
    padding: 0 10px;
    border-radius: 8px;
    text-align: left;
  }
  .row:hover:not(:disabled),
  .row.on {
    background: var(--side-sel);
  }
  .row:disabled {
    opacity: 0.45;
  }
  .row.on .name {
    font-weight: 600;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 4px;
    flex-shrink: 0;
  }
  .dot.diamond {
    border-radius: 1px;
    transform: rotate(45deg);
    background: var(--tag-bg);
    box-shadow: inset 0 0 0 1px var(--tag-border);
  }
  .dot.commit {
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--text2);
  }
  .name {
    flex-grow: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .mono {
    font-family: var(--mono);
    font-size: 12px;
  }
  .note {
    font-size: 11px;
    color: var(--text2);
    white-space: nowrap;
  }
  .check {
    width: 12px;
    font-size: 12px;
    font-weight: 700;
  }
  .empty {
    padding: 10px;
    font-size: 12px;
    color: var(--text2);
  }
</style>
