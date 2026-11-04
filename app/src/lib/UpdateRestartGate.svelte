<script lang="ts">
  import { updateRestart } from "./updateRestart.svelte";
</script>

{#if updateRestart.active}
  <div class="restart" role="status" aria-live="polite">
    <div class="notice">
      <strong>{updateRestart.installing ? "Installing the update…" : "Preparing to restart…"}</strong>
      <span>{updateRestart.installing ? "Oxbow will restart when installation finishes." : "Saving drafts and checking running commands."}</span>
    </div>
  </div>
{/if}

<style>
  .restart { position: fixed; inset: 0; z-index: 10000; background: var(--dim); display: grid; place-items: center; padding: var(--space-6); }
  .notice { max-width: 400px; background: var(--sheet); color: var(--text); box-shadow: var(--shadow-sheet); border: 0.5px solid var(--sep); border-radius: var(--radius-sheet); padding: var(--space-6); display: flex; flex-direction: column; gap: var(--space-2); }
  strong { font-size: var(--text-heading); }
  span { font-size: var(--text-body); color: var(--text2); line-height: 1.5; }
</style>
