<script lang="ts">
  // A submodule picked in the sidebar, in place of the commit's details: the commit this
  // repository records against the one its folder has, the commits between them, and the way to
  // bring the folder in line.

  import { api } from "./api";
  import { confirm, type Request } from "./confirm.svelte";
  import { shortId, shortTime } from "./format";
  import { followsWord, latestRequest, updateRequest } from "./submodules";
  import type { CommitBrief, Submodule, SubmoduleMove } from "./types";

  let {
    sub,
    subs,
    repoPath,
    version,
    run,
    onClose,
  }: {
    sub: Submodule;
    /** All of the repository's submodules, for the requests. */
    subs: Submodule[];
    repoPath: string;
    /** Changes when the repository does, to read the folder again. */
    version: number;
    run: (request: Request) => void;
    onClose: () => void;
  } = $props();

  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
  const repoName = $derived(repoPath.split(/[\\/]/).filter(Boolean).pop() ?? "This repository");

  let move = $state<SubmoduleMove | null>(null);

  $effect(() => {
    const { path, recorded, current, changed } = sub;
    void version;
    let live = true;
    api.submoduleMove(path, recorded, current, changed).then(
      (m) => live && (move = m),
      () => live && (move = null),
    );
    return () => {
      live = false;
    };
  });

  const synced = $derived(sub.cloned && sub.current === sub.recorded);

  /** Why the folder isn't at the recorded commit, in words. */
  const why = $derived.by((): { text: string; warn?: boolean } | null => {
    if (!sub.cloned) return { text: `The folder is empty: ${sub.title} isn’t cloned here yet. Clone checks out the commit ${repoName} records.` };
    if (sub.missing) return { text: `The clone doesn’t have ${shortId(sub.recorded ?? "")} yet. Update fetches it first.`, warn: true };
    if (synced || !move || move.path !== sub.path) return null;
    switch (move.moved) {
      case "back":
        return { text: `The folder is ${plural(move.count, "commit")} behind what ${repoName} records, usually after a pull.`, warn: true };
      case "forward":
        return { text: `The folder is ${plural(move.count, "commit")} ahead of what ${repoName} records. Stage it in Changes to record them, or update to go back.`, warn: true };
      case "sideways":
        return { text: `The folder is on another line of commits than the one ${repoName} records.`, warn: true };
      default:
        return { text: `The folder has another commit than the one ${repoName} records.`, warn: true };
    }
  });

  const listLabel = $derived.by(() => {
    if (!move || synced) return null;
    if (move.moved === "back") return `${plural(move.count, "commit")} the folder is missing`;
    if (move.moved === "forward") return `${plural(move.count, "commit")} the folder has that ${repoName} doesn’t record`;
    if (move.moved === "sideways") return `${plural(move.count, "commit")} of the folder’s line`;
    return null;
  });

  /** The recorded commit and the folder's, as far as the clone knows them. */
  const recordedCommit = $derived(move?.oldCommit ?? null);
  const currentCommit = $derived(move?.newCommit ?? null);

  const staged = $derived(sub.committed !== sub.recorded && sub.recorded !== null);

  function openWindow() {
    api.openWindow(sub.path).catch((err) => confirm.fail(err));
  }
  function reveal() {
    api.revealWorktree(sub.path).catch((err) => confirm.fail(err));
  }
</script>

{#snippet tile(label: string, id: string | null, commit: CommitBrief | null, extra: string | null)}
  <div class="tile">
    <span class="label">{label}</span>
    {#if id}
      <span class="sha mono">{shortId(id)}{#if extra}<span class="extra"> · {extra}</span>{/if}</span>
      {#if commit}
        <span class="summary ellipsis" title={commit.summary}>{commit.summary}</span>
        <span class="who">{commit.authorName} · {shortTime(commit.time)}</span>
      {:else}
        <span class="who">Not in its clone yet</span>
      {/if}
    {:else}
      <span class="sha empty">{extra ?? "nothing"}</span>
    {/if}
  </div>
{/snippet}

<div class="panel">
  <div class="top">
    <div class="words">
      <h2>{sub.title} <span class="cap">Submodule</span></h2>
      <p class="sub ellipsis" title={[sub.path, sub.url].filter(Boolean).join(" · ")}>
        {[sub.path, sub.url, `follows ${followsWord(sub)}`].filter(Boolean).join(" · ")}
      </p>
    </div>
    <button class="close" onclick={onClose} aria-label="Close" title="Close (Esc)">
      <svg class="icon" viewBox="0 0 16 16"><path d="M4 4l8 8M12 4l-8 8" /></svg>
    </button>
  </div>

  <div class="tiles">
    {@render tile(staged ? `${repoName} records (staged)` : `${repoName} records`, sub.recorded, recordedCommit, null)}
    {@render tile("The folder has", sub.cloned ? sub.current : null, currentCommit, sub.cloned ? (sub.current ? (sub.branch ?? "detached") : "no commit") : "not cloned")}
  </div>

  {#if why}
    <p class="note" class:warn={why.warn}>
      {#if why.warn}
        <svg class="icon small" viewBox="0 0 16 16"><path d="M8 2.5 14 13H2zM8 6.5v3M8 11.2v.1" /></svg>
      {:else}
        <svg class="icon small" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" /><path d="M8 7.2v3.6M8 5.1v.1" /></svg>
      {/if}
      {why.text}
    </p>
  {/if}
  {#if staged}
    <p class="note">
      <svg class="icon small" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" /><path d="M8 7.2v3.6M8 5.1v.1" /></svg>
      Staged: the next commit records {shortId(sub.recorded ?? "")}; {sub.committed ? `the last one recorded ${shortId(sub.committed)}` : "it is new"}.
    </p>
  {/if}

  <div class="actions">
    {#if !sub.cloned}
      <button class="button small primary" onclick={() => run(updateRequest(subs, sub, repoPath))}>Clone</button>
    {:else if !synced && sub.recorded}
      <button class="button small primary" onclick={() => run(updateRequest(subs, sub, repoPath))}>Update to {shortId(sub.recorded)}</button>
    {/if}
    {#if sub.cloned}
      {#if synced}<button class="button small" onclick={() => run(latestRequest(sub, repoPath))}>Pull Latest</button>{/if}
      <button class="button small" onclick={openWindow}>Open in New Window</button>
      <button class="button small" onclick={reveal}>Show in Finder</button>
    {/if}
  </div>

  {#if listLabel && move?.commits.length}
    <p class="list-label">{listLabel}</p>
    <div class="commits">
      {#each move.commits as c (c.id)}
        <div class="commit">
          <span class="dot"></span>
          <span class="text">
            <span class="ellipsis">{c.summary}</span>
            <span class="who">{c.authorName} · {shortTime(c.time)}</span>
          </span>
          <span class="sha-small mono">{shortId(c.id)}</span>
        </div>
      {/each}
      {#if move.count > move.commits.length}<div class="more">and {plural(move.count - move.commits.length, "more commit")}</div>{/if}
    </div>
  {/if}

  {#if sub.cloned}
    {#if sub.changed}
      <p class="note">
        <svg class="icon small" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" /><path d="M8 7.2v3.6M8 5.1v.1" /></svg>
        {plural(sub.changed, "file")} changed inside. They are committed in {sub.title}, not here:
        <button class="link" onclick={openWindow}>open it in a new window</button>.
      </p>
    {:else if move && move.path === sub.path}
      <p class="note">
        <svg class="icon small" viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg>
        No uncommitted changes inside{#if sub.current && !move.unpushed && currentCommit}&nbsp;· {shortId(sub.current)} is on its remote{/if}
      </p>
    {/if}
    {#if move?.unpushed && sub.current}
      <p class="note warn">
        <svg class="icon small" viewBox="0 0 16 16"><path d="M8 2.5 14 13H2zM8 6.5v3M8 11.2v.1" /></svg>
        {shortId(sub.current)} isn’t on {sub.title}’s remote yet. Push it there before pushing a commit that records it.
      </p>
    {/if}
  {/if}
</div>

<style>
  .panel {
    flex-grow: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 12px 16px 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .top {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }
  .words {
    flex-grow: 1;
    min-width: 0;
  }
  h2 {
    margin: 0 0 3px;
    font-size: var(--text-heading);
    font-weight: 700;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .cap {
    font-size: var(--text-caption);
    font-weight: 500;
    padding: 2px 8px;
    border-radius: var(--radius-pill);
    background: var(--field);
    color: var(--text2);
  }
  .sub {
    margin: 0;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .close {
    width: 22px;
    height: 22px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-pill);
    color: var(--icon);
  }
  .close:hover {
    background: var(--field);
  }
  .close .icon {
    width: 12px;
    height: 12px;
  }
  .tiles {
    display: flex;
    gap: 10px;
  }
  .tile {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 10px 12px;
    border: 1px solid var(--sep);
    border-radius: var(--radius-l);
  }
  .label {
    font-size: var(--text-caption);
    font-weight: 500;
    color: var(--text2);
  }
  .sha {
    margin-top: 2px;
    font-size: var(--text-body);
    font-weight: 600;
  }
  .sha.empty {
    font-weight: 400;
    color: var(--text2);
  }
  .extra {
    font-family: var(--font);
    font-weight: 400;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .summary {
    font-size: var(--text-small);
  }
  .who {
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .note {
    margin: 0;
    display: flex;
    gap: 8px;
    align-items: flex-start;
    font-size: var(--text-small);
    line-height: 1.45;
    color: var(--text2);
  }
  .note.warn {
    color: var(--orange);
  }
  /* Only the icons: a small button shares the class name. */
  .icon.small {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    margin-top: 1px;
  }
  .link {
    color: var(--accent-text);
    font-size: inherit;
    padding: 0;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .list-label {
    margin: 4px 0 -6px;
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
  }
  .commits {
    border: 1px solid var(--sep);
    border-radius: var(--radius-l);
    overflow: hidden;
    flex-shrink: 0;
  }
  .commit {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 7px 12px;
  }
  .commit + .commit {
    border-top: 1px solid var(--sep);
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    border: 2px solid var(--lane-1);
    flex-shrink: 0;
  }
  .text {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.3;
  }
  .sha-small {
    font-size: var(--text-caption);
    color: var(--text2);
    flex-shrink: 0;
  }
  .more {
    padding: 6px 12px 7px 32px;
    border-top: 1px solid var(--sep);
    font-size: var(--text-small);
    color: var(--text2);
  }
  .mono {
    font-family: var(--mono);
  }
</style>
