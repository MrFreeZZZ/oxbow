<script lang="ts">
  import type { Changelist, FileChange } from "./types";
  import type { ListKey } from "./changelists";
  import type { ChangesGroupEvents } from "./changesList";
  import { keysTo } from "./changesList";
  import { plate, tint, splitPath } from "./format";
  import ChangesCheckbox from "./ChangesCheckbox.svelte";

  let { group, files, total, color, needle, expanded, selected, ticked, picked,
    changelist = null, active = false, foldDisabled = false, events }: {
    group: ListKey; files: FileChange[]; total: number; color: number; needle: string;
    expanded: boolean; selected: boolean; ticked: Set<string>; picked: FileChange[];
    changelist?: Changelist | null; active?: boolean; foldDisabled?: boolean; events: ChangesGroupEvents;
  } = $props();
  const n = $derived(files.filter((f) => ticked.has(f.path)).length);
  const all = $derived(n > 0 && n === files.length);
  const name = $derived(changelist?.name ?? (group === "staged" ? "Staged" : group === "untracked" ? "Untracked" : "Unstaged"));
  const verb = $derived(group === "staged" ? "Unstage" : "Stage");
  const foldLabel = $derived(changelist ? `${expanded ? "Hide" : "Show"} the files of ${name}` :
    group === "untracked" && !expanded ? "Show the new files git does not track yet" : `${expanded ? "Hide" : "Show"} the ${group} files`);
  const showTitle = $derived(changelist ? (!selected ? `Show every change of ${name}` : expanded ? "Hide its files" : "Show its files") :
    !selected ? (group === "staged" ? "Show the next commit: every staged change" : group === "untracked" ? "Show every new file git does not track yet" : "Show every unstaged change") :
    `${expanded ? "Hide" : "Show"} the ${group} files`);
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="ghead" class:on={selected} style:background={selected ? tint(color) : undefined}
  data-group={group} oncontextmenu={events.menu}>
  <ChangesCheckbox on={all} some={n > 0 && !all} placement="heading"
    label={changelist ? `Select every file of ${changelist.name}` : `Select every ${group} file`}
    title={all ? "Select none" : "Select all"} disabled={!files.length}
    onclick={(e) => { keysTo(e, ".gname"); events.selectAll(); }} />
  <button class="chev" aria-expanded={expanded} aria-label={foldLabel}
    title={foldDisabled ? "Shown while the filter is on" : foldLabel} disabled={foldDisabled} onclick={events.toggle}>
    <svg class="chevron" class:open={expanded} viewBox="0 0 16 16"><path d="M6 4l4 4-4 4" /></svg>
  </button>
  <button class="gname" onkeydown={events.keyDown} onkeyup={events.keyUp} onclick={events.open}
    aria-current={selected ? "true" : undefined} aria-expanded={expanded} title={showTitle}>
    {#if changelist}<svg class="icon clmark" style:color={plate(color)} viewBox="0 0 16 16"><path d="M3 3.5h10v9.5l-5-2.6L3 13z" /></svg>{/if}
    <span class="gtitle" class:cltitle={!!changelist} style:color={plate(color)}>{name}</span><span class="n">{needle ? `${files.length} of ${total}` : total}</span>
    {#if changelist && active}<span class="active" title="New changes go here: files that start changing join this list">Active</span>{/if}
  </button>
  {#if picked.length}
    {#if group !== "staged"}
      {@const fresh = picked.every((f) => f.status === "untracked")}
      {@const what = picked.length === 1 ? splitPath(picked[0].path).name : `${picked.length} files`}
      <button class="discard held" onclick={() => events.discard?.()} aria-label={fresh ? `Delete ${what}` : `Discard changes in ${what}`} title={fresh ? `Delete ${what}…` : `Discard changes in ${what}…`}>
        <svg class="icon" viewBox="0 0 16 16"><path d="M2.5 4.5h11M6 4.5V3a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1v1.5M4 4.5l.7 9a1 1 0 0 0 1 .9h4.6a1 1 0 0 0 1-.9l.7-9" /></svg>
      </button>
    {/if}
    <button class="button mini primary" onclick={events.picked}>
      {#if picked.length === 1}{verb} File{:else}{verb} {picked.length}<span class="word"> Files</span>{/if}
    </button>
  {:else if changelist ? total > 0 : files.length > 0}
    <button class="button mini" onclick={events.all} title={changelist ? `Commit the changes of ${name}, and only those` : undefined}>
      {changelist ? "Commit…" : needle && files.length < total ? `${verb} Shown` : `${verb} All`}
    </button>
  {/if}
  {#if changelist}
    <button class="button icon-only mini" onclick={events.menu} aria-label="{name} menu" title="Stage, stash, rename or delete {name}">
      <svg class="icon" viewBox="0 0 16 16"><path d="M3.5 8h.01M8 8h.01M12.5 8h.01" /></svg>
    </button>
  {/if}
</div>

<style>
  .ghead {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 30px;
    padding: 0 var(--space-1) 0 2px;
    border-radius: var(--radius-m);
    font-size: var(--text-caption);
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text2);
    flex-shrink: 0;
  }
  .gname {
    flex-grow: 1;
    min-width: 0;
    height: 100%;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 4px;
    font: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    color: inherit;
    text-align: left;
  }
  .gname:disabled {
    opacity: 1;
    cursor: default;
  }
  .chev {
    width: 18px;
    height: 22px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text2);
    flex-shrink: 0;
  }
  .chev:disabled {
    opacity: 1;
    cursor: default;
  }
  .gtitle {
    font-weight: 700;
  }
  .cltitle {
    min-width: 2em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-small);
    letter-spacing: normal;
    text-transform: none;
  }
  .clmark {
    width: 13px;
    height: 13px;
    flex-shrink: 0;
  }
  .active {
    display: inline-flex;
    align-items: center;
    height: 16px;
    padding: 0 6px;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: normal;
    text-transform: none;
    color: var(--accent-ink);
    background: var(--accent-quiet);
    flex-shrink: 0;
  }
  .ghead .n,
  .ghead .button {
    letter-spacing: normal;
    text-transform: none;
  }
  .ghead .button {
    flex-shrink: 0;
    white-space: nowrap;
  }
  .ghead {
    container-type: inline-size;
  }
  .n {
    font-weight: 400;
  }
  .ghead.on {
    box-shadow: inset 0 0 0 1px var(--selection-border);
  }
  .ghead {
    position: relative;
  }
  .gname:focus-visible,
  .gname:global(.key-focus) {
    outline: none;
  }
  .gname:focus-visible::after,
  .gname:global(.key-focus)::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: var(--radius-s);
    outline: 2px solid var(--accent);
    outline-offset: -2px;
    pointer-events: none;
  }
  .gname:focus-visible::after,
  .gname:global(.key-focus)::after {
    border-radius: var(--radius-m);
  }
  .ghead.on .gname::after {
    content: none;
  }
  .chevron {
    width: 10px;
    height: 10px;
    flex-shrink: 0;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
    transition: transform 0.15s;
  }
  .chevron.open {
    transform: rotate(90deg);
  }
  .discard {
    position: absolute;
    right: 2px;
    top: 2px;
    width: 24px;
    height: 24px;
    border-radius: var(--radius-pill);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text2);
    opacity: 0;
  }
  .discard .icon {
    width: 14px;
    height: 14px;
  }
  .discard:focus-visible {
    opacity: 1;
    background: var(--win);
  }
  .discard:hover {
    color: var(--red);
  }
  .discard.held {
    position: static;
    flex-shrink: 0;
    margin: 0 2px 0 4px;
    opacity: 1;
  }
  .discard.held:hover,
  .discard.held:focus-visible {
    background: var(--win);
  }
  @container (max-width: 300px) {
    .ghead .word {
      display: none;
    }
    /* No list mark, and Active as a dot: the list's name keeps the room. */
    .clmark {
      display: none;
    }
    .active {
      width: 7px;
      height: 7px;
      padding: 0;
      font-size: 0;
      background: var(--accent);
    }
  }

  :global(:root[data-contrast="more"]) .gname:focus-visible::after { outline-width: 3px; }
</style>
