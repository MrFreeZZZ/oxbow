<script lang="ts">
  import type { HistoryRow } from "./types";
  import { lane, plate, shortId, tint } from "./format";

  let {
    row,
    childIds,
    lookup,
    onSelect,
  }: { row: HistoryRow; childIds: string[]; lookup: (id: string) => HistoryRow | undefined; onSelect: (id: string) => void } = $props();

  // Soft arrows, as in the design's stack strip: newest on the left, each arrow points left.
  const HEIGHT = 42;
  const TIP = 14;
  const GAP = 4;
  const MAX_IDS = 2;

  let width = $state(0);

  type Block = { ids: string[]; label: string; current: boolean };

  const blocks = $derived.by(() => {
    const list: Block[] = [];
    if (childIds.length) list.push({ ids: childIds, label: childIds.length === 1 ? "child" : `${childIds.length} children`, current: false });
    list.push({ ids: [row.id], label: "this commit", current: true });
    if (row.parents.length) list.push({ ids: row.parents, label: row.parents.length === 1 ? "parent" : `${row.parents.length} parents`, current: false });
    return list;
  });

  /** Polygon with rounded corners, as an SVG path. */
  function roundPoly(points: [number, number][], radii: number[]): string {
    return (
      points
        .map((p, i) => {
          const a = points[(i - 1 + points.length) % points.length];
          const b = points[(i + 1) % points.length];
          const la = Math.hypot(p[0] - a[0], p[1] - a[1]);
          const lb = Math.hypot(b[0] - p[0], b[1] - p[1]);
          const ra = Math.min(radii[i], la / 2);
          const rb = Math.min(radii[i], lb / 2);
          const s = [p[0] + ((a[0] - p[0]) * ra) / la, p[1] + ((a[1] - p[1]) * ra) / la];
          const e = [p[0] + ((b[0] - p[0]) * rb) / lb, p[1] + ((b[1] - p[1]) * rb) / lb];
          return `${i ? "L" : "M"}${s[0].toFixed(1)} ${s[1].toFixed(1)}Q${p[0]} ${p[1]} ${e[0].toFixed(1)} ${e[1].toFixed(1)}`;
        })
        .join("") + "Z"
    );
  }

  // Shapes are built oldest first, left to right (tip on the right, notch on the left), then mirrored.
  const shapes = $derived.by(() => {
    const n = blocks.length;
    const cw = (width + (n - 1) * (TIP - GAP)) / n;
    return blocks.map((block, display) => {
      const i = n - 1 - display;
      const x0 = i * (cw - TIP + GAP);
      const x1 = x0 + cw;
      const points: [number, number][] = [
        [x0, 0],
        [x1 - TIP, 0],
        [x1, HEIGHT / 2],
        [x1 - TIP, HEIGHT],
        [x0, HEIGHT],
      ];
      const radii = [10, 10, 12, 10, 10];
      if (i > 0) {
        points.push([x0 + TIP, HEIGHT / 2]);
        radii.push(7);
      }
      const bx = x0 + (i ? TIP : 0);
      const bw = cw - TIP - (i ? TIP : 0);
      return { block, path: roundPoly(points.map(([x, y]) => [width - x, y]), radii), left: width - bx - bw, width: bw };
    });
  });

  const color = $derived(row.graph.color);
</script>

<div class="chain" bind:clientWidth={width} role="group" aria-label="Commit chain">
  {#if width > 0}
    {#each shapes as shape (shape.block.label)}
      <svg class="shape" aria-hidden="true" width={width} height={HEIGHT} viewBox="0 0 {width} {HEIGHT}">
        <path
          d={shape.path}
          style:fill={shape.block.current ? tint(color) : "var(--field)"}
          style:stroke={shape.block.current ? `color-mix(in srgb, ${lane(color)} 40%, transparent)` : "transparent"}
        />
      </svg>
      <div class="content" style:left="{shape.left}px" style:width="{shape.width}px">
        <span class="ids">
          {#if shape.block.current}
            <span class="id mono selectable" style:color={plate(color)} title={row.id}>{shortId(row.id)}</span>
          {:else}
            {#each shape.block.ids.slice(0, MAX_IDS) as id (id)}
              {@const target = lookup(id)}
              <button class="id mono link" disabled={!target} onclick={() => onSelect(id)} title={target ? `${shortId(id)} ${target.summary}` : `${id} is not loaded`}
                >{shortId(id)}</button
              >
            {/each}
            {#if shape.block.ids.length > MAX_IDS}
              <span class="more" title={shape.block.ids.slice(MAX_IDS).map((id) => `${shortId(id)} ${lookup(id)?.summary ?? ""}`).join("\n")}
                >+{shape.block.ids.length - MAX_IDS}</span
              >
            {/if}
          {/if}
        </span>
        <span class="label">{shape.block.label}</span>
      </div>
    {/each}
  {/if}
</div>

<style>
  .chain {
    position: relative;
    height: 42px;
  }
  .shape {
    position: absolute;
    left: 0;
    top: 0;
    overflow: visible;
    pointer-events: none;
  }
  .shape path {
    stroke-width: 1.5;
  }
  .content {
    position: absolute;
    top: 0;
    height: 42px;
    padding-left: 12px;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 1px;
    min-width: 0;
  }
  .ids {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
  }
  .id {
    font-size: 12px;
    font-weight: 600;
  }
  .link {
    color: var(--accent-text);
  }
  .link:hover:not(:disabled) {
    text-decoration: underline;
  }
  .link:disabled {
    color: var(--text2);
    cursor: default;
  }
  .more,
  .label {
    font-size: 11px;
    color: var(--text2);
  }
</style>
