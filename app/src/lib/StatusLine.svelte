<script lang="ts">
  // Under the toolbar of History: where the work is, in one line. "feature/login · 3 changed ·
  // 1 staged · 2 to push · origin checked 4 min ago". A click opens the card with the four
  // places a change passes through: the working folder, the staging area, the local branch and
  // the server, with when each remote was last fetched. The ahead and behind counts are only
  // as fresh as that fetch, so the line always says how old they are.

  import type { History, ReadPart } from "./types";
  import { activity, failureShort } from "./activity.svelte";
  import { PART_NAMES, errorLine, readAgo, troubled, type ReadStates } from "./reads";
  import { lane, plate, relativeTime, tint } from "./format";
  import { isProtected } from "./protect";

  let {
    history,
    reads,
    onRetry,
    color,
    onChanges,
    onPush,
    onPull,
    onFetch,
    onTheirs,
    onPublish,
  }: {
    history: History;
    /** How well each part besides the commits was read; null before the first read. */
    reads: ReadStates | null;
    /** Read a part again after it failed. */
    onRetry: (part: ReadPart) => void;
    /** Lane color of the checked-out branch. */
    color: number;
    /** Show the uncommitted changes. */
    onChanges: () => void;
    onPush: () => void;
    /** Pull, the way Settings says, or with a merge or rebase picked here. */
    onPull: (mode?: "merge" | "rebase") => void;
    onFetch: () => void;
    /** Compare: what the upstream has that the branch doesn't. */
    onTheirs: (upstream: string) => void;
    /** Make a repository on GitHub for a project without a remote. */
    onPublish: () => void;
  } = $props();

  let open = $state(false);
  // The relative times move on while the window stays open.
  let now = $state(Date.now() / 1000);
  $effect(() => {
    const timer = setInterval(() => (now = Date.now() / 1000), 30_000);
    return () => clearInterval(timer);
  });

  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
  const branch = $derived(history.head.branch);
  const changes = $derived(history.rows.find((r) => r.worktree)?.worktree ?? null);
  const t = $derived(history.tracking);
  const upstream = $derived(t && !t.gone ? `${t.remote}/${t.branch}` : null);
  /** The remote the counts are about: the upstream's, else where Publish would go. */
  const remote = $derived(t?.remote ?? history.defaultRemote ?? history.remotes[0] ?? null);
  const fetchedAt = (name: string | null) => (name ? (history.fetched.find((f) => f.remote === name)?.time ?? null) : null);
  const checked = $derived(fetchedAt(remote));
  /** The remote the counts are about first. */
  const fetched = $derived([...history.fetched].sort((a, b) => Number(b.remote === remote) - Number(a.remote === remote)));
  /** Over a day without a fetch: the counts may be far off. */
  const stale = $derived(!!remote && (checked === null || now - checked > 86_400));
  const ahead = $derived(t && !t.gone ? t.ahead : 0);
  const behind = $derived(t && !t.gone && !t.rewritten ? t.behind : 0);
  const diverged = $derived(ahead > 0 && behind > 0);
  /** `git status` failed: the counts are not known, which is not the same as none. */
  const unknown = $derived(!!changes?.unknown);
  /** The parts not read this time, shown from an earlier read or not at all. */
  const problems = $derived(troubled(reads));
  const trackingPhase = $derived(reads?.tracking.phase ?? "ready");
  /** No upstream known because none could be read, not because there is none. */
  const noTracking = $derived(!t && trackingPhase === "failed");

  /** "4 min ago", "just now", "yesterday 14:05": inside a sentence. */
  function ago(time: number): string {
    const text = relativeTime(time, now);
    return text === "Just now" || text.startsWith("Yesterday") ? text[0].toLowerCase() + text.slice(1) : text;
  }
  function checkedText(name: string, time: number | null): string {
    return time === null ? `${name} never fetched` : `${name} checked ${ago(time)}`;
  }
  const exact = (time: number | null) => (time === null ? "No fetch from this clone yet" : `Last fetch: ${new Date(time * 1000).toLocaleString()}`);

  /** The parts of the line after the branch name. */
  const parts = $derived.by(() => {
    const out: { text: string; warn?: boolean; title?: string }[] = [];
    if (unknown) out.push({ text: "Changes unknown", warn: true, title: errorLine(reads?.changes.error ?? null) });
    if (changes?.conflicted) out.push({ text: `${changes.conflicted} conflicted`, warn: true });
    const changed = (changes?.unstaged ?? 0) - (changes?.conflicted ?? 0);
    if (changed > 0) out.push({ text: `${changed} changed` });
    if (changes?.staged) out.push({ text: `${changes.staged} staged` });
    if (!changes) out.push({ text: "No changes" });
    if (!history.head.commit) out.push({ text: "No commits yet" });
    else if (!history.remotes.length) out.push({ text: "Local only" });
    else if (noTracking) out.push({ text: "Upstream unknown", warn: true, title: errorLine(reads?.tracking.error ?? null) });
    else if (!t) out.push({ text: "Not published" });
    else if (t.gone) out.push({ text: `${t.remote}/${t.branch} is gone`, warn: true });
    else if (t.rewritten) out.push({ text: t.ahead ? `${t.ahead} to force push` : "Force push needed" });
    else {
      if (ahead) out.push({ text: `${ahead} to push` });
      if (behind) out.push({ text: `${behind} to pull` });
      if (!ahead && !behind) out.push({ text: `Up to date with ${upstream}` });
    }
    // Shown from an earlier read: the upstream, or that there was none, may have changed.
    if (trackingPhase === "stale") out.push({ text: t ? "Counts may be stale" : "May be stale", warn: true, title: `Couldn’t read the upstreams again: ${errorLine(reads?.tracking.error ?? null)}` });
    for (const part of ["operation", "bisect"] as const) {
      if (problems.includes(part)) out.push({ text: part === "operation" ? "Merge state unknown" : "Bisect state unknown", warn: true, title: errorLine(reads?.[part].error ?? null) });
    }
    return out;
  });

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape" && open) {
      event.preventDefault();
      open = false;
    }
  }
  /** Close the card, then act. */
  const then = (action: () => void) => () => {
    open = false;
    action();
  };
</script>

<svelte:window onkeydown={onKey} />

<div class="status">
  <button class="line" class:open onclick={() => (open = !open)} aria-expanded={open} aria-haspopup="dialog">
    <span class="branch" style:color={plate(color)}><span class="dot" style:background={lane(color)}></span>{branch}</span>
    {#each parts as part, i (i)}
      <span class="sep">·</span><span class:warn={part.warn} title={part.title}>{part.text}</span>
    {/each}
    <span class="grow"></span>
    {#if remote}
      {#if activity.running}
        <span class="when">Fetching…</span>
      {:else if activity.problem?.failure}
        <span class="when warn">Fetch failed · {failureShort(activity.problem.failure)}</span>
      {:else}
        <span class="when" class:warn={stale} title={exact(checked)}>
          <svg class="clock" viewBox="0 0 16 16"><circle cx="8" cy="8" r="5.5" /><path d="M8 5v3.2l2 1.3" /></svg>
          {checkedText(remote, checked)}
        </span>
      {/if}
    {/if}
    <svg class="chev" viewBox="0 0 16 16"><path d="M4.5 6.5 8 10l3.5-3.5" /></svg>
  </button>

  {#if open}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="catcher" onclick={() => (open = false)}></div>
    <div class="card" role="dialog" aria-label="Where your work is">
      <div class="head">
        <span class="h">Where your work is</span>
        <span class="hint">{noTracking ? `Oxbow couldn’t read where ${branch} pushes and pulls` : upstream ? `${branch} tracks ${upstream}` : remote && history.head.commit ? `${branch} is not on ${remote} yet` : `${branch} is only on this computer`}</span>
      </div>
      <div class="flow">
        <div class="place">
          <span class="label">Working Folder</span>
          {#if unknown}
            <span class="n warn">?</span>
            <span class="what">changes not known</span>
            <span class="detail warn" title={errorLine(reads?.changes.error ?? null)}>git status failed</span>
            <button class="button small act" disabled={reads?.changes.phase === "loading"} onclick={() => onRetry("changes")}>{reads?.changes.phase === "loading" ? "Reading…" : "Retry"}</button>
          {:else}
            <span class="n" class:zero={!(changes?.unstaged ?? 0)}>{changes?.unstaged ?? 0}</span>
            <span class="what">{changes?.conflicted ? `${plural(changes.unstaged, "file")}, ${changes.conflicted} conflicted` : `${(changes?.unstaged ?? 0) === 1 ? "file" : "files"} changed`}</span>
            <span class="detail">Edited, not staged yet</span>
            <button class="button small act" disabled={!changes} onclick={then(onChanges)}>Review</button>
          {/if}
        </div>
        <svg class="arrow" viewBox="0 0 16 16"><path d="M6 3.5 10.5 8 6 12.5" /></svg>
        <div class="place">
          <span class="label">Staged</span>
          {#if unknown}
            <span class="n warn">?</span>
            <span class="what">not known</span>
          {:else}
            <span class="n" class:zero={!changes?.staged}>{changes?.staged ?? 0}</span>
            <span class="what">{(changes?.staged ?? 0) === 1 ? "file" : "files"} staged</span>
          {/if}
          <span class="detail">Go into the next commit</span>
          <button class="button small act" disabled={!changes?.staged} onclick={then(onChanges)}>Commit…</button>
        </div>
        <svg class="arrow" viewBox="0 0 16 16"><path d="M6 3.5 10.5 8 6 12.5" /></svg>
        <div class="place mine" style:background={tint(color, "soft")} style:border-color="color-mix(in srgb, {lane(color)} 28%, transparent)">
          <span class="label" style:color={plate(color)}>{branch}</span>
          {#if noTracking}
            <span class="n warn">?</span>
            <span class="what">upstream not known</span>
            <span class="detail warn" title={errorLine(reads?.tracking.error ?? null)}>Couldn’t read the upstreams</span>
            <button class="button small act" disabled={trackingPhase === "loading"} onclick={() => onRetry("tracking")}>{trackingPhase === "loading" ? "Reading…" : "Retry"}</button>
          {:else if t && !t.gone}
            <span class="n" class:zero={!ahead}>{ahead}</span>
            <span class="what">{ahead === 1 ? "commit" : "commits"} to push</span>
            {@const force = !!t.rewritten && !isProtected(history.protected, t.branch)}
            <span class="detail">{force ? `Replace old versions on ${t.remote}` : t.rewritten ? `Old versions stay on ${t.remote}: protected` : ahead ? `Not on ${t.remote} yet` : `All on ${t.remote}`}</span>
            <button class="button small act" disabled={!ahead && !t.rewritten} onclick={then(onPush)}>{force ? "Force Push…" : "Push…"}</button>
          {:else}
            <span class="n">{history.head.commit ? "—" : 0}</span>
            <span class="what">{history.head.commit ? "no upstream" : "no commits yet"}</span>
            <span class="detail">{t?.gone ? `${t.remote}/${t.branch} was deleted` : remote ? `Publish creates it on ${remote}` : "Only on this computer"}</span>
            {#if remote}
              <button class="button small act" disabled={!history.head.commit} onclick={then(onPush)}>Publish…</button>
            {:else}
              <button class="button small act" disabled={!history.head.commit} onclick={then(onPublish)}>Publish to GitHub…</button>
            {/if}
          {/if}
        </div>
        <svg class="arrow" viewBox="0 0 16 16"><path d="M6 3.5 10.5 8 6 12.5" /></svg>
        <div class="place">
          <span class="label">{upstream ?? remote ?? "Server"}</span>
          {#if noTracking}
            <span class="n warn">?</span>
            <span class="what">not known</span>
            <span class="detail">Read the upstream first</span>
            <button class="button small act" disabled={!remote || !!activity.running} onclick={then(onFetch)}>Fetch</button>
          {:else if upstream && t}
            <span class="n" class:zero={!behind}>{behind}</span>
            <span class="what">{t.rewritten ? "old versions there" : `new ${behind === 1 ? "commit" : "commits"} there`}</span>
            <span class="detail" class:warn={stale}>{checked === null ? "Not fetched yet" : `As of the fetch ${ago(checked)}`}</span>
            {#if behind}
              <button class="button small act" onclick={then(() => onPull())}>Pull…</button>
            {:else}
              <button class="button small act" disabled={!!activity.running} onclick={then(onFetch)}>Fetch</button>
            {/if}
          {:else}
            <span class="n zero">—</span>
            <span class="what">{remote ? "nothing to compare" : "no remote"}</span>
            <span class="detail">{remote ? "The branch isn’t there" : "Add one in Settings"}</span>
            <button class="button small act" disabled={!remote || !!activity.running} onclick={then(onFetch)}>Fetch</button>
          {/if}
        </div>
      </div>

      {#if diverged && upstream}
        <div class="split">
          <span class="words">
            <span class="title">Both sides have new commits</span>
            <span class="detail">{plural(ahead, "commit")} here, {behind} on {upstream}. Pull has to combine them.</span>
          </span>
          <button class="button small act" onclick={then(() => onTheirs(upstream!))}>See Their Commits</button>
          <button class="button small act" disabled={!!activity.running} onclick={then(() => onPull("merge"))} title="Pull and join both sides with a merge commit">Merge…</button>
          <button class="button small act" disabled={!!activity.running} onclick={then(() => onPull("rebase"))} title="Pull and replay your commits on top of theirs">Rebase…</button>
        </div>
      {/if}

      {#if problems.length && reads}
        <div class="problems">
          {#each problems as part (part)}
            {@const state = reads[part]}
            <div class="problem">
              <svg class="mark" viewBox="0 0 16 16"><path d="M8 2.2 14.2 13H1.8z" /><path d="M8 6.4v3M8 11.2v.1" /></svg>
              <span class="words">
                <span class="title">Couldn’t read the {PART_NAMES[part]}</span>
                <span class="detail" title={state.error ?? ""}>{state.phase === "stale" ? `Showing what was there before (${readAgo(state, Date.now()).toLowerCase()}). ` : "Not known, so not shown as none. "}{errorLine(state.error)}</span>
              </span>
              <button class="button small act" disabled={state.phase === "loading"} onclick={() => onRetry(part)}>{state.phase === "loading" ? "Reading…" : "Retry"}</button>
            </div>
          {/each}
        </div>
      {/if}

      {#if history.remotes.length}
        <div class="remotes">
          <span class="sub">Last fetch</span>
          {#each fetched as f (f.remote)}
            <div class="remote" title={exact(f.time)}>
              <svg class="clock" viewBox="0 0 16 16"><circle cx="8" cy="8" r="5.5" /><path d="M8 5v3.2l2 1.3" /></svg>
              <span class="name">{f.remote}</span>
              {#if activity.running && (activity.running.remote === null || activity.running.remote === f.remote)}
                <span class="time">Fetching…</span>
              {:else if activity.problem && (activity.problem.remote === null || activity.problem.remote === f.remote)}
                <span class="time warn">Failed · {activity.problem.sub}</span>
              {:else}
                <span class="time" class:warn={f.time === null || now - f.time > 86_400}>{f.time === null ? "Never from this clone" : ago(f.time)}</span>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
      <div class="foot">
        <span>Push and pull counts are as of the last fetch: the server may have moved since.</span>
        {#if history.remotes.length}
          <button class="link" disabled={!!activity.running} onclick={then(onFetch)}>{history.remotes.length > 1 ? "Fetch All" : "Fetch Now"}</button>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .status {
    position: relative;
    margin: 0;
    flex-shrink: 0;
  }
  .line {
    width: 100%;
    height: 30px;
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 0 14px 0 16px;
    color: var(--text2);
    font-size: var(--text-small);
    white-space: nowrap;
    overflow: hidden;
    text-align: left;
  }
  .line:hover,
  .line.open {
    background: var(--field);
  }
  .branch {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .sep {
    opacity: 0.6;
  }
  .grow {
    flex-grow: 1;
  }
  .when {
    display: flex;
    align-items: center;
    gap: 5px;
    flex-shrink: 0;
  }
  .warn {
    color: var(--orange);
  }
  .clock {
    width: 13px;
    height: 13px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.3;
    stroke-linecap: round;
    flex-shrink: 0;
  }
  .chev {
    width: 13px;
    height: 13px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
    flex-shrink: 0;
  }
  .catcher {
    position: fixed;
    inset: 0;
    z-index: 19;
  }
  .card {
    position: absolute;
    left: 0;
    top: 36px;
    z-index: 20;
    width: 760px;
    max-width: 100%;
    padding: 16px 14px 10px;
    border-radius: var(--radius-sheet);
    background: var(--sheet);
    -webkit-backdrop-filter: blur(30px) saturate(1.8);
    backdrop-filter: blur(30px) saturate(1.8);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--shadow-pop);
    display: flex;
    flex-direction: column;
    gap: 12px;
    color: var(--text);
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
    padding: 0 4px;
  }
  .h {
    font-size: var(--text-heading);
    font-weight: 700;
  }
  .hint {
    font-size: var(--text-small);
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .flow {
    display: flex;
    align-items: stretch;
    gap: 4px;
  }
  .arrow {
    width: 14px;
    flex-shrink: 0;
    align-self: center;
    fill: none;
    stroke: var(--text2);
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
    opacity: 0.7;
  }
  .place {
    flex: 1 1 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 10px 12px 12px;
    border-radius: var(--radius-l);
    background: var(--panel);
    border: 0.5px solid var(--panel-border);
  }
  .label {
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .n {
    margin-top: 4px;
    font-size: var(--text-large);
    font-weight: 700;
    line-height: 1.15;
  }
  .n.zero {
    color: var(--text2);
    font-weight: 600;
  }
  .what {
    font-size: var(--text-small);
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .detail {
    font-size: var(--text-small);
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .detail.warn {
    color: var(--orange);
  }
  .act {
    align-self: flex-start;
    margin-top: 10px;
    min-width: 76px;
  }

  .split {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    border-radius: var(--radius-l);
    background: var(--field);
  }
  .split .act {
    margin-top: 0;
    align-self: center;
  }
  .words {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .title {
    font-size: var(--text-body);
    font-weight: 600;
  }
  .problems {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .problem {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    border-radius: var(--radius-l);
    background: color-mix(in srgb, var(--orange) 9%, transparent);
  }
  .problem .act {
    margin-top: 0;
    align-self: center;
  }
  .mark {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    fill: none;
    stroke: var(--orange);
    stroke-width: 1.4;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .n.warn {
    color: var(--orange);
  }
  .remotes {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0 4px;
  }
  .sub {
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
    margin-bottom: 2px;
  }
  .remote {
    display: flex;
    align-items: center;
    gap: 7px;
    height: 22px;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .remote .name {
    color: var(--text);
    font-weight: 500;
    min-width: 90px;
  }
  .remote .time.warn {
    color: var(--orange);
  }
  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 4px 2px;
    border-top: 1px solid var(--sep);
    font-size: var(--text-small);
    color: var(--text2);
  }
  .link {
    flex-shrink: 0;
    color: var(--accent-text);
    font-size: var(--text-small);
    white-space: nowrap;
  }
  .link:disabled {
    opacity: 0.45;
  }
</style>
