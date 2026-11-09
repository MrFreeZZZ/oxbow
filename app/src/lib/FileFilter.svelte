<script lang="ts">
  import { lane } from "./format";
  let { value = $bindable(""), onkeydown, color }: { value?: string; onkeydown?: (event: KeyboardEvent) => void; color?: number } = $props();

  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape" && value) {
      event.stopPropagation();
      value = "";
      return;
    }
    onkeydown?.(event);
  }
</script>

<label class="filter search-control" style:--search-focus-shadow={color === undefined ? undefined : `inset 0 0 0 1px color-mix(in srgb, ${lane(color)} 40%, transparent)`}>
  <svg class="icon" viewBox="0 0 16 16" aria-hidden="true"><circle cx="7" cy="7" r="4.5" /><path d="m10.5 10.5 3 3" /></svg>
  <input bind:value placeholder="Filter files" aria-label="Filter files" spellcheck="false" autocomplete="off" onkeydown={keydown} />
  {#if value}<button type="button" class="clear" onclick={() => (value = "")} aria-label="Clear the filter" title="Clear the filter">×</button>{/if}
</label>

<style>
  .filter {
    width: 100%;
  }
</style>
