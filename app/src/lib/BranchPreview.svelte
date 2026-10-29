<script lang="ts">
  // Settings › Themes › Branches & graph: a small history in the chosen branch palette, drawn with
  // the same tokens as the real graph (they are set on the window's root by followLook).
  import { lane, plate, tint } from "./format";

  const ROW = 24;
  const TOP = 14;
  const LANE = 14;
  const LEFT = 16;
  const COLORS = 7;
  const x = (column: number) => LEFT + LANE * column;
  const y = (row: number) => TOP + ROW * row;

  // Production runs down column 0 in color 0. Branch n (1..7) has column n and color n: its tip
  // is in row n, and it forks from production three rows lower, so every color of the palette
  // shows side by side.
  const ROWS = COLORS + 4;
  const names = ["main", "feature/auth", "sync", "cache", "release/2.4", "fix/login", "docs", "ui/theme"];
  const texts = [150, 120, 96, 130, 88, 110, 140, 100, 124, 90, 116];
  const forkOf = (n: number) => Math.min(ROWS - 1, n + 3);
  const branches = Array.from({ length: COLORS }, (_, i) => i + 1);
  // Dots: [row, column, color].
  const dots: [number, number, number][] = [
    ...[0, ...new Set(branches.map(forkOf))].map((row): [number, number, number] => [row, 0, 0]),
    ...branches.flatMap((n): [number, number, number][] => (n + 1 < forkOf(n) ? [[n, n, n], [n + 1, n, n]] : [[n, n, n]])),
  ];
  function path(n: number): string {
    // Straight down its own column, then an S-curve into the commit it forks from.
    const bend = y(forkOf(n)) - ROW;
    return `M${x(n)} ${y(n)}V${bend}C${x(n)} ${bend + ROW / 2} ${x(0)} ${bend + ROW / 2} ${x(0)} ${y(forkOf(n))}`;
  }
  const textLeft = x(COLORS) + 18;
</script>

<div class="preview">
  <svg width={textLeft} height={y(ROWS - 1) + TOP} aria-hidden="true">
    <path d="M{x(0)} {y(0)}V{y(ROWS - 1)}" stroke={lane(0)} stroke-width="3" stroke-linecap="round" fill="none" />
    {#each branches as n (n)}
      <path d={path(n)} stroke={lane(n)} stroke-width="2" stroke-linecap="round" fill="none" />
    {/each}
    {#each dots as [row, column, color], i (i)}
      <circle cx={x(column)} cy={y(row)} r={color === 0 ? 5.5 : 4.5} fill={lane(color)} stroke="var(--graph-bg)" stroke-width="2" />
    {/each}
  </svg>
  <div class="texts" style:margin-left="{textLeft}px">
    {#each texts as width, row (row)}
      <div class="line" style:top="{y(row) - 8}px">
        <span class="bar" style:width="{width}px"></span>
        {#if row <= COLORS}
          <span class="pill" class:production={row === 0} style:color={plate(row)} style:background={tint(row, "label")}>{names[row]}</span>
        {/if}
      </div>
    {/each}
  </div>
  <div class="all" title="Every branch color of this palette; production first">
    {#each [0, ...branches] as color (color)}
      <span class="chip" style:background={tint(color, "label")} style:color={plate(color)}>
        <span class="dot" style:background={lane(color)}></span>{color === 0 ? "production" : color}
      </span>
    {/each}
  </div>
</div>

<style>
  .preview {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 4px 0 12px;
    border-radius: var(--radius-l);
    border: 0.5px solid var(--panel-border);
    background: var(--graph-bg);
    overflow: hidden;
  }
  svg {
    display: block;
  }
  .texts {
    position: absolute;
    inset: 4px 12px 0 0;
    pointer-events: none;
  }
  .line {
    position: absolute;
    left: 0;
    right: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 16px;
  }
  .bar {
    height: 6px;
    border-radius: var(--radius-pill);
    background: var(--text2);
    opacity: 0.35;
    flex-shrink: 0;
  }
  .pill {
    display: inline-flex;
    align-items: center;
    height: 16px;
    padding: 0 6px;
    border-radius: var(--radius-pill);
    font-size: var(--text-caption);
    font-weight: 500;
    white-space: nowrap;
  }
  .pill.production {
    font-weight: 700;
  }
  .all {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 0 12px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 18px;
    padding: 0 7px 0 5px;
    border-radius: var(--radius-pill);
    font-size: var(--text-caption);
    font-weight: 500;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
</style>
