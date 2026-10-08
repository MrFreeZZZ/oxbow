<script lang="ts">
  import type { HistoryRow } from "./types";
  import { lane, plate, shortId, tint } from "./format";
  import { softArrows } from "./shapes";

  let {
    row,
    childIds,
    lookup,
    onSelect,
  }: { row: HistoryRow; childIds: string[]; lookup: (id: string) => HistoryRow | undefined; onSelect: (id: string) => void } = $props();

  // Soft arrows, as in the design's stack strip: newest on the left, each arrow points left.
  const HEIGHT = 42;
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

  const shapes = $derived(softArrows(blocks.length, width, HEIGHT).map((shape, i) => ({ ...shape, block: blocks[i] })));

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
