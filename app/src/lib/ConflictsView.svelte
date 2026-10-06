<script lang="ts">
  // The Conflicts screen: conflicted files on the left; for the open file, both sides on top
  // (named by branch, never "ours" and "theirs") and the result underneath, where each conflict
  // gets a side, or both, before the file is marked resolved.

  import { untrack } from "svelte";
  import { api } from "./api";
  import type { Request } from "./confirm.svelte";
  import type { Chunk, ConflictFile, ConflictSide, History, Operation, Pick, WorkingTree } from "./types";
  import { lane, NO_BRANCH_COLOR, plate, shortId, splitPath, tint } from "./format";

  let {
    history,
    op,
    colorOf,
    version,
    run,
  }: {
    history: History;
    op: Operation;
    colorOf: (name: string) => number;
    /** Goes up on every reload, so the lists reload too. */
    version: number;
    run: (request: Request) => void;
  } = $props();

  type Choice = "left" | "right" | "both";

  let tree = $state<WorkingTree | null>(null);
  let current = $state<string | null>(null);
  let file = $state<ConflictFile | null>(null);
  let error = $state<string | null>(null);
  /** Choices per file, one per conflict, kept while the user moves between files. */
  let choices = $state<Record<string, (Choice | null)[]>>({});
  /** Every file that was conflicted in this step, so resolved ones still show as done. */
  let seen = $state<string[]>([]);
  let seenFor = "";

  // On top the side being brought in, on the right of it the user's own work.
  const leftSide = $derived<ConflictSide>(op.yours === "ours" ? "theirs" : "ours");
  const rightSide = $derived<ConflictSide>(op.yours);
  const labelOf = (side: ConflictSide) => (side === "ours" ? op.oursLabel : op.theirsLabel);
  const leftLabel = $derived(labelOf(leftSide));
  const rightLabel = $derived(labelOf(rightSide));
  const isRef = (name: string) => history.refs.some((r) => r.name === name);
  const leftColor = $derived(isRef(leftLabel) ? colorOf(leftLabel) : NO_BRANCH_COLOR);
  const rightColor = $derived(op.branch ? colorOf(op.branch) : NO_BRANCH_COLOR);
  const colorFor = (c: Choice) => (c === "left" ? leftColor : rightColor);

  const leftNote = $derived(
    op.kind === "merge"
      ? `Incoming · ${op.incomingCount} new ${op.incomingCount === 1 ? "commit" : "commits"} on ${leftLabel}`
      : op.kind === "rebase"
        ? `Upstream · the new base`
        : op.kind === "cherryPick"
          ? `Picked commit${op.commit ? ` · ${op.commit.summary}` : ""}`
          : op.kind === "revert"
            ? "The revert"
            : op.incoming
              ? `Incoming · all changes of ${op.incoming}, as one commit`
              : "Squashed changes",
  );
  const rightNote = $derived(
    op.kind === "rebase"
      ? `Your commit${op.commit ? ` · ${shortId(op.commit.id)} ${op.commit.summary}` : ""}`
      : `Yours · ${rightLabel} is checked out`,
  );

  const conflicted = $derived(tree?.conflicted.map((f) => f.path) ?? []);
  const resolved = $derived(seen.filter((p) => !conflicted.includes(p)));
  const automatic = $derived((tree?.staged ?? []).map((f) => f.path).filter((p) => !seen.includes(p)));

  async function reload() {
    try {
      const next = await api.workingTree();
      tree = next;
      error = null;
      const key = `${op.kind}:${op.commit?.id ?? ""}`;
      const paths = next.conflicted.map((f) => f.path);
      if (key !== seenFor) {
        seenFor = key;
        seen = paths;
        choices = {};
      } else {
        seen = [...seen, ...paths.filter((p) => !seen.includes(p))];
      }
      if (!current || !paths.includes(current)) current = paths[0] ?? null;
      await open(current);
    } catch (err) {
      error = String(err);
    }
  }

  async function open(path: string | null) {
    current = path;
    if (!path) {
      file = null;
      return;
    }
    try {
      const next = await api.conflictFile(path);
      if (current !== path) return;
      file = next;
      const count = next.chunks.filter((c) => c.kind === "conflict").length;
      if (choices[path]?.length !== count) choices[path] = Array(count).fill(null);
    } catch (err) {
      error = String(err);
    }
  }

  $effect(() => {
    version;
    op;
    untrack(() => reload());
  });

  const picked = $derived(current ? (choices[current] ?? []) : []);
  const open_ = $derived(picked.filter((c) => c === null).length);
  const lineByLine = $derived(!!file && file.chunks.length > 0);

  function choose(k: number, choice: Choice | null) {
    if (!current) return;
    const list = [...(choices[current] ?? [])];
    list[k] = choice;
    choices[current] = list;
  }

  /** What each side looks like around the conflicts: one line of context, the rest folded. */
  type SideItem = { kind: "line"; no: number; text: string; k: number | null } | { kind: "fold"; count: number } | { kind: "head"; k: number } | { kind: "empty"; k: number };

  function sideItems(chunks: Chunk[], side: ConflictSide): SideItem[] {
    const items: SideItem[] = [];
    let no = 1;
    let k = 0;
    chunks.forEach((chunk, ci) => {
      if (chunk.kind === "same") {
        const n = chunk.lines.length;
        let folded = 0;
        chunk.lines.forEach((text, i) => {
          const keep = (ci > 0 && i === 0) || (ci < chunks.length - 1 && i === n - 1);
          if (!keep) {
            folded++;
            return;
          }
          if (folded) items.push({ kind: "fold", count: folded });
          folded = 0;
          items.push({ kind: "line", no: no + i, text, k: null });
        });
        if (folded) items.push({ kind: "fold", count: folded });
        no += n;
      } else {
        items.push({ kind: "head", k });
        const lines = chunk[side];
        if (!lines.length) items.push({ kind: "empty", k });
        for (const text of lines) items.push({ kind: "line", no: no++, text, k });
        k++;
      }
    });
    return dedent(items);
  }

  /** Narrow panes drop the indentation every shown line shares, like the design. */
  function dedent(items: SideItem[]): SideItem[] {
    const indents = items.flatMap((i) => (i.kind === "line" && i.text.trim() ? [/^[ \t]*/.exec(i.text)![0].length] : []));
    const cut = indents.length ? Math.min(...indents) : 0;
    return cut ? items.map((i) => (i.kind === "line" ? { ...i, text: i.text.slice(cut) } : i)) : items;
  }

  /** The whole result: agreed lines, picked lines tinted by where they came from, open conflicts. */
  type ResultItem =
    | { kind: "line"; no: number; text: string; from: Choice | null; k: number | null; edge: "top" | "bottom" | "both" | null }
    | { kind: "open"; k: number }
    | { kind: "bar"; k: number; choice: Choice };

  const resultItems = $derived.by((): ResultItem[] => {
    if (!file) return [];
    const items: ResultItem[] = [];
    let no = 1;
    let k = 0;
    for (const chunk of file.chunks) {
      if (chunk.kind === "same") {
        for (const text of chunk.lines) items.push({ kind: "line", no: no++, text, from: null, k: null, edge: null });
        continue;
      }
      const choice = picked[k] ?? null;
      if (!choice) items.push({ kind: "open", k });
      else {
        items.push({ kind: "bar", k, choice });
        const left = chunk[leftSide].map((text) => ({ text, from: "left" as Choice }));
        const right = chunk[rightSide].map((text) => ({ text, from: "right" as Choice }));
        const lines = choice === "left" ? left : choice === "right" ? right : [...left, ...right];
        lines.forEach((l, i) => {
          const top = i === 0 || lines[i - 1].from !== l.from;
          const bottom = i === lines.length - 1 || lines[i + 1].from !== l.from;
          items.push({ kind: "line", no: no++, text: l.text, from: l.from, k, edge: top && bottom ? "both" : top ? "top" : bottom ? "bottom" : null });
        });
      }
      k++;
    }
    return items;
  });

  const leftItems = $derived(file ? sideItems(file.chunks, leftSide) : []);
  const rightItems = $derived(file ? sideItems(file.chunks, rightSide) : []);

  function toPick(choice: Choice): Pick {
    if (choice === "left") return leftSide;
    if (choice === "right") return rightSide;
    return leftSide === "theirs" ? "theirsThenOurs" : "oursThenTheirs";
  }

  const conflictsIn = (path: string) => {
    const list = choices[path];
    return list ? list.filter((c) => c === null).length : null;
  };

  function markResolved() {
    if (!current || !file || open_) return;
    const name = splitPath(current).name;
    const list = picked as Choice[];
    const words = list.map((c) => (c === "both" ? "both" : c === "left" ? leftLabel : rightLabel));
    const same = words.every((w) => w === words[0]);
    run({
      title: `Mark ${name} as resolved?`,
      body: [
        "Oxbow writes your choices into ",
        { code: current },
        same && words.length > 1 ? `: ${words[0] === "both" ? "both sides" : words[0]} in all ${words.length} conflicts. ` : ". ",
        "Then git stages the file, which marks the conflict resolved.",
      ],
      icon: "stage",
      button: "Mark Resolved",
      status: `Resolving ${name}…`,
      done: `${name} is resolved.`,
      action: { kind: "resolve", path: current, picks: list.map(toPick) },
    });
  }

  /** One side's whole file, for files that can't be merged line by line. */
  function takeFile(side: ConflictSide) {
    if (!current || !file) return;
    const name = splitPath(current).name;
    const label = labelOf(side);
    const exists = side === "ours" ? file.ours : file.theirs;
    run({
      title: exists ? `Use ${label}’s ${name}?` : `Delete ${name}, as on ${label}?`,
      body: exists
        ? ["The whole file comes from ", { branch: label, color: side === leftSide ? leftColor : rightColor }, "; the other side’s changes to it are dropped."]
        : [{ branch: label, color: side === leftSide ? leftColor : rightColor }, " deleted ", { code: current }, ". Taking that side deletes it here too."],
      icon: exists ? "stage" : "discard",
      danger: !exists,
      button: exists ? `Use ${label}’s Version` : "Delete File",
      status: `Resolving ${name}…`,
      done: `${name} is resolved.`,
      action: { kind: "takeFile", path: current, side },
    });
  }

  /** Why the open file can't be merged line by line. */
  const wholeOnly = $derived.by(() => {
    if (!file || lineByLine) return null;
    if (file.binary) return "This is a binary file, so it can’t be merged line by line. Pick one side’s version.";
    if (!file.ours || !file.theirs) {
      const gone = !file.ours ? labelOf("ours") : labelOf("theirs");
      const kept = !file.ours ? labelOf("theirs") : labelOf("ours");
      return `${gone} deleted this file, while ${kept} changed it. Keep ${kept}’s version, or delete it.`;
    }
    return "Both sides changed this file in a way git can’t show line by line. Pick one side’s version.";
  });

  const radius = (edge: string | null) => (edge === "both" ? "9px" : edge === "top" ? "9px 9px 0 0" : edge === "bottom" ? "0 0 9px 9px" : "0");
</script>

{#snippet sidePane(items: SideItem[], side: ConflictSide, label: string, note: string, color: number, choice: Choice)}
  <section class="pane" aria-label="{label}’s version">
    <header class="pane-head">
      <span class="cap" style:background={tint(color, "label")} style:color={plate(color)}>{label}</span>
      <span class="ellipsis note">{note}</span>
    </header>
    <div class="code side mono selectable">
      {#each items as item, i (i)}
        {#if item.kind === "head"}
          {@const chosen = picked[item.k] === choice || picked[item.k] === "both"}
          <div class="chead" style:background={tint(color, "bar")}>
            <span style:color={plate(color)}>Conflict {item.k + 1}</span>
            <span class="spacer"></span>
            {#if chosen}
              <span class="in" style:color={plate(color)}>✓ In result</span>
            {:else}
              <button class="take" onclick={() => choose(item.k, choice)}>Take {label}</button>
            {/if}
          </div>
        {:else if item.kind === "empty"}
          <div class="line inblock" style:background={tint(color, "soft")}><span class="no"></span><span class="text gone">Nothing here: {label} removed these lines</span></div>
        {:else if item.kind === "fold"}
          <div class="fold">⋯ {item.count} unchanged {item.count === 1 ? "line" : "lines"}</div>
        {:else}
          <div class="line" class:inblock={item.k !== null} style:background={item.k !== null ? tint(color, "soft") : undefined}>
            <span class="no">{item.no}</span><span class="text">{item.text}</span>
          </div>
        {/if}
      {/each}
    </div>
  </section>
{/snippet}

<div class="conflicts">
  <aside class="files" aria-label="Conflicted files">
    <div class="ghead">
      <span>Conflicts</span>
      <span class="count">{resolved.length} of {seen.length} {seen.length === 1 ? "file" : "files"} resolved</span>
    </div>
    <div class="track"><span class="fill" style:width="{seen.length ? (resolved.length / seen.length) * 100 : 0}%"></span></div>
    <div class="list">
      {#each conflicted as path (path)}
        {@const p = splitPath(path)}
        {@const left = conflictsIn(path)}
        <button class="file" class:on={current === path} onclick={() => open(path)}>
          <span class="mark warn">!</span>
          <span class="fwords">
            <span class="ellipsis"><span class="dir">{p.dir}</span>{p.name}</span>
            <span class="sub" class:orange={left !== 0} class:green={left === 0}>{left === null ? "Conflicted" : left === 0 ? "Ready to mark resolved" : `${left} ${left === 1 ? "conflict" : "conflicts"} left`}</span>
          </span>
        </button>
      {/each}
      {#each resolved as path (path)}
        {@const p = splitPath(path)}
        <div class="file done">
          <span class="mark ok">✓</span>
          <span class="fwords">
            <span class="ellipsis"><span class="dir">{p.dir}</span>{p.name}</span>
            <span class="sub green">Resolved · staged</span>
          </span>
        </div>
      {/each}
      {#if automatic.length}
        <div class="ghead sub-head"><span>Merged automatically</span></div>
        {#each automatic as path (path)}
          {@const p = splitPath(path)}
          <div class="auto"><span class="check">✓</span><span class="ellipsis"><span class="dir">{p.dir}</span>{p.name}</span></div>
        {/each}
      {/if}
    </div>
    <p class="hint">Nothing here is final until you {op.kind === "merge" || op.kind === "squash" ? "commit" : "continue"}. Abort puts everything back as it was before the {op.kind === "cherryPick" ? "cherry-pick" : op.kind}.</p>
  </aside>

  <div class="work">
    {#if error}
      <p class="empty error">{error}</p>
    {:else if !current}
      <div class="empty">
        <span class="big">✓</span>
        <span class="title">Every conflict is resolved</span>
        <span>{op.kind === "merge" || op.kind === "squash" ? "Commit the merge" : "Continue"} with the button in the banner above.</span>
      </div>
    {:else if file && wholeOnly}
      <div class="empty">
        <span class="title">{splitPath(file.path).name}</span>
        <span class="explain">{wholeOnly}</span>
        <div class="buttons">
          <button class="btn" onclick={() => takeFile(leftSide)}>{(leftSide === "ours" ? file.ours : file.theirs) ? `Use ${leftLabel}’s Version` : `Delete, as on ${leftLabel}`}</button>
          <button class="btn" onclick={() => takeFile(rightSide)}>{(rightSide === "ours" ? file.ours : file.theirs) ? `Use ${rightLabel}’s Version` : `Delete, as on ${rightLabel}`}</button>
        </div>
      </div>
    {:else if file}
      <div class="sides">
        {@render sidePane(leftItems, leftSide, leftLabel, leftNote, leftColor, "left")}
        {@render sidePane(rightItems, rightSide, rightLabel, rightNote, rightColor, "right")}
      </div>
      <section class="pane result" aria-label="Result">
        <header class="pane-head">
          <b>Result</b>
          <span class="ellipsis note">{file.path}</span>
          <span class="spacer"></span>
          {#if open_}
            <span class="orange small">{open_} {open_ === 1 ? "conflict" : "conflicts"} left</span>
          {:else}
            <span class="green small">All decided</span>
          {/if}
          <button class="btn small-btn" onclick={() => takeFile(leftSide)} title="Use {leftLabel}’s whole file">All {leftLabel}</button>
          <button class="btn small-btn" onclick={() => takeFile(rightSide)} title="Use {rightLabel}’s whole file">All {rightLabel}</button>
          <button class="btn go small-btn" disabled={open_ > 0} title={open_ ? "Pick a side in every conflict first" : undefined} onclick={markResolved}>Mark Resolved</button>
        </header>
        <div class="code mono selectable">
          {#each resultItems as item, i (i)}
            {#if item.kind === "open"}
              <div class="open">
                <div class="open-head">
                  <span>Conflict {item.k + 1} · both sides changed these lines</span>
                  <span class="spacer"></span>
                  <button class="pick" style:color={plate(leftColor)} onclick={() => choose(item.k, "left")}>{leftLabel}</button>
                  <button class="pick" style:color={plate(rightColor)} onclick={() => choose(item.k, "right")}>{rightLabel}</button>
                  <button class="pick" onclick={() => choose(item.k, "both")}>Both</button>
                </div>
                <div class="placeholder">Pick a side, or keep both</div>
              </div>
            {:else if item.kind === "bar"}
              <div class="bar">
                <span>
                  Conflict {item.k + 1}: {item.choice === "both" ? `both, ${leftLabel} first` : `took ${item.choice === "left" ? leftLabel : rightLabel}`}
                </span>
                <button class="undo" onclick={() => choose(item.k, null)}>Undo</button>
              </div>
            {:else}
              <div
                class="line"
                class:picked={item.from !== null}
                style:background={item.from ? tint(colorFor(item.from), "bar") : undefined}
                style:border-radius={radius(item.edge)}
              >
                <span class="no">{item.no}</span><span class="text">{item.text}</span>
              </div>
            {/if}
          {/each}
        </div>
      </section>
    {/if}
  </div>
</div>

<style>
  .conflicts {
    flex-grow: 1;
    display: flex;
    gap: 12px;
    min-height: 0;
    padding: 0 12px 12px;
  }
  .files {
    width: 260px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .ghead {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    padding: 6px 8px 6px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
  }
  .sub-head {
    margin-top: 12px;
  }
  .count {
    font-weight: 400;
  }
  .track {
    margin: 0 8px 8px;
    height: 4px;
    border-radius: 2px;
    background: var(--field);
    overflow: hidden;
  }
  .fill {
    display: block;
    height: 4px;
    background: var(--green);
    transition: width 0.25s;
  }
  .list {
    flex-grow: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .file {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 7px 8px;
    border-radius: 10px;
    text-align: left;
    width: 100%;
  }
  .file.on {
    background: var(--side-sel);
  }
  .mark {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    margin-top: 1px;
    border-radius: 8px;
    font-size: 10px;
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .mark.warn {
    background: var(--orange-soft);
    color: var(--orange);
  }
  .mark.ok {
    background: var(--green-soft);
    color: var(--green);
  }
  .fwords {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .dir {
    color: var(--text2);
  }
  .sub {
    font-size: 11px;
  }
  .orange {
    color: var(--orange);
  }
  .green {
    color: var(--green);
  }
  .small {
    font-size: 12px;
    white-space: nowrap;
  }
  .auto {
    display: flex;
    gap: 8px;
    padding: 4px 8px 4px 12px;
    font-size: 12px;
    color: var(--text2);
  }
  .check {
    color: var(--green);
  }
  .hint {
    margin: 8px 0 0;
    padding: 10px 12px;
    border-radius: 12px;
    background: var(--field);
    font-size: 11px;
    line-height: 1.45;
    color: var(--text2);
  }
  .work {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
  }
  .sides {
    display: flex;
    gap: 10px;
    height: 42%;
    min-height: 150px;
  }
  .pane {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--sep);
    border-radius: 12px;
    background: var(--win);
    overflow: hidden;
  }
  .result {
    flex: 1;
  }
  .pane-head {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 38px;
    flex-shrink: 0;
    padding: 0 10px;
    border-bottom: 1px solid var(--sep);
    font-size: 12px;
  }
  .cap {
    padding: 1px 8px;
    border-radius: 8px;
    font-weight: 500;
    white-space: nowrap;
  }
  .note {
    color: var(--text2);
    min-width: 0;
  }
  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .spacer {
    flex-grow: 1;
  }
  .code {
    flex-grow: 1;
    overflow: auto;
    padding: 6px 8px 10px;
    font-size: 12px;
    line-height: 20px;
    color: var(--code);
  }
  .line {
    display: grid;
    grid-template-columns: 36px 1fr;
  }
  /* Side panes are narrow: lines scroll sideways instead of wrapping. */


  .no {
    color: var(--text2);
    opacity: 0.7;
    text-align: right;
    padding-right: 10px;
    user-select: none;
    -webkit-user-select: none;
  }
  .text {
    white-space: pre-wrap;
    word-break: break-all;
    padding-right: 8px;
  }
  .gone {
    font-style: italic;
    color: var(--text2);
    font-family: var(--font);
  }
  .chead {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 26px;
    margin-top: 4px;
    padding: 0 4px 0 10px;
    border-radius: 9px 9px 0 0;
    font-family: var(--font);
    font-size: 11px;
    font-weight: 600;
  }
  .take,
  .pick {
    height: 20px;
    padding: 0 9px;
    border-radius: 10px;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    font-family: var(--font);
    font-size: 11px;
    font-weight: 600;
  }
  .in {
    font-weight: 500;
    padding-right: 6px;
  }
  .inblock:last-child,
  .inblock:has(+ :not(.inblock)) {
    border-radius: 0 0 9px 9px;
    margin-bottom: 4px;
  }
  .fold {
    margin: 4px 0;
    padding: 0 12px;
    height: 22px;
    line-height: 22px;
    border-radius: 9px;
    background: var(--field);
    color: var(--text2);
    font-family: var(--font);
    font-size: 11px;
  }
  .open {
    margin: 4px 0;
    border-radius: 9px;
    background: var(--orange-soft);
  }
  .open-head {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 6px 0 10px;
    font-family: var(--font);
    font-size: 11px;
    font-weight: 600;
    color: var(--orange);
  }
  .placeholder {
    padding: 0 10px 6px 46px;
    font-style: italic;
    color: var(--text2);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 4px;
    padding: 0 6px 2px 46px;
    font-family: var(--font);
    font-size: 11px;
    color: var(--text2);
  }
  .undo {
    font-size: 11px;
    color: var(--accent-text);
  }
  .empty {
    flex-grow: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--text2);
    border: 1px solid var(--sep);
    border-radius: 12px;
    background: var(--win);
    text-align: center;
    padding: 24px;
  }
  .empty .title {
    font-size: 15px;
    font-weight: 700;
    color: var(--text);
  }
  .empty .big {
    font-size: 28px;
    color: var(--green);
  }
  .explain {
    max-width: 420px;
    line-height: 1.5;
  }
  .error {
    color: var(--red);
  }
  .buttons {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }
  .btn {
    flex-shrink: 0;
    height: 30px;
    padding: 0 14px;
    border-radius: 15px;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    font-weight: 500;
    white-space: nowrap;
  }
  .small-btn {
    height: 26px;
    padding: 0 11px;
    font-size: 12px;
  }
  .btn.go {
    background: var(--accent);
    border-color: transparent;
    color: #fff;
    font-weight: 600;
  }
  .btn:disabled {
    background: var(--field);
    color: var(--text2);
    box-shadow: none;
  }
</style>
