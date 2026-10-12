<script lang="ts">
  // Under the toolbar while a bisect goes on: the range, the commit under test with Good, Bad
  // and Skip, and what git said to the last answer. Once git found the first bad commit: that
  // commit, Show Commit and Finish.

  import type { Bisect, HistoryRow } from "./types";
  import { lane, NO_BRANCH_COLOR, plate, shortId, tint } from "./format";
  import { actionIcons } from "./icons";
  import { withKeys } from "./keys";
  import { estimateSteps } from "./bisect";

  let {
    bisect,
    lookup,
    onStop,
    onSkip,
    onGood,
    onBad,
    onShow,
    onFinish,
    copy,
  }: {
    bisect: Bisect;
    lookup: (id: string) => HistoryRow | undefined;
    onStop: () => void;
    onSkip: () => void;
    onGood: () => void;
    onBad: () => void;
    /** Select a commit in the graph. */
    onShow: (id: string) => void;
    onFinish: () => void;
    copy: (text: string) => void;
  } = $props();

  const color = NO_BRANCH_COLOR;
  /** "Sep 29, 16:02", as in the design. */
  const when = (seconds: number) => new Date(seconds * 1000).toLocaleString("en-US", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
  const ready = $derived(bisect.good.length > 0 && bisect.bad.length > 0);
  const found = $derived(bisect.found ? lookup(bisect.found) : undefined);
  const testing = $derived(bisect.testing ? lookup(bisect.testing) : undefined);
  const stuck = $derived(!bisect.found && bisect.possible.length > 0);

  const title = $derived.by(() => {
    if (bisect.found) return `Found it: ${shortId(bisect.found)} is the first ${bisect.termBad} commit`;
    if (stuck) return `Only skipped commits are left: it is one of ${bisect.possible.length}`;
    if (!ready) return bisect.bad.length ? `Bisecting: mark a commit where it still worked as ${bisect.termGood}` : `Bisecting: mark a commit where it is broken as ${bisect.termBad}`;
    return `Bisecting between ${bisect.firstGoodLabel} (${bisect.termGood}) and ${bisect.firstBadLabel} (${bisect.termBad})`;
  });
  const sub = $derived.by(() => {
    if (bisect.found) {
      return found
        ? `“${found.summary}” by ${found.authorName}, ${when(found.time)}. Everything before it was ${bisect.termGood}.`
        : `Everything before it was ${bisect.termGood}.`;
    }
    if (stuck) return `Git can’t tell which: ${bisect.possible.map(shortId).join(", ")}. Test one of them, or Finish.`;
    const at = testing ? `Now on ${shortId(testing.id)} “${testing.summary}”.` : bisect.testing ? `Now on ${shortId(bisect.testing)}.` : "";
    return `${at} Build and test it, then tell Oxbow how it went.`;
  });
  const progress = $derived.by(() => {
    if (bisect.found || stuck) return plural(bisect.steps, "step");
    if (!ready) return "";
    return `Step ${bisect.steps + 1} · ${bisect.stepsLeft ? `about ${plural(bisect.stepsLeft, "more step")}` : "the last one"}`;
  });

  let copied = $state(false);
  function copyLog() {
    copy(bisect.log);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  const lineKind = (line: string) => (line.startsWith("$ ") ? "cmd" : / is the first \S+ commit$/.test(line) ? "ok" : "out");
  // A long range says so before the first answer: git's estimate covers every step.
  const total = $derived(bisect.candidates.length ? estimateSteps(bisect.candidates.length) + 1 : 0);
</script>

<section
  class="banner"
  aria-label="Bisect"
  style:background={tint(color, "soft")}
  style:border-color="color-mix(in srgb, {lane(color)} 30%, transparent)"
>
  <div class="top">
    <span class="tile" style:background={tint(color, "bar")} style:color={plate(color)}>
      <svg class="icon" viewBox="0 0 16 16"><path d={actionIcons.bisect} /></svg>
    </span>
    <span class="words">
      <span class="title">{title}</span>
      <span class="sub">{sub}</span>
    </span>
    {#if progress}<span class="steps" title={total && !bisect.found ? `About ${plural(total, "step")} for the whole range` : undefined}>{progress}</span>{/if}
    {#if bisect.found || stuck}
      {#if bisect.found}<button class="btn" onclick={() => onShow(bisect.found!)}>Show Commit</button>{/if}
      <button class="btn go" onclick={onFinish} title="Back to {bisect.startIsBranch ? bisect.start : shortId(bisect.start)}">Finish</button>
    {:else}
      <button class="btn" onclick={onStop} title="Stop bisecting and go back to {bisect.startIsBranch ? bisect.start : shortId(bisect.start)}">Stop</button>
      {#if ready}<button class="btn" onclick={onSkip} title={withKeys("Can’t test this commit: skip it", "S")}>Skip</button>{/if}
      <button class="btn good" onclick={onGood} title={withKeys(`It works here: mark as ${bisect.termGood}`, "G")}>{bisect.termGood === "good" ? "Good" : bisect.termGood}</button>
      <button class="btn bad" onclick={onBad} title={withKeys(`It is broken here: mark as ${bisect.termBad}`, "B")}>{bisect.termBad === "bad" ? "Bad" : bisect.termBad}</button>
    {/if}
  </div>
  {#if bisect.output.length}
    <div class="log mono selectable" aria-label="What git said">
      {#each bisect.output as line, i (i)}
        <div class={lineKind(line)}>{line}</div>
      {/each}
      <button class="copy" onclick={copyLog} title="Copy git bisect log, to replay it with git bisect replay">{copied ? "Copied" : "Copy Log"}</button>
    </div>
  {/if}
</section>

<style>
  .banner {
    margin: 0 12px 10px 12px;
    padding: 10px 12px 12px 14px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    border-radius: 16px;
    border: 0.5px solid;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 38px;
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
  .steps {
    font-size: 12px;
    color: var(--text2);
    white-space: nowrap;
    flex-shrink: 0;
  }
  .btn {
    flex-shrink: 0;
    min-width: 76px;
    height: 30px;
    padding: 0 14px;
    border-radius: 15px;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    font-weight: 500;
    white-space: nowrap;
  }
  .btn.good {
    color: var(--green);
    font-weight: 600;
  }
  .btn.bad {
    color: var(--red);
    font-weight: 600;
  }
  .btn.go {
    background: var(--accent);
    border-color: transparent;
    color: #fff;
    font-weight: 600;
    min-width: 110px;
  }
  .log {
    position: relative;
    background: var(--term);
    border: 0.5px solid var(--term-border);
    box-shadow: var(--term-shadow);
    border-radius: 10px;
    padding: 8px 96px 8px 12px;
    font-size: 11.5px;
    line-height: 16px;
    color: var(--term-out);
    max-height: 82px;
    overflow-y: auto;
  }
  .log div {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .log .cmd {
    color: var(--term-git);
    font-weight: 700;
  }
  .log .ok {
    color: var(--term-ok);
    font-weight: 700;
  }
  .copy {
    position: absolute;
    top: 6px;
    right: 8px;
    height: 22px;
    padding: 0 8px;
    border-radius: 6px;
    font-size: 11px;
    color: var(--term-dim);
    background: var(--term-button);
  }
</style>
