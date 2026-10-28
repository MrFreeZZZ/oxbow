<script lang="ts">
  // «Do you really want to let it run these programs?»: a repository Oxbow didn't clone or
  // create has settings that make git run programs. Opening, it can open Restricted; once open
  // Restricted, the title bar's capsule brings the same list back to trust it.

  import { modal } from "./modal";
  import { withKeys } from "./keys";
  import { tilde } from "./term";
  import { trustWhy } from "./trust";
  import type { TrustFinding } from "./types";

  let {
    name,
    findings,
    opening,
    home,
    onAnswer,
  }: {
    name: string;
    findings: TrustFinding[];
    /** Asked while opening; otherwise the repository is open Restricted already. */
    opening: boolean;
    home: string;
    onAnswer: (answer: "trust" | "restricted" | null) => void;
  } = $props();

  let box = $state<HTMLDivElement>();
  $effect(() => {
    box?.querySelector<HTMLButtonElement>("button.primary")?.focus();
  });

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onAnswer(null);
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="dim">
  <div class="sheet" use:modal role="dialog" aria-modal="true" aria-labelledby="trust-title" bind:this={box}>
    <div class="top">
      <span class="tile"><svg class="icon" viewBox="0 0 16 16"><path d="M8 1.6 2.8 3.5v4c0 3.2 2.2 5.7 5.2 6.9 3-1.2 5.2-3.7 5.2-6.9v-4z" /><path d="M8 5.2v3.4M8 10.6v.1" /></svg></span>
      <div class="words">
        <span class="title" id="trust-title">Do you really want to let “{name}” run these programs?</span>
        {#if opening}
          <span class="body">Oxbow didn’t clone or create this folder. Its Git settings run programs whenever Git reads the repository, before you do anything. Trust it only if you know where it came from.</span>
        {:else}
          <span class="body">It is open Restricted, with these off. Trusted, Git runs them as it would in Terminal, now and every time the repository opens.</span>
        {/if}
      </div>
    </div>
    <div class="found">
      {#each findings as f (f.key)}
        <div class="row">
          <div class="key"><b>{f.kind === "hook" && f.key.startsWith("hooks/") ? `Hook ${f.key.slice(6)}` : f.key}</b><span class="origin">{tilde(f.origin, home)}</span></div>
          <div class="why">{trustWhy(f)}</div>
          <code class="value">{tilde(f.value, home)}</code>
        </div>
      {/each}
    </div>
    {#if opening}
      <div class="note">
        <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="5.75" /><path d="M8 7.2v3.6M8 5.2v.01" /></svg>
        <span>Restricted opens it with these settings and hooks off. Your own Git settings still apply, so fetch, commit and push work. You can trust it later from the title bar.</span>
      </div>
    {/if}
    <div class="foot">
      <button class="button" onclick={() => onAnswer(null)} title={withKeys("Cancel", "Escape")}>Cancel</button>
      <span class="grow"></span>
      {#if opening}
        <button class="button" onclick={() => onAnswer("trust")}>Trust and Open</button>
        <button class="button primary" onclick={() => onAnswer("restricted")} title={withKeys("Open Restricted", "Enter")}>Open Restricted</button>
      {:else}
        <button class="button primary" onclick={() => onAnswer("trust")}>Trust This Repository</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .dim {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: var(--dim);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .sheet {
    width: min(580px, calc(100vw - 48px));
    max-height: calc(100vh - 48px);
    overflow-y: auto;
    padding: 22px 22px 18px;
    border-radius: var(--radius-sheet);
    background: var(--sheet);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--shadow-sheet);
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .top {
    display: flex;
    gap: 14px;
    align-items: flex-start;
  }
  .tile {
    width: 44px;
    height: 44px;
    flex-shrink: 0;
    border-radius: var(--radius-l);
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent);
  }
  .tile .icon {
    width: 22px;
    height: 22px;
    stroke-width: 1.4;
  }
  .words {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .title {
    font-size: var(--text-heading);
    font-weight: 700;
    line-height: 1.3;
  }
  .body {
    line-height: 1.6;
    color: var(--text2);
  }
  .found {
    border-radius: var(--radius-l);
    background: var(--panel);
    border: 0.5px solid var(--panel-border);
    overflow: hidden;
  }
  .row {
    padding: 9px 12px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .row + .row {
    border-top: 0.5px solid var(--sep);
  }
  .key {
    display: flex;
    gap: 8px;
    align-items: baseline;
  }
  .key b {
    font-weight: 600;
  }
  .origin {
    margin-left: auto;
    color: var(--text2);
    font-size: var(--text-small);
    text-align: right;
    overflow-wrap: anywhere;
  }
  .why {
    color: var(--text2);
    font-size: var(--text-small);
  }
  .value {
    margin-top: 4px;
    padding: 4px 8px;
    border-radius: var(--radius-s);
    background: var(--term);
    border: 0.5px solid var(--term-border);
    font-family: var(--mono);
    font-size: var(--text-small);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .note {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    font-size: var(--text-small);
    line-height: 1.4;
    color: var(--text2);
  }
  .note .icon {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    margin-top: 1px;
  }
  .foot {
    display: flex;
    gap: 8px;
    padding-top: 2px;
  }
  .grow {
    flex-grow: 1;
  }
  .button {
    min-width: 96px;
    padding: 0 16px;
  }
</style>
