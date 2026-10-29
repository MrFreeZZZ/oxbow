<script lang="ts">
  // A review comment's text with its suggested changes: the lines it is on, and the lines
  // suggested in their place, with Apply Suggestion for whoever has the branch checked out.
  import { bodyParts, type BodyPart } from "./suggestion";

  type Suggestion = Extract<BodyPart, { kind: "suggestion" }>;

  let {
    body,
    before = () => null,
    onApply,
    applyOff = null,
  }: {
    body: string;
    /** The lines a suggestion goes in place of, when they are known. */
    before?: (part: Suggestion) => string[] | null;
    /** Apply Suggestion; none where it can't be applied, e.g. on an outdated thread. */
    onApply?: (part: Suggestion) => void;
    /** Why Apply Suggestion is off, e.g. the branch isn't checked out. */
    applyOff?: string | null;
  } = $props();

  const parts = $derived(bodyParts(body));
</script>

{#each parts as part, i (i)}
  {#if part.kind === "text"}
    <p class="body selectable">{part.text}</p>
  {:else}
    {@const old = before(part)}
    <div class="suggestion">
      <div class="head">
        <svg class="icon" viewBox="0 0 16 16"><path d="M4 5.5h8M8 1.5v8M4 13.5h8" /></svg>
        <span>Suggested change</span>
        <span class="grow"></span>
        {#if onApply}
          <button class="button mini" disabled={!!applyOff} title={applyOff ?? "Put these lines in your file, uncommitted"} onclick={() => onApply(part)}>Apply Suggestion</button>
        {/if}
      </div>
      <div class="lines selectable">
        {#each old ?? [] as line, j (j)}<div class="line removed">{line || " "}</div>{/each}
        {#each part.lines as line, j (j)}<div class="line added">{line || " "}</div>{/each}
        {#if !part.lines.length}<div class="none">The lines are taken out.</div>{/if}
      </div>
    </div>
  {/if}
{/each}

<style>
  .body {
    margin: 0;
    font-size: var(--text-body);
    line-height: 1.45;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .suggestion {
    margin: 4px 0;
    border-radius: var(--radius-m);
    border: 0.5px solid var(--panel-border);
    background: var(--win);
    overflow: hidden;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 6px 0 10px;
    border-bottom: 0.5px solid var(--sep);
    font-size: var(--text-small);
    font-weight: 600;
    color: var(--text2);
  }
  .grow {
    flex-grow: 1;
  }
  .icon {
    width: 13px;
    height: 13px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
    stroke-linecap: round;
  }
  .lines {
    padding: 4px 0;
  }
  .line {
    padding: 0 10px;
    min-height: 19px;
    line-height: 19px;
    font-family: var(--mono);
    font-size: var(--text-small);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .line.removed {
    background: var(--del);
  }
  .line.added {
    background: var(--add);
  }
  .none {
    padding: 0 10px;
    font-size: var(--text-small);
    color: var(--text2);
  }
</style>
