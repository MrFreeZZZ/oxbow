<script lang="ts">
  // Under the toolbar while a merge, rebase, cherry-pick or revert waits: what is going on, how
  // many conflicts are left, and the ways to finish or abort it.

  import type { Operation } from "./types";
  import { lane, plate, shortId, tint } from "./format";
  import { describe } from "./merge";
  import { menuIcons } from "./Menu.svelte";

  let {
    op,
    color,
    resolving,
    onResolve,
    onAbort,
    onSkip,
    onContinue,
  }: {
    op: Operation;
    color: number;
    /** On the Conflicts screen rather than in History. */
    resolving: boolean;
    onResolve: () => void;
    onAbort: () => void;
    onSkip: () => void;
    onContinue: () => void;
  } = $props();

  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
  const words = $derived(describe(op));
  const commits = $derived(op.kind === "merge" || op.kind === "squash");
  const finish = $derived(commits ? `Commit ${words.noun}` : `Continue ${words.noun}`);
  const title = $derived(
    resolving
      ? words.title
      : op.kind === "squash" && !op.incoming
        ? `Squash merge in progress on ${op.branch ?? "HEAD"}`
        : `${words.noun} in progress: ${words.title.replace(/^\S+ /, "")}`,
  );
  const sub = $derived.by(() => {
    if (!resolving) {
      return op.conflicted
        ? `Git stopped on ${op.conflicted === 1 ? "a conflict" : "conflicts"}. Finish or abort the ${words.verb} before you switch branches, pull or push.`
        : `Every conflict is resolved. ${commits ? `Commit the ${words.verb}` : `Continue the ${words.verb}`} to finish it.`;
    }
    const c = op.commit;
    switch (op.kind) {
      case "merge":
        return `Bringing in ${plural(op.incomingCount, "commit")} from ${op.incoming}`;
      case "squash":
        return op.incoming ? `All changes of ${op.incoming}, as one new commit` : "All changes of the squashed branch, as one new commit";
      case "rebase":
        return `${op.step ? `Commit ${op.step[0]} of ${op.step[1]}` : "Replaying"}${c ? `: ${shortId(c.id)} ${c.summary}` : ""}`;
      default:
        return c ? `${shortId(c.id)} ${c.summary}${c.authorName ? ` by ${c.authorName}` : ""}` : "";
    }
  });
</script>

<section
  class="banner"
  aria-label="{words.noun} in progress"
  style:background={tint(color, "soft")}
  style:border-color="color-mix(in srgb, {lane(color)} 30%, transparent)"
>
  <span class="tile" style:background={tint(color, "bar")} style:color={plate(color)}>
    {#if op.kind === "rebase"}
      <svg class="icon" viewBox="0 0 16 16"><path d={menuIcons.rebase} /></svg>
    {:else if op.kind === "cherryPick"}
      <svg class="icon" viewBox="0 0 16 16"><path d={menuIcons.cherry} /></svg>
    {:else if op.kind === "revert"}
      <svg class="icon" viewBox="0 0 16 16"><path d={menuIcons.revert} /></svg>
    {:else}
      <svg class="icon" viewBox="0 0 16 16"><path d="M4.5 2v12M4.5 4.5c0 3.5 7 2.5 7 6v3.5M2.5 12 4.5 14l2-2" /></svg>
    {/if}
  </span>
  <span class="words">
    <span class="title">{title}</span>
    <span class="sub">{sub}</span>
  </span>
  {#if op.conflicted}
    <span class="left">{plural(op.conflicted, "file")} to resolve</span>
  {:else}
    <span class="done">All resolved</span>
  {/if}
  <button class="btn" onclick={onAbort}>Abort {words.noun}</button>
  {#if resolving && (op.kind === "rebase" || op.kind === "cherryPick")}
    <button class="btn" onclick={onSkip}>Skip Commit</button>
  {/if}
  {#if !resolving && op.conflicted}
    <button class="btn go" onclick={onResolve}>Resolve Conflicts</button>
  {:else}
    <button class="btn go" disabled={op.conflicted > 0} title={op.conflicted ? "Resolve every conflict first" : undefined} onclick={onContinue}>{finish}</button>
  {/if}
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
  .left,
  .done {
    font-size: 12px;
    white-space: nowrap;
    flex-shrink: 0;
  }
  .left {
    color: var(--orange);
  }
  .done {
    color: var(--green);
  }
  .btn {
    flex-shrink: 0;
    min-width: 110px;
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
  .btn:disabled {
    background: var(--field);
    color: var(--text2);
    box-shadow: none;
  }
</style>
