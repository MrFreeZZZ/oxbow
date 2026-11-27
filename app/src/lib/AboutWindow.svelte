<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "./api";
  import type { UpdateInfo } from "./types";

  let info = $state<UpdateInfo | null>(null);
  let error = $state("");
  onMount(() => { void api.updateInfo().then((value) => { info = value; }).catch((err) => { error = String(err); }); });
  async function openGitHub() {
    try { await api.openOxbowGitHub(); } catch (err) { error = String(err); }
  }
</script>

<div class="about">
  <header data-tauri-drag-region></header>
  <main class="thin-scroll">
    <img src="/app-icon.png" alt="" width="96" height="96" />
    <h1>Oxbow</h1>
    {#if info}
      <p class="version">Version {info.version}{info.buildChannel === "release" ? ` (${info.commit})` : ""}</p>
      {#if info.buildChannel === "preview" && info.baseRelease && info.commitsSinceRelease !== null}
        <p class="base">Based on {info.baseRelease} · {info.commitsSinceRelease} {info.commitsSinceRelease === 1 ? "commit" : "commits"} since release</p>
      {/if}
      <p class="built">Built {info.built}{info.buildChannel === "preview" ? ` (${info.commit})` : ""}</p>
    {/if}
    <button class="button text" onclick={openGitHub}>View on GitHub ↗</button>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </main>
</div>

<style>
  .about { height: 100vh; display: flex; flex-direction: column; background: var(--win); color: var(--text); }
  header { height: 56px; flex-shrink: 0; }
  main { flex: 1; min-height: 0; overflow: auto; text-align: center; padding: var(--space-4); }
  img { display: block; margin: 0 auto var(--space-3); }
  h1 { font-size: 24px; font-weight: 600; margin: 0 0 var(--space-3); }
  p { margin: 0 0 var(--space-3); overflow-wrap: anywhere; }
  .version { font-size: 16px; }
  .base, .built { font-size: 14px; color: var(--text2); }
  .button { margin-top: var(--space-2); color: var(--accent-text); }
  .error { color: var(--danger); font-size: var(--text-small); margin-top: var(--space-3); }
</style>
