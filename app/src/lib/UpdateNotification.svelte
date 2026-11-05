<script lang="ts">
  import { api } from "./api";
  import { updates } from "./updates.svelte";
  import { confirm } from "./confirm.svelte";
  let dismissed = $state<string | null>(null);
  const key = $derived(updates.available ? `${updates.channel}:${updates.available.version}` : null);
  const show = $derived(key !== null && key !== dismissed && !updates.busy && !confirm.toast && !confirm.request);
</script>

{#if show}
  <aside class="notice" aria-label="Oxbow update" aria-live="polite">
    <div class="words">
      <strong>Oxbow {updates.available!.version} {updates.installed ? "is installed" : updates.ready ? "is ready to install" : "is available"}</strong>
      <p>{updates.installed ? "Restart Oxbow to use the installed update." : "Review the update and choose when to install it."}</p>
      <button class="button small primary" onclick={() => api.openSettings("updates").catch((error) => confirm.fail(error))}>Review Update</button>
    </div>
    <button class="button mini quiet icon-only" aria-label="Dismiss update notice" onclick={() => dismissed = key}>
      <svg class="icon" viewBox="0 0 16 16"><path d="m5 5 6 6m0-6-6 6" /></svg>
    </button>
  </aside>
{/if}

<style>
  .notice { position: fixed; top: 64px; right: var(--space-4); z-index: 40; display: flex; align-items: flex-start; gap: var(--space-2); width: min(400px, calc(100vw - 32px)); padding: var(--space-3); border-radius: var(--radius-l); background: var(--sheet); color: var(--text); border: 0.5px solid var(--glass-border); box-shadow: var(--shadow-pop); }
  .words { min-width: 0; flex: 1; display: flex; flex-direction: column; gap: var(--space-2); }
  strong { font-size: var(--text-body); overflow-wrap: anywhere; }
  p { margin: 0; font-size: var(--text-small); color: var(--text2); line-height: 1.5; }
  .words button { align-self: flex-end; }
</style>
