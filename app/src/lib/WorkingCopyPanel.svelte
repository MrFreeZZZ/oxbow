<script lang="ts">
  // Compare with Working Copy, from a file of a commit: the file as the commit has it beside the
  // file on disk now, so it is plain what Take This Version would replace.

  import { api } from "./api";
  import { confirm } from "./confirm.svelte";
  import type { Comparison } from "./types";
  import { shortId, splitPath } from "./format";
  import { takeVersionRequest, type CommitFile } from "./commitFiles";
  import CompareChanges from "./CompareChanges.svelte";

  let {
    commit,
    summary,
    file,
    color,
    version = 0,
    onClose,
  }: {
    commit: string;
    summary: string | null;
    file: CommitFile;
    color: number;
    /** Goes up on every reload, so the comparison follows the file on disk. */
    version?: number;
    onClose: () => void;
  } = $props();

  let result = $state<Comparison | null>(null);
  let error = $state<string | null>(null);
  let copied = $state(false);

  $effect(() => {
    const id = commit;
    const path = file.path;
    version;
    error = null;
    api.compareWithWorkingCopy(id, path).then(
      (r) => {
        if (commit === id && file.path === path) result = r;
      },
      (err) => {
        if (commit !== id || file.path !== path) return;
        result = null;
        error = String(err);
      },
    );
  });

  const name = $derived(splitPath(file.path).name);
  const sha = $derived(shortId(commit));

  function copyCommand() {
    if (!result) return;
    navigator.clipboard.writeText(result.command).then(() => {
      copied = true;
      setTimeout(() => (copied = false), 1500);
    }, () => {});
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape" && !event.defaultPrevented && !confirm.request) onClose();
  }
</script>

<svelte:window onkeydown={onKey} />

{#snippet header()}
  <div class="head">
    <div class="top">
      <span class="count">Compare with Working Copy</span>
      <span class="hint">· {file.path}</span>
      <span class="grow"></span>
      <button class="close" onclick={onClose} aria-label="Close the comparison" title="Close (Esc)">
        <svg class="icon" viewBox="0 0 16 16"><path d="M4 4l8 8M12 4l-8 8" /></svg>
      </button>
    </div>
    <div class="cards">
      <div class="card">
        <span class="label">In {sha}</span>
        <span class="summary">{summary ?? sha}</span>
        <span class="who">{name} as this commit has it</span>
      </div>
      <svg class="icon arrow" viewBox="0 0 16 16" aria-hidden="true"><path d="M3 8h10M9 4l4 4-4 4" /></svg>
      <div class="card">
        <span class="label">Now</span>
        <span class="summary">Your file</span>
        <span class="who">{name} on disk, with any edits</span>
      </div>
    </div>
    {#if result}
      <h1>{result.files.length ? `${name} differs from ${sha}` : `${name} is as it is in ${sha}`}</h1>
      <p class="sub">
        {#if result.files.length}The diff reads from the commit to your file: Take This Version replaces these changes.{:else}Nothing changed in it since, staged or not.{/if}
      </p>
      <div class="actions">
        <button class="term" onclick={copyCommand} title="Copy the command" aria-label="Copy {result.command}">
          <span class="prompt">$</span>
          <span class="words"><b style:color="var(--term-git)">git</b> <span style:color="var(--term-sub)">diff</span> <span style:color="var(--term-text)">{result.command.replace(/^git diff /, "")}</span></span>
          {#if copied}
            <svg class="icon copy" viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg>
          {:else}
            <svg class="icon copy" viewBox="0 0 16 16"><path d="M5.5 5.5h7v8h-7zM3.5 10.5v-8h7" /></svg>
          {/if}
        </button>
        <span class="grow"></span>
        {#if result.files.length}<button class="button small" onclick={() => confirm.runner?.(takeVersionRequest(file, commit, summary))}>Take This Version…</button>{/if}
      </div>
    {/if}
  </div>
{/snippet}

{#if error}
  {@render header()}
  <p class="error" role="alert">{error}</p>
{:else if result}
  <div class="changes">
    <CompareChanges comparison={result} base={sha} target="now" {color} onPickCommit={() => {}} {header} />
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
    white-space: nowrap;
  }
  .hint {
    font-family: var(--mono);
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
