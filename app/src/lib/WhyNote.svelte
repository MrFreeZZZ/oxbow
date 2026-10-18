<script lang="ts">
  // Beside a grey button: why it can't run now, so the reason isn't only in a tooltip.

  let { text, warn = false, grow = false }: { text: string; warn?: boolean; grow?: boolean } = $props();
</script>

<span class="why" class:warn class:grow title={text}>
  <svg class="icon" viewBox="0 0 16 16">
    {#if warn}<path d="M8 2.5 14 13H2zM8 6.5v3M8 11.2v.1" />{:else}<circle cx="8" cy="8" r="6" /><path d="M8 7.2v3.6M8 5.1v.1" />{/if}
  </svg>
  <span class="text">{text}</span>
</span>

<style>
  .why {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    font-size: var(--text-small);
    line-height: 1.35;
    color: var(--text2);
  }
  .why.warn {
    color: var(--orange);
  }
  .grow {
    flex-grow: 1;
  }
  .icon {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
  }
  .text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
