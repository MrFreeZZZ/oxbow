<script lang="ts">
  import CommentBody from "./CommentBody.svelte";
  import { updates } from "./updates.svelte";
  import { downloadProgress } from "./updates";
</script>

<section class="status" class:problem={updates.notice.error} aria-label="Update status" aria-live="polite" aria-busy={updates.busy || updates.loading}>
  <h3>{updates.notice.title}</h3>
  {#if updates.notice.message}<p>{updates.notice.message}</p>{/if}
  {#if updates.status?.state === "downloading"}
    {@const progress = downloadProgress(updates.status.received, updates.status.total)}
    <div class="track" role="progressbar" aria-label="Update download" aria-valuemin="0" aria-valuemax="100" aria-valuenow={progress.percent ?? undefined} aria-valuetext={progress.label}>
      <span class="fill" class:busy={progress.percent === null} style:width="{progress.percent ?? 30}%"></span>
    </div>
  {/if}
  {#if updates.available?.notes?.trim()}
    <details>
      <summary>Release Notes</summary>
      <div class="notes"><CommentBody body={updates.available.notes} /></div>
    </details>
  {/if}
  {#if updates.available || updates.notice.error}
    <div class="actions">
      {#if updates.notice.error && !updates.available}<button class="button small" disabled={updates.loading || updates.busy} onclick={() => updates.check()}>Retry</button>{/if}
      <button class="button small" disabled={updates.restarting} onclick={() => updates.open()}>{updates.available ? "View Release" : "View Releases"}</button>
      {#if updates.status?.state === "downloading"}<button class="button small" onclick={() => updates.cancel()}>Cancel</button>{/if}
      {#if updates.canDownload}<button class="button small primary" disabled={updates.loading} onclick={() => updates.download()}>{updates.status?.state === "downloadFailed" ? "Retry Download" : "Download Update"}</button>{/if}
      {#if updates.ready || updates.installed}<button class="button small primary" disabled={updates.loading || updates.busy} onclick={() => updates.install()}>{updates.installed ? "Restart Now" : "Restart & Install"}</button>{/if}
    </div>
  {/if}
</section>

<style>
  .status {
    padding: var(--space-4);
    border-top: 0.5px solid var(--sep);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    color: var(--text);
  }
  .status.problem { background: color-mix(in srgb, var(--orange-soft) 55%, transparent); }
  h3 { margin: 0; font-size: var(--text-body); font-weight: 600; }
  p { margin: 0; color: var(--text2); font-size: var(--text-small); line-height: 1.5; overflow-wrap: anywhere; }
  .problem p { color: var(--text); }
  details { margin-top: var(--space-1); min-width: 0; }
  summary { width: fit-content; color: var(--accent-text); font-size: var(--text-small); }
  .notes { margin-top: var(--space-3); padding: var(--space-3); border-radius: var(--radius-m); background: var(--win); max-height: 260px; overflow: auto; }
  .actions { margin-top: var(--space-2); display: flex; justify-content: flex-end; flex-wrap: wrap; gap: var(--space-2); }
  .track { height: 4px; border-radius: var(--radius-pill); background: var(--field); overflow: hidden; }
  .fill { display: block; height: 4px; border-radius: var(--radius-pill); background: var(--accent); transition: width 0.25s; }
  .fill.busy { animation: slide 1.2s ease-in-out infinite alternate; }
  @keyframes slide { from { transform: translateX(-60%); } to { transform: translateX(300%); } }
  @media (prefers-reduced-motion: reduce) { .fill { transition: none; } .fill.busy { animation: none; } }
</style>
