<script lang="ts">
  import { withKeys } from "./keys";
  import { untrack } from "svelte";
  import type { History, HistoryRow, Label, RowLayout, Segment, WorktreeSummary } from "./types";
  import { lane, plate, relativeTime, tint } from "./format";
  import Menu, { type MenuEntry } from "./Menu.svelte";
  import { prefs } from "./prefs.svelte";
  import { split, type Found } from "./search.svelte";
  import { bisectPills, bisectRange, type BisectPill } from "./bisect";
  import { segmentsAt } from "./graph";

  let {
    history,
    selected,
    onSelect,
    menuFor,
    found = null,
    second = null,
    onSecond,
    paging = null,
    onMore,
    onUnshallow = null,
    pointed = null,
  }: {
    history: History;
    selected: string | null;
    onSelect: (id: string) => void;
    /** Entries of a commit's right-click menu; none, no menu. */
    menuFor: (row: HistoryRow) => MenuEntry[];
    /** A search: other commits fade, or only the matches are listed. */
    found?: Found | null;
    /** A second commit picked with ⌘-click, compared with the selected one. */
    second?: string | null;
    /** ⌘-click (Ctrl-click off the Mac) on a commit. */
    onSecond?: (id: string) => void;
    /** Older commits are on their way: "more" for the next page, or the id of a commit the
     *  history is loading down to. */
    paging?: string | null;
    /** The list scrolled near the end of a history that has more commits. */
    onMore?: () => void;
    /** Fetch Full History, at the end of a shallow clone's history. */
    onUnshallow?: (() => void) | null;
    /** A commit a branch name or SHA in a git command points at, lit up while its popover is open. */
    pointed?: string | null;
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
  /** Layout and row of each commit and the lead-ins before the change, while it animates. */
  let from = $state<{ history: History; rows: Map<string, { graph: RowLayout; index: number }>; leadIns: Map<string, number> } | null>(null);
  let progress = $state(1);
  let frame = 0;
  let shown: History | null = null;
  let shownLeadIns = new Map<string, number>();

  $effect.pre(() => {
    const next = history;
    untrack(() => {
      const leadIns = new Map(next.leadIns.map((l) => [next.rows[l.row]?.id ?? "", l.column]));
      if (shown && shown.rows !== next.rows) animate(shown, shownLeadIns, next.rows);
      shown = next;
      shownLeadIns = leadIns;
    });
  });

  function animate(old: History, oldLeadIns: Map<string, number>, next: HistoryRow[]) {
    const rows = new Map(old.rows.map((r, index) => [r.id, { graph: r.graph, index }]));
    const moved = next.some((r) => {
      const before = rows.get(r.id)?.graph;
      return before && (before.column !== r.graph.column || before.width !== r.graph.width);
    });
    if (!moved || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    cancelAnimationFrame(frame);
    from = { history: old, rows, leadIns: oldLeadIns };
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
    const old = from?.rows.get(row.id)?.graph;
    return old ? mix(old.column, row.graph.column) : row.graph.column;
  }

  /** The lines of the row at `index`, on their way from the old columns; new ones fade in. */
  function segmentsOf(row: HistoryRow, index: number): (Segment & { opacity: number })[] {
    const segments = segmentsAt(history, index);
    const old = from?.rows.get(row.id);
    if (!old || !from) return segments.map((seg) => ({ ...seg, opacity: 1 }));
    // A copy: the segments may be the old history's own, which a template must not change.
    const pool = [...segmentsAt(from.history, old.index)];
    return segments.map((seg) => {
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
  // A bisect fades what is outside its range, and marks the commits it answered.
  const pills = $derived(bisectPills(history.bisect));
  const inRange = $derived(bisectRange(history.bisect));
  const PILL: Record<BisectPill, string> = { good: "good", bad: "bad", skipped: "skipped", testing: "HEAD · testing", found: "first bad commit", possible: "maybe first bad" };
  const faded = (row: HistoryRow) => (!!found && !found.hits.has(row.id)) || (!!inRange && !row.worktree && !inRange.has(row.id));
  const indexOf = $derived(new Map(rows.map((row, i) => [row.id, i])));
  /** The selected commit's parents, each with what it is called: "parent", or for a merge
   *  "parent 1" (the line it continues) and "parent 2" (the branch it brought in). */
  const kin = $derived.by(() => {
    const at = selected ? indexOf.get(selected) : undefined;
    const row = at === undefined ? null : rows[at];
    const marks = new Map<string, string>();
    if (!row || row.worktree) return { marks, color: 0 };
    row.parents.forEach((id, i) => marks.set(id, row.parents.length > 1 ? `parent ${i + 1}` : "parent"));
    return { marks, color: row.graph.color };
  });
  const first = $derived(Math.max(0, Math.floor((scrollTop - PAD_TOP) / ROW) - OVERSCAN));
  const last = $derived(Math.min(rows.length, Math.ceil((scrollTop + height) / ROW) + OVERSCAN));
  const visible = $derived(rows.slice(first, last).map((row, k) => ({ row, index: first + k })));

  const x = (column: number) => PAD_LEFT + FIRST_LANE + LANE * column;

  // The end of the list: older commits load as it comes into view, or a shallow clone says why
  // the history stops. Only Matches lists matches alone: the search bar loads theirs.
  const loadingOlder = $derived(!only && (history.truncated || !!paging));
  const shallowEnd = $derived(!only && !loadingOlder && history.shallow && !!onUnshallow);
  // The shallow clone's card leaves room under it for the Top button.
  const FOOT = $derived(loadingOlder ? ROW : shallowEnd ? 130 : 0);
  $effect(() => {
    if (loadingOlder && !paging && last >= rows.length - OVERSCAN) untrack(() => onMore?.());
  });

  // Lines: 3px for the production branch, 2px for the others. Lanes sit on whole pixels, so a
  // 2px line covers two pixels exactly; a 3px one would smear over four on a 1x screen and moves
  // half a pixel there. On Retina 3px is six device pixels and needs no nudge.
  let dpr = $state(window.devicePixelRatio || 1);
  $effect(() => {
    const media = matchMedia(`(resolution: ${dpr}dppx)`);
    const changed = () => (dpr = window.devicePixelRatio || 1);
    media.addEventListener("change", changed);
    return () => media.removeEventListener("change", changed);
  });
  const nudge = $derived(Number.isInteger(dpr) && dpr % 2 === 1 ? 0.5 : 0);
  /** The production branch's name, local or on a remote. */
  const isProduction = (name: string) => !!history.trunk && (name === history.trunk || name.endsWith(`/${history.trunk}`));
  const y = (index: number) => PAD_TOP + ROW / 2 + ROW * index;

  /** A line to a parent Current Branch Only hides: half a row down, curving out if it changes column. */
  function cutPath(index: number, from: number, to: number): string {
    const y0 = y(index);
    const y1 = y0 + ROW * 0.36;
    if (from === to) return `M${x(from)} ${y0}V${y1}`;
    return `M${x(from)} ${y0}C${x(from)} ${y0 + ROW * 0.3} ${x(to)} ${y0 + ROW * 0.12} ${x(to)} ${y1}`;
  }

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
    const isSelected = row.id === selected || row.id === second;
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
    const old = from?.rows.get(row.id)?.graph;
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
  /** Hands the keyboard to the list, as Tab from the details panel or opening History do. */
  export function focus() {
    viewport?.focus({ preventScroll: true });
  }

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
  // A local branch and its copy on a remote at the same commit share one capsule, with a cloud.
  const hasLocal = (row: HistoryRow, name: string) => row.labels.some((l) => l.kind === "local" && l.name === name);
  /** The remote branch at the same commit as the local branch `name`, as origin/x for x. */
  const remoteTwin = (row: HistoryRow, name: string) => row.labels.find((l) => l.kind === "remote" && l.name.slice(l.name.indexOf("/") + 1) === name)?.name ?? null;

  // Capsules that don't fit beside the author and time fold into a "+N" chip, instead of being
  // cut down to a letter or two. Widths come from the text in the app's own font, so no row is
  // laid out twice.
  let width = $state(0);
  let pen: CanvasRenderingContext2D | null = null;
  const widths = new Map<string, number>();
  function textWidth(text: string, weight: number, size = 11): number {
    const key = `${weight} ${size} ${text}`;
    let w = widths.get(key);
    if (w === undefined) {
      pen ??= document.createElement("canvas").getContext("2d");
      if (!pen) return text.length * size * 0.6;
      pen.font = `${weight} ${size}px ${getComputedStyle(document.documentElement).getPropertyValue("--font")}`;
      w = pen.measureText(text).width;
      widths.set(key, w);
    }
    return w;
  }
  const PILL_GAP = 6;
  /** A capsule's width: its text, 6 px each side, the border, and an icon for some. */
  const pillWidth = (text: string, weight: number, icon: boolean) => Math.min(190, Math.ceil(textWidth(text, weight)) + 14 + (icon ? 13 : 0));
  /** Narrower than this, a cut capsule says too little to be worth showing. */
  const CUT_MIN = 64;

  /** Which capsule shows first: the checked-out branch, then the production branch, then the
   *  rest as git lists them. */
  const rank = (l: Label) => (l.kind === "head" ? 0 : l.head ? 1 : l.color === 0 && isProduction(l.name) ? 2 : 3);

  /** The labels a row shows as capsules, in order, and those folded into "+N". HEAD and the
   *  bisect marks always show; the rest give way from the end. */
  function fitLabels(row: HistoryRow, pill: BisectPill | undefined): { shown: Label[]; folded: Label[]; last: Label | null } {
    const own = row.labels
      .filter((l) => !(l.kind === "remote" && hasLocal(row, l.name.slice(l.name.indexOf("/") + 1))))
      .map((l, i) => ({ l, i }))
      .sort((a, b) => rank(a.l) - rank(b.l) || a.i - b.i)
      .map(({ l }) => l);
    const foldable = own.filter((l) => l.kind !== "head");
    if (!width || !foldable.length) return { shown: own, folded: [], last: own.at(-1) ?? null };
    const content = width - 16 - textStart(row) - 12;
    let room = content;
    if (compact) room -= 10 + Math.min(Math.ceil(textWidth(row.summary, 400, 13)), content * 0.55);
    const byline = row.worktree ? worktreeLine(row.worktree) : `${row.authorName} · ${relativeTime(row.time)}`;
    room -= Math.ceil(textWidth(byline, 400));
    if (pill && pill !== "testing") room -= pillWidth(PILL[pill], 600, false) + PILL_GAP;
    const mark = kin.marks.get(row.id);
    if (mark) room -= Math.ceil(textWidth(`↑ ${mark} · `, 600));
    for (const l of own) if (l.kind === "head") room -= pillWidth(pill === "testing" ? PILL.testing : "HEAD", 600, false) + PILL_GAP;
    const shown = new Set(own.filter((l) => l.kind === "head"));
    let used = 0;
    for (let i = 0; i < foldable.length; i++) {
      const l = foldable[i];
      const twin = l.kind === "local" && !!remoteTwin(row, l.name);
      const w = pillWidth(l.name, l.color === 0 ? 700 : 500, twin || l.kind === "tag" || l.kind === "stash") + PILL_GAP;
      const rest = foldable.length - i - 1;
      const chip = rest ? pillWidth(`+${rest}`, 600, false) + PILL_GAP : 0;
      if (used + w + chip <= room) {
        shown.add(l);
        used += w;
        continue;
      }
      // The first capsule that doesn't fit whole is cut with an ellipsis, if enough of it shows.
      const chipAfter = rest ? chip : 0;
      if (room - used - chipAfter - PILL_GAP >= CUT_MIN) shown.add(l);
      break;
    }
    const list = own.filter((l) => shown.has(l));
    return { shown: list, folded: foldable.filter((l) => !shown.has(l)), last: list.at(-1) ?? null };
  }
</script>

<div class="wrap">
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="viewport"
    bind:this={viewport}
    bind:clientHeight={height}
    bind:clientWidth={width}
    onscroll={() => (scrollTop = viewport?.scrollTop ?? 0)}
    onkeydown={onKey}
    tabindex="0"
    role="listbox"
    aria-label="Commits"
  >
    <div class="canvas" style:height="{PAD_TOP * 2 + rows.length * ROW + FOOT}px">
      {#each visible as { row, index } (row.id)}
        {@const isSelected = row.id === selected}
        {@const picked = isSelected || row.id === second}
        {@const pill = pills.get(row.id)}
        {@const fit = fitLabels(row, pill)}
        <div
          class="row"
          class:compact
          class:faded={faded(row)}
          class:flash={flash === row.id}
          class:pointed={pointed === row.id}
          class:menu-open={menu?.id === row.id}
          role="option"
          aria-selected={picked}
          tabindex="-1"
          style:top="{PAD_TOP + index * ROW}px"
          style:padding-left="{textStart(row)}px"
          class:picked
          style:--flash={tint(row.graph.color)}
          style:--ring={lane(row.graph.color)}
          onclick={(e) => ((e.metaKey || e.ctrlKey) && onSecond && !row.worktree ? onSecond(row.id) : onSelect(row.id))}
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
            {#if pill && pill !== "testing"}
              <span class="pill bisect {pill}">{pill === "found" && history.bisect?.termBad !== "bad" ? `first ${history.bisect?.termBad} commit` : PILL[pill]}</span>
            {/if}
            <!-- origin/x at the same commit as x rides on x's capsule; the ones with no room are in "+N". -->
            {#each fit.shown as label (label.kind + label.name)}
              {@const twin = label.kind === "local" ? remoteTwin(row, label.name) : null}
              {#if label.kind === "tag"}
                <span class="pill tag" style:flex-shrink={label === fit.last ? 1 : 0} class:local={history.localTags.includes(label.name)} title={history.localTags.includes(label.name) ? `Not on ${history.defaultRemote} yet` : undefined}>
                  <svg class="icon tiny" viewBox="0 0 16 16"><path d="M2.5 2.5h5l6 6-5 5-6-6z" /><circle cx="5.5" cy="5.5" r="0.8" /></svg>
                  {label.name}
                </span>
              {:else if label.kind === "stash"}
                <span class="pill" style:flex-shrink={label === fit.last ? 1 : 0} style:color={plate(label.color)} style:background={tint(label.color, "label")}>
                  <svg class="icon tiny" viewBox="0 0 16 16"><rect x="3" y="6.5" width="10" height="7" rx="1.5" /><path d="M4.5 4.5h7M6 2.5h4" /></svg>
                  {label.name}
                </span>
              {:else if label.kind === "head" && pill === "testing"}
                <span class="pill head testing" title="The commit under test">{PILL.testing}</span>
              {:else if label.kind === "head"}
                <span class="pill head" style:color={plate(label.color)} style:border-color="color-mix(in srgb, {lane(label.color)} 55%, transparent)">HEAD</span>
              {:else if label.kind === "remote"}
                <span class="pill" class:production={label.color === 0 && isProduction(label.name)} style:flex-shrink={label === fit.last ? 1 : 0} style:color={plate(label.color)} style:border-color="color-mix(in srgb, {lane(label.color)} 40%, transparent)" title={label.name}>{label.name}</span>
              {:else}
                <!-- Only the last capsule gives way, with an ellipsis; the others show whole. -->
                <span class="pill" class:head={label.head} class:twin class:production={label.color === 0 && isProduction(label.name)} style:flex-shrink={label === fit.last ? 1 : 0} style:color={plate(label.color)} style:background={tint(label.color, "label")} title={twin ? `${label.name}, same as ${twin}` : label.name}>{#if twin}<svg class="icon tiny" viewBox="0 0 16 16"><path d="M4.5 12.5h7a3 3 0 0 0 .4-6 4 4 0 0 0-7.6-1A3.5 3.5 0 0 0 4.5 12.5z" /></svg>{/if}{label.name}</span>
              {/if}
            {/each}
            {#if fit.folded.length}
              <span class="pill more" title={fit.folded.map((l) => l.name).join("\n")}>+{fit.folded.length}</span>
            {/if}
            {#if row.worktree}
              <span class="byline">{worktreeLine(row.worktree)}</span>
            {:else}
              <span class="byline">
                {#if kin.marks.has(row.id)}<span class="kin" style:color={plate(kin.color)} title="The selected commit was made on this one">↑ {kin.marks.get(row.id)}</span> ·{/if}
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
          {#each segmentsOf(row, index) as seg, k (k)}
            <path
              d={segmentPath(index, seg.from, seg.to)}
              stroke={lane(seg.color)}
              stroke-width={seg.thick ? 3 : 2}
              transform={seg.thick && nudge ? `translate(${nudge} 0)` : undefined}
              opacity={seg.opacity}
              stroke-dasharray={seg.dashed ? "2 5" : undefined}
              stroke-linecap="round"
              fill="none"
            />
          {/each}
          <!-- Where the line goes is hidden: it stops, and three dots say it goes on. -->
          {#each row.graph.cuts ?? [] as cut, k (k)}
            <path d={cutPath(index, cut.from, cut.to)} stroke={lane(cut.color)} stroke-width="2" stroke-dasharray={cut.dashed ? "2 5" : undefined} stroke-linecap="round" fill="none" />
            {#each [0, 1, 2] as d (d)}
              <circle cx={x(cut.to)} cy={y(index) + ROW * 0.36 + 4.5 + d * 4} r="1.3" fill={lane(cut.color)} opacity={1 - d * 0.25} />
            {/each}
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

      {#if loadingOlder}
        <div class="foot" style:top="{PAD_TOP + rows.length * ROW}px" style:height="{ROW}px" style:padding-left="{x(0) + 4}px" role="status">
          <span class="spinner" aria-hidden="true"></span>
          {#if paging && paging !== "more"}
            Loading history down to <code>{paging.slice(0, 7)}</code>…
          {:else}
            Loading older commits…
          {/if}
        </div>
      {:else if shallowEnd}
        <div class="end" style:top="{PAD_TOP + rows.length * ROW + 8}px">
          <svg class="icon" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="6.3" /><path d="M8 7.2v4M8 4.9v.1" /></svg>
          <span class="end-text">
            History ends here
            <span class="sub">This is a shallow clone: it has only the latest {history.loaded.toLocaleString()} {history.loaded === 1 ? "commit" : "commits"}.</span>
          </span>
          <button class="button small" onclick={() => onUnshallow?.()}>Fetch Full History…</button>
        </div>
      {/if}
    </div>
  </div>

  {#if menu}
    <Menu x={menu.x} y={menu.y} label="Commit menu" entries={menu.entries} onClose={() => (menu = null)} />
  {/if}

  {#if scrollTop > ROW * 3}
    <button class="top" onclick={toTop} aria-label="Go to the newest commit" title={withKeys("Go to the Newest Commit", "Home")}>
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
    border-radius: var(--radius-pill);
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    -webkit-backdrop-filter: blur(20px);
    backdrop-filter: blur(20px);
    color: var(--icon);
    font-size: var(--text-small);
    font-weight: 600;
  }
  .top span {
    color: var(--text);
  }
  .foot {
    position: absolute;
    left: 0;
    right: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .foot code {
    font-family: var(--mono);
    color: var(--text);
  }
  .spinner {
    width: 11px;
    height: 11px;
    flex-shrink: 0;
    border-radius: 50%;
    border: 1.5px solid color-mix(in srgb, var(--text2) 30%, transparent);
    border-top-color: var(--text2);
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .end {
    position: absolute;
    left: 12px;
    right: 12px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: var(--space-3);
    border-radius: var(--radius-l);
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    font-size: var(--text-body);
  }
  .end .icon {
    width: 15px;
    height: 15px;
    flex-shrink: 0;
    color: var(--text2);
  }
  .end-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex-grow: 1;
    font-weight: 500;
  }
  .end-text .sub {
    font-weight: 400;
    font-size: var(--text-small);
    color: var(--text2);
    white-space: normal;
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
    padding-right: var(--space-3);
    border-radius: var(--radius-m);
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
  /* A soft wash under the pointer, so it's clear which commit a click picks. */
  .row:hover {
    background: var(--row-hover);
    transition-duration: 0.1s;
  }
  /* The selected commit: its branch tint while the list has the focus, grey once the focus is
     elsewhere or the window is in the background, as macOS lists do. */
  .row.picked {
    background: var(--sel-idle);
  }
  .viewport:focus-within .row.picked {
    background: var(--flash);
  }
  :global(:root[data-window="inactive"]) .viewport .row.picked {
    background: var(--sel-idle);
  }
  .row.flash {
    background: var(--flash);
  }
  .row.menu-open,
  .row.pointed {
    box-shadow: inset 0 0 0 1.5px var(--ring);
  }
  .row.pointed {
    background: var(--flash);
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
    font-size: var(--text-caption);
    overflow: hidden;
  }
  /* Capsules show whole; the last one may give way with an ellipsis. Author and time keep their room. */
  .pill {
    display: inline-block;
    height: 16px;
    line-height: 14px;
    padding: 0 6px;
    border-radius: var(--radius-pill);
    border: 1px solid transparent;
    font-weight: 500;
    flex-shrink: 0;
    min-width: 34px;
    max-width: 190px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pill .tiny {
    vertical-align: -1px;
    margin-right: 3px;
  }
  .pill.twin {
    min-width: 48px;
  }
  /* The capsules with no room: "+2", their names in the tooltip. */
  .pill.more {
    flex-shrink: 0;
    min-width: 0;
    background: var(--field);
    color: var(--text2);
    font-weight: 600;
  }
  /* HEAD and the bisect marks are short and say what the row is: they keep their whole word. */
  .pill.head,
  .pill.bisect {
    flex-shrink: 0;
  }
  .pill.head {
    font-weight: 600;
  }
  .pill.production {
    font-weight: 700;
  }
  /* "↑ parent": a parent of the selected commit, in its branch color. */
  .kin {
    font-weight: 600;
  }
  /* Bisect answers, in the colors of the banner's Good and Bad. */
  .pill.bisect {
    font-weight: 600;
  }
  .pill.good {
    background: var(--green-soft);
    color: var(--green);
  }
  .pill.bad,
  .pill.found {
    background: var(--red-soft);
    color: var(--red);
  }
  .pill.found {
    font-weight: 700;
  }
  .pill.skipped {
    background: var(--field);
    color: var(--text2);
  }
  .pill.possible {
    background: var(--orange-soft);
    color: var(--orange);
  }
  .pill.testing {
    border-color: var(--text);
    color: var(--text);
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
    flex-shrink: 0;
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
    border-radius: var(--radius-xs);
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
    border-radius: var(--radius-pill);
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
