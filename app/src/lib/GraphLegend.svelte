<script lang="ts">
  // "How to Read the Graph": what each mark of the graph means, each with an example from the
  // open repository that a click selects.

  import type { History, HistoryRow } from "./types";
  import { lane, relativeTime } from "./format";

  let {
    history,
    shown,
    onShow,
  }: {
    /** The whole history, where the examples come from. */
    history: History;
    /** The history the graph shows: the whole one, or Current Branch Only with its cut lines. */
    shown: History;
    /** Select a commit in the graph and scroll to it. */
    onShow: (id: string) => void;
  } = $props();

  let open = $state(false);

  type Mark = "head" | "trunk" | "branch" | "merge" | "unpushed" | "fork" | "tag" | "stash" | "leadIn" | "noBranch" | "cut" | "parent";
  interface Entry {
    mark: Mark;
    name: string;
    text: string;
    /** The first commit in the graph that shows the mark; none, the repository has none. */
    row?: HistoryRow;
  }

  /** Lane colors outside the branch palettes: stashes and commits on no branch. */
  const STASH = 10;
  const NO_BRANCH = 11;
  const commits = $derived(history.rows.filter((r) => !r.worktree));
  const find = (test: (r: HistoryRow) => boolean) => commits.find(test);
  const isStash = (r: HistoryRow) => r.graph.color === STASH;
  const cut = $derived(shown.rows.find((r) => !!r.graph.cuts?.length));

  const entries = $derived.by((): Entry[] => {
    const trunk = history.trunk ?? "main";
    const list: Entry[] = [
      { mark: "head", name: "HEAD", text: "The commit you have checked out. Its branch takes the leftmost line.", row: find((r) => r.id === history.head.commit) },
      { mark: "trunk", name: trunk, text: `The production branch: its first-parent line, thicker and in Steel.`, row: find((r) => r.graph.color === 0) },
      { mark: "branch", name: "Branch", text: "Every other branch keeps one color, picked by its name.", row: find((r) => r.id !== history.head.commit && r.graph.color > 0 && r.graph.color < STASH && r.labels.some((l) => l.kind === "local" || l.kind === "remote")) },
      { mark: "merge", name: "Merge", text: "Two halves: the line it continues and the branch it brought in.", row: find((r) => r.parents.length > 1 && !isStash(r)) },
      { mark: "unpushed", name: "Not pushed", text: "Dashed line, hollow dot: the remote doesn’t have this commit yet.", row: find((r) => r.unpushed && !isStash(r) && r.id !== history.head.commit) ?? find((r) => r.unpushed && !isStash(r)) },
      { mark: "fork", name: "Branch point", text: "A thin ring in the color of the branch that starts here.", row: find((r) => r.graph.forkColors.length > 0) },
      { mark: "tag", name: "Tag", text: "Yellow capsule. A dashed edge: the remote doesn’t have the tag yet.", row: find((r) => r.labels.some((l) => l.kind === "tag")) },
      { mark: "stash", name: "Stash", text: "Gray dashed line down to the commit it was made on.", row: find(isStash) },
      { mark: "leadIn", name: "Starts lower", text: "Gray dotted line from the top: newer commits of other branches come first.", row: history.leadIns.length ? history.rows[history.leadIns[0].row] : undefined },
      { mark: "noBranch", name: "On no branch", text: "Gray: made on a detached HEAD. Nothing keeps it once you switch away.", row: find((r) => r.noBranch) },
    ];
    if (cut) list.push({ mark: "cut", name: "Hidden line", text: "Current Branch Only hides where this line goes: it stops short.", row: cut });
    list.push({ mark: "parent", name: "Parent", text: "Select a commit: the commits it was made on say ↑ parent (↑ parent 1 and 2 for a merge)." });
    return list;
  });

  /** The colors a mark's picture is drawn in: its example's, or the trunk's and a branch color. */
  function colors(entry: Entry): { a: number; b: number } {
    const g = entry.row?.graph;
    if (!g) return { a: entry.mark === "trunk" ? 0 : entry.mark === "stash" ? STASH : entry.mark === "noBranch" ? NO_BRANCH : 4, b: 0 };
    if (entry.mark === "merge") return { a: g.color, b: g.mergeColors[0] ?? 4 };
    if (entry.mark === "fork") return { a: g.color, b: g.forkColors[0] };
    if (entry.mark === "cut") return { a: g.color, b: g.cuts?.[0]?.color ?? g.color };
    return { a: g.color, b: 0 };
  }

  function onKey(event: KeyboardEvent) {
    if (open && event.key === "Escape") {
      event.preventDefault();
      open = false;
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<span class="anchor">
  <button class="legend-button" class:on={open} onclick={() => (open = !open)} aria-expanded={open} aria-label="How to read the graph" title="How to Read the Graph">
    <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6.3" /><path d="M6.3 6.3a1.8 1.8 0 1 1 2.4 1.7c-.5.2-.7.6-.7 1.1v.4M8 11.4v.1" /></svg>
  </button>
  {#if open}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="catcher" onclick={() => (open = false)}></div>
    <div class="pop" role="dialog" aria-label="How to read the graph">
      <span class="title">How to Read the Graph</span>
      <span class="lead">Examples come from this repository: click one to find it in the graph.</span>
      <ul>
        {#each entries as entry (entry.mark)}
          {@const c = colors(entry)}
          <li>
            <svg class="glyph" viewBox="0 0 44 34" aria-hidden="true">
              {#if entry.mark === "head"}
                <path d="M32 0V34" stroke={lane(0)} stroke-width="3" opacity="0.35" />
                <path d="M12 0V34" stroke={lane(c.a)} stroke-width="2" />
                <circle cx="12" cy="17" r="7.5" fill={lane(c.a)} />
              {:else if entry.mark === "trunk"}
                <path d="M14 0V34" stroke={lane(0)} stroke-width="3" />
                <circle cx="14" cy="17" r="6.5" fill={lane(0)} />
              {:else if entry.mark === "branch"}
                <path d="M14 0V34" stroke={lane(0)} stroke-width="3" opacity="0.35" />
                <path d="M32 0V9C32 17 14 17 14 26" stroke={lane(c.a)} stroke-width="2" fill="none" />
                <circle cx="32" cy="8" r="5.5" fill={lane(c.a)} />
              {:else if entry.mark === "merge"}
                <path d="M14 0V34" stroke={lane(c.a)} stroke-width={c.a === 0 ? 3 : 2} />
                <path d="M14 17C14 26 32 26 32 34" stroke={lane(c.b)} stroke-width="2" fill="none" />
                <path d="M14 8a9 9 0 0 0 0 18z" fill={lane(c.a)} />
                <path d="M14 8a9 9 0 0 1 0 18z" fill={lane(c.b)} />
                <circle cx="14" cy="17" r="4" fill="var(--graph-bg)" />
              {:else if entry.mark === "unpushed"}
                <path d="M14 0V34" stroke={lane(c.a)} stroke-width="2" stroke-dasharray="2 5" stroke-linecap="round" />
                <circle cx="14" cy="17" r="5.5" fill="var(--graph-bg)" stroke={lane(c.a)} stroke-width="2" />
              {:else if entry.mark === "fork"}
                <path d="M14 0V34" stroke={lane(c.a)} stroke-width={c.a === 0 ? 3 : 2} />
                <path d="M32 0C32 10 14 8 14 17" stroke={lane(c.b)} stroke-width="2" fill="none" />
                <circle cx="14" cy="17" r="10" fill={lane(c.b)} />
                <circle cx="14" cy="17" r="8.4" fill="var(--graph-bg)" />
                <circle cx="14" cy="17" r="6.5" fill={lane(c.a)} />
              {:else if entry.mark === "tag"}
                <rect x="4" y="10" width="36" height="14" rx="7" fill="var(--tag-bg)" stroke="var(--tag-border)" />
                <path d="M9.5 14.5h3.2l2.8 2.5-2.8 2.5H9.5z" fill="none" stroke="var(--tag-fg)" stroke-width="1.1" stroke-linejoin="round" />
                <text x="18" y="20.2" fill="var(--tag-fg)">v1.2</text>
              {:else if entry.mark === "stash"}
                <path d="M14 0V34" stroke={lane(0)} stroke-width="3" opacity="0.35" />
                <path d="M32 8C32 17 14 17 14 26" stroke={lane(STASH)} stroke-width="2" stroke-dasharray="2 5" stroke-linecap="round" fill="none" />
                <circle cx="32" cy="8" r="5.5" fill="var(--graph-bg)" stroke={lane(STASH)} stroke-width="2" />
                <circle cx="14" cy="26" r="5.5" fill={lane(0)} />
              {:else if entry.mark === "leadIn"}
                <path d="M14 0V17" stroke="var(--lead-in)" stroke-width="2" stroke-dasharray="2 5" stroke-linecap="round" />
                <path d="M14 22V34" stroke={lane(c.a)} stroke-width="2" />
                <circle cx="14" cy="24" r="5.5" fill={lane(c.a)} />
              {:else if entry.mark === "noBranch"}
                <path d="M14 0V34" stroke={lane(NO_BRANCH)} stroke-width="2" />
                <circle cx="14" cy="17" r="5.5" fill={lane(NO_BRANCH)} />
              {:else if entry.mark === "cut"}
                <path d="M14 0V34" stroke={lane(c.a)} stroke-width={c.a === 0 ? 3 : 2} />
                <path d="M14 13C14 19 30 17 30 23" stroke={lane(c.b)} stroke-width="2" fill="none" />
                <circle cx="30" cy="27" r="1.3" fill={lane(c.b)} />
                <circle cx="30" cy="31" r="1.3" fill={lane(c.b)} />
                <path d="M14 4a9 9 0 0 0 0 18z" fill={lane(c.a)} />
                <path d="M14 4a9 9 0 0 1 0 18z" fill={lane(c.b)} />
                <circle cx="14" cy="13" r="4" fill="var(--graph-bg)" />
              {:else}
                <path d="M14 0V34" stroke={lane(4)} stroke-width="2" />
                <circle cx="14" cy="8" r="5" fill={lane(4)} />
                <circle cx="14" cy="25" r="5" fill={lane(4)} />
                <text x="22" y="28" fill={lane(4)}>↑</text>
              {/if}
            </svg>
            <span class="words">
              <span class="name">{entry.name}</span>
              <span class="text">{entry.text}</span>
              {#if entry.row}
                {@const row = entry.row}
                <button class="example" onclick={() => onShow(row.id)} title="Show in the graph">
                  <span class="summary">{row.summary || row.id.slice(0, 7)}</span>
                  <span class="when">{relativeTime(row.time)}</span>
                </button>
              {:else if entry.mark !== "parent"}
                <span class="none">None in this repository</span>
              {/if}
            </span>
          </li>
        {/each}
      </ul>
    </div>
  {/if}
</span>

<style>
  .anchor {
    position: relative;
    display: inline-flex;
    flex-shrink: 0;
  }
  .legend-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: var(--radius-pill);
    color: var(--text2);
  }
  .legend-button:hover,
  .legend-button.on {
    background: var(--field);
    color: var(--text);
  }
  .legend-button .icon {
    width: 15px;
    height: 15px;
  }
  .catcher {
    position: fixed;
    inset: 0;
    z-index: 19;
  }
  .pop {
    position: absolute;
    right: -6px;
    top: 28px;
    z-index: 20;
    width: 360px;
    max-width: calc(100vw - 32px);
    max-height: min(560px, calc(100vh - 140px));
    overflow-y: auto;
    padding: 14px 8px 8px 14px;
    border-radius: var(--radius-xl);
    background: var(--sheet);
    -webkit-backdrop-filter: blur(30px) saturate(1.8);
    backdrop-filter: blur(30px) saturate(1.8);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--shadow-pop);
    display: flex;
    flex-direction: column;
    gap: 4px;
    text-align: left;
    color: var(--text);
    font-weight: 400;
  }
  .title {
    font-size: var(--text-body);
    font-weight: 600;
  }
  .lead {
    font-size: var(--text-small);
    color: var(--text2);
    padding-right: 6px;
  }
  ul {
    list-style: none;
    margin: 6px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  li {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 5px 6px 5px 0;
  }
  .glyph {
    width: 44px;
    height: 34px;
    flex-shrink: 0;
    border-radius: var(--radius-m);
    background: var(--graph-bg);
  }
  .glyph text {
    font-size: 8.5px;
    font-weight: 600;
  }
  .words {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .name {
    font-size: var(--text-small);
    font-weight: 600;
  }
  .text {
    font-size: var(--text-small);
    color: var(--text2);
    line-height: 1.35;
  }
  .example {
    display: flex;
    align-items: baseline;
    gap: 6px;
    min-width: 0;
    max-width: 100%;
    margin-top: 2px;
    padding: 1px 6px;
    margin-left: -6px;
    border-radius: var(--radius-s);
    font-size: var(--text-small);
    color: var(--accent-text);
    text-align: left;
  }
  .example:hover {
    background: var(--field);
  }
  .example .summary {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .example .when {
    color: var(--text2);
    font-size: var(--text-caption);
    white-space: nowrap;
    flex-shrink: 0;
  }
  .none {
    margin-top: 2px;
    font-size: var(--text-caption);
    color: var(--text2);
    opacity: 0.75;
  }
</style>
