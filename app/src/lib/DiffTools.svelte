<script lang="ts">
  // Over a diff, once for all its files: Previous and Next Change, then how the diff is drawn.
  // A file's own bar keeps only what acts on that file.

  import { prefs } from "./prefs.svelte";
  import { stepChange } from "./diffRows";
  import { withKeys } from "./keys";
  import { combo } from "./shortcuts";

  let {
    whole,
    onToggleWhole,
    working = false,
  }: {
    whole: boolean;
    onToggleWhole: () => void;
    /** Uncommitted changes: git stages whitespace as it is, so there is no Ignore Whitespace. */
    working?: boolean;
  } = $props();

  const split = $derived(prefs.get("oxbow.diff.layout") === "split");
  const wrap = $derived(prefs.get("oxbow.diff.wrap"));
  const ignoring = $derived(!working && prefs.get("oxbow.diff.ignoreWhitespace"));
</script>

<span class="tools" role="group" aria-label="Diff view">
  <button onclick={() => stepChange(-1)} aria-label="Previous change" title={withKeys("Previous Change", combo("previousChange"))}>
    <svg class="icon" viewBox="0 0 16 16"><path d="M4.5 10 8 6.5l3.5 3.5" /></svg>
  </button>
  <button onclick={() => stepChange(1)} aria-label="Next change" title={withKeys("Next Change", combo("nextChange"))}>
    <svg class="icon" viewBox="0 0 16 16"><path d="M4.5 6 8 9.5 11.5 6" /></svg>
  </button>
  <span class="divider" aria-hidden="true"></span>
  {#if !working}
    <button
      class:on={ignoring}
      onclick={() => prefs.set("oxbow.diff.ignoreWhitespace", !prefs.get("oxbow.diff.ignoreWhitespace"))}
      aria-pressed={ignoring}
      aria-label="Ignore whitespace changes"
      title={ignoring ? "Whitespace changes are left out, so the counts can be lower than in the list of files. Click to show them" : "Leave out changes of whitespace only"}
    >
      <svg class="icon" viewBox="0 0 16 16"><path d="M3.5 5.5v5h9v-5" /><path d="M2 13.5h12" /></svg>
    </button>
  {/if}
  <button
    class:on={!wrap}
    onclick={() => prefs.set("oxbow.diff.wrap", !wrap)}
    aria-pressed={!wrap}
    aria-label="No wrap"
    title={withKeys(wrap ? "Don't wrap long lines: they run on and scroll sideways" : "Wrap long lines at the edge", combo("wrap"))}
  >
    <svg class="icon" viewBox="0 0 16 16">
      {#if wrap}<path d="M2.5 4h11M2.5 8h9a2 2 0 0 1 0 4H8.5M10 10.5 8.5 12l1.5 1.5M2.5 12h3" />{:else}<path d="M2.5 4h11M2.5 8h11M2.5 12h6M11.5 10.5 13 12l-1.5 1.5" />{/if}
    </svg>
  </button>
  <button
    class:on={split}
    onclick={() => prefs.set("oxbow.diff.layout", split ? "unified" : "split")}
    aria-pressed={split}
    aria-label={split ? "Show unified" : "Show side by side"}
    title={split ? "Show old and new lines one under the other" : "Show old and new lines side by side"}
  >
    <svg class="icon" viewBox="0 0 16 16">
      {#if split}<rect x="2" y="2.5" width="12" height="11" rx="2" /><path d="M8 2.5v11M4 6h2M10 6h2M4 9h2M10 9h2" />{:else}<rect x="2" y="2.5" width="12" height="11" rx="2" /><path d="M5 6h6M5 9h6" />{/if}
    </svg>
  </button>
  <button class:on={whole} onclick={onToggleWhole} aria-pressed={whole} aria-label={whole ? "Show changes only" : "Show whole file"} title={whole ? "Show changes only" : "Show whole file"}>
    <svg class="icon" viewBox="0 0 16 16">
      {#if whole}<path d="M3 5.5h10M3 10.5h10M8 1.5v2.5M8 12v2.5M6 3l2 1.5L10 3M6 13l2-1.5 2 1.5" />{:else}<path d="M3 3h10M3 13h10M3 8h10M8 4.5v-3M8 11.5v3" />{/if}
    </svg>
  </button>
</span>

<style>
  .tools {
    display: flex;
    align-items: center;
    gap: 2px;
    flex-shrink: 0;
  }
  button {
    width: 26px;
    height: 26px;
    border-radius: var(--radius-pill);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--icon);
  }
  button:hover {
    background: var(--field);
  }
  button.on {
    background: var(--side-sel);
    color: var(--text);
  }
  .divider {
    width: 1px;
    height: 16px;
    margin: 0 4px;
    background: var(--sep);
  }
  .icon {
    width: 15px;
    height: 15px;
  }
</style>
