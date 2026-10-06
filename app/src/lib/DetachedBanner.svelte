<script lang="ts">
  // Under the toolbar while HEAD is not on a branch: what that means and the two ways out.

  import type { History } from "./types";
  import { lane, NO_BRANCH_COLOR, plate, shortId, tint } from "./format";
  import { onNoBranch } from "./branches";

  let { history, back, onBack, onKeep }: { history: History; back: string | null; onBack: () => void; onKeep: () => void } = $props();

  const commit = $derived(history.head.commit ?? "");
  const lost = $derived(onNoBranch(history));
  const tag = $derived(history.refs.find((r) => r.kind === "tag" && r.target === commit)?.name);
  // Gray, the color of what belongs to no branch.
  const color = NO_BRANCH_COLOR;
  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
  // The commit the detached commits were made on top of.
  const base = $derived.by(() => {
    const oldest = lost[lost.length - 1];
    return oldest?.parents[0] ? shortId(oldest.parents[0]) : null;
  });
</script>

<section
  class="banner"
  aria-label="Detached HEAD"
  style:background={tint(color, "soft")}
  style:border-color="color-mix(in srgb, {lane(color)} 30%, transparent)"
>
  <span class="tile" style:background={tint(color, "bar")} style:color={plate(color)}>
    <svg class="icon" viewBox="0 0 16 16"><path d="M2 8h3M11 8h3M5 8a3 3 0 1 0 6 0a3 3 0 1 0-6 0" /></svg>
  </span>
  <span class="words">
    <span class="title">{lost.length ? "HEAD is not on a branch" : `HEAD is at ${tag ?? shortId(commit)}, not on a branch`}</span>
    <span class="sub">
      {#if lost.length}
        You made {plural(lost.length, "commit")}{base ? ` on top of ${base}` : ""}. Create a branch to keep them: switching away leaves them behind.
      {:else}
        HEAD points at {tag ? `the tagged commit ${shortId(commit)}` : `commit ${shortId(commit)}`}. Fine for building and testing. To change something, create a branch first.
      {/if}
    </span>
  </span>
  {#if lost.length}<span class="warn">{plural(lost.length, "commit")} on no branch</span>{/if}
  {#if back}<button class="btn" onclick={onBack}>Back to {back}</button>{/if}
  <button class="btn go" onclick={onKeep}>Create Branch…</button>
</section>

<style>
  .banner {
    margin: 0 12px 10px 12px;
    padding: 0 12px 0 14px;
    height: 58px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 12px;
    border-radius: 16px;
    border: 0.5px solid;
  }
  .tile {
    width: 32px;
    height: 32px;
    flex-shrink: 0;
    border-radius: 10px;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .words {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .title {
    font-weight: 700;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub {
    font-size: 12px;
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .warn {
    font-size: 12px;
    color: var(--orange);
    white-space: nowrap;
    flex-shrink: 0;
  }
  .btn {
    flex-shrink: 0;
    min-width: 120px;
    height: 30px;
    padding: 0 14px;
    border-radius: 15px;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    font-weight: 500;
    white-space: nowrap;
  }
  .btn.go {
    background: var(--accent);
    border-color: transparent;
    color: #fff;
    font-weight: 600;
  }
</style>
