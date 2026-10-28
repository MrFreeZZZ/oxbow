<script lang="ts">
  // The title at the start of a repository window's header: what it shows, and under it the
  // path that led there, as quiet links back.

  import type { Title } from "./windowTitle";

  let { title, steps, onStep }: { title: Title; steps: { label: string; at: number }[]; onStep: (at: number) => void } = $props();
</script>

<div class="title" data-tauri-drag-region>
  <span class="name">{title.name}</span>
  <span class="sub" class:amber={title.amber}
    >{#if title.steps}{#each steps as step (step.at)}<button class="crumb" onclick={() => onStep(step.at)} title="Back to {step.label}">{step.label}</button><span class="crumb-sep" aria-hidden="true">›</span>{/each}{/if}{title.sub}</span
  >
</div>

<style>
  .title {
    display: flex;
    flex-direction: column;
    line-height: 1.2;
    margin-right: 4px;
    min-width: 0;
    flex-shrink: 100;
  }
  .title span {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .name {
    font-size: var(--text-heading);
    font-weight: 700;
  }
  .sub {
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .sub.amber {
    color: var(--orange);
  }
  /* The path that led here, before the screen's own subtitle: quiet links, as in Finder's path bar. */
  .crumb {
    padding: 0;
    color: inherit;
    font: inherit;
  }
  .crumb:hover {
    color: var(--accent-text);
    text-decoration: underline;
  }
  .crumb-sep {
    margin: 0 5px;
    opacity: 0.7;
  }
</style>
