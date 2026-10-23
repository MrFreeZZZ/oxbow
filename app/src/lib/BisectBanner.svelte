<script lang="ts">
  // Under the toolbar while a bisect goes on: the range, the commit under test with Good, Bad
  // and Skip, the journal of answers so far, and what git said to the last one. Once git found
  // the first bad commit: that commit, Show Commit, Copy Report and Finish.

  import type { Bisect, BisectMark, HistoryRow } from "./types";
  import { lane, NO_BRANCH_COLOR, plate, shortId, tint } from "./format";
  import { actionIcons } from "./icons";
  import { withKeys } from "./keys";
  import { combo } from "./shortcuts";
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
    /** Copy `text`; the toast says `said`. */
    copy: (text: string, said: string) => void;
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
    copy(bisect.log, "Copied the bisect log.");
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  const word = (mark: BisectMark) => (mark === "good" ? bisect.termGood : mark === "bad" ? bisect.termBad : "skip");
  const label = (mark: BisectMark) => {
    const w = word(mark);
    return w === "good" || w === "bad" || w === "skip" ? w[0].toUpperCase() + w.slice(1) : w;
  };
  /** The journal's rows: each answer, then the commit under test or the one found. */
  const rows = $derived.by(() => {
    let n = 0;
    const out: { key: string; at: string; commit: string; summary: string; kind: string; label: string; note?: string }[] = bisect.journal.map((step, i) => ({
      key: `${i}`,
      at: step.range ? "" : `${++n}`,
      commit: step.commit,
      summary: step.summary,
      kind: step.mark === "skip" ? "skipped" : step.mark,
      label: label(step.mark),
      note: step.range ? "sets the range" : undefined,
    }));
    const summaryOf = (id: string) => lookup(id)?.summary ?? "";
    if (bisect.found) out.push({ key: "end", at: "", commit: bisect.found, summary: found?.summary ?? summaryOf(bisect.found), kind: "found", label: `First ${bisect.termBad}` });
    else if (!stuck && ready && bisect.testing) out.push({ key: "end", at: `${n + 1}`, commit: bisect.testing, summary: testing?.summary ?? "", kind: "testing", label: "Testing" });
    return out;
  });

  let journalBox = $state<HTMLDivElement>();
  // The newest answer stays in sight.
  $effect(() => {
    void rows.length;
    if (journalBox) journalBox.scrollTop = journalBox.scrollHeight;
  });

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
      {#if bisect.found}<button class="button" onclick={() => onShow(bisect.found!)}>Show Commit</button>{/if}
      {#if bisect.report}<button class="button" onclick={() => copy(bisect.report!, "Copied the bisect report.")} title="Copy what the bisect found, its range and every step, as text for an issue or a chat">Copy Report</button>{/if}
      <button class="button primary" onclick={onFinish} title="Back to {bisect.startIsBranch ? bisect.start : shortId(bisect.start)}">Finish</button>
    {:else}
      <button class="button" onclick={onStop} title="Stop bisecting and go back to {bisect.startIsBranch ? bisect.start : shortId(bisect.start)}">Stop</button>
      {#if ready}<button class="button" onclick={onSkip} title={withKeys("Can’t test this commit: skip it", combo("bisectSkip"))}>Skip</button>{/if}
      <button class="button good" onclick={onGood} title={withKeys(`It works here: mark as ${bisect.termGood}`, combo("bisectGood"))}>{bisect.termGood === "good" ? "Good" : bisect.termGood}</button>
      <button class="button bad" onclick={onBad} title={withKeys(`It is broken here: mark as ${bisect.termBad}`, combo("bisectBad"))}>{bisect.termBad === "bad" ? "Bad" : bisect.termBad}</button>
    {/if}
  </div>
  {#if rows.length || bisect.output.length}
    <div class="below">
      {#if rows.length}
        <div class="journal" aria-label="Steps" bind:this={journalBox}>
          {#each rows as row (row.key)}
            <button class="step" class:range={!!row.note} onclick={() => onShow(row.commit)} title="Show {shortId(row.commit)} in the graph">
              <span class="at">{row.at}</span>
              <span class="pill {row.kind}">{row.label}</span>
              <span class="mono sha">{shortId(row.commit)}</span>
              <span class="summary">{row.summary}</span>
              {#if row.note}<span class="note">{row.note}</span>{/if}
            </button>
          {/each}
        </div>
      {/if}
      {#if bisect.output.length}
        <div class="log mono selectable" aria-label="What git said">
          {#each bisect.output as line, i (i)}
            <div class={lineKind(line)}>{line}</div>
          {/each}
          <button class="copy" onclick={copyLog} title="Copy git bisect log, to replay it with git bisect replay">{copied ? "Copied" : "Copy Log"}</button>
        </div>
      {/if}
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
    border-radius: var(--radius-xl);
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
    border-radius: var(--radius-m);
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
    font-size: var(--text-small);
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .steps {
    font-size: var(--text-small);
    color: var(--text2);
    white-space: nowrap;
    flex-shrink: 0;
  }
  .button {
    min-width: 76px;
  }
  .button.primary {
    min-width: 110px;
  }
  .button.good {
    color: var(--green);
    font-weight: 600;
  }
  .button.bad {
    color: var(--red);
    font-weight: 600;
  }

  .below {
    display: flex;
    gap: 10px;
    max-height: 96px;
  }
  .journal {
    flex: 1 1 0;
    min-width: 0;
    overflow-y: auto;
    padding: 3px 0;
    border-radius: var(--radius-l);
    border: 0.5px solid var(--sep);
    background: color-mix(in srgb, var(--win) 75%, transparent);
  }
  .step {
    width: 100%;
    height: 22px;
    padding: 0 10px 0 6px;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--text-small);
    text-align: left;
    border-radius: 0;
  }
  .step:hover {
    background: var(--row-hover);
  }
  .at {
    width: 16px;
    flex-shrink: 0;
    text-align: right;
    color: var(--text2);
    font-variant-numeric: tabular-nums;
  }
  .pill {
    height: 16px;
    line-height: 16px;
    width: 62px;
    padding: 0 6px;
    flex-shrink: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    border-radius: var(--radius-pill);
    font-size: var(--text-caption);
    font-weight: 600;
    text-align: center;
    white-space: nowrap;
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
  .pill.testing {
    box-shadow: inset 0 0 0 1px var(--text);
    color: var(--text);
  }
  .sha {
    flex-shrink: 0;
    color: var(--text2);
  }
  .summary {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .step.range .summary,
  .step.range .sha {
    color: var(--text2);
  }
  .note {
    flex-shrink: 0;
    color: var(--text2);
    font-size: var(--text-caption);
  }
  .below .log {
    flex: 1 1 0;
    min-width: 0;
    max-height: none;
  }
  .log {
    position: relative;
    background: var(--term);
    border: 0.5px solid var(--term-border);
    box-shadow: var(--term-shadow);
    border-radius: var(--radius-l);
    padding: 8px 96px 8px 12px;
    font-size: var(--text-small);
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
    border-radius: var(--radius-s);
    font-size: var(--text-caption);
    color: var(--term-dim);
    background: var(--term-button);
  }
</style>
