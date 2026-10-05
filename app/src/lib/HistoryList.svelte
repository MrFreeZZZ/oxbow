<script lang="ts">
  import type { History, HistoryRow } from "./types";
  import { lane, plate, relativeTime, tint } from "./format";

  let { history, selected, onSelect }: { history: History; selected: string | null; onSelect: (id: string) => void } = $props();

  // Geometry from the design: 44px two-line rows, lanes 18px apart, trunk lane centered 14px in.
  const ROW = 44;
  const PAD_TOP = 4;
  const PAD_LEFT = 8;
  const LANE = 18;
  const FIRST_LANE = 14;
  const OVERSCAN = 12;

  let viewport = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let height = $state(600);
  let flash = $state<string | null>(null);

  const rows = $derived(history.rows);
  const indexOf = $derived(new Map(rows.map((row, i) => [row.id, i])));
  const first = $derived(Math.max(0, Math.floor((scrollTop - PAD_TOP) / ROW) - OVERSCAN));
  const last = $derived(Math.min(rows.length, Math.ceil((scrollTop + height) / ROW) + OVERSCAN));
  const visible = $derived(rows.slice(first, last).map((row, k) => ({ row, index: first + k })));

  const x = (column: number) => PAD_LEFT + FIRST_LANE + LANE * column;
  const y = (index: number) => PAD_TOP + ROW / 2 + ROW * index;

  /** Straight line, or an S-curve over one row when the column changes. */
  function segmentPath(index: number, from: number, to: number): string {
    const y0 = y(index);
    const y1 = y(index + 1);
    if (from === to) return `M${x(from)} ${y0}V${y1}`;
    const mid = (y0 + y1) / 2;
    return `M${x(from)} ${y0}C${x(from)} ${mid} ${x(to)} ${mid} ${x(to)} ${y1}`;
  }

  // Segments of the row above the first visible one, so curves entering the view are drawn too.
  const drawn = $derived(
    rows.slice(Math.max(0, first - 1), last).map((row, k) => ({ row, index: Math.max(0, first - 1) + k })),
  );

  function dot(row: HistoryRow, index: number) {
    const g = row.graph;
    const isMerge = row.parents.length > 1;
    const isSelected = row.id === selected;
    const size = isMerge ? 18 : isSelected ? 15 : g.color === 0 ? 13 : 11;
    const forks = g.forkColors;
    const halo =
      forks.length === 0
        ? "transparent"
        : forks.length === 1
          ? lane(forks[0])
          : `conic-gradient(${forks.map((c, j) => `${lane(c)} ${(100 * j) / forks.length}% ${(100 * (j + 1)) / forks.length}%`).join(", ")})`;
    const merged = g.mergeColors[0] ?? g.color;
    return {
      size,
      outer: size + 8,
      left: x(g.column) - (size + 8) / 2,
      top: y(index) - (size + 8) / 2,
      halo,
      gap: forks.length ? "var(--win)" : "transparent",
      fill: isMerge
        ? `linear-gradient(90deg, ${lane(g.color)} 50%, ${lane(merged)} 50%)`
        : row.unpushed
          ? "var(--win)"
          : lane(g.color),
      ring: isMerge ? "transparent" : row.unpushed ? lane(g.color) : "var(--win)",
      hole: isMerge,
    };
  }

  function textStart(row: HistoryRow): number {
    return FIRST_LANE + LANE * row.graph.width + 18;
  }

  /** Scroll so the commit is visible, centering it when it is far away. */
  export function reveal(id: string) {
    const index = indexOf.get(id);
    if (index === undefined || !viewport) return;
    const top = PAD_TOP + index * ROW;
    if (top < viewport.scrollTop || top + ROW > viewport.scrollTop + viewport.clientHeight) {
      viewport.scrollTop = top - viewport.clientHeight / 2 + ROW / 2;
    }
    if (id !== selected) {
      flash = id;
      setTimeout(() => (flash = flash === id ? null : flash), 900);
    }
  }

  function onKey(event: KeyboardEvent) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const current = selected ? (indexOf.get(selected) ?? -1) : -1;
    const next = Math.min(rows.length - 1, Math.max(0, current + (event.key === "ArrowDown" ? 1 : -1)));
    if (rows[next]) onSelect(rows[next].id);
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="viewport"
  bind:this={viewport}
  bind:clientHeight={height}
  onscroll={() => (scrollTop = viewport?.scrollTop ?? 0)}
  onkeydown={onKey}
  tabindex="0"
  role="listbox"
  aria-label="Commits"
>
  <div class="canvas" style:height="{PAD_TOP * 2 + rows.length * ROW}px">
    {#each visible as { row, index } (row.id)}
      {@const isSelected = row.id === selected}
      <div
        class="row"
        class:flash={flash === row.id}
        role="option"
        aria-selected={isSelected}
        tabindex="-1"
        style:top="{PAD_TOP + index * ROW}px"
        style:padding-left="{textStart(row)}px"
        style:background={isSelected ? tint(row.graph.color) : undefined}
        style:--flash={tint(row.graph.color)}
        onclick={() => onSelect(row.id)}
        onkeydown={() => {}}
      >
        <span class="summary">{row.summary}</span>
        <span class="meta">
          {#each row.labels as label (label.kind + label.name)}
            {#if label.kind === "tag"}
              <span class="pill tag">
                <svg class="icon tiny" viewBox="0 0 16 16"><path d="M2.5 2.5h5l6 6-5 5-6-6z" /><circle cx="5.5" cy="5.5" r="0.8" /></svg>
                {label.name}
              </span>
            {:else if label.kind === "remote"}
              <span class="pill" style:color={plate(label.color)} style:border-color="color-mix(in srgb, {lane(label.color)} 40%, transparent)">{label.name}</span>
            {:else}
              <span class="pill" class:head={label.head} style:color={plate(label.color)} style:background={tint(label.color, "label")}>{label.name}</span>
            {/if}
          {/each}
          <span class="byline">{row.authorName} · {relativeTime(row.time)}</span>
        </span>
      </div>
    {/each}

    <svg class="lines" width="100%" height={PAD_TOP * 2 + rows.length * ROW} aria-hidden="true">
      {#if history.trunkTipRow && history.trunkTipRow > first - OVERSCAN}
        <!-- The trunk's newest commit is below newer work on other branches: a gray lead-in fills its column. -->
        <path d="M{x(0)} 0V{y(history.trunkTipRow) - 10}" stroke="var(--lead-in)" stroke-width="2" stroke-dasharray="2 5" stroke-linecap="round" fill="none" />
      {/if}
      {#each drawn as { row, index } (row.id)}
        {#each row.graph.segments as seg, k (k)}
          <path
            d={segmentPath(index, seg.from, seg.to)}
            stroke={lane(seg.color)}
            stroke-width={seg.color === 0 && seg.from === 0 && seg.to === 0 ? 4 : 2}
            stroke-dasharray={seg.dashed ? "2 5" : undefined}
            stroke-linecap="round"
            fill="none"
          />
        {/each}
      {/each}
    </svg>

    {#each visible as { row, index } (row.id)}
      {@const d = dot(row, index)}
      <span class="halo" style:left="{d.left}px" style:top="{d.top}px" style:width="{d.outer}px" style:height="{d.outer}px" style:background={d.halo}>
        <span class="gap" style:width="{d.size + 4}px" style:height="{d.size + 4}px" style:background={d.gap}>
          <span class="dot" style:width="{d.size}px" style:height="{d.size}px" style:background={d.fill} style:border-color={d.ring}>
            {#if d.hole}<span class="hole"></span>{/if}
          </span>
        </span>
      </span>
    {/each}
  </div>
</div>

<style>
  .viewport {
    position: relative;
    flex-grow: 1;
    overflow-y: auto;
    overflow-x: hidden;
    outline: none;
  }
  .canvas {
    position: relative;
  }
  .row {
    position: absolute;
    left: 8px;
    right: 8px;
    height: 44px;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 3px;
    padding-right: 10px;
    border-radius: 8px;
    transition: background-color 0.3s;
  }
  .row.flash {
    background: var(--flash);
  }
  .summary {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    font-size: 11px;
    overflow: hidden;
  }
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    height: 16px;
    padding: 0 6px;
    border-radius: 8px;
    border: 1px solid transparent;
    font-weight: 500;
    flex-shrink: 0;
    white-space: nowrap;
  }
  .pill.head {
    font-weight: 600;
  }
  .pill.tag {
    background: var(--tag-bg);
    border-color: var(--tag-border);
    color: var(--tag-fg);
  }
  .tiny {
    width: 10px;
    height: 10px;
  }
  .byline {
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .lines {
    position: absolute;
    left: 0;
    top: 0;
    pointer-events: none;
    overflow: visible;
  }
  .halo,
  .gap,
  .dot {
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .halo {
    position: absolute;
    pointer-events: none;
  }
  .dot {
    border: 2px solid transparent;
  }
  .hole {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--win);
  }
</style>
