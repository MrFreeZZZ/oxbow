<script lang="ts">
  let { add, del }: { add: number; del: number } = $props();
</script>

<span class="file-line-counts mono"><span>+{add}</span> <span>−{del}</span></span>

<style>
  .file-line-counts {
    display: inline-flex;
    gap: 6px;
    margin-inline-start: auto;
    flex: none;
    white-space: nowrap;
    font-size: var(--text-caption);
    text-align: right;
  }
  .file-line-counts > :first-child {
    color: var(--green);
  }
  .file-line-counts > :last-child {
    color: var(--red);
    inline-size: var(--file-deletion-width, 2ch);
  }
</style>
