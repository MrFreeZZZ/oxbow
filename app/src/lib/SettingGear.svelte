<script lang="ts">
  import { GEAR } from "./settingsRows";

  let { label, on, open }: { label: string; on: boolean; open: (button: HTMLButtonElement) => void } = $props();
</script>

<button
  type="button"
  class="setting-gear"
  class:on
  aria-label="More actions for {label}"
  aria-haspopup="menu"
  aria-expanded={on}
  onclick={(event) => open(event.currentTarget)}
>
  <svg class="icon" viewBox="0 0 16 16" aria-hidden="true"><path d={GEAR} /></svg>
</button>

<style>
  .setting-gear {
    width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-pill);
    color: var(--text2);
    opacity: 0;
  }

  .setting-gear .icon {
    width: 14px;
    height: 14px;
    stroke-width: 1.3;
  }

  .setting-gear.on,
  .setting-gear:focus-visible {
    opacity: 1;
  }

  .setting-gear.on {
    background: var(--field);
  }
</style>
