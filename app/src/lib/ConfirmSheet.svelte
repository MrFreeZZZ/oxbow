<script lang="ts">
  import { confirm, type Icon } from "./confirm.svelte";
  import { plate, tint } from "./format";

  let { repo, branch, color }: { repo: string; branch: string | null; color: number } = $props();

  let copied = $state(false);
  let goButton = $state<HTMLButtonElement>();

  const request = $derived(confirm.request);
  const failed = $derived(confirm.phase === "failed");
  const running = $derived(confirm.phase === "running");

  const icons: Record<Icon, string> = {
    stage: "M8 12.5V4M4.5 7.5 8 4l3.5 3.5M3 1.5h10",
    unstage: "M8 3.5V12M4.5 8.5 8 12l3.5-3.5M3 14.5h10",
    discard: "M2.5 4.5h11M6 4.5V3a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1v1.5M4 4.5l.7 9a1 1 0 0 0 1 .9h4.6a1 1 0 0 0 1-.9l.7-9",
    commit: "M1.5 8h3.5M11 8h3.5M8 5a3 3 0 1 1 0 6 3 3 0 0 1 0-6z",
  };

  // Token colors of the design's terminal block.
  const GIT = "#F2F2F7";
  const SUB = "#C9A9E8";
  const FLAG = "#E6B08A";
  const SHA = "#7FD1C4";
  const STR = "#A9D59A";
  const TEXT = "#E6E6EA";

  /** Split a shell-quoted command into colored words. */
  function tokens(display: string) {
    const words = display.match(/'(?:[^']|'\\'')*'|"[^"]*"|\S+/g) ?? [];
    return words.map((word, i) => ({
      text: word,
      bold: i === 0,
      color:
        i === 0 ? GIT : i === 1 ? SUB : word.startsWith("-") ? FLAG : /^['"]/.test(word) ? STR : /^[0-9a-f]{7,40}$/.test(word) ? SHA : TEXT,
    }));
  }

  async function copy() {
    await navigator.clipboard.writeText(confirm.commands.map((c) => c.display).join("\n")).catch(() => {});
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  function onKey(event: KeyboardEvent) {
    if (!request) return;
    if (event.key === "Escape") {
      event.preventDefault();
      confirm.cancel();
    } else if (event.key === "Enter" && !failed && !running && !(event.target instanceof HTMLButtonElement)) {
      event.preventDefault();
      confirm.go();
    }
  }

  // The action button takes the focus, so Return runs it and Escape cancels.
  $effect(() => {
    if (request && confirm.phase === "ask") requestAnimationFrame(() => goButton?.focus());
  });
</script>

<svelte:window onkeydown={onKey} />

{#if request}
  <div class="dim">
    <div class="sheet" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title">
      <div class="top">
        <span
          class="tile"
          style:background={request.danger || failed ? "var(--danger-soft)" : tint(color, "label")}
          style:color={request.danger || failed ? "var(--red)" : plate(color)}
        >
          <svg class="icon" viewBox="0 0 16 16"><path d={icons[request.icon]} /></svg>
        </span>
        <div class="words">
          <span class="title" id="confirm-title">{failed ? "Git stopped with an error" : request.title}</span>
          <span class="body">
            {#each request.body as part, i (i)}
              {#if typeof part === "string"}{part}{:else if "branch" in part}<span class="chip" style:background={tint(part.color, "label")} style:color={plate(part.color)}>{part.branch}</span
                >{:else if "code" in part}<span class="chip mono">{part.code}</span>{:else}<span class="quote">“{part.quote}”</span>{/if}
            {/each}
          </span>
        </div>
      </div>

      <div class="term" aria-label="Git command">
        <div class="bar">
          <span class="light"></span><span class="light"></span><span class="light"></span>
          <span class="where">zsh · {repo}</span>
          <button class="copy" onclick={copy} disabled={!confirm.commands.length}>
            <svg class="icon" viewBox="0 0 16 16"><path d="M5.5 5.5h7v8h-7zM3.5 10.5v-8h7" /></svg>
            {copied ? "Copied" : "Copy"}
          </button>
        </div>
        <div class="lines mono selectable">
          {#each confirm.commands as command, i (i)}
            {#if command.comment}<div class="comment"># {command.comment}</div>{/if}
            <div class="cmd">
              <span class="prompt">{repo} </span><span style:color="var(--lane-{color})">({branch ?? "HEAD"})</span><span class="prompt"> % </span>
              {#each tokens(command.display) as token, k (k)}<span style:color={token.color} class:bold={token.bold}>{token.text}</span>{" "}{/each}
            </div>
          {/each}
          {#if failed}<div class="error">{confirm.output}</div>{/if}
        </div>
      </div>

      <div class="foot">
        <span class="note">{failed ? "Nothing else was run. Fix the problem and try again." : running ? "Running…" : (request.note ?? "")}</span>
        {#if failed}
          <button class="btn" onclick={() => confirm.cancel()}>Close</button>
        {:else}
          <button class="btn" onclick={() => confirm.cancel()} disabled={running}>Cancel</button>
          <button class="btn go" class:danger={request.danger} bind:this={goButton} onclick={() => confirm.go()} disabled={running}>{request.button}</button>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .dim {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: var(--dim);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .sheet {
    width: min(620px, calc(100vw - 48px));
    padding: 22px 22px 18px;
    border-radius: 22px;
    background: var(--sheet);
    border: 0.5px solid var(--glass-border);
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.28);
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .top {
    display: flex;
    gap: 14px;
    align-items: flex-start;
  }
  .tile {
    width: 44px;
    height: 44px;
    flex-shrink: 0;
    border-radius: 12px;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .tile .icon {
    width: 22px;
    height: 22px;
    stroke-width: 1.4;
  }
  .words {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .title {
    font-size: 15px;
    font-weight: 700;
    line-height: 1.3;
  }
  .body {
    line-height: 1.75;
    overflow-wrap: anywhere;
  }
  .chip {
    padding: 1px 6px;
    border-radius: 7px;
    background: var(--field);
    font-weight: 500;
    white-space: pre-wrap;
  }
  .chip.mono {
    padding: 1px 5px;
    font-weight: 400;
    font-size: 12px;
  }
  .quote {
    font-weight: 600;
  }
  .term {
    border-radius: 12px;
    overflow: hidden;
    background: var(--term);
    border: 0.5px solid rgba(255, 255, 255, 0.08);
    box-shadow: inset 0 0 0 0.5px rgba(0, 0, 0, 0.4);
  }
  .bar {
    height: 28px;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 8px 0 11px;
    background: var(--term-bar);
    border-bottom: 0.5px solid rgba(255, 255, 255, 0.06);
  }
  .light {
    width: 9px;
    height: 9px;
    border-radius: 5px;
    background: #4a4c52;
  }
  .where {
    flex-grow: 1;
    text-align: center;
    font-size: 11px;
    color: #8e8e93;
  }
  .copy {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 20px;
    padding: 0 8px;
    border-radius: 10px;
    font-size: 11px;
    color: #a1a1a8;
    background: rgba(255, 255, 255, 0.06);
  }
  .copy .icon {
    width: 11px;
    height: 11px;
  }
  .lines {
    padding: 10px 14px 12px;
    font-size: 12px;
    line-height: 1.7;
    max-height: 240px;
    overflow-y: auto;
  }
  .cmd {
    white-space: pre-wrap;
    padding-left: 24px;
    text-indent: -24px;
    overflow-wrap: anywhere;
  }
  .prompt {
    color: #8e8e93;
  }
  .bold {
    font-weight: 600;
  }
  .comment {
    color: #7c8088;
    font-style: italic;
  }
  .error {
    color: #ff8a80;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    padding-top: 4px;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-top: 2px;
  }
  .note {
    flex-grow: 1;
    min-width: 0;
    font-size: 11px;
    line-height: 1.35;
    color: var(--text2);
  }
  .btn {
    flex-shrink: 0;
    min-width: 96px;
    height: 30px;
    padding: 0 16px;
    border-radius: 15px;
    text-align: center;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    font-weight: 500;
  }
  .btn.go {
    background: var(--accent);
    border-color: transparent;
    color: #fff;
    font-weight: 600;
  }
  .btn.go.danger {
    background: var(--danger);
  }
  .btn:disabled {
    opacity: 0.6;
  }
  .btn:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
