<script lang="ts" module>
  /** A line of the block: a command after a prompt, a `# comment`, or what git printed. */
  export type TermLine =
    | { kind: "cmd"; text: string; prompt?: string }
    | { kind: "comment" | "out" | "err" | "ok"; text: string };
</script>

<script lang="ts">
  // The terminal block of the sheets that run git outside a repository (Clone, New Repository):
  // the exact commands, then what git prints while they run.

  import { tokens } from "./term";

  let { where, lines, copy }: { where: string; lines: TermLine[]; copy: string[] } = $props();

  let copied = $state(false);
  let box = $state<HTMLDivElement>();

  const OUTPUT: Record<string, string> = {
    out: "var(--term-out)",
    err: "var(--term-err)",
    ok: "var(--term-ok)",
  };

  async function copyAll() {
    await navigator.clipboard.writeText(copy.join("\n")).catch(() => {});
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  // The newest line stays in view, as in a terminal.
  $effect(() => {
    lines.length;
    if (box) requestAnimationFrame(() => box && (box.scrollTop = box.scrollHeight));
  });
</script>

<div class="term" aria-label="Git commands">
  <div class="bar">
    <span class="light"></span><span class="light"></span><span class="light"></span>
    <span class="where">{where}</span>
    <button class="copy" onclick={copyAll} disabled={!copy.length}>
      <svg class="icon" viewBox="0 0 16 16"><path d="M5.5 5.5h7v8h-7zM3.5 10.5v-8h7" /></svg>
      {copied ? "Copied" : "Copy"}
    </button>
  </div>
  <div class="lines mono selectable" bind:this={box} aria-live="polite">
    {#each lines as line, i (i)}
      {#if line.kind === "cmd"}
        <div class="cmd">
          <span class="prompt">{`${line.prompt ?? "~"} % `}</span>{#each tokens(line.text) as token, k (k)}<span style:color={token.color} class:bold={token.bold}>{token.text}</span>{" "}{/each}
        </div>
      {:else if line.kind === "comment"}
        <div class="comment"># {line.text}</div>
      {:else}
        <div class="output" style:color={OUTPUT[line.kind]}>{line.text}</div>
      {/if}
    {/each}
  </div>
</div>

<style>
  .term {
    border-radius: 12px;
    overflow: hidden;
    background: var(--term);
    border: 0.5px solid var(--term-border);
    box-shadow: var(--term-shadow);
    flex-shrink: 0;
  }
  .bar {
    height: 28px;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 8px 0 11px;
    background: var(--term-bar);
    border-bottom: 0.5px solid var(--term-border);
  }
  .light {
    width: 9px;
    height: 9px;
    border-radius: 5px;
    background: var(--term-dot);
  }
  .where {
    flex-grow: 1;
    text-align: center;
    font-size: 11px;
    color: var(--term-dim);
  }
  .copy {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 20px;
    padding: 0 8px;
    border-radius: 10px;
    font-size: 11px;
    color: var(--term-dim);
    background: var(--term-button);
  }
  .copy .icon {
    width: 11px;
    height: 11px;
  }
  .lines {
    padding: 10px 14px 12px;
    font-size: 12px;
    line-height: 1.7;
    max-height: 200px;
    overflow-y: auto;
  }
  .cmd {
    white-space: pre-wrap;
    padding-left: 24px;
    text-indent: -24px;
    overflow-wrap: anywhere;
  }
  .prompt {
    color: var(--term-dim);
  }
  .bold {
    font-weight: 600;
  }
  .comment {
    color: var(--term-comment);
    font-style: italic;
  }
  .output {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    padding-left: 16px;
  }
</style>
