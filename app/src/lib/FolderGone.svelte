<script lang="ts">
  // The open repository's folder isn't there any more: why, what is safe, and the ways out.
  // Shown on the Welcome window's right, in place of Recent Repositories.
  import type { Gone } from "./types";
  import type { GoneAction, GoneScreen } from "./gone";

  let {
    gone,
    screen,
    shown,
    busy,
    note,
    onMain,
    onFind,
    onRetry,
    onClose,
  }: {
    gone: Gone;
    screen: GoneScreen;
    /** The path as the screen shows it (`~/src/acme`). */
    shown: (path: string) => string;
    /** Opening or reconnecting: the buttons wait. */
    busy: boolean;
    /** What the last Retry or Find Folder… found, when it didn't work. */
    note: string | null;
    onMain: () => void;
    onFind: () => void;
    onRetry: () => void;
    onClose: () => void;
  } = $props();

  let first = $state<HTMLButtonElement>();
  // Enter takes the way out that fits the reason best.
  $effect(() => {
    gone.path;
    requestAnimationFrame(() => first?.focus());
  });

  const mainName = $derived(screen.main?.split(/[/\\]/).filter(Boolean).pop() ?? "");
  const buttons = $derived(
    (
      [
        screen.main ? { id: "main", label: "Open Main Folder", title: `Open ${mainName}, the repository’s main folder`, run: onMain } : null,
        { id: "find", label: "Find Folder…", title: gone.worktree ? "Choose where the folder is now: git is pointed at it there" : "Choose where the folder is now", run: onFind },
        { id: "retry", label: "Retry", title: "Look for the folder again", run: onRetry },
      ] as ({ id: GoneAction; label: string; title: string; run: () => void } | null)[]
    ).filter((b) => b !== null),
  );
</script>

<div class="gone" aria-labelledby="gone-title">
  <span class="gone-icon" class:disk={gone.why === "disk"}>
    {#if gone.why === "disk"}
      <svg viewBox="0 0 16 16"><rect x="2" y="5" width="12" height="6" rx="1.5" /><path d="M5 8h.01M11.5 8h-3" /></svg>
    {:else}
      <svg viewBox="0 0 16 16"><path d="M1.5 4.5a1 1 0 0 1 1-1H6l1.5 1.5h6a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" stroke-dasharray="2 1.6" /></svg>
    {/if}
  </span>
  <h2 class="title" id="gone-title">{screen.title}</h2>
  <div class="path" title={gone.path}>{shown(gone.path)}</div>
  <p class="reason">{screen.reason}</p>

  {#if screen.facts.length}
    <ul class="facts">
      {#each screen.facts as fact, i (i)}
        <li class={fact.tone}>
          <span class="dot">
            <svg viewBox="0 0 16 16"><path d={fact.tone === "safe" ? "M3.5 8.5 6.5 11.5 12.5 4.5" : fact.tone === "warn" ? "M8 4v5M8 11.5v.1" : "M8 7.5v4M8 4.5v.1"} /></svg>
          </span>
          <span>{fact.text}</span>
        </li>
      {/each}
    </ul>
  {/if}

  <div class="buttons">
    {#each buttons as button (button.id)}
      {#if button.id === screen.primary}
        <button class="button primary" bind:this={first} disabled={busy} title={button.title} onclick={button.run}>{button.label}</button>
      {:else}
        <button class="button" disabled={busy} title={button.title} onclick={button.run}>{button.label}</button>
      {/if}
    {/each}
    <button class="button" disabled={busy} onclick={onClose}>Close</button>
  </div>
  {#if note && !busy}<p class="after" role="status">{note}</p>{/if}
</div>

<style>
  .gone {
    flex-grow: 1;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    justify-content: center;
    padding: 24px 36px 28px;
    overflow-y: auto;
  }
  .gone-icon {
    width: 48px;
    height: 48px;
    border-radius: var(--radius-xl);
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--field);
    color: var(--text2);
    flex-shrink: 0;
  }
  .gone-icon svg {
    width: 26px;
    height: 26px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .title {
    margin: 16px 0 0;
    font-size: var(--text-heading);
    font-weight: 700;
    line-height: 1.3;
    overflow-wrap: anywhere;
  }
  .path {
    margin-top: 4px;
    max-width: 100%;
    font-family: var(--mono);
    font-size: var(--text-small);
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .reason {
    margin: 14px 0 0;
    line-height: 1.45;
    overflow-wrap: anywhere;
  }
  .facts {
    list-style: none;
    margin: 16px 0 0;
    padding: 10px 12px;
    align-self: stretch;
    display: flex;
    flex-direction: column;
    gap: 8px;
    border-radius: var(--radius-l);
    border: 0.5px solid var(--sep);
    background: var(--win);
    font-size: var(--text-small);
    line-height: 1.4;
  }
  .facts li {
    display: flex;
    gap: 8px;
    align-items: flex-start;
  }
  .dot {
    width: 15px;
    height: 15px;
    margin-top: 1px;
    border-radius: var(--radius-pill);
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--green-soft);
    color: var(--green);
    flex-shrink: 0;
  }
  .warn .dot {
    background: var(--orange-soft);
    color: var(--orange);
  }
  .note .dot {
    background: var(--accent-quiet);
    color: var(--accent-ink);
  }
  .dot svg {
    width: 10px;
    height: 10px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .buttons {
    margin-top: 20px;
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .buttons .button {
    white-space: nowrap;
    min-width: 76px;
  }
  .after {
    margin: 10px 0 0;
    font-size: var(--text-small);
    color: var(--text2);
    line-height: 1.4;
    overflow-wrap: anywhere;
  }
</style>
