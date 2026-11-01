<script lang="ts">
  import { lineKind } from "./confirm.svelte";
  import type { ConsoleLine } from "./console";
  let { lines }: { lines: ConsoleLine[] } = $props();
  const MASK = "••••••";
  /** Parts of an output line: masked secrets and commit ids get their own color. */
  function parts(text: string): { text: string; kind: "text" | "mask" | "sha" }[] {
    const out: { text: string; kind: "text" | "mask" | "sha" }[] = [];
    // A commit id stands alone, as in `7a6ce6d..d7855a5`; hex in a path or a word stays text.
    for (const piece of text.split(/(••••••|(?<![\w/-])[0-9a-f]{7,40}(?![\w/-]))/)) {
      if (!piece) continue;
      out.push({ text: piece, kind: piece === MASK ? "mask" : /^[0-9a-f]{7,40}$/.test(piece) && /\d/.test(piece) ? "sha" : "text" });
    }
    return out;
  }
</script>

<div class="out mono selectable">
  {#each lines as line, i (i)}
    {@const kind = lineKind(line.t, !!line.e)}
    <div class="line" class:hint={kind === "hint"} class:cut={/^⋯ \d+ more lines ⋯$/.test(line.t)}>
      {#each parts(line.t) as part, k (k)}<span class={part.kind}>{part.text}</span>{/each}
    </div>
  {/each}
</div>

<style>
  .mono { font-family: var(--term-font); }
  .out {
    margin: 4px 0 2px;
    padding-left: 14px;
    border-left: 2px solid var(--term-border);
    color: var(--term-out);
    font-size: var(--term-size);
    line-height: var(--term-line);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .out .hint {
    color: var(--term-hint);
  }
  .out .cut {
    color: var(--term-dim);
    font-style: italic;
  }
  .out .mask {
    color: var(--term-dim);
  }
  .out .sha {
    color: var(--term-sha);
  }
</style>
