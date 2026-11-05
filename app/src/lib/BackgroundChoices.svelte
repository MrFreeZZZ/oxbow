<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { prefs } from "./prefs.svelte";
  import { background, followBackground, retryBackground } from "./background.svelte";
  import spring from "../assets/backgrounds/spring.jpg";
  import summer from "../assets/backgrounds/summer.jpg";
  import autumn from "../assets/backgrounds/autumn.jpg";
  import winter from "../assets/backgrounds/winter.jpg";

  const choices = [
    { id: "none", name: "None", image: "" },
    { id: "spring", name: "Spring", image: spring },
    { id: "summer", name: "Summer", image: summer },
    { id: "autumn", name: "Autumn", image: autumn },
    { id: "winter", name: "Winter", image: winter },
  ] as const;
  let choosing = $state(false);
  let problem = $state("");
  const source = $derived(prefs.get("oxbow.background.source"));
  const file = $derived(prefs.get("oxbow.background.image"));
  const filename = $derived(file.split(/[\\/]/).pop() || "Choose Image…");
  followBackground();

  async function chooseImage() {
    if (choosing) return;
    choosing = true;
    problem = "";
    try {
      const path = await open({ title: "Choose Background Image", multiple: false, directory: false, filters: [{ name: "Images", extensions: ["jpg", "jpeg", "png", "webp"] }] });
      if (typeof path === "string" && await prefs.set("oxbow.background.image", path)) await prefs.set("oxbow.background.source", "custom");
    } catch (err) {
      problem = `Couldn’t choose an image. ${String(err)}`;
    } finally {
      choosing = false;
    }
  }

  function move(event: KeyboardEvent) {
    const offset = event.key === "ArrowRight" || event.key === "ArrowDown" ? 1 : event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 0;
    if (!offset && event.key !== "Home" && event.key !== "End") return;
    const radios = [...(event.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>('[role="radio"]')];
    const at = radios.indexOf(event.target as HTMLButtonElement);
    if (at < 0) return;
    event.preventDefault();
    const next = event.key === "Home" ? 0 : event.key === "End" ? radios.length - 1 : (at + offset + radios.length) % radios.length;
    radios[next].focus();
    radios[next].click();
  }
</script>

<div class="cards" role="radiogroup" aria-label="Background" tabindex="-1" onkeydown={move}>
  {#each choices as choice (choice.id)}
    {@const on = source === choice.id}
    <button class="card" role="radio" aria-checked={on} tabindex={on || (source === "custom" && choice.id === "none") ? 0 : -1} onclick={() => prefs.set("oxbow.background.source", choice.id)}>
      <span class="swatch" class:on>
        {#if choice.image}<img src={choice.image} alt="" />
        {:else}<svg class="icon" viewBox="0 0 16 16"><path d="M3 3l10 10M6.5 2.5H13v8M3 5v8h8M5 10l2-2 4 4" /></svg>{/if}
      </span>
      <span class="name" class:strong={on}>{choice.name}</span>
    </button>
  {/each}
  <button class="card" aria-pressed={source === "custom"} aria-label={source === "custom" ? `Choose background image, current file: ${file || "None"}` : "Choose background image"} title={file || "Choose Image…"} disabled={choosing} onclick={chooseImage}>
    <span class="swatch" class:on={source === "custom"}><svg class="icon" viewBox="0 0 16 16"><path d="M2.5 2.5h11v11h-11zM3 11l3.5-3.5 2.5 2 2-1.5 2 3M6 5.5h.01" /></svg></span>
    <span class="name" class:strong={source === "custom"}>{choosing ? "Choosing…" : source === "custom" ? filename : "Choose Image…"}</span>
  </button>
</div>
{#if problem}<p class="problem" role="alert">{problem}</p>{/if}
{#if background.problem}
  <div class="load-problem" role="alert"><span>{background.problem} The usual window background is being used.</span><button class="button small" onclick={retryBackground}>Retry</button></div>
{/if}

<style>
  .cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(132px, 1fr)); gap: 10px 12px; }
  .card { display: flex; flex-direction: column; gap: 6px; min-width: 0; text-align: left; }
  .swatch { display: flex; align-items: center; justify-content: center; width: 100%; height: 76px; border-radius: var(--radius-l); border: 0.5px solid var(--panel-border); overflow: hidden; background: var(--field); color: var(--text2); }
  .swatch.on { box-shadow: 0 0 0 2px var(--accent); }
  .swatch img { width: 100%; height: 100%; object-fit: cover; }
  .swatch .icon { width: 24px; height: 24px; }
  .name { padding: 0 2px; font-size: var(--text-small); max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .strong { font-weight: 600; }
  .problem { font-size: var(--text-small); color: var(--text2); }
  .load-problem { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-2); margin-top: var(--space-3); padding: var(--space-3); border-radius: var(--radius-m); background: var(--orange-soft); font-size: var(--text-small); }
  .load-problem span { flex: 1 1 200px; }
</style>
