<script lang="ts">
  // The Diff & Text preview: a small change drawn with the same code styles as the real diffs,
  // in the layout, context lines and word highlighting chosen above it.

  import { prefs } from "./prefs.svelte";

  type Line = { kind: "ctx" | "add" | "del" | "fold"; old?: number; new?: number; before: string; word?: string; after?: string };
  const preview = $derived.by((): Line[] => {
    const full = prefs.get("oxbow.diff.view") === "full";
    const context = full ? 3 : Math.min(3, prefs.get("oxbow.diff.contextLines"));
    const above: [number, string][] = [
      [19, "\tfn default() -> Self {"],
      [20, "\t\tSelf {"],
      [21, "\t\t\trequest_timeout: Duration::from_secs(10),"],
    ];
    const below: [number, string][] = [
      [23, "\t\t\tretries: 2,"],
      [24, "\t\t}"],
      [25, "\t}"],
    ];
    const ctx = ([n, line]: [number, string]): Line => ({ kind: "ctx", old: n, new: n, before: line });
    const fold = (n: number): Line => ({ kind: "fold", before: `⋯  ${n} unchanged lines` });
    return [
      ...(full ? [] : [fold(18 + 3 - context)]),
      ...above.slice(3 - context).map(ctx),
      { kind: "del", old: 22, before: "\t\t\tlogin_timeout: Duration::from_secs(", word: "10", after: ")," },
      { kind: "add", new: 22, before: "\t\t\tlogin_timeout: Duration::from_secs(", word: "30", after: ")," },
      ...below.slice(0, context).map(ctx),
      ...(full ? [] : [fold(9 + 3 - context)]),
    ];
  });
  /** The preview Side by Side: the removed line faces the added one. */
  const previewRows = $derived.by(() => {
    const rows: { fold?: Line; left?: Line; right?: Line }[] = [];
    for (let i = 0; i < preview.length; i++) {
      const line = preview[i];
      if (line.kind === "fold") rows.push({ fold: line });
      else if (line.kind === "ctx") rows.push({ left: line, right: line });
      else if (line.kind === "del" && preview[i + 1]?.kind === "add") rows.push({ left: line, right: preview[++i] });
      else rows.push(line.kind === "del" ? { left: line } : { right: line });
    }
    return rows;
  });
</script>

<div class="preview" class:words={prefs.get("oxbow.diff.wordHighlight")}>
  {#if prefs.get("oxbow.diff.layout") === "split"}
    {#each previewRows as row, i (i)}
      {#if row.fold}
        <div class="line fold first"><span class="fold">{row.fold.before}</span></div>
      {:else}
        <div class="pair">
          {#each [row.left, row.right] as line, s (s)}
            {#if line}
              <div class="line half {line.kind}" class:first={line.kind !== "ctx"} class:last={line.kind !== "ctx"}>
                <span class="n">{(s === 0 ? line.old : line.new) ?? ""}</span>
                <span class="src">{line.before}{#if line.word}<span class="word">{line.word}</span>{line.after}{/if}</span>
              </div>
            {:else}
              <div class="line half"></div>
            {/if}
          {/each}
        </div>
      {/if}
    {/each}
  {:else}
  {#each preview as line, i (i)}
    <div class="line {line.kind}" class:first={line.kind !== preview[i - 1]?.kind} class:last={line.kind !== preview[i + 1]?.kind}>
      {#if line.kind === "fold"}
        <span class="fold">{line.before}</span>
      {:else}
        <span class="n">{line.old ?? ""}</span>
        <span class="n">{line.new ?? ""}</span>
        <span class="src">{line.before}{#if line.word}<span class="word">{line.word}</span>{line.after}{/if}</span>
      {/if}
    </div>
  {/each}
  {/if}
</div>

<style>
  .preview {
    border-radius: var(--radius-l);
    background: var(--code-bg, var(--win));
    border: 0.5px solid var(--panel-border);
    padding: 6px 0 8px;
    font-family: var(--code-font);
    font-size: var(--code-size);
    tab-size: var(--tab);
    color: var(--code-fg, var(--code));
    overflow: hidden;
  }

  .line {
    display: flex;
    align-items: center;
    min-height: var(--code-line);
    margin: 0 8px;
  }

  .line.first {
    margin-top: 2px;
  }

  .line.add {
    background: var(--add);
  }

  .line.del {
    background: var(--del);
  }

  .line.add.first,
  .line.del.first {
    border-top-left-radius: 9px;
    border-top-right-radius: 9px;
  }

  .line.add.last,
  .line.del.last {
    border-bottom-left-radius: 9px;
    border-bottom-right-radius: 9px;
  }

  .line.del + .line.add {
    margin-top: 3px;
  }

  /* Side by Side: the old side and the new side as two columns. */
  .pair {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    column-gap: 4px;
    margin: 0 8px;
  }

  .pair .line {
    margin: 0;
    min-width: 0;
    align-items: flex-start;
  }

  .pair .line.first {
    margin-top: 2px;
  }

  .half .n {
    padding-right: 12px;
    width: 42px;
  }

  .half .src {
    min-width: 0;
    white-space: pre-wrap;
    word-break: break-all;
  }

  .line.fold {
    margin-top: 4px;
    border-radius: var(--radius-m);
    background: var(--fold-bg, var(--field));
    color: var(--code-dim, var(--text2));
  }

  .line.fold:first-child {
    margin-top: 2px;
  }

  .fold {
    padding-left: 72px;
  }

  .n {
    width: 30px;
    text-align: right;
    color: var(--code-ln, color-mix(in srgb, var(--code-dim, var(--text2)) 70%, transparent));
    flex-shrink: 0;
  }

  .n + .n {
    padding-right: 12px;
    width: 42px;
  }

  .src {
    white-space: pre;
  }

  .words .add .word {
    background: var(--add-word);
  }

  .words .del .word {
    background: var(--del-word);
  }

  .words .word {
    border-radius: var(--radius-s);
    padding: 0 2px;
  }
</style>
