<script lang="ts">
  // The picture in the merge sheet: the commits the result is made of, on one or two lines, and
  // the dry run's verdict under it.
  import type { Preview } from "./confirm.svelte";
  import { lane, plate, tint } from "./format";

  let { preview }: { preview: Preview } = $props();

  const ROW = 22;
  const LANE = 18;
  const X0 = 12;
  const x = (l: number) => X0 + l * LANE;
  const y = (i: number) => i * ROW + ROW / 2;

  /** A straight line on one lane, or a curve over the row next to the main line. */
  function path(from: number, to: number): string {
    const a = preview.rows[from];
    const b = preview.rows[to];
    const [x1, y1, x2, y2] = [x(a.lane), y(from), x(b.lane), y(to)];
    if (x1 === x2) return `M${x1} ${y1}V${y2}`;
    if (a.lane < b.lane) {
      // Out to the side right under the upper commit.
      const mid = y1 + ROW;
      return `M${x1} ${y1}C${x1} ${y1 + ROW * 0.6} ${x2} ${mid - ROW * 0.6} ${x2} ${mid}V${y2}`;
    }
    // Back to the main line right above the lower commit.
    const mid = y2 - ROW;
    return `M${x1} ${y1}V${mid}C${x1} ${mid + ROW * 0.6} ${x2} ${y2 - ROW * 0.6} ${x2} ${y2}`;
  }

  const links = $derived(
    preview.rows.flatMap((row, i) =>
      row.links.map((to) => {
        const other = preview.rows[to];
        // A line takes the color of the side line it belongs to.
        const color = row.lane > other.lane ? row.color : other.lane > row.lane ? other.color : row.color;
        return { d: path(i, to), color, muted: !!(row.muted || (other.muted && other.node !== "base")), dashed: row.node === "new" || row.node === "merge" || row.node === "more" };
      }),
    ),
  );
  const width = $derived(X0 + (preview.rows.some((r) => r.lane === 1) ? LANE : 0) + 12);

  const verdictIcon = { ok: "M4 8.5l2.5 2.5L12 5.5", warn: "M8 4.5v4.5M8 11.5v.2", info: "M8 7.5v4M8 4.8v.2" };
</script>

{#if preview.rows.length}
  <div class="box">
    <svg class="graph" width={width} height={preview.rows.length * ROW} aria-hidden="true">
      {#each links as link, i (i)}
        <path d={link.d} stroke={lane(link.color)} stroke-width="2" fill="none" opacity={link.muted ? 0.4 : 1} stroke-dasharray={link.dashed ? "3 3" : undefined} />
      {/each}
      {#each preview.rows as row, i (i)}
        {#if row.node === "merge"}
          <circle cx={x(row.lane)} cy={y(i)} r="6" fill="var(--sheet)" stroke={lane(row.color)} stroke-width="2" />
          <path d="M{x(row.lane)} {y(i) - 6}a6 6 0 0 1 0 12" fill="none" stroke={lane(row.other ?? row.color)} stroke-width="2" />
          <circle cx={x(row.lane)} cy={y(i)} r="2.5" fill={lane(row.color)} />
        {:else if row.node === "new"}
          <circle cx={x(row.lane)} cy={y(i)} r="4.5" fill="var(--sheet)" stroke={lane(row.color)} stroke-width="2" />
        {:else if row.node === "base"}
          <circle cx={x(row.lane)} cy={y(i)} r="3.5" fill="var(--text2)" />
        {:else if row.node === "commit"}
          <circle cx={x(row.lane)} cy={y(i)} r="4" fill={lane(row.color)} opacity={row.muted ? 0.4 : 1} />
        {/if}
      {/each}
    </svg>
    <div class="rows">
      {#each preview.rows as row, i (i)}
        <div class="row" class:muted={row.muted} class:bold={row.node === "merge" || (row.node === "new" && i === 0)} style:height="{ROW}px">
          <span class="summary">{row.summary}</span>
          {#if row.node === "base"}<span class="tag">fork point</span>{/if}
          {#if row.sha}<span class="sha mono">{row.sha}</span>{:else if row.node !== "more"}<span class="new" style:color={plate(row.color)} style:background={tint(row.color, "label")}>new</span>{/if}
        </div>
      {/each}
    </div>
  </div>
{/if}
{#if preview.verdict}
  {@const v = preview.verdict}
  <div class="verdict {v.tone}">
    <span class="mark"><svg viewBox="0 0 16 16"><path d={verdictIcon[v.tone]} /></svg></span>
    <span class="words">
      {#each v.parts as part, i (i)}
        {#if typeof part === "string"}{part}{:else if "quote" in part}<b>{part.quote}</b>{:else if "code" in part}<span class="code mono">{part.code}</span>{:else}{part.branch}{/if}
      {/each}
    </span>
  </div>
{/if}

<style>
  .box {
    display: flex;
    gap: 6px;
    padding: 10px 12px 10px 6px;
    border-radius: 12px;
    border: 1px solid var(--sep);
    background: var(--graph-bg);
    max-height: 250px;
    overflow-y: auto;
  }
  .graph {
    flex-shrink: 0;
  }
  .rows {
    flex-grow: 1;
    min-width: 0;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }
  .row.muted {
    color: var(--text2);
  }
  .row.bold .summary {
    font-weight: 600;
  }
  .summary {
    flex-grow: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tag {
    font-size: 11px;
    color: var(--text2);
  }
  .sha {
    font-size: 11px;
    color: var(--text2);
  }
  .new {
    font-size: 10px;
    font-weight: 600;
    padding: 0 6px;
    border-radius: 6px;
    line-height: 16px;
  }
  .verdict {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 9px 12px;
    border-radius: 10px;
    font-size: 12px;
    line-height: 1.45;
  }
  .verdict.ok {
    background: var(--green-soft);
  }
  .verdict.ok b {
    color: var(--green);
  }
  .verdict.warn {
    background: var(--orange-soft);
  }
  .verdict.warn b {
    color: var(--orange);
  }
  .verdict.info {
    background: var(--field);
  }
  .mark {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    margin-top: 0.5px;
    border-radius: 8px;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .ok .mark {
    background: color-mix(in srgb, var(--green) 18%, transparent);
    color: var(--green);
  }
  .warn .mark {
    background: color-mix(in srgb, var(--orange) 18%, transparent);
    color: var(--orange);
  }
  .info .mark {
    background: var(--sep);
    color: var(--text2);
  }
  .mark svg {
    width: 12px;
    height: 12px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .code {
    font-size: 11px;
    padding: 0 4px;
    border-radius: 5px;
    background: var(--field);
  }
</style>
