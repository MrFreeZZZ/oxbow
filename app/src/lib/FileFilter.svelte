<script lang="ts">
  let { value = $bindable(""), onkeydown }: { value?: string; onkeydown?: (event: KeyboardEvent) => void } = $props();

  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape" && value) {
      event.stopPropagation();
      value = "";
      return;
    }
    onkeydown?.(event);
  }
</script>

<label class="filter search-control">
  <svg class="icon" viewBox="0 0 16 16" aria-hidden="true"><circle cx="7" cy="7" r="4.5" /><path d="m10.5 10.5 3 3" /></svg>
  <input bind:value placeholder="Filter files" aria-label="Filter files" spellcheck="false" autocomplete="off" onkeydown={keydown} />
  {#if value}<button type="button" class="clear" onclick={() => (value = "")} aria-label="Clear the filter" title="Clear the filter">×</button>{/if}
</label>

<style>
  .filter {
    width: 100%;
  }
</style>
