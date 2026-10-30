<script lang="ts">
  // Settings › Appearance: a card for each popular theme, a few lines of code in its colors.

  import { effectiveLook, prefs } from "./prefs.svelte";
  import { paletteOf, THEMES, type Palette } from "./themes";

  const look = $derived(effectiveLook());

  /** Bars of a theme card: a few lines of code in the theme's colors. */
  function bars(palette: Palette) {
    return [
      { x: 10, y: 10, w: 14, c: palette.kw },
      { x: 28, y: 10, w: 22, c: palette.fn },
      { x: 54, y: 10, w: 10, c: palette.fg },
      { x: 18, y: 21, w: 34, c: palette.com },
      { x: 18, y: 32, w: 12, c: palette.type },
      { x: 34, y: 32, w: 26, c: palette.str },
      { x: 64, y: 32, w: 8, c: palette.num },
      { x: 6, y: 43, w: 74, c: palette.add, soft: true },
    ];
  }
</script>

<div class="cards" role="radiogroup" aria-label="Theme">
  {#each THEMES as theme (theme.id)}
    {@const { palette } = paletteOf(theme, prefs.get("oxbow.theme.variant"), look)}
    {@const on = prefs.get("oxbow.theme") === theme.id}
    <button class="card" role="radio" aria-checked={on} onclick={() => prefs.set("oxbow.theme", theme.id)} title={theme.name}>
      <span class="swatch" class:on style:background={palette.bg}>
        {#each bars(palette) as bar, i (i)}
          <span style:left="{bar.x}px" style:top="{bar.y}px" style:width="{bar.w}px" style:background={bar.c} style:opacity={bar.soft ? 0.25 : 1}></span>
        {/each}
      </span>
      <span class="card-name">
        {#if theme.rank}<span class="rank">{theme.rank}</span>{/if}
        <span class:strong={on}>{theme.name}</span>
      </span>
      <span class="card-sub">{theme.installs ? `${theme.installs} · ` : theme.id === "oxbow" ? "Default · " : ""}{theme.light && theme.dark ? "Light, Dark" : theme.dark ? "Dark" : "Light"}</span>
    </button>
  {/each}
</div>

<style>
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(132px, 1fr));
    gap: 10px 12px;
  }

  .card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }

  .swatch {
    position: relative;
    display: block;
    width: 100%;
    height: 56px;
    border-radius: var(--radius-l);
    border: 0.5px solid var(--panel-border);
    overflow: hidden;
  }

  .swatch.on {
    box-shadow: 0 0 0 2px var(--accent);
  }

  .swatch span {
    position: absolute;
    height: 5px;
    border-radius: var(--radius-pill);
  }

  .card-name {
    display: flex;
    align-items: baseline;
    gap: 5px;
    padding: 0 2px;
    font-size: var(--text-small);
    min-width: 0;
  }

  .card-name span:last-child {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .rank {
    color: var(--text2);
    font-size: var(--text-caption);
  }

  .strong {
    font-weight: 600;
  }

  .card-sub {
    margin-top: -4px;
    padding: 0 2px;
    font-size: var(--text-caption);
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
