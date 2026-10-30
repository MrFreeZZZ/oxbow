<script lang="ts">
  import { modal } from "./modal";
  import UserCommands from "./UserCommands.svelte";
  import { api } from "./api";
  import { tick } from "svelte";
  let panel: HTMLElement;
  $effect(() => { tick().then(() => panel?.focus()); });
  let { title, running, result, context, manage, onClose }: { title: string; running: boolean; result: string; context: string; manage: boolean; onClose: () => void } = $props();
</script>
<div class="shade">
  <section bind:this={panel} onkeydown={(e) => { if (e.key === "Escape" && !running) { e.stopPropagation(); onClose(); } }} use:modal role="dialog" aria-modal="true" aria-label={title} tabindex="-1">
    <h2>{title}</h2>
    {#if context}<p class="context">{context}</p>{/if}
    {#if manage}<UserCommands />{:else}<p role="status">{running ? "Running… Output is available in Console. Waiting commands can also be stopped." : result}</p>{/if}
    <footer>
      {#if !manage}<button class="button" onclick={() => api.openConsole()}>Show Console</button>{/if}
      {#if running}<button class="button" onclick={() => api.stopAction()}>Stop</button>{:else}<button class="button primary" onclick={onClose}>Done</button>{/if}
    </footer>
  </section>
</div>
<style>
  .shade { position: fixed; inset: 0; z-index: 50; background: rgba(0,0,0,.16); display: grid; place-items: center; }
  section { width: min(660px, calc(100vw - 60px)); max-height: calc(100vh - 100px); overflow: auto; padding: 24px; border-radius: var(--radius-xl); background: var(--win); border: 1px solid var(--sep); box-shadow: var(--shadow-sheet); }
  h2 { margin: 0 0 18px; font-size: var(--text-heading); }
  p { white-space: pre-wrap; overflow-wrap: anywhere; line-height: 1.6; color: var(--text2); }
  footer { display: flex; justify-content: flex-end; gap: 10px; margin-top: 22px; }
</style>
