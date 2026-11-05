<script lang="ts">
  import { backgroundOpacity } from "./background";
  import { background, followBackground } from "./background.svelte";
  import { prefs } from "./prefs.svelte";
  import { paletteOf, themeById } from "./themes";

  followBackground();
  let dark = $state(document.documentElement.dataset.theme === "dark");
  $effect(() => {
    const root = document.documentElement;
    const update = () => { dark = root.dataset.theme === "dark"; };
    const observer = new MutationObserver(update);
    observer.observe(root, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  });
  const opacity = $derived(backgroundOpacity(prefs.get("oxbow.background.intensity"), dark));
  const code = $derived(paletteOf(themeById(prefs.get("oxbow.theme")), prefs.get("oxbow.theme.variant"), dark ? "dark" : "light"));
  const visible = $derived(!!background.url && opacity > 0);
  const codeSurface = $derived(code.dark === dark ? "blend" : "opaque");
  $effect(() => {
    const root = document.documentElement;
    if (visible) root.dataset.background = "on";
    else delete root.dataset.background;
    root.dataset.backgroundCode = codeSurface;
    return () => { delete root.dataset.background; delete root.dataset.backgroundCode; };
  });
</script>

{#if background.url}
  <div id="window-background" aria-hidden="true" style:opacity>
    <img src={background.url} alt="" style:object-position={background.position} />
  </div>
{/if}

<style>
  #window-background { position: fixed; inset: 0; z-index: 0; pointer-events: none; overflow: hidden; }
  img { width: 100%; height: 100%; object-fit: cover; }
  :global(:root[data-theme="dark"]) img { filter: brightness(0.55) saturate(0.85); }
</style>
