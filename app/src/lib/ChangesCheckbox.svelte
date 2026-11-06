<script lang="ts">
  let { on, some = false, label, title, disabled = false, placement = "row", onclick }: {
    on: boolean; some?: boolean; label: string; title: string; disabled?: boolean;
    placement?: "row" | "heading"; onclick: (event: MouseEvent) => void;
  } = $props();
</script>

<button class="tick" class:on class:some class:heading={placement === "heading"}
  role="checkbox" aria-checked={some ? "mixed" : on} tabindex="-1" aria-label={label}
  {title} {disabled} {onclick}>
  <svg viewBox="0 0 16 16"><path d={some ? "M4 8h8" : "M3.5 8.5l3 3 6-7"} /></svg>
</button>

<style>
  .tick {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    padding: 0;
    border-radius: var(--radius-xs);
    border: 1.2px solid var(--text2);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .tick:disabled {
    opacity: 0.4;
  }
  .tick svg {
    width: 11px;
    height: 11px;
    fill: none;
    stroke: transparent;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .tick.on,
  .tick.some {
    background: var(--accent);
    border-color: var(--accent);
  }
  .tick.on svg,
  .tick.some svg {
    stroke: #fff;
  }
  .tick:not(.heading) {
    position: absolute;
    left: 4px;
    top: calc(50% - 7px);
  }
  .tick.heading { margin: 0 2px; }
</style>
