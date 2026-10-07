<script lang="ts">
  import { untrack } from "svelte";
  import type { History, HistoryRow, RowLayout, Segment, WorktreeSummary } from "./types";
  import { lane, plate, relativeTime, tint } from "./format";
  import Menu, { type MenuEntry } from "./Menu.svelte";
  import { prefs } from "./prefs.svelte";
  import { split, type Found } from "./search.svelte";

  let {
    history,
    selected,
    onSelect,
    menuFor,
    found = null,
  }: {
    history: History;
    selected: string | null;
    onSelect: (id: string) => void;
    /** Entries of a commit's right-click menu; none, no menu. */
    menuFor: (row: HistoryRow) => MenuEntry[];
    /** A search: other commits fade, or only the matches are listed. */
    found?: Found | null;
  } = $props();

  // Geometry from the design: 44px two-line rows, lanes 18px apart, trunk lane centered 14px in.
  // Compact rows (General → History rows) put everything on one 28px line.
  const compact = $derived(prefs.get("oxbow.history.rowStyle") === "compact");
  const ROW = $derived(compact ? 28 : 44);
  const PAD_TOP = 4;
  const PAD_LEFT = 8;
  const LANE = 18;
  const FIRST_LANE = 14;
  const OVERSCAN = 12;

  let viewport = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let height = $state(600);
  let flash = $state<string | null>(null);
  let menu = $state<{ x: number; y: number; id: string; entries: MenuEntry[] } | null>(null);

  function openMenu(event: MouseEvent, row: HistoryRow) {
    const entries = menuFor(row);
    if (!entries.length) return;
    event.preventDefault();
    menu = { x: event.clientX, y: event.clientY, id: row.id, entries };
  }

  // When the layout changes, e.g. a checkout moves a branch to the left column, lines and dots
  // glide from their old columns to the new ones instead of jumping.
  const MOVE_MS = 220;
  /** Layout of each row and the lead-ins before the change, while it animates. */
  let from = $state<{ rows: Map<string, RowLayout>; leadIns: Map<string, number> } | null>(null);
  let progress = $state(1);
  let frame = 0;
  let shownRows: HistoryRow[] | null = null;
  let shownLeadIns = new Map<string, number>();

  $effect.pre(() => {
    const next = history;
    untrack(() => {
      const leadIns = new Map(next.leadIns.map((l) => [next.rows[l.row]?.id ?? "", l.column]));
      if (shownRows && shownRows !== next.rows) animate(shownRows, shownLeadIns, next.rows);
      shownRows = next.rows;
      shownLeadIns = leadIns;
    });
  });

  function animate(old: HistoryRow[], oldLeadIns: Map<string, number>, next: HistoryRow[]) {
    const rows = new Map(old.map((r) => [r.id, r.graph]));
    const moved = next.some((r) => {
      const before = rows.get(r.id);
      return before && (before.column !== r.graph.column || before.width !== r.graph.width);
    });
    if (!moved || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    cancelAnimationFrame(frame);
    from = { rows, leadIns: oldLeadIns };
    progress = 0;
    const start = performance.now();
    const step = (now: number) => {
      const t = Math.min(1, (now - start) / MOVE_MS);
      // Ease out: quick start, soft landing.
      progress = 1 - (1 - t) ** 3;
      if (t < 1) frame = requestAnimationFrame(step);
      else from = null;
    };
    frame = requestAnimationFrame(step);
  }

  const mix = (a: number, b: number) => a + (b - a) * progress;

  function columnOf(row: HistoryRow): number {
    const old = from?.rows.get(row.id);
    return old ? mix(old.column, row.graph.column) : row.graph.column;
  }

  /** The row's lines, on their way from the old columns; new ones fade in. */
  function segmentsOf(row: HistoryRow): (Segment & { opacity: number })[] {
    const old = from?.rows.get(row.id);
    if (!old) return row.graph.segments.map((seg) => ({ ...seg, opacity: 1 }));
    const pool = [...old.segments];
    return row.graph.segments.map((seg) => {
      const i = pool.findIndex((o) => o.color === seg.color && o.dashed === seg.dashed && o.thick === seg.thick);
      if (i < 0) return { ...seg, opacity: progress };
      const [o] = pool.splice(i, 1);
      return { ...seg, from: mix(o.from, seg.from), to: mix(o.to, seg.to), opacity: 1 };
    });
  }

  const leadIns = $derived(
    history.leadIns.map((l) => {
      const old = from?.leadIns.get(history.rows[l.row]?.id ?? "");
      return { row: l.row, column: old === undefined ? l.column : mix(old, l.column), opacity: old === undefined && from ? progress : 1 };
    }),
  );

  /** Only Matches lists the matching commits alone, without the graph. */
  const only = $derived(!!found?.only);
  const rows = $derived(found?.only ? history.rows.filter((r) => found.hits.has(r.id)) : history.rows);
  const faded = (row: HistoryRow) => !!found && !found.hits.has(row.id);
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
    const size = compact ? (isMerge ? 14 : isSelected ? 12 : g.color === 0 ? 11 : 9) : isMerge ? 18 : isSelected ? 15 : g.color === 0 ? 13 : 11;
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
      left: x(columnOf(row)) - (size + 8) / 2,
      top: y(index) - (size + 8) / 2,
      halo,
      gap: forks.length ? "var(--graph-bg)" : "transparent",
      fill: isMerge
        ? `linear-gradient(90deg, ${lane(g.color)} 50%, ${lane(merged)} 50%)`
        : row.unpushed
          ? "var(--graph-bg)"
          : lane(g.color),
      ring: isMerge ? "transparent" : row.unpushed ? lane(g.color) : "var(--graph-bg)",
      hole: isMerge,
    };
  }

  /** "5 files · 3 staged · 2 unstaged" for the row of uncommitted changes. */
  function worktreeLine(w: WorktreeSummary): string {
    const parts = [`${w.files} ${w.files === 1 ? "file" : "files"}`];
    if (w.conflicted) parts.push(`${w.conflicted} conflicted`);
    if (w.staged) parts.push(`${w.staged} staged`);
    if (w.unstaged) parts.push(`${w.unstaged} unstaged`);
    return parts.join(" · ");
  }

  function textStart(row: HistoryRow): number {
    if (only) return 30;
    const old = from?.rows.get(row.id);
    return FIRST_LANE + LANE * (old ? mix(old.width, row.graph.width) : row.graph.width) + 18;
  }

  /** Where a match is when the summary doesn't show it: the files, or the description. */
  function foundNote(row: HistoryRow): string | null {
    if (!found || !found.hits.has(row.id)) return null;
    const files = found.hits.get(row.id) ?? [];
    if (files.length) {
      const names = files.slice(0, 2).map((f) => f.slice(f.lastIndexOf("/") + 1));
      return names.join(", ") + (files.length > 2 ? ` +${files.length - 2}` : "");
    }
    if (found.mode === "message" && !row.summary.toLowerCase().includes(found.text.toLowerCase())) return "in description";
    return null;
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

  /** Back to the newest commit: scroll to the top and select it. */
  export function toTop() {
    if (viewport) viewport.scrollTo({ top: 0, behavior: "smooth" });
    if (rows[0]) onSelect(rows[0].id);
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "Home" || (event.key === "ArrowUp" && (event.metaKey || event.ctrlKey))) {
      event.preventDefault();
      toTop();
      return;
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const current = selected ? (indexOf.get(selected) ?? -1) : -1;
    const next = Math.min(rows.length - 1, Math.max(0, current + (event.key === "ArrowDown" ? 1 : -1)));
    if (rows[next]) onSelect(rows[next].id);
  }
</script>

<div class="wrap">
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
          class:compact
          class:faded={faded(row)}
          class:flash={flash === row.id}
          class:menu-open={menu?.id === row.id}
          role="option"
          aria-selected={isSelected}
          tabindex="-1"
          style:top="{PAD_TOP + index * ROW}px"
          style:padding-left="{textStart(row)}px"
          style:background={isSelected ? tint(row.graph.color) : undefined}
          style:--flash={tint(row.graph.color)}
          style:--ring={lane(row.graph.color)}
          onclick={() => onSelect(row.id)}
          oncontextmenu={(e) => openMenu(e, row)}
          onkeydown={() => {}}
        >
          <span class="summary" class:selected={isSelected}>
            {#if found?.mode === "message"}
              {#each split(row.summary, found.text) as piece, k (k)}{#if piece.hit}<mark>{piece.text}</mark>{:else}{piece.text}{/if}{/each}
            {:else}
              {row.summary}
            {/if}
          </span>
          <span class="meta">
            {#each row.labels as label (label.kind + label.name)}
              {#if label.kind === "tag"}
                <span class="pill tag" class:local={history.localTags.includes(label.name)} title={history.localTags.includes(label.name) ? `Not on ${history.defaultRemote} yet` : undefined}>
                  <svg class="icon tiny" viewBox="0 0 16 16"><path d="M2.5 2.5h5l6 6-5 5-6-6z" /><circle cx="5.5" cy="5.5" r="0.8" /></svg>
                  {label.name}
                </span>
              {:else if label.kind === "stash"}
                <span class="pill" style:color={plate(label.color)} style:background={tint(label.color, "label")}>
                  <svg class="icon tiny" viewBox="0 0 16 16"><rect x="3" y="6.5" width="10" height="7" rx="1.5" /><path d="M4.5 4.5h7M6 2.5h4" /></svg>
                  {label.name}
                </span>
              {:else if label.kind === "head"}
                <span class="pill head" style:color={plate(label.color)} style:border-color="color-mix(in srgb, {lane(label.color)} 55%, transparent)">HEAD</span>
              {:else if label.kind === "remote"}
                <span class="pill" style:color={plate(label.color)} style:border-color="color-mix(in srgb, {lane(label.color)} 40%, transparent)">{label.name}</span>
              {:else}
                <span class="pill" class:head={label.head} style:color={plate(label.color)} style:background={tint(label.color, "label")}>{label.name}</span>
              {/if}
            {/each}
            {#if row.worktree}
              <span class="byline">{worktreeLine(row.worktree)}</span>
            {:else}
              <span class="byline">
                {#if found?.mode === "author"}
                  {#each split(row.authorName, found.text) as piece, k (k)}{#if piece.hit}<mark>{piece.text}</mark>{:else}{piece.text}{/if}{/each}
                {:else}
                  {row.authorName}
                {/if}
                · {relativeTime(row.time)}
              </span>
              {@const note = foundNote(row)}
              {#if note}
                <span class="found-note">
                  <svg class="icon tiny" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5" /><path d="M10.3 10.3 14 14" /></svg>
                  {note}
                </span>
              {/if}
            {/if}
          </span>
        </div>
      {/each}

      {#if only}
        {#each visible as { row, index } (row.id)}
          <span class="flat-dot" style:top="{y(index) - 4}px" style:background={lane(row.graph.color)}></span>
        {/each}
      {:else}
      <svg class="lines" class:faded={!!found} width="100%" height={PAD_TOP * 2 + rows.length * ROW} aria-hidden="true">
        <!-- A line that starts below newer work on other branches: a gray lead-in fills its column above it. -->
        {#each leadIns as lead, k (k)}
          {#if lead.row > first - OVERSCAN}
            <path d="M{x(lead.column)} 0V{y(lead.row) - 10}" stroke="var(--lead-in)" stroke-width="2" stroke-dasharray="2 5" stroke-linecap="round" fill="none" opacity={lead.opacity} />
          {/if}
        {/each}
        {#each drawn as { row, index } (row.id)}
          {#each segmentsOf(row) as seg, k (k)}
            <path
              d={segmentPath(index, seg.from, seg.to)}
              stroke={lane(seg.color)}
              stroke-width={seg.thick ? 4 : 2}
              opacity={seg.opacity}
              stroke-dasharray={seg.dashed ? "2 5" : undefined}
              stroke-linecap="round"
              fill="none"
            />
          {/each}
        {/each}
      </svg>

      {#each visible as { row, index } (row.id)}
        {@const d = dot(row, index)}
        <span class="halo" class:faded={faded(row)} style:left="{d.left}px" style:top="{d.top}px" style:width="{d.outer}px" style:height="{d.outer}px" style:background={d.halo}>
          <span class="gap" style:width="{d.size + 4}px" style:height="{d.size + 4}px" style:background={d.gap}>
            <span class="dot" style:width="{d.size}px" style:height="{d.size}px" style:background={d.fill} style:border-color={d.ring}>
              {#if d.hole}<span class="hole"></span>{/if}
            </span>
          </span>
        </span>
      {/each}
      {/if}
    </div>
  </div>

  {#if menu}
    <Menu x={menu.x} y={menu.y} label="Commit menu" entries={menu.entries} onClose={() => (menu = null)} />
  {/if}

  {#if scrollTop > ROW * 3}
    <button class="top" onclick={toTop} aria-label="Go to the newest commit" title="Go to the newest commit (Home)">
      <svg class="icon" viewBox="0 0 16 16"><path d="M8 13V3.5M3.5 8 8 3.5 12.5 8" /></svg>
      <span>Top</span>
    </button>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
    flex-grow: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .top {
    position: absolute;
    right: 16px;
    bottom: 16px;
    z-index: 3;
    display: flex;
    align-items: center;
    gap: 4px;
    height: 32px;
    padding: 0 12px 0 10px;
    border-radius: 16px;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    -webkit-backdrop-filter: blur(20px);
    backdrop-filter: blur(20px);
    color: var(--icon);
    font-size: 12px;
    font-weight: 600;
  }
  .top span {
    color: var(--text);
  }
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
  /* One line: the message, then labels, author and time at the right. */
  .row.compact {
    height: 28px;
    flex-direction: row;
    align-items: center;
    gap: 10px;
  }
  .row.compact .summary {
    flex: 0 1 auto;
    min-width: min(45%, 160px);
  }
  /* Kept at the right while there is room; when there isn't, it gives way from its right end. */
  .row.compact .meta {
    flex: 0 1 auto;
    margin-left: auto;
  }
  .row.flash {
    background: var(--flash);
  }
  .row.menu-open {
    box-shadow: inset 0 0 0 1.5px var(--ring);
  }
  .summary {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .summary.selected {
    font-weight: 600;
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
  /* Not on the remote yet, like an unpushed commit's dashed line. */
  .pill.tag.local {
    border-style: dashed;
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
  /* A search fades the commits that don't match, and the lines between them. */
  .row.faded {
    opacity: 0.4;
  }
  .lines.faded {
    opacity: 0.35;
  }
  .halo.faded {
    opacity: 0.35;
  }
  mark {
    color: inherit;
    font-weight: 600;
    background: var(--found-bg);
    box-shadow: 0 0 0 1px var(--found-ring);
    border-radius: 3px;
    padding: 0 1px;
  }
  .found-note {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex-shrink: 1;
    min-width: 0;
  }
  .flat-dot {
    position: absolute;
    left: 22px;
    width: 8px;
    height: 8px;
    border-radius: 4px;
    pointer-events: none;
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
    background: var(--graph-bg);
  }
</style>
