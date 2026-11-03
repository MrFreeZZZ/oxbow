<script lang="ts">
  import type { LoadIssue } from "./types";
  let { issues, busy = false, provider, retry, open }: {
    issues: LoadIssue[];
    busy?: boolean;
    provider: string;
    retry: () => void;
    open: () => void;
  } = $props();
</script>

{#if issues.length}
  <section class="notice" aria-label="Loading details" aria-live="polite">
    <div class="messages">
      {#each issues as issue, i (`${issue.section}:${i}`)}
        <div><strong>{issue.section} {issue.state === "partial" ? "incomplete" : "unavailable"}.</strong> {issue.message}</div>
      {/each}
    </div>
    <div class="actions">
      <button class="button small" disabled={busy} onclick={retry}>{busy ? "Loading…" : "Retry"}</button>
      <button class="button small" onclick={open}>Open on {provider}</button>
    </div>
  </section>
{/if}

<style>
  .notice {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--sep);
    flex-shrink: 0;
    background: color-mix(in srgb, var(--orange-soft) 55%, transparent);
    font-size: var(--text-small);
    color: var(--text2);
  }
  .messages { flex: 1; min-width: 180px; overflow-wrap: anywhere; }
  strong { font-weight: 600; color: var(--text); }
  .actions { display: flex; gap: var(--space-2); flex-wrap: wrap; }
</style>
