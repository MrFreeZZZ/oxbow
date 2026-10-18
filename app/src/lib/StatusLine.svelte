<script lang="ts">
  // Under the toolbar of History: where the work is, in one line. "feature/login · 3 changed ·
  // 1 staged · 2 to push · origin checked 4 min ago". A click opens the card with the four
  // places a change passes through: the working folder, the staging area, the local branch and
  // the server, with when each remote was last fetched. The ahead and behind counts are only
  // as fresh as that fetch, so the line always says how old they are.

  import type { History } from "./types";
  import { activity, failureShort } from "./activity.svelte";
  import { lane, plate, relativeTime, tint } from "./format";

  let {
    history,
    color,
    onChanges,
    onPush,
    onPull,
    onFetch,
    onTheirs,
    onPublish,
  }: {
    history: History;
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
    const out: { text: string; warn?: boolean }[] = [];
    if (changes?.conflicted) out.push({ text: `${changes.conflicted} conflicted`, warn: true });
    const changed = (changes?.unstaged ?? 0) - (changes?.conflicted ?? 0);
    if (changed > 0) out.push({ text: `${changed} changed` });
    if (changes?.staged) out.push({ text: `${changes.staged} staged` });
    if (!changes) out.push({ text: "No changes" });
    if (!history.head.commit) out.push({ text: "No commits yet" });
    else if (!history.remotes.length) out.push({ text: "Local only" });
    else if (!t) out.push({ text: "Not published" });
    else if (t.gone) out.push({ text: `${t.remote}/${t.branch} is gone`, warn: true });
    else if (t.rewritten) out.push({ text: t.ahead ? `${t.ahead} to force push` : "Force push needed" });
    else {
      if (ahead) out.push({ text: `${ahead} to push` });
      if (behind) out.push({ text: `${behind} to pull` });
      if (!ahead && !behind) out.push({ text: `Up to date with ${upstream}` });
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
      <span class="sep">·</span><span class:warn={part.warn}>{part.text}</span>
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
        <span class="hint">{upstream ? `${branch} tracks ${upstream}` : remote && history.head.commit ? `${branch} is not on ${remote} yet` : `${branch} is only on this computer`}</span>
      </div>
      <div class="flow">
        <div class="place">
          <span class="label">Working Folder</span>
          <span class="n" class:zero={!(changes?.unstaged ?? 0)}>{changes?.unstaged ?? 0}</span>
          <span class="what">{changes?.conflicted ? `${plural(changes.unstaged, "file")}, ${changes.conflicted} conflicted` : `${(changes?.unstaged ?? 0) === 1 ? "file" : "files"} changed`}</span>
          <span class="detail">Edited, not staged yet</span>
          <button class="act" disabled={!changes} onclick={then(onChanges)}>Review</button>
        </div>
        <svg class="arrow" viewBox="0 0 16 16"><path d="M6 3.5 10.5 8 6 12.5" /></svg>
        <div class="place">
          <span class="label">Staged</span>
          <span class="n" class:zero={!changes?.staged}>{changes?.staged ?? 0}</span>
          <span class="what">{(changes?.staged ?? 0) === 1 ? "file" : "files"} staged</span>
          <span class="detail">Go into the next commit</span>
          <button class="act" disabled={!changes?.staged} onclick={then(onChanges)}>Commit…</button>
        </div>
        <svg class="arrow" viewBox="0 0 16 16"><path d="M6 3.5 10.5 8 6 12.5" /></svg>
        <div class="place mine" style:background={tint(color, "soft")} style:border-color="color-mix(in srgb, {lane(color)} 28%, transparent)">
          <span class="label" style:color={plate(color)}>{branch}</span>
          {#if t && !t.gone}
            <span class="n" class:zero={!ahead}>{ahead}</span>
            <span class="what">{ahead === 1 ? "commit" : "commits"} to push</span>
            <span class="detail">{t.rewritten ? `Replace old versions on ${t.remote}` : ahead ? `Not on ${t.remote} yet` : `All on ${t.remote}`}</span>
            <button class="act" disabled={!ahead && !t.rewritten} onclick={then(onPush)}>{t.rewritten ? "Force Push…" : "Push…"}</button>
          {:else}
            <span class="n">{history.head.commit ? "—" : 0}</span>
            <span class="what">{history.head.commit ? "no upstream" : "no commits yet"}</span>
            <span class="detail">{t?.gone ? `${t.remote}/${t.branch} was deleted` : remote ? `Publish creates it on ${remote}` : "Only on this computer"}</span>
            {#if remote}
              <button class="act" disabled={!history.head.commit} onclick={then(onPush)}>Publish…</button>
            {:else}
              <button class="act" disabled={!history.head.commit} onclick={then(onPublish)}>Publish to GitHub…</button>
            {/if}
          {/if}
        </div>
        <svg class="arrow" viewBox="0 0 16 16"><path d="M6 3.5 10.5 8 6 12.5" /></svg>
        <div class="place">
          <span class="label">{upstream ?? remote ?? "Server"}</span>
          {#if upstream && t}
            <span class="n" class:zero={!behind}>{behind}</span>
            <span class="what">{t.rewritten ? "old versions there" : `new ${behind === 1 ? "commit" : "commits"} there`}</span>
            <span class="detail" class:warn={stale}>{checked === null ? "Not fetched yet" : `As of the fetch ${ago(checked)}`}</span>
            {#if behind}
              <button class="act" onclick={then(() => onPull())}>Pull…</button>
            {:else}
              <button class="act" disabled={!!activity.running} onclick={then(onFetch)}>Fetch</button>
            {/if}
          {:else}
            <span class="n zero">—</span>
            <span class="what">{remote ? "nothing to compare" : "no remote"}</span>
            <span class="detail">{remote ? "The branch isn’t there" : "Add one in Settings"}</span>
            <button class="act" disabled={!remote || !!activity.running} onclick={then(onFetch)}>Fetch</button>
          {/if}
        </div>
      </div>

      {#if diverged && upstream}
        <div class="split">
          <span class="words">
            <span class="title">Both sides have new commits</span>
            <span class="detail">{plural(ahead, "commit")} here, {behind} on {upstream}. Pull has to combine them.</span>
          </span>
          <button class="act" onclick={then(() => onTheirs(upstream!))}>See Their Commits</button>
          <button class="act" disabled={!!activity.running} onclick={then(() => onPull("merge"))} title="Pull and join both sides with a merge commit">Merge…</button>
          <button class="act" disabled={!!activity.running} onclick={then(() => onPull("rebase"))} title="Pull and replay your commits on top of theirs">Rebase…</button>
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
    font-size: 12px;
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
    border-radius: 22px;
    background: var(--sheet);
    -webkit-backdrop-filter: blur(30px) saturate(1.8);
    backdrop-filter: blur(30px) saturate(1.8);
    border: 0.5px solid var(--glass-border);
    box-shadow: 0 16px 44px rgba(0, 0, 0, 0.22);
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
    font-size: 14px;
    font-weight: 700;
  }
  .hint {
    font-size: 11.5px;
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
    border-radius: 14px;
    background: var(--panel);
    border: 0.5px solid var(--panel-border);
  }
  .label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .n {
    margin-top: 4px;
    font-size: 22px;
    font-weight: 700;
    line-height: 1.15;
  }
  .n.zero {
    color: var(--text2);
    font-weight: 600;
  }
  .what {
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .detail {
    font-size: 11.5px;
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .detail.warn {
    color: var(--orange);
  }
  .act {
    flex-shrink: 0;
    align-self: flex-start;
    margin-top: 10px;
    min-width: 76px;
    height: 26px;
    padding: 0 12px;
    border-radius: 13px;
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    color: var(--text);
  }
  .act:disabled {
    opacity: 0.45;
    box-shadow: none;
  }
  .split {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    border-radius: 14px;
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
    font-size: 12.5px;
    font-weight: 600;
  }
  .remotes {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0 4px;
  }
  .sub {
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
    margin-bottom: 2px;
  }
  .remote {
    display: flex;
    align-items: center;
    gap: 7px;
    height: 22px;
    font-size: 12px;
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
    font-size: 11.5px;
    color: var(--text2);
  }
  .link {
    flex-shrink: 0;
    color: var(--accent-text);
    font-size: 11.5px;
    white-space: nowrap;
  }
  .link:disabled {
    opacity: 0.45;
  }
</style>
