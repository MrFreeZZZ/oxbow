<script lang="ts">
  // The end of a row in a file list: a small badge and note for an unusual change (Image, Binary,
  // Large, +x, CRLF → LF, Renamed), otherwise the added and removed line counts.

  import type { FileChange } from "./types";
  import { fileBadge } from "./unusual";

  let { file }: { file: FileChange } = $props();

  const badge = $derived(fileBadge(file));
  const renamed = $derived(file.status === "renamed" || file.status === "copied");
</script>

{#if badge}
  <span class="badge" title={badge.title}>{badge.badge}</span>
  <span class="note" class:warn={badge.warn}>{badge.note}</span>
{:else}
  {#if renamed}<span class="badge" title={file.oldPath ? `From ${file.oldPath}` : undefined}>{file.status === "copied" ? "Copied" : "Renamed"}</span>{/if}
  <span class="mono add">+{file.additions}</span>
  <span class="mono del">−{file.deletions}</span>
{/if}

<style>
  .badge {
    height: 17px;
    padding: 0 6px;
    border-radius: 6px;
    background: var(--field);
    color: var(--text2);
    font-size: 10.5px;
    font-weight: 600;
    display: inline-flex;
    align-items: center;
    white-space: nowrap;
    flex-shrink: 0;
  }
  .note {
    font-size: 11px;
    color: var(--text2);
    white-space: nowrap;
    flex-shrink: 0;
  }
  .note.warn {
    color: var(--orange);
  }
  .add {
    color: var(--green);
    font-size: 11px;
  }
  .del {
    color: var(--red);
    font-size: 11px;
    min-width: 22px;
    text-align: right;
  }
</style>
