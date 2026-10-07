<script lang="ts">
  // Collapse All and Expand All for the diffs under a list of changed files.

  let { onFold }: { onFold: (fold: boolean) => void } = $props();
</script>

<span class="fold-all" role="group" aria-label="Fold diffs">
  <button onclick={() => onFold(true)} aria-label="Collapse all files" title="Collapse All Files">
    <svg class="icon" viewBox="0 0 16 16"><path d="M5 2.5 8 5.5l3-3M5 13.5l3-3 3 3" /></svg>
  </button>
  <button onclick={() => onFold(false)} aria-label="Expand all files" title="Expand All Files">
    <svg class="icon" viewBox="0 0 16 16"><path d="M5 5.5 8 2.5l3 3M5 10.5l3 3 3-3" /></svg>
  </button>
</span>

<style>
  .fold-all {
    display: flex;
    gap: 2px;
    margin-left: 4px;
  }
  button {
    width: 22px;
    height: 22px;
    border-radius: 11px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--icon);
  }
  .icon {
    width: 14px;
    height: 14px;
  }
</style>
