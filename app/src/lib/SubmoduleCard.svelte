<script lang="ts">
  // In place of a diff: a submodule's recorded commit moved. Which way, the commits it gained or
  // dropped as far as its clone has them, and the two lines git itself shows for it.

  import { api } from "./api";
  import { confirm } from "./confirm.svelte";
  import { shortId, shortTime } from "./format";
  import type { Side, SubmoduleMove } from "./types";

  let {
    move,
    side = null,
  }: {
    move: SubmoduleMove;
    /** For the working copy: staged or not, which says what committing records. */
    side?: Side | null;
  } = $props();

  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
  const short = (id: string | null) => (id ? shortId(id) : "");

  const title = $derived.by(() => {
    const m = move;
    switch (m.moved) {
      case "forward":
        return `${m.title} moved forward ${plural(m.count, "commit")}`;
      case "back":
        return `${m.title} moved back ${plural(m.count, "commit")}`;
      case "sideways":
        return `${m.title} switched to another line of commits`;
      case "added":
        return `${m.title} was added at ${short(m.new)}`;
      case "removed":
        return `${m.title} was removed`;
      case "same":
        return m.changed ? `${m.title} has changes inside` : `${m.title} is at the commit recorded`;
      default:
        return `${m.title} moved to ${short(m.new)}`;
    }
  });

  const sub = $derived.by(() => {
    if (side === "unstaged" && move.moved !== "same" && move.new) return `Stage it and the next commit records ${short(move.new)}`;
    if (side === "staged" && move.new) return `The next commit records ${short(move.new)}`;
    return [move.url, move.follows ? `follows ${move.follows === "." ? "the branch here" : move.follows}` : null].filter(Boolean).join(" · ");
  });

  const listLabel = $derived(
    move.moved === "back"
      ? `${plural(move.count, "commit")} it no longer has`
      : move.moved === "sideways"
        ? `${plural(move.count, "new commit")}; ${plural(move.dropped, "commit")} of the old line left out`
        : null,
  );

  const why = $derived.by(() => {
    if (move.moved !== "unknown") return null;
    if (!move.cloned) return `${move.title} isn’t cloned here, so its commits can’t be listed. Update clones it.`;
    return `${move.title}’s clone doesn’t have one of these commits yet: open it and fetch.`;
  });

  function open() {
    api.openWindow(move.path).catch((err) => confirm.fail(err));
  }
</script>

<div class="card">
  <div class="top">
    <span class="tile"><svg class="icon" viewBox="0 0 16 16"><path d="M2.5 5 8 2l5.5 3v6L8 14l-5.5-3zM2.5 5 8 8l5.5-3M8 8v6" /></svg></span>
    <div class="words">
      <h3>{title}</h3>
      {#if sub}<p class="sub ellipsis" title={sub}>{sub}</p>{/if}
    </div>
    {#if move.old || move.new}
      <span class="shas mono">
        {#if move.old && move.moved !== "same"}<span>{short(move.old)}</span>{/if}
        {#if move.old && move.new && move.moved !== "same"}<svg class="arrow" viewBox="0 0 16 16"><path d="M3 8h10M9 4.5 12.5 8 9 11.5" /></svg>{/if}
        {#if move.new}<span class="new">{short(move.new)}</span>{/if}
      </span>
    {/if}
  </div>

  {#if move.commits.length}
    {#if listLabel}<p class="label">{listLabel}</p>{/if}
    <div class="commits" class:dropped={move.moved === "back"}>
      {#each move.commits as c (c.id)}
        <div class="commit">
          <span class="dot"></span>
          <span class="text">
            <span class="ellipsis">{c.summary}</span>
            <span class="meta">{c.authorName} · {shortTime(c.time)}</span>
          </span>
          <span class="sha mono">{short(c.id)}</span>
        </div>
      {/each}
      {#if move.count > move.commits.length}<div class="more">and {plural(move.count - move.commits.length, "more commit")}</div>{/if}
    </div>
  {/if}

  {#if why}
    <p class="note"><svg class="icon small" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" /><path d="M8 7.2v3.6M8 5.1v.1" /></svg>{why}</p>
  {/if}
  {#if move.unpushed && move.new}
    <p class="note warn">
      <svg class="icon small" viewBox="0 0 16 16"><path d="M8 2.5 14 13H2zM8 6.5v3M8 11.2v.1" /></svg>{short(move.new)} isn’t on {move.title}’s remote yet. Push it there before pushing this repository.
    </p>
  {/if}
  {#if move.changed}
    <p class="note">
      <svg class="icon small" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" /><path d="M8 7.2v3.6M8 5.1v.1" /></svg>{plural(move.changed, "file")} changed inside {move.title}. They are committed there, not here: open it as a repository.
    </p>
  {/if}

  <!-- In Changes the file's header has the button already. -->
  {#if move.cloned && move.moved !== "removed" && !side}
    <div class="actions">
      <button class="button" onclick={open}>Open in New Window</button>
    </div>
  {/if}

  {#if move.moved !== "same"}
    <div class="raw mono selectable">
      {#if move.old}<div class="del">-Subproject commit {move.old}</div>{/if}
      {#if move.new}<div class="add">+Subproject commit {move.new}</div>{/if}
    </div>
  {/if}
</div>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 8px 6px 4px;
  }
  .top {
    display: flex;
    gap: 12px;
    align-items: center;
  }
  .tile {
    width: 40px;
    height: 40px;
    border-radius: 10px;
    background: var(--field);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    color: var(--lane-5);
  }
  .tile .icon {
    width: 18px;
    height: 18px;
  }
  .words {
    flex-grow: 1;
    min-width: 0;
  }
  h3 {
    margin: 0 0 3px;
    font-size: 13px;
    font-weight: 600;
  }
  .sub {
    margin: 0;
    font-size: 11.5px;
    color: var(--text2);
  }
  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .shas {
    display: flex;
    align-items: center;
    gap: 5px;
    flex-shrink: 0;
    font-size: 11.5px;
  }
  .shas span {
    padding: 2px 7px;
    border-radius: 6px;
    background: var(--field);
    color: var(--text2);
  }
  .shas .new {
    color: var(--text);
    font-weight: 600;
  }
  .arrow {
    width: 12px;
    height: 12px;
    color: var(--text2);
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .label {
    margin: 0 0 -6px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
  }
  .commits {
    border: 1px solid var(--sep);
    border-radius: 10px;
    overflow: hidden;
  }
  .commit {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 7px 12px;
  }
  .commit + .commit {
    border-top: 1px solid var(--sep);
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    border: 2px solid var(--lane-1);
    flex-shrink: 0;
  }
  .dropped .dot {
    border-color: var(--lane-10);
  }
  .dropped .text > .ellipsis {
    color: var(--text2);
  }
  .text {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.3;
  }
  .meta {
    font-size: 11px;
    color: var(--text2);
  }
  .sha {
    font-size: 11px;
    color: var(--text2);
    flex-shrink: 0;
  }
  .more {
    padding: 6px 12px 7px 32px;
    border-top: 1px solid var(--sep);
    font-size: 11.5px;
    color: var(--text2);
  }
  .note {
    margin: 0;
    display: flex;
    gap: 7px;
    align-items: flex-start;
    font-size: 12px;
    line-height: 1.4;
    color: var(--text2);
  }
  .note.warn {
    color: var(--orange);
  }
  .small {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    margin-top: 1px;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .button {
    height: 28px;
    flex-shrink: 0;
    white-space: nowrap;
    padding: 0 14px;
    border-radius: 14px;
    border: 1px solid var(--sep);
    font-size: 12px;
  }
  .raw {
    padding: 8px 12px;
    border-radius: 9px;
    background: var(--field);
    font-size: 11px;
    line-height: 1.6;
    overflow-x: auto;
    white-space: nowrap;
  }
  .raw .del {
    color: var(--red);
  }
  .raw .add {
    color: var(--green);
  }
</style>
