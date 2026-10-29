<script lang="ts">
  // A version of Local History beside what it changed: since the version saved before it (the
  // last commit for the oldest), or with Compare with Current File, how the file on disk now
  // differs from it. Restore puts it back on disk.

  import { api } from "./api";
  import { relativeTime } from "./format";
  import { bytes } from "./unusual";
  import { sinceWhat, whenSaved } from "./localHistory";
  import { localVersionEntries, restoreVersion } from "./localHistoryMenu";
  import { diffSettings, wholeByDefault } from "./prefs.svelte";
  import type { FileDiff, LocalVersion } from "./types";
  import DiffView from "./DiffView.svelte";
  import Menu, { type MenuEntry } from "./Menu.svelte";

  let {
    version: v,
    color,
    reload = 0,
    history = false,
    comparing = $bindable(false),
  }: {
    version: LocalVersion;
    color: number;
    /** Goes up on every reload, so Compare with Current File follows the file on disk. */
    reload?: number;
    /** Offer File History of the file: on the Local History screen. */
    history?: boolean;
    /** Compare with Current File: the file on disk beside the version, in place of its changes. */
    comparing?: boolean;
  } = $props();

  let diffs = $state<FileDiff[]>([]);
  let error = $state<string | null>(null);
  let whole = $state(wholeByDefault());
  let menu = $state<{ x: number; y: number; entries: MenuEntry[] } | null>(null);

  const DIFF = "local-diff";
  const FULL = "local-full";
  $effect(() => () => {
    api.stopReads([DIFF, FULL]).catch(() => {});
  });

  $effect(() => {
    const shown = v;
    const against = comparing;
    const wholeFile = whole;
    diffSettings();
    if (against) reload;
    error = null;
    if (!shown.kept) {
      diffs = [];
      return;
    }
    api.localVersionDiff(shown.path, shown.id, shown.base, against, wholeFile, false, DIFF).then(
      (d) => {
        if (v === shown && comparing === against && whole === wholeFile) diffs = d;
      },
      (err) => {
        if (v !== shown) return;
        diffs = [];
        error = String(err);
      },
    );
  });

  const what = $derived(comparing ? (v.gone ? "the file isn’t on disk now" : "compared with the file on disk now") : sinceWhat(v));

  function openMenu(event: MouseEvent) {
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    menu = { x: box.left, y: box.bottom + 4, entries: localVersionEntries(v, { compare: () => (comparing = true), history }) };
  }
</script>

<div class="head">
  <span class="badge" aria-hidden="true">
    <svg class="icon" viewBox="0 0 16 16"><path d="M8 2.5a5.5 5.5 0 1 1-5.5 5.5M8 5v3l2 1.5M2.5 3v2.5H5" /></svg>
  </span>
  <span class="who">
    <span class="title">Saved {whenSaved(v.at)}</span>
    <span class="byline">{relativeTime(v.at / 1000)} · {bytes(v.size)} · {what}</span>
  </span>
  <button class="button" class:on={comparing} aria-pressed={comparing} onclick={() => (comparing = !comparing)} disabled={!v.kept} title={comparing ? "Show what this version changed" : "How the file on disk now differs from this version"}>
    {comparing ? "Show Its Changes" : "Compare with Current"}
  </button>
  <button class="button" onclick={() => restoreVersion(v)} disabled={!v.kept} title="Put the file on disk back as it was saved; Undo takes it back">Restore…</button>
  <button class="button more" onclick={openMenu} aria-label="More for this version" title="Restore, Compare, Save As">
    <svg class="icon" viewBox="0 0 16 16"><path d="M3.5 8h.01M8 8h.01M12.5 8h.01" /></svg>
  </button>
</div>
{#if !v.kept}
  <p class="note">Local History no longer has this version: it was older than the days kept, or its space ran out.</p>
{:else if v.gone && !comparing}
  <p class="note">The file isn’t on disk now: Restore brings it back.</p>
{/if}
{#if error}
  <p class="note error" role="alert">{error}</p>
{:else if v.kept}
  <div class="scroll">
    <DiffView
      {diffs}
      {color}
      {whole}
      history={false}
      onToggleWhole={() => (whole = !whole)}
      loadFull={(d) => api.localVersionDiff(v.path, v.id, v.base, comparing, whole, true, FULL).then((all) => all[0] ?? d)}
    />
  </div>
{/if}

{#if menu}
  <Menu x={menu.x} y={menu.y} label="Version menu" entries={menu.entries} onClose={() => (menu = null)} />
{/if}

<style>
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--sep);
    flex-shrink: 0;
  }
  .badge {
    width: 30px;
    height: 30px;
    border-radius: var(--radius-pill);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    background: var(--field);
    color: var(--text2);
  }
  .badge .icon {
    width: 16px;
    height: 16px;
  }
  .who {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .title {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .byline {
    font-size: var(--text-caption);
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .button.on {
    background: var(--accent-quiet);
    color: var(--accent-ink);
  }
  .button .icon {
    width: 13px;
    height: 13px;
  }
  .note {
    margin: 8px 16px 0;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .error {
    color: var(--red);
  }
  .scroll {
    flex-grow: 1;
    overflow-y: auto;
    min-height: 0;
  }
</style>
