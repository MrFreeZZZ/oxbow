<script lang="ts">
  // Settings › Themes › Branches & graph: a small history in the chosen branch palette, drawn with
  // the same tokens as the real graph (they are set on the window's root by followLook).
  import { lane, plate, tint } from "./format";

  let { colors }: { colors: number } = $props();

  const ROW = 30;
  const TOP = 16;
  const LANE = 18;
  const LEFT = 16;
  const x = (column: number) => LEFT + LANE * column;
  const y = (row: number) => TOP + ROW * row;

  // Rows top to bottom: a line's tip carries its capsule. Production is column 0, color 0.
  const rows = [
    { column: 1, color: 1, name: "feature/auth", text: 120 },
    { column: 0, color: 0, name: "main", text: 150 },
    { column: 2, color: 2, name: "sync", text: 96 },
    { column: 3, color: 7, name: "cache", text: 110 },
    { column: 1, color: 1, name: null, text: 132 },
    { column: 0, color: 0, name: null, text: 90 },
  ];
  // Lines: [from row, column, to row, column, color]; a curve when the columns differ.
  const lines: [number, number, number, number, number][] = [
    [1, 0, 5, 0, 0],
    [0, 1, 4, 1, 1],
    [4, 1, 5, 0, 1],
    [2, 2, 5, 0, 2],
    [3, 3, 5, 0, 7],
  ];
  function path([r1, c1, r2, c2]: [number, number, number, number, number]): string {
    if (c1 === c2) return `M${x(c1)} ${y(r1)}V${y(r2)}`;
    // Straight down its own column, then an S-curve into the commit it forks from.
    const bend = y(r2) - ROW;
    return `M${x(c1)} ${y(r1)}V${bend}C${x(c1)} ${bend + ROW / 2} ${x(c2)} ${bend + ROW / 2} ${x(c2)} ${y(r2)}`;
  }
  const textLeft = x(3) + 18;
</script>

<div class="preview">
  <svg width={textLeft} height={y(rows.length - 1) + TOP} aria-hidden="true">
    {#each lines as line, i (i)}
      <path d={path(line)} stroke={lane(line[4])} stroke-width={line[4] === 0 ? 3 : 2} stroke-linecap="round" fill="none" />
    {/each}
    {#each rows as row, i (i)}
      <circle cx={x(row.column)} cy={y(i)} r={row.color === 0 ? 5.5 : 4.5} fill={lane(row.color)} stroke="var(--graph-bg)" stroke-width="2" />
    {/each}
  </svg>
  <div class="texts">
    {#each rows as row, i (i)}
      <div class="line" style:top="{y(i) - 8}px">
        <span class="bar" style:width="{row.text}px"></span>
        {#if row.name}
          <span class="pill" class:production={row.color === 0} style:color={plate(row.color)} style:background={tint(row.color, "label")}>{row.name}</span>
        {/if}
      </div>
    {/each}
  </div>
  <div class="all" title="Every branch color of this palette; production first">
    {#each Array.from({ length: colors + 1 }, (_, i) => i) as color (color)}
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
    border-radius: 12px;
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
    margin-left: 88px;
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
    border-radius: 3px;
    background: var(--text2);
    opacity: 0.35;
    flex-shrink: 0;
  }
  .pill {
    display: inline-flex;
    align-items: center;
    height: 16px;
    padding: 0 6px;
    border-radius: 8px;
    font-size: 11px;
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
    border-radius: 9px;
    font-size: 11px;
    font-weight: 500;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
</style>
