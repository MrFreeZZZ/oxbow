<script lang="ts">
  // The toolbar's branch button: a list of local branches, and picking one checks it out.

  import type { History } from "./types";
  import { lane, NO_BRANCH_COLOR, shortId, tint } from "./format";
  import { onNoBranch } from "./branches";

  let {
    history,
    colorOf,
    onPick,
  }: {
    history: History;
    colorOf: (name: string) => number;
    onPick: (branch: string) => void;
  } = $props();

  let open = $state(false);
  let query = $state("");
  let field = $state<HTMLInputElement>();
  let active = $state(0);

  // A rebase detaches HEAD, but the branch it is rebasing is the one the user is on.
  const head = $derived(history.head.branch ?? (history.operation?.kind === "rebase" ? history.operation.branch : null));
  /** While an operation is in progress, checking out another branch is refused by git. */
  const busy = $derived(history.operation ? `Finish or abort the ${history.operation.kind === "cherryPick" ? "cherry-pick" : history.operation.kind} first` : null);
  const lost = $derived(onNoBranch(history).length);
  /** While detached: the tag HEAD is at, unless commits were made there since. */
  const detachedAt = $derived.by(() => {
    const commit = history.head.commit;
    if (head || !commit) return null;
    const tag = lost ? null : history.refs.find((r) => r.kind === "tag" && r.target === commit)?.name;
    return tag ?? shortId(commit);
  });
  const time = $derived(new Map(history.rows.map((r) => [r.id, r.time])));
  /** The checked-out branch first, then the rest by latest commit, like the sidebar. */
  const branches = $derived(
    history.refs
      .filter((r) => r.kind === "local")
      .sort((a, b) => Number(b.name === head) - Number(a.name === head) || (time.get(b.target) ?? 0) - (time.get(a.target) ?? 0) || a.name.localeCompare(b.name)),
  );
  const shown = $derived(query.trim() ? branches.filter((b) => b.name.toLowerCase().includes(query.trim().toLowerCase())) : branches);

  function toggle() {
    if (busy) return;
    open = !open;
    query = "";
    active = 0;
    if (open) requestAnimationFrame(() => field?.focus());
  }

  function pick(name: string) {
    open = false;
    if (name !== head) onPick(name);
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      open = false;
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      active = (active + step + shown.length) % Math.max(shown.length, 1);
    } else if (event.key === "Enter" && shown[active]) {
      event.preventDefault();
      pick(shown[active].name);
    }
  }
</script>

<div class="picker">
  <button class="capsule" class:off={!!busy} onclick={toggle} aria-haspopup="menu" aria-expanded={open} title={busy ?? (open ? undefined : "Check out a branch")}>
    <svg class="icon" viewBox="0 0 16 16" style:color={head ? lane(colorOf(head)) : lane(NO_BRANCH_COLOR)}
      ><circle cx="4.5" cy="3.5" r="1.5" /><circle cx="4.5" cy="12.5" r="1.5" /><circle cx="11.5" cy="5.5" r="1.5" /><path d="M4.5 5v6M11.5 7c0 3-7 2-7 4" /></svg
    >
    <span>{head ?? (detachedAt ? `Detached at ${detachedAt}` : "No commits")}</span>
    {#if !busy}<svg class="icon small" viewBox="0 0 16 16"><path d="M4 6l4 4 4-4" /></svg>{/if}
  </button>
  {#if open}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="catcher" onclick={() => (open = false)}></div>
    <div class="pop" role="menu" aria-label="Check out branch">
      <label class="search">
        <svg class="icon small" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5" /><path d="m10.5 10.5 3 3" /></svg>
        <input bind:this={field} bind:value={query} oninput={() => (active = 0)} onkeydown={onKey} placeholder="Switch to branch…" spellcheck="false" autocomplete="off" />
      </label>
      <div class="heading">Local branches</div>
      <div class="list">
        {#each shown as b, i (b.name)}
          {@const color = colorOf(b.name)}
          {@const current = b.name === head}
          <button
            role="menuitemradio"
            aria-checked={current}
            style:background={current ? tint(color) : i === active && query ? "var(--side-sel)" : undefined}
            onclick={() => pick(b.name)}
            onpointerenter={() => (active = i)}
          >
            <span class="dot" style:background={lane(color)}></span>
            <span class="name" class:bold={current}>{b.name}</span>
            {#if b.tracking?.ahead}<span class="note">↑{b.tracking.ahead}</span>{/if}
            {#if b.tracking?.gone}<span class="note">gone</span>{/if}
            <span class="check" style:color={lane(color)}>{current ? "✓" : ""}</span>
          </button>
        {:else}
          <div class="empty">No branch matches “{query.trim()}”.</div>
        {/each}
      </div>
      <div class="sep"></div>
      <div class="foot">
        {lost
          ? `The ${lost} ${lost === 1 ? "commit" : "commits"} on no branch stay behind when you switch. Create a branch first to keep them.`
          : "Uncommitted changes move with you to the branch you check out."}
      </div>
    </div>
  {/if}
</div>

<style>
  .picker {
    position: relative;
    display: flex;
  }
  .capsule {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 34px;
    padding: 0 12px;
    border-radius: 17px;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    color: var(--icon);
    font-weight: 500;
  }
  .capsule span {
    color: var(--text);
  }
  .small {
    width: 14px;
    height: 14px;
    color: var(--text2);
  }
  .catcher {
    position: fixed;
    inset: 0;
    z-index: 19;
  }
  .pop {
    position: absolute;
    left: 0;
    top: 40px;
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
  .heading {
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
    padding: 6px 10px 4px;
  }
  .list {
    max-height: 340px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }
  .list button {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 28px;
    flex-shrink: 0;
    padding: 0 10px;
    border-radius: 8px;
  }
  .list button:hover {
    background: var(--side-sel);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 4px;
    flex-shrink: 0;
  }
  .name {
    flex-grow: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .bold {
    font-weight: 600;
  }
  .note {
    font-size: 11px;
    color: var(--text2);
  }
  .check {
    width: 14px;
    font-size: 12px;
    font-weight: 700;
  }
  .empty {
    padding: 6px 10px;
    font-size: 12px;
    color: var(--text2);
  }
  .sep {
    height: 1px;
    margin: 6px 8px;
    background: var(--sep);
  }
  .foot {
    font-size: 11.5px;
    color: var(--text2);
    padding: 2px 10px 6px;
    line-height: 1.4;
  }
  .capsule.off {
    opacity: 0.75;
  }
</style>
