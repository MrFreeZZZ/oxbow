<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { api } from "./api";
  import { shortId } from "./format";
  import { mac } from "./keys";
  import { summaryText } from "./commitSummary";

  const win = getCurrentWindow();
  let snapshot = $state.raw<Awaited<ReturnType<typeof api.commitSummaryState>> | null>(null);
  const summary = $derived(snapshot?.summary);
  const context = $derived(snapshot?.context);
  let folded = $state<Record<string, boolean>>({});
  let status = $state("");
  let loadError = $state("");
  let content: HTMLElement;

  onMount(() => {
    let request = 0;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const refresh = async () => {
      const ticket = ++request;
      try {
        const next = await api.commitSummaryState();
        if (disposed || ticket !== request) return;
        if (next.summary.id !== summary?.id || next.context.path !== context?.path) {
          content.scrollTop = 0;
          folded = {};
        }
        snapshot = next;
        status = "";
        loadError = "";
      } catch (err) {
        if (!disposed && ticket === request) loadError = String(err);
      }
    };
    void win.listen("commit-summary-changed", refresh).then((stop) => {
      if (disposed) { stop(); return; }
      unlisten = stop;
      return refresh();
    }).catch((err) => { if (!disposed) loadError = String(err); });
    return () => { disposed = true; unlisten?.(); };
  });

  async function copy(kind: "summary" | "sha") {
    const selected = snapshot;
    if (!selected) return;
    let result: string;
    try {
      await navigator.clipboard.writeText(kind === "sha" ? selected.summary.id : summaryText(selected.summary, selected.context));
      result = kind === "sha" ? "Copied SHA" : "Copied Summary";
    } catch {
      result = kind === "sha" ? "Could not copy SHA." : "Could not copy summary.";
    }
    if (selected === snapshot) status = result;
  }
  async function openCommitLink() {
    try {
      await api.openSummaryCommitLink();
    } catch (err) {
      loadError = String(err);
    }
  }
</script>

<svelte:window onkeydown={(event) => {
  if (event.key === "Escape" && !event.defaultPrevented) {
    event.preventDefault();
    void win.close().catch((err) => loadError = String(err));
  }
}} />

<div class="summary-window" class:mac>
  <header data-tauri-drag-region>
    <div class="title" data-tauri-drag-region>Commit Summary</div>
    {#if summary}
      <button class="button small sha mono" aria-label="Copy SHA" title="Copy SHA {summary.id}" onclick={() => copy("sha")}>
        {shortId(summary.id)}
        <svg class="icon" viewBox="0 0 16 16" aria-hidden="true">
          <path d={status === "Copied SHA" ? "M3.5 8l3 3 6-6" : "M6 5.5h7v8H6zM10 5.5v-3H3v8h3"} />
        </svg>
      </button>
    {/if}
    <span class="spacer" data-tauri-drag-region></span>
    <button class="button pill" aria-label="Copy Summary" disabled={!summary} onclick={() => copy("summary")}>
      <svg class="icon" viewBox="0 0 16 16" aria-hidden="true"><path d="M6 5.5h7v8H6zM10 5.5v-3H3v8h3" /></svg>
      <span aria-hidden="true">{status === "Copied Summary" ? "Copied" : "Copy Summary"}</span>
    </button>
  </header>
  <span class="sr-only" role="status">{status}</span>
  {#if status.startsWith("Could not")}<p class="notice" aria-hidden="true">{status}</p>{/if}
  <main class="thin-scroll selectable" bind:this={content}>
    {#if loadError}<p class="notice" role="alert">{loadError}</p>{/if}
    {#if summary && context}
      <div class="settings-panel repository">
        <h2 class="repo-name">{context.repository}</h2>
        <p class="repo-path">{context.path}</p>
        <dl class="repo-fields">
          <div><dt>Branches</dt><dd>{context.branches}</dd></div>
          <div><dt>Current branch</dt><dd>{context.branch}</dd></div>
        </dl>
        <dl class="repo-remotes">
          {#each context.remotes as remote (remote.name)}
            <div>
              <dt>Remote · {remote.name}</dt>
              <dd>{remote.fetchUrl}</dd>
              {#if remote.pushUrl !== remote.fetchUrl}<dd>Push: {remote.pushUrl}</dd>{/if}
            </div>
          {:else}
            <div><dt>Remotes</dt><dd>No remotes</dd></div>
          {/each}
        </dl>
        <div class="publication">
          <span>{context.publication}</span>
          {#if context.url}
            <button class="commit-link" title={context.url} onclick={openCommitLink}>Open commit in browser ↗</button>
          {/if}
        </div>
      </div>
      <article class="settings-panel commit">
        <h1>{summary.title}</h1>
        <p class="description">{summary.description}</p>
        <div class="metadata">
          <div><span class="author-label">Author</span> {summary.author}</div>
          <span>{summary.date}</span>
        </div>
      </article>
      {#each summary.groups as group (group.name)}
        <section>
          <h2>
            <button class="group-toggle" aria-expanded={!folded[group.name]} aria-controls={`summary-${group.name}`} onclick={() => folded[group.name] = !folded[group.name]}>
              <svg class="chevron" class:open={!folded[group.name]} viewBox="0 0 16 16" aria-hidden="true"><path d="M6 4l4 4-4 4" /></svg>
              {group.name} <span class="group-count">{group.files.length.toLocaleString()}</span>
            </button>
          </h2>
          <ul class="settings-panel" role="list" id={`summary-${group.name}`} hidden={folded[group.name]}>
            {#each group.files as file (file.path)}
              <li>
                <div class="paths">
                  <div class="mono">{file.path}</div>
                  {#if file.origin}<div class="origin">{file.origin}</div>{/if}
                </div>
                {#if file.counts}
                  <span class="counts mono">
                    <span class="add">+{file.counts.additions}</span>
                    <span class="del">−{file.counts.deletions}</span>
                  </span>
                {/if}
              </li>
            {/each}
          </ul>
        </section>
      {:else}
        <p class="settings-panel empty">{summary.empty}</p>
      {/each}
    {:else if !loadError}
      <p class="notice">Loading commit summary…</p>
    {/if}
  </main>
</div>

<style>
  .summary-window { height: 100%; display: flex; flex-direction: column; overflow: hidden; background: var(--side); }
  .mac header { padding-left: 92px; }
  header { height: 56px; flex-shrink: 0; display: flex; align-items: center; gap: 10px; padding: 0 14px 0 18px; border-bottom: .5px solid var(--sep); }
  .title { font-size: var(--text-body); font-weight: 600; }
  .sha { flex-shrink: 0; }
  .metadata, .origin { color: var(--text2); font-size: var(--text-small); }
  .spacer { flex: 1; align-self: stretch; }
  .button.pill { min-width: 124px; }
  header .icon { width: 14px; height: 14px; }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
  main { flex: 1; min-height: 0; overflow: auto; padding: 16px 30px 20px 28px; overflow-wrap: anywhere; }
  .repository, .commit { padding: 12px; }
  .repository { margin-bottom: 18px; font-size: var(--text-small); }
  .repo-name { font-size: var(--text-title); }
  .repo-path { color: var(--text2); }
  .repo-fields { margin: var(--space-3) 0 0; display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--space-3); }
  .repo-remotes { margin-bottom: 0; display: grid; gap: var(--space-3); }
  dt, .publication { color: var(--text2); }
  dt { font-weight: 600; }
  dd { margin: var(--space-1) 0 0; }
  .repo-remotes, .publication { margin-top: var(--space-3); padding-top: var(--space-3); border-top: .5px solid var(--sep); }
  .publication { display: flex; flex-wrap: wrap; gap: var(--space-2) var(--space-3); }
  .commit-link { color: var(--accent-text); }
  .commit-link:hover { text-decoration: underline; }
  h1 { margin: 0; font-size: var(--text-title); font-weight: 600; }
  p { margin: 0; }
  .description { margin-top: var(--space-2); white-space: pre-wrap; }
  .metadata { display: flex; flex-wrap: wrap; justify-content: space-between; gap: 6px 12px; margin-top: var(--space-3); padding-top: var(--space-3); border-top: .5px solid var(--sep); }
  .author-label { color: var(--text); font-weight: 500; margin-right: var(--space-1); }
  section { margin-top: 18px; }
  h2 { margin: 0 0 6px; font-size: var(--text-body); font-weight: 600; }
  .group-toggle { width: 100%; padding: 4px 12px; display: flex; align-items: center; gap: 8px; border-radius: var(--radius-m); text-align: left; }
  .group-toggle:hover { background: var(--hover-wash); }
  .group-count { color: var(--text2); font-size: var(--text-caption); font-variant-numeric: tabular-nums; }
  .chevron { width: 10px; height: 10px; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
  .chevron.open { transform: rotate(90deg); }
  ul { margin: 0; padding: 0 12px; list-style: none; }
  li { min-height: 34px; display: flex; align-items: center; justify-content: space-between; gap: var(--space-3); padding: var(--space-1) 0; font-size: var(--text-small); }
  li + li { border-top: .5px solid var(--sep); }
  .empty { margin-top: 18px; padding: 12px; color: var(--text2); }
  .paths { min-width: 0; }
  .paths > div { white-space: pre-wrap; }
  .origin { margin-top: var(--space-1); }
  .counts { display: flex; gap: var(--space-2); flex-shrink: 0; }
  .add { color: var(--green); }
  .del { color: var(--red); }
  .notice { padding: var(--space-2) var(--space-4); color: var(--text2); font-size: var(--text-small); }
</style>
