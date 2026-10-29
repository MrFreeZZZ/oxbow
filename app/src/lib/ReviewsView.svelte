<script lang="ts">
  // Reviews: the repository's open pull requests, the ones waiting for your review first, then
  // yours, then the others; or the recently closed ones. A row opens the Pull Request screen, or
  // checks the pull request out in a worktree of its own. Merge requests on GitLab show the same
  // way once signed in there; a repository on both picks one at the top.
  import { api } from "./api";
  import { github } from "./github.svelte";
  import type { Request } from "./confirm.svelte";
  import type { BranchContext } from "./branches";
  import type { PullSummary, ReviewDraft } from "./types";
  import { person, plate, relativeTime, tint } from "./format";
  import { pullWorktreeRequest, worktreeOf } from "./worktrees";
  import { listFor } from "./reviews";
  import { countWords } from "./reviewDraft";
  import { wordsOf, type ForgeKind } from "./forge";

  let {
    ctx,
    colorOf,
    run,
    onOpen,
  }: {
    ctx: BranchContext;
    colorOf: (name: string) => number;
    run: (request: Request | Promise<Request>) => Promise<boolean>;
    onOpen: (pull: PullSummary, forge: ForgeKind) => void;
  } = $props();

  let closed = $state(false);
  /** GitHub's pull requests or GitLab's merge requests, when the repository has both. */
  let picked = $state<ForgeKind | null>(null);
  const forge = $derived(picked && (picked === "gitlab" ? !!github.mrProject : !!github.repo) ? picked : github.forge);
  const lab = $derived(forge === "gitlab");
  const w = $derived(wordsOf(forge));
  /** Draft reviews kept here, by pull request number. */
  let drafts = $state<Record<string, ReviewDraft>>({});
  api.reviewDrafts().then(
    (d) => (drafts = d),
    () => {},
  );

  const gh = $derived(github.repo);
  /** Where the list comes from: the remote, and the repository's name there. */
  const place = $derived(lab ? (github.mrProject ? { remote: github.mrProject.remote, name: github.mrProject.path } : null) : gh ? { remote: gh.remote, name: `${gh.owner}/${gh.name}` } : null);
  const me = $derived(github.meOf(forge));
  const statuses = $derived(github.statusesOf(forge));
  const problem = $derived(lab ? github.mrProblem : github.problem);
  const list = $derived(github.listOf(forge));
  const sections = $derived(listFor(list, me, closed));
  const repoPath = $derived(ctx.history.worktrees.find((t) => t.current)?.path ?? "");

  /** A fork's: GitLab names its project, GitHub its owner. */
  const fork = (p: PullSummary) => !!p.headOwner && (lab || p.headOwner !== gh?.owner);

  function draftNote(number: number): string | null {
    const d = lab ? null : drafts[String(number)];
    if (!d) return null;
    const what = countWords(d.comments.length, d.replies.length);
    return what ? `Your draft · ${what}` : "Your draft";
  }

  /** What GitHub or GitLab says of it, in one capsule. */
  function verdict(p: PullSummary): { text: string; tone: "ok" | "warn" | "grey" } {
    if (p.state === "merged") return { text: "Merged", tone: "grey" };
    if (p.state === "closed") return { text: "Closed", tone: "grey" };
    if (p.draft) return { text: "Draft", tone: "grey" };
    const s = statuses[p.number];
    if (s?.decision === "approved") return { text: "Approved", tone: "ok" };
    if (s?.decision === "changesRequested") return { text: "Changes requested", tone: "warn" };
    if (s?.decision === "reviewRequired") return { text: "Review required", tone: "warn" };
    return { text: "Open", tone: "grey" };
  }

  const toneOf = (tone: string) => (tone === "ok" ? ["var(--green-soft)", "var(--green)"] : tone === "warn" ? ["var(--orange-soft)", "var(--orange)"] : ["var(--field)", "var(--text2)"]);

  /** The checks of its head together, as a mark: passed, failing or running. */
  function checks(p: PullSummary): { mark: "ok" | "fail" | "run"; title: string } | null {
    const state = statuses[p.number]?.checks;
    if (!state || p.state !== "open") return null;
    const what = lab ? "Pipeline" : "Checks";
    if (state === "success") return { mark: "ok", title: `${what} passed` };
    if (state === "failure" || state === "error") return { mark: "fail", title: lab ? "Pipeline failed" : "Checks failing" };
    return { mark: "run", title: `${what} running` };
  }

  function checkOut(p: PullSummary) {
    if (!place) return;
    const elsewhere = worktreeOf(ctx.history, p.head);
    if (elsewhere && !fork(p)) {
      api.openWindow(elsewhere.path).catch(() => {});
      return;
    }
    run(pullWorktreeRequest(ctx, repoPath, p, place.remote, fork(p), lab));
  }

  /** The worktree button: none when it is checked out here. */
  function worktreeLabel(p: PullSummary): string | null {
    if (fork(p)) return "Check Out in Worktree";
    if (ctx.history.head.branch === p.head) return null;
    return worktreeOf(ctx.history, p.head) ? "Open Its Worktree" : "Check Out in Worktree";
  }

  const author = (p: PullSummary) => (p.author.login === me ? "you" : p.author.login);
</script>

{#snippet avatar(login: string, url: string)}
  <span class="avatar" style:background={person(login)}>
    {(login[0] ?? "?").toUpperCase()}
    {#if url}<img src={url} alt="" onerror={(e) => ((e.currentTarget as HTMLImageElement).style.display = "none")} />{/if}
  </span>
{/snippet}

{#snippet capsule(name: string)}
  <span class="capsule-label" style:background={tint(colorOf(name), "label")} style:color={plate(colorOf(name))}>{name}</span>
{/snippet}

{#snippet row(p: PullSummary, asked: boolean)}
  {@const v = verdict(p)}
  {@const [bg, fg] = toneOf(v.tone)}
  {@const ck = checks(p)}
  {@const note = draftNote(p.number)}
  {@const wt = worktreeLabel(p)}
  <div class="row">
    <button class="pull" onclick={() => onOpen(p, forge)} title="Open {w.noun} {w.mark}{p.number}">
      {@render avatar(p.author.login, p.author.avatarUrl)}
      <span class="text">
        <span class="title-line">
          <span class="title">{p.title}</span>
          <span class="number">{w.mark}{p.number}</span>
          {#if note}<span class="draft">{note}</span>{/if}
        </span>
        <span class="sub-line">
          {@render capsule(p.head)}
          <svg class="icon small" viewBox="0 0 16 16"><path d="M13 8H3.5M7 4.5 3.5 8 7 11.5" /></svg>
          {@render capsule(p.base)}
          <span class="ellipsis">{author(p)}{fork(p) ? ` · from ${lab ? "a fork" : p.headOwner}` : ""} · {asked ? "asked" : p.state === "open" ? "opened" : "updated"} {relativeTime(asked || p.state !== "open" ? p.updated : p.created).toLowerCase()}</span>
        </span>
      </span>
    </button>
    <span class="end">
      {#if ck}
        <span class="check {ck.mark}" title={ck.title}>
          <svg class="icon small" viewBox="0 0 16 16"><path d={ck.mark === "ok" ? "M3.5 8.5 6.5 11.5 12.5 4.5" : ck.mark === "fail" ? "M5 5l6 6M11 5l-6 6" : "M8 2.5a5.5 5.5 0 1 1 0 11a5.5 5.5 0 0 1 0-11M8 5v3l2 1.5"} /></svg>
        </span>
      {/if}
      <span class="status" style:background={bg} style:color={fg}>{v.text}</span>
      <span class="actions">
        {#if wt && p.state === "open"}
          <button class="button small" onclick={() => checkOut(p)} title={wt === "Open Its Worktree" ? `${p.head} is checked out in another worktree` : `A working folder of its own with ${w.mark}${p.number} checked out`}>
            <svg class="icon small" viewBox="0 0 16 16"><path d="M2.5 4.5a1 1 0 0 1 1-1h3l1.5 1.5h4.5a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1zM8 7.5v3.5M6.25 9.25h3.5" /></svg>
            {wt}
          </button>
        {/if}
        <button class="button small primary" onclick={() => onOpen(p, forge)}>{asked ? "Review" : "Open"}</button>
      </span>
    </span>
  </div>
{/snippet}

<div class="reviews">
  <div class="head">
    <h1>Reviews</h1>
    {#if place}<span class="sub">{closed ? "Recently closed" : "Open"} {w.noun}s on {place.name}</span>{/if}
    <span class="grow"></span>
    {#if gh && github.mrProject}
      <span class="segmented" role="radiogroup" aria-label="Where">
        <button role="radio" aria-checked={!lab} class:on={!lab} onclick={() => (picked = "github")}>GitHub</button>
        <button role="radio" aria-checked={lab} class:on={lab} onclick={() => (picked = "gitlab")}>GitLab</button>
      </span>
    {/if}
    <span class="segmented" role="radiogroup" aria-label="Which {w.noun}s">
      <button role="radio" aria-checked={!closed} class:on={!closed} onclick={() => (closed = false)}>Open</button>
      <button role="radio" aria-checked={closed} class:on={closed} onclick={() => (closed = true)}>Closed</button>
    </span>
  </div>
  {#if !place}
    <div class="empty">This repository has no remote on github.com, nor a GitLab project on a server you are signed in to.</div>
  {:else if problem && !list.length}
    <div class="empty">
      <span>{problem}</span>
      <button class="button" onclick={() => api.openSettings()}>Open Settings</button>
    </div>
  {:else if !sections.some((s) => s.pulls.length)}
    <div class="empty">{closed ? `No ${w.noun} was closed lately.` : `No open ${w.noun}s.`}</div>
  {:else}
    <div class="scroll">
      {#each sections as s (s.title)}
        {#if s.pulls.length}
          <div class="section">{s.title}<span class="count">{s.pulls.length}</span></div>
          {#each s.pulls as p (p.number)}
            {@render row(p, s.asked)}
          {/each}
        {/if}
      {/each}
    </div>
  {/if}
</div>

<style>
  .reviews {
    flex-grow: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-top: 1px solid var(--sep);
    padding: 14px 22px 0;
  }
  .head {
    display: flex;
    align-items: baseline;
    gap: 10px;
    flex-shrink: 0;
  }
  h1 {
    margin: 0;
    font-size: var(--text-title);
    font-weight: 600;
  }
  .sub {
    color: var(--text2);
  }
  .grow {
    flex-grow: 1;
  }
  .segmented {
    display: flex;
    align-self: center;
    flex-shrink: 0;
    padding: 2px;
    border-radius: var(--radius-pill);
    background: var(--field);
  }
  .segmented button {
    height: 24px;
    padding: 0 12px;
    border-radius: var(--radius-pill);
    font-size: var(--text-small);
    color: var(--text2);
  }
  .segmented button.on {
    background: var(--win);
    color: var(--text);
    font-weight: 600;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
  }
  .empty {
    flex-grow: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: var(--text2);
    text-align: center;
  }
  .scroll {
    flex-grow: 1;
    min-height: 0;
    overflow-y: auto;
    margin: 0 -10px;
    padding: 0 10px 16px;
  }
  .section {
    display: flex;
    gap: 8px;
    margin: 18px 12px 6px;
    font-size: var(--text-caption);
    font-weight: 600;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    color: var(--text2);
  }
  .count {
    font-weight: 500;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 12px 0 0;
    border-radius: var(--radius-l);
  }
  .row:hover {
    background: var(--row-hover);
  }
  .pull {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 0 10px 12px;
    text-align: left;
  }
  .avatar {
    position: relative;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    color: #fff;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 12px;
    font-weight: 700;
    flex-shrink: 0;
    overflow: hidden;
  }
  .avatar img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .title-line,
  .sub-line {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    white-space: nowrap;
  }
  .title {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .number {
    color: var(--text2);
    flex-shrink: 0;
  }
  .draft {
    height: 18px;
    line-height: 18px;
    padding: 0 7px;
    border-radius: var(--radius-xs);
    background: var(--accent-quiet);
    color: var(--accent-ink);
    font-size: var(--text-caption);
    font-weight: 600;
    flex-shrink: 0;
  }
  .sub-line {
    font-size: var(--text-small);
    color: var(--text2);
  }
  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .capsule-label {
    line-height: 18px;
    height: 18px;
    padding: 0 8px;
    border-radius: var(--radius-pill);
    font-size: var(--text-caption);
    font-weight: 500;
    flex-shrink: 0;
  }
  .icon.small {
    width: 13px;
    height: 13px;
  }
  .end {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-shrink: 0;
  }
  .check {
    display: inline-flex;
  }
  .check.ok {
    color: var(--green);
  }
  .check.fail {
    color: var(--red);
  }
  .check.run {
    color: var(--orange);
  }
  .status {
    line-height: 22px;
    height: 22px;
    padding: 0 10px;
    border-radius: var(--radius-pill);
    font-size: var(--text-small);
    font-weight: 600;
    white-space: nowrap;
  }
  /* The buttons show on the row under the pointer, in place of nothing: the capsules stay put. */
  .actions {
    display: none;
    gap: 8px;
  }
  .row:hover .actions,
  .row:focus-within .actions {
    display: flex;
  }
</style>
