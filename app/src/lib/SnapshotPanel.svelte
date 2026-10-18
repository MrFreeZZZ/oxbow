<script lang="ts">
  // Compare with Now, from the Operation Log: the files as they were at a step beside the files
  // now, so it is plain what Undo or Restore to Here would take back before running it.

  import { api } from "./api";
  import type { Request } from "./confirm.svelte";
  import type { OpEntry, RefMove, SnapshotComparison } from "./types";
  import { relativeTime, shortId } from "./format";
  import { keptUntil, oplog, restoreRequest, undoRequest } from "./oplog.svelte";
  import CompareChanges from "./CompareChanges.svelte";

  let {
    entry,
    after,
    color,
    version,
    copy,
    run,
    onClose,
  }: {
    entry: OpEntry;
    /** Right after the step (Restore to Here), or before it (Undo). */
    after: boolean;
    color: number;
    /** Goes up on every reload, so the comparison follows the files. */
    version: number;
    copy: (text: string) => void;
    run: (request: Request) => void;
    onClose: () => void;
  } = $props();

  let result = $state<SnapshotComparison | null>(null);
  let error = $state<string | null>(null);
  let copied = $state(false);

  $effect(() => {
    const id = entry.id;
    const side = after;
    version;
    error = null;
    api.compareSnapshot(id, side).then(
      (r) => {
        if (entry.id === id && after === side) result = r;
      },
      (err) => {
        if (entry.id !== id || after !== side) return;
        result = null;
        error = String(err);
      },
    );
  });

  const c = $derived(result?.comparison ?? null);
  const then = $derived(after ? `Right after “${entry.title}”` : `Before “${entry.title}”`);
  const command = $derived(c ? `git diff ${shortId(c.from)} ${shortId(c.targetId)}` : "");
  /** Steps newer than this one: an Undo of the top step, or Restore to Here, takes them back too. */
  const later = $derived(oplog.entries.findIndex((e) => e.id === entry.id));

  function refLine(move: RefMove): string {
    const name = move.name.replace(/^refs\/heads\//, "").replace(/^refs\/tags\//, "tag ");
    if (!move.before) return `${name} is new since then`;
    if (!move.after) return `${name} was at ${shortId(move.before)}, deleted since`;
    return `${name} was at ${shortId(move.before)}, now ${shortId(move.after)}`;
  }

  function goBack() {
    run(after ? restoreRequest(entry, later) : undoRequest(entry, later));
  }

  function copyCommand() {
    copy(command);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }
</script>

{#snippet header()}
  <div class="head">
    <div class="top">
      <span class="count">Compare with Now</span>
      <span class="hint">· snapshot kept {keptUntil(entry)}</span>
      <span class="grow"></span>
      <button class="close" onclick={onClose} aria-label="Close the comparison" title="Close (Esc)">
        <svg class="icon" viewBox="0 0 16 16"><path d="M4 4l8 8M12 4l-8 8" /></svg>
      </button>
    </div>
    <div class="cards">
      <div class="card then">
        <span class="label">Then</span>
        <span class="summary">{then}</span>
        <span class="who">{relativeTime(entry.time)}{result?.headThen.branch ? ` · on ${result.headThen.branch}` : ""}</span>
      </div>
      <svg class="icon arrow" viewBox="0 0 16 16" aria-hidden="true"><path d="M3 8h10M9 4l4 4-4 4" /></svg>
      <div class="card now">
        <span class="label">Now</span>
        <span class="summary">Your files</span>
        <span class="who">{result?.headNow.branch ? `on ${result.headNow.branch}` : "HEAD detached"}</span>
      </div>
    </div>
    {#if c}
      <h1>
        {#if c.files.length}
          {c.files.length} {c.files.length === 1 ? "file differs" : "files differ"} from then
        {:else}
          Your files are as they were then
        {/if}
      </h1>
      <p class="sub">
        {#if c.files.length}The diff reads from then to now: {after ? "Restore to Here" : "Undo"} takes these changes back.{:else}{after ? "Restore to Here" : "Undo"} would change no files{result?.refs.length ? ", only branches" : ""}.{/if}
        {#if result?.stagingDiffers} What is staged differs too; the diff shows the files on disk.{/if}
      </p>
      {#if result?.refs.length}
        <ul class="refs">
          {#each result.refs as move (move.name)}
            <li class="mono">{refLine(move)}</li>
          {/each}
        </ul>
      {/if}
      <div class="actions">
        <button class="term" onclick={copyCommand} title="Copy the command" aria-label="Copy {command}">
          <span class="prompt">$</span>
          <span class="words"><b style:color="var(--term-git)">git</b> <span style:color="var(--term-sub)">diff</span> <span style:color="var(--term-text)">{shortId(c.from)} {shortId(c.targetId)}</span></span>
          {#if copied}
            <svg class="icon copy" viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg>
          {:else}
            <svg class="icon copy" viewBox="0 0 16 16"><path d="M5.5 5.5h7v8h-7zM3.5 10.5v-8h7" /></svg>
          {/if}
        </button>
        <span class="grow"></span>
        <button class="button small primary" onclick={goBack}>{after ? "Restore to Here…" : "Undo…"}</button>
      </div>
    {/if}
  </div>
{/snippet}

{#if error}
  {@render header()}
  <p class="error" role="alert">{error}</p>
{:else if c}
  <div class="changes">
    <CompareChanges comparison={c} base="then" target="now" {color} onPickCommit={() => {}} {header} />
  </div>
{:else}
  {@render header()}
{/if}

<style>
  .changes {
    position: relative;
    flex-grow: 1;
    min-height: 0;
  }
  .head {
    margin: 4px 12px 0;
    padding: 12px 14px 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    border: 1px solid var(--sep);
    border-radius: var(--radius-l);
    flex-shrink: 0;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .count {
    font-weight: 600;
  }
  .hint {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .grow {
    flex-grow: 1;
  }
  .close {
    width: 22px;
    height: 22px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-pill);
    color: var(--icon);
    flex-shrink: 0;
  }
  .close:hover {
    background: var(--field);
  }
  .close .icon {
    width: 12px;
    height: 12px;
  }
  .cards {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .arrow {
    width: 14px;
    height: 14px;
    color: var(--text2);
    flex-shrink: 0;
  }
  .card {
    flex: 1 1 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 3px;
    padding: 9px 12px;
    border-radius: var(--radius-l);
    background: var(--field);
  }
  .label {
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
  }
  .summary {
    max-width: 100%;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .who {
    max-width: 100%;
    font-size: var(--text-caption);
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  h1 {
    margin: 4px 0 0;
    font-size: var(--text-heading);
    font-weight: 600;
    line-height: 1.3;
  }
  .sub {
    margin: 0;
    color: var(--text2);
    font-size: var(--text-small);
    line-height: 1.45;
  }
  .refs {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .mono {
    font-family: var(--mono);
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 4px;
  }
  .term {
    display: flex;
    align-items: center;
    gap: 7px;
    height: 28px;
    min-width: 0;
    padding: 0 10px;
    border-radius: var(--radius-m);
    background: var(--term);
    border: 1px solid var(--term-border);
    font-family: var(--mono);
    font-size: var(--text-caption);
  }
  .prompt {
    color: var(--term-dim);
  }
  .words {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .copy {
    width: 13px;
    height: 13px;
    color: var(--term-dim);
    flex-shrink: 0;
  }

  .error {
    margin: 20px;
    color: var(--red);
  }
</style>
