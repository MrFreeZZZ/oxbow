<script lang="ts">
  // How the second version of a commit differs from the first, from `git range-diff`: the old
  // version's change on the left, the new one's on the right. A line's tint is what the commit
  // does to it, as in any diff; the Steel bar marks the lines the two versions don't share.

  import type { InterLine, InterSection } from "./types";
  import { interRows, sameFiles, wordDiff, type WordPiece } from "./series";
  import { splitPath } from "./format";

  let {
    sections,
    before,
    after,
  }: {
    sections: InterSection[];
    /** The column heads: which commit each side is. */
    before: string;
    after: string;
  } = $props();

  const shown = $derived(sections.filter((s) => s.changed || s.kind === "message"));
  const same = $derived(sameFiles(sections));

  /** The words of each side of a row, the ones the other side lacks marked. */
  function pieces(left: InterLine | null, right: InterLine | null): [WordPiece[], WordPiece[]] {
    const whole = (line: InterLine | null): WordPiece[] => (line ? [{ text: line.text || " ", changed: false }] : []);
    if (!left || !right || left === right || left.outer === "both") return [whole(left), whole(right)];
    return wordDiff(left.text, right.text);
  }

  const title = (s: InterSection) => (s.kind === "message" ? "Message" : s.kind === "metadata" ? "Author" : s.title);
  /** git's note on a file's section, e.g. "(new)", kept apart from its path. */
  const note = (s: InterSection) => /\s(\([^)]*\))$/.exec(s.title)?.[1] ?? "";
  const path = (s: InterSection) => s.title.replace(/\s\([^)]*\)$/, "");
</script>

{#snippet half(line: InterLine | null, words: WordPiece[], prose: boolean)}
  <div class="half {line ? `k-${line.inner}` : 'empty'}" class:differs={line && line.outer !== "both"} class:prose>
    {#if line}<span class="text">{#each words as w, i (i)}<span class:word={w.changed}>{w.text}</span>{/each}</span>{/if}
  </div>
{/snippet}

<div class="inter">
  <div class="cols" aria-hidden="true">
    <span>Before <span class="mono">{before}</span></span>
    <span>After <span class="mono">{after}</span></span>
  </div>
  {#each shown as s, i (i)}
    {@const prose = s.kind !== "file"}
    <section class="card" aria-label={title(s)}>
      <div class="card-head">
        {#if s.kind === "file"}
          {@const parts = splitPath(path(s))}
          <svg class="icon" viewBox="0 0 16 16"><path d="M4 2.5h5.5L12 5v8.5H4z" /></svg>
          <span class="path"><span class="dir">{parts.dir}</span><b>{parts.name}</b></span>
          {#if note(s)}<span class="note">{note(s).slice(1, -1)}</span>{/if}
        {:else}
          <svg class="icon" viewBox="0 0 16 16"
            ><path d={s.kind === "message" ? "M2.5 3.5h11v7h-6l-3 2.5v-2.5h-2z" : "M8 8a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5zM3 13.5c.6-2.4 2.6-3.7 5-3.7s4.4 1.3 5 3.7"} /></svg
          >
          <b class="path">{title(s)}</b>
        {/if}
        <span class="grow"></span>
        {#if !s.changed}<span class="note">Same in both</span>{/if}
      </div>
      <div class="rows" class:code={!prose}>
        {#each interRows(s.lines) as row, j (j)}
          {#if row.kind === "across"}
            <div class="across">{row.line.inner === "gap" ? "⋯" : `⋯ ${row.line.text}`}</div>
          {:else}
            {@const [left, right] = pieces(row.left, row.right)}
            <div class="row">
              {@render half(row.left, left, prose)}
              {@render half(row.right, right, prose)}
            </div>
          {/if}
        {/each}
      </div>
    </section>
  {/each}
  {#if same.length}
    <p class="same">
      Same change in both versions: {same.map((t) => t.replace(/\s\([^)]*\)$/, "")).join(", ")}
    </p>
  {/if}
  <div class="legend">
    <span><i class="key bar"></i>Differs between the versions</span>
    <span><i class="key add"></i>Added by the commit</span>
    <span><i class="key del"></i>Removed by the commit</span>
  </div>
</div>

<style>
  .inter {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: 0 var(--space-4) var(--space-4);
  }
  /* The two column heads stay as the sections scroll under them. */
  .cols {
    position: sticky;
    top: 0;
    z-index: 1;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    column-gap: 4px;
    padding: var(--space-2) 0 6px;
    background: var(--win);
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
  }
  .cols > span {
    padding-left: 12px;
  }
  .mono {
    font-family: var(--mono);
    font-weight: 400;
    margin-left: 4px;
  }
  .card {
    border: 1px solid var(--sep);
    border-radius: var(--radius-l);
    overflow: hidden;
    background: var(--code-bg, var(--win));
  }
  .card-head {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 12px;
    background: var(--field);
    font-size: var(--text-small);
  }
  .card-head .icon {
    width: 14px;
    height: 14px;
    color: var(--icon);
    flex-shrink: 0;
  }
  .path {
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dir {
    color: var(--text2);
  }
  .note {
    flex-shrink: 0;
    padding: 1px 8px;
    border-radius: var(--radius-pill);
    background: var(--win);
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .grow {
    flex-grow: 1;
  }
  .rows {
    padding: 4px 0;
    font-size: var(--text-small);
    line-height: 20px;
  }
  .rows.code {
    font-family: var(--code-font);
    font-size: var(--code-size);
    line-height: var(--code-line);
    color: var(--code-fg, var(--code));
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    column-gap: 4px;
  }
  .half {
    position: relative;
    padding: 0 10px 0 12px;
    min-width: 0;
  }
  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .half.k-added {
    background: var(--add);
  }
  .half.k-removed {
    background: var(--del);
  }
  .half.k-hunk,
  .half.k-text:not(.prose) {
    color: var(--text2);
  }
  /* No line on this side: soft hatching, as in Side by Side. */
  .half.empty {
    background: repeating-linear-gradient(-45deg, transparent 0 6px, color-mix(in srgb, var(--text2) 10%, transparent) 6px 7px);
  }
  /* A line only one version has: a Steel bar at its edge, its own words in a soft capsule. */
  .half.differs::before {
    content: "";
    position: absolute;
    left: 3px;
    top: 2px;
    bottom: 2px;
    width: 3px;
    border-radius: var(--radius-pill);
    background: var(--accent);
  }
  .half.differs.prose,
  .half.differs.k-context {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  .word {
    border-radius: var(--radius-s);
    padding: 1px 0;
    background: color-mix(in srgb, var(--accent) 22%, transparent);
  }
  .across {
    margin: 4px 0;
    padding: 0 12px;
    height: 22px;
    line-height: 22px;
    background: var(--fold-bg, var(--field));
    color: var(--code-dim, var(--text2));
    font-family: var(--font);
    font-size: var(--text-caption);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .same {
    margin: 0 4px;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 16px;
    margin: 0 4px;
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .key {
    display: inline-block;
    width: 14px;
    height: 10px;
    border-radius: var(--radius-xs);
  }
  .key.bar {
    width: 3px;
    border-radius: var(--radius-pill);
    background: var(--accent);
  }
  .key.add {
    background: var(--add-word);
  }
  .key.del {
    background: var(--del-word);
  }
</style>
