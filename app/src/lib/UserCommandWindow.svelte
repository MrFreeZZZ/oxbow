<script lang="ts">
  import { api } from "./api";
  import { onMount, tick, untrack } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { mac } from "./keys";
  import CommandWords from "./CommandWords.svelte";
  import ConsoleOutput from "./ConsoleOutput.svelte";
  import { clock, dayOf, duration, exitOf, ownCommands, type CommandWindowState } from "./console";

  let view = $state<CommandWindowState | null>(null);
  let loadError = $state("");
  const title = $derived(view?.title ?? "Run Command");
  const running = $derived(view?.running ?? !loadError);
  const result = $derived(loadError || view?.result || "Loading command…");
  const path = $derived(view?.path ?? "");
  const branch = $derived(view?.branch ?? "");
  const commit = $derived(view?.commit ?? "");
  const entry = $derived(view?.entry ?? null);
  const win = getCurrentWindow();
  const onClose = () => win.close().catch((err) => loadError = String(err));
  onMount(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const take = (next: CommandWindowState) => {
      if (!disposed && (!view || next.revision > view.revision)) {
        const newRun = !view || view.path !== next.path || (view.entry && next.entry === null);
        view = next;
        if (newRun) { copied = false; copyError = ""; stopError = ""; following = true; }
      }
    };
    void (async () => {
      try {
        unlisten = await win.listen<CommandWindowState>("user-command-state", (event) => take(event.payload));
        if (disposed) { unlisten(); return; }
        take(await api.userCommandState());
      } catch (err) { if (!disposed) loadError = String(err); }
    })();
    return () => { disposed = true; unlisten?.(); };
  });
  let log: HTMLDivElement;
  let following = $state(true);
  let copied = $state(false);
  let copyError = $state("");
  let stopError = $state("");
  const name = $derived(path.split(/[\\/]/).filter(Boolean).pop() ?? path);
  const commands = $derived(entry ? ownCommands(entry) : []);
  const summary = $derived(running ? "Running…" : entry?.stopped ? "Stopped" : entry?.failed ? (exitOf(entry) === null ? result : `Failed · Exit code ${exitOf(entry)}`) : entry ? "Completed · Exit code 0" : result);
  $effect(() => {
    entry;
    if (untrack(() => following)) void tick().then(() => { if (log) log.scrollTop = log.scrollHeight; });
  });
  async function copyOutput() {
    try {
      await navigator.clipboard.writeText(commands.map((ran) => `% ${ran.display}\n${(ran.output ?? []).map((line) => line.t).join("\n")}`).join("\n\n"));
      copied = true;
      copyError = "";
    } catch { copyError = "Could not copy output."; }
  }
  async function stop() {
    try { await api.stopAction(); } catch (err) { stopError = String(err); }
  }
</script>

<svelte:window onkeydown={(event) => { if (event.key === "Escape" && !running) { event.preventDefault(); void onClose(); } }} />

<div class="command-window" class:mac>
    <header data-tauri-drag-region>
      <div class="title" data-tauri-drag-region>{title}<small>{name}</small></div>
      <span class="spacer" data-tauri-drag-region></span>
      <button class="button small" disabled={!commands.length} onclick={copyOutput}>
        <svg class="icon" viewBox="0 0 16 16" aria-hidden="true"><path d="M6 5.5h7v8H6zM10 5.5v-3H3v8h3" /></svg>{copied ? "Copied" : "Copy Output"}
      </button>
    </header>
    <div class="context"><span>{path}</span><span class="ref">{branch || "Detached HEAD"} · {commit ? commit.slice(0, 7) : "No commit"}</span></div>
    <div class="log" bind:this={log} onscroll={() => following = log.scrollHeight - log.scrollTop - log.clientHeight < 24}>
      {#if entry}
        <div class="day">{dayOf(entry.time)}</div>
        <div class="entry">
          <span class="time mono">{clock(entry.time)}</span>
          <div class="body">
            <div class="head">
              <span class="entry-title">{title}</span><span class="ms">{running ? "running…" : duration(entry.millis)}</span>
              {#if running}<span class="spinner" aria-label="Running"></span>
              {:else if entry.stopped}<span class="badge">stopped</span>
              {:else if entry.failed}<span class="badge exit">{exitOf(entry) === null ? "failed" : `exit ${exitOf(entry)}`}</span>
              {:else}<svg class="icon ok" viewBox="0 0 16 16" aria-label="Done"><path d="M3.5 8.5 6.5 11.5 12.5 4.5" /></svg>{/if}
            </div>
            {#each commands as ran}
              <div class="cmd mono selectable"><span class="prompt">%</span> <CommandWords text={ran.display} /></div>
              {#if ran.output?.length}<ConsoleOutput lines={ran.output} />{:else if !running}<div class="empty-output">no output</div>{/if}
            {/each}
          </div>
        </div>
      {:else}<div class="waiting">{running ? "Waiting to run…" : result}</div>{/if}
    </div>
    <footer>
      <span class="summary" class:failed={entry?.failed} role="status">{copyError || stopError || summary}</span><span class="spacer"></span>
      <button class="button" onclick={() => api.openConsole()}>Show Console</button>
      {#if running}<button class="button" onclick={stop}>Stop</button>{:else}<button class="button primary" onclick={onClose}>Done</button>{/if}
    </footer>
</div>

<style>
  .command-window { height: 100%; display: flex; flex-direction: column; overflow: hidden; background: var(--win); }
  .mac header { padding-left: 92px; }
  header { height: 56px; flex-shrink: 0; display: flex; align-items: center; gap: 10px; padding: 10px 14px 10px 18px; border-bottom: .5px solid var(--sep); }
  .title { font-size: var(--text-body); font-weight: 600; min-width: 0; overflow-wrap: anywhere; }
  .title small { font-size: var(--text-small); font-weight: 400; color: var(--text2); margin-left: 6px; }
  .spacer { flex: 1; }
  header .icon { width: 14px; height: 14px; }
  .context { flex-shrink: 0; display: flex; flex-wrap: wrap; justify-content: space-between; gap: 8px; padding: 8px 18px; border-bottom: .5px solid var(--sep); font-size: var(--text-caption); color: var(--text2); overflow-wrap: anywhere; }
  .ref, .mono { font-family: var(--term-font); }
  .log { flex: 1; min-height: 0; overflow: auto; background: var(--term); padding-bottom: 12px; }
  .day { position: sticky; top: 0; padding: 10px 18px 6px; background: var(--term); color: var(--term-dim); font-size: var(--text-caption); font-weight: 600; text-transform: uppercase; letter-spacing: .02em; }
  .entry { display: grid; grid-template-columns: max-content minmax(0, 1fr); column-gap: 18px; padding: 8px 18px; border-top: .5px solid var(--term-border); }
  .time { color: var(--term-dim); font-size: calc(var(--term-size) - 1px); font-variant-numeric: tabular-nums; white-space: nowrap; padding-top: 2px; }
  .head { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; font-size: var(--text-small); color: var(--text2); }
  .entry-title { color: var(--text); font-weight: 600; }
  .ms { font-size: var(--text-caption); color: var(--term-dim); }
  .ok { width: 13px; height: 13px; color: var(--term-dim); }
  .badge { padding: 1px 7px; border-radius: var(--radius-pill); font-size: var(--text-caption); font-weight: 500; background: var(--field); color: var(--text2); }
  .badge.exit { background: var(--orange-soft); color: var(--orange); }
  .spinner { width: 11px; height: 11px; border-radius: 50%; border: 1.5px solid var(--term-border); border-top-color: var(--accent); animation: spin .8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .spinner { animation: none; } }
  .cmd { margin-top: 3px; color: var(--term-git); font-size: var(--term-size); line-height: var(--term-line); white-space: pre-wrap; word-break: break-word; text-indent: -1.3em; padding-left: 1.3em; }
  .prompt, .empty-output { color: var(--term-dim); }
  .empty-output { margin-top: 3px; font-size: var(--text-caption); }
  .waiting { padding: 24px 18px; color: var(--text2); font-size: var(--text-small); overflow-wrap: anywhere; }
  footer { flex-shrink: 0; display: flex; align-items: center; flex-wrap: wrap; gap: 8px; padding: 12px 14px 12px 18px; border-top: .5px solid var(--sep); }
  .summary { font-size: var(--text-caption); color: var(--text2); overflow-wrap: anywhere; min-width: 0; flex-shrink: 1; }
  .summary.failed { color: var(--orange); }
  @media (max-width: 520px) { .entry { gap: 10px; padding: 8px 12px; } .summary { flex-basis: 100%; } }
</style>
