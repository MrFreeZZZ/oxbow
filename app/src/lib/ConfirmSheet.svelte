<script lang="ts">
  import { confirm, type Part } from "./confirm.svelte";
  import { actionIcons as icons } from "./icons";
  import { withKeys } from "./keys";
  import { plate, tint } from "./format";
  import MergePreview from "./MergePreview.svelte";
  import { prefs } from "./prefs.svelte";
  import { tokens } from "./term";
  import { bytes } from "./unusual";
  import type { NotKept } from "./types";

  let { repo, branch, color }: { repo: string; branch: string | null; color: number } = $props();

  let copied = $state(false);
  let goButton = $state<HTMLButtonElement>();
  let fieldsBox = $state<HTMLDivElement>();
  let linesBox = $state<HTMLDivElement>();

  const request = $derived(confirm.request);
  const failed = $derived(confirm.phase === "failed");
  const running = $derived(confirm.phase === "running");
  /** A commit message: Return makes a new line, so the action runs with ⌘Return. */
  const multiline = $derived(!!request?.fields?.some((f) => f.text?.multiline));
  const recovery = $derived(failed ? confirm.recovery : null);
  // What the sheet says: the request, or after a failure the way out of it.
  const shown = $derived.by(() => {
    if (!request) return null;
    if (running) return { title: confirm.status, body: [] as Part[], icon: request.icon, tone: request.danger ? "err" : "" };
    if (recovery) return { title: recovery.title, body: recovery.body, icon: recovery.icon, tone: recovery.tone };
    if (failed) return { title: "Git stopped with an error", body: [] as Part[], icon: request.icon, tone: "err" };
    return { title: request.title, body: request.body, icon: request.icon, tone: request.danger ? "err" : (request.tone ?? "") };
  });

  /** The hunk part of a patch fed on stdin, without the file header, and at most `max` lines. */
  function patchLines(input: string, max = 16): { kind: string; text: string }[] {
    const all = input.replace(/\n$/, "").split("\n");
    const start = all.findIndex((l) => l.startsWith("@@"));
    const body = (start < 0 ? all : all.slice(start)).map((text) => ({
      kind: text.startsWith("@@") ? "at" : text.startsWith("+") ? "add" : text.startsWith("-") ? "del" : "ctx",
      text,
    }));
    if (body.length <= max) return body;
    return [...body.slice(0, max - 1), { kind: "ctx", text: `… ${body.length - max + 1} more lines` }];
  }

  /** The lines of a rebase todo list, with the command word apart, at most `max` lines. */
  function todoLines(todo: string, max = 24): { kind: string; word: string; rest: string }[] {
    const all = todo.replace(/\n$/, "").split("\n").map((text) => {
      const space = text.indexOf(" ");
      const word = space < 0 ? text : text.slice(0, space);
      return { kind: word, word, rest: space < 0 ? "" : text.slice(space) };
    });
    if (all.length <= max) return all;
    return [...all.slice(0, max - 1), { kind: "more", word: "", rest: `… ${all.length - max + 1} more lines` }];
  }

  const plural = (n: number, word: string) => `${n.toLocaleString()} ${word}${n === 1 ? "" : "s"}`;

  /** The headline of one thing Undo can't bring back. */
  function gapTitle(gap: NotKept): string {
    switch (gap.kind) {
      case "bigFiles":
        return `Undo can’t bring back ${gap.files.length + gap.more === 1 ? "this file" : "these files"}`;
      case "ignored":
        return `Files git ignores in ${gap.folder} are deleted for good`;
      case "worktree":
        return gap.removed ? `${plural(gap.changed, "uncommitted file")} in ${gap.folder} ${gap.changed === 1 ? "is" : "are"} lost` : `${gap.branch} is checked out in ${gap.folder}`;
      case "notLogged":
        return "Undo can’t put the folder back";
      case "pushed":
        return `${gap.remote} already has ${gap.commits.length + gap.more === 1 ? "this commit" : `${plural(gap.commits.length + gap.more, "commit")}`}`;
    }
  }

  /** The line under the headline: why, and what to do instead. */
  function gapWhy(gap: NotKept): string {
    switch (gap.kind) {
      case "bigFiles":
        return `Untracked files over ${Math.round(gap.limit / 1048576)} MB stay out of snapshots, so the Operation Log has no copy.`;
      case "ignored":
        return `They are not in git or in the Operation Log, ${bytes(gap.bytes)} in all. Copy anything you need, like a .env file, first.`;
      case "worktree":
        return gap.removed
          ? "Another folder’s changes are not in this repository’s Operation Log. Stash them first to keep them."
          : "Undo moves the branch, but the files in that folder stay as they are and show up there as changes.";
      case "notLogged":
        return `Worktree steps are not in the Operation Log. The branch and its commits stay, so you can make the folder again.`;
      case "pushed":
        return `Undo changes only your copy of ${gap.branch}: ${gap.remote.split("/")[0]} keeps ${gap.commits.length + gap.more === 1 ? "it" : "them"}, and a later pull brings ${gap.commits.length + gap.more === 1 ? "it" : "them"} back. To take ${gap.commits.length + gap.more === 1 ? "it" : "them"} back for everyone, revert instead.`;
    }
  }

  /** The footer's note; "Undo brings it back" isn't true of what the gaps above name. */
  const note = $derived.by(() => {
    const text = request?.note ?? "";
    const lost = confirm.notKept.some((g) => g.kind !== "pushed" && !(g.kind === "worktree" && !g.removed));
    return lost && /^Undo brings/.test(text) ? "Undo brings back everything else." : text;
  });

  const OUTPUT: Record<string, string> = {
    out: "var(--term-out)",
    err: "var(--term-err)",
    hint: "var(--term-hint)",
    ok: "var(--term-ok)",
  };

  async function copy() {
    const commands = failed ? confirm.recoveryCommands : confirm.commands;
    await navigator.clipboard.writeText(commands.map((c) => c.display).join("\n")).catch(() => {});
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  function onKey(event: KeyboardEvent) {
    if (!request) return;
    if (event.key === "Escape") {
      event.preventDefault();
      if (running) confirm.stop();
      else confirm.cancel();
    } else if (
      event.key === "Enter" &&
      confirm.phase === "ask" &&
      !(event.target instanceof HTMLButtonElement) &&
      // Return makes a new line in a message; ⌘Return runs the action.
      !(event.target instanceof HTMLTextAreaElement && !event.metaKey && !event.ctrlKey)
    ) {
      event.preventDefault();
      confirm.go();
    }
  }

  // The action button takes the focus, so Return runs it and Escape cancels; a sheet that asks
  // for a name puts the cursor in its text box instead. Only when the sheet opens or changes
  // phase, not on every keystroke.
  let focusedFor: unknown = null;

  /** Puts the cursor in a text box that appears with `focus`, e.g. a search box. */
  function focusIf(node: HTMLInputElement, on: boolean | undefined) {
    if (on) requestAnimationFrame(() => node.focus());
  }
  $effect(() => {
    // A closed sheet forgets, so the same sheet opened again gets the focus again.
    if (!request) focusedFor = null;
    const key = request && (confirm.phase === "ask" || recovery?.button) ? `${confirm.phase}:${request.title}` : null;
    if (!key || key === focusedFor) return;
    focusedFor = key;
    requestAnimationFrame(() => {
      // The first text box, when a sheet has several.
      const textBox = fieldsBox?.querySelector<HTMLInputElement | HTMLTextAreaElement>(".text");
      if (confirm.phase === "ask" && textBox) {
        textBox.focus();
        textBox.select();
      } else goButton?.focus();
    });
  });

  // The live output follows the newest line, as a terminal does.
  $effect(() => {
    confirm.lines.length;
    confirm.recoveryCommands.length;
    // The box is a new element after the phase changes.
    const box = linesBox;
    if (box) requestAnimationFrame(() => (box.scrollTop = box.scrollHeight));
  });
</script>

<svelte:window onkeydown={onKey} />

{#snippet prompt(display: string)}
  <div class="cmd">
    <!-- A step Oxbow does itself runs outside the repository, from the home folder. -->
    {#if request?.local}<span class="prompt">~ %</span>{:else}<span class="prompt">{`${repo} `}</span><span style:color="var(--plate-{color})">({branch ?? "HEAD"})</span><span class="prompt">{" %"}</span>{/if}
    {#each tokens(display) as token, k (k)}<span style:color={token.color} class:bold={token.bold}>{token.text}</span>{" "}{/each}
  </div>
{/snippet}

{#if request && shown}
  <div class="dim">
    <div class="sheet" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title" aria-busy={running}>
      <div class="top">
        <span
          class="tile"
          style:background={shown.tone === "err" ? "var(--danger-soft)" : shown.tone === "warn" ? "var(--orange-soft)" : tint(color, "label")}
          style:color={shown.tone === "err" ? "var(--red)" : shown.tone === "warn" ? "var(--orange)" : plate(color)}
        >
          <svg class="icon" viewBox="0 0 16 16"><path d={icons[shown.icon]} /></svg>
        </span>
        <div class="words">
          <span class="title" id="confirm-title">{shown.title}</span>
          {#if shown.body.length}
            <span class="body">
              {#each shown.body as part, i (i)}
                {#if typeof part === "string"}{part}{:else if "branch" in part}<span class="chip" style:background={tint(part.color, "label")} style:color={plate(part.color)}>{part.branch}</span
                  >{:else if "code" in part}<span class="chip mono">{part.code}</span>{:else}<span class="quote">“{part.quote}”</span>{/if}
              {/each}
            </span>
          {/if}
        </div>
      </div>

      {#if recovery?.secrets?.length}
        <div class="secrets" aria-label="Possible secrets">
          {#each recovery.secrets as found, i (i)}
            <div class="secret">
              <div class="secret-head">
                <svg class="lock" viewBox="0 0 16 16"><path d="M4.5 7.5h7v6h-7zM6 7.5V5.5a2 2 0 0 1 4 0v2" /></svg>
                <span class="mono where-line">{found.path}<span class="line-no">:{found.line}</span></span>
                <span class="kind">{found.kind}</span>
              </div>
              <div class="mono masked selectable">{found.masked}</div>
            </div>
          {/each}
        </div>
      {/if}

      {#if request.fields?.length && confirm.phase === "ask"}
        <div class="fields" bind:this={fieldsBox}>
          {#each request.fields as field, i (i)}
            <div class="field">
              <span class="label">{field.label}</span>
              <div class="inputs">
                {#if field.text?.multiline}
                  {@const text = field.text}
                  <textarea
                    class="text multiline"
                    value={text.value}
                    placeholder={text.placeholder}
                    aria-label={field.label}
                    spellcheck="true"
                    rows="4"
                    oninput={(e) => confirm.edit(text.edit(e.currentTarget.value))}
                  ></textarea>
                {:else if field.text}
                  {@const text = field.text}
                  <input
                    class="text"
                    value={text.value}
                    placeholder={text.placeholder}
                    aria-label={field.label}
                    spellcheck="false"
                    autocomplete="off"
                    use:focusIf={text.focus}
                    oninput={(e) => confirm.edit(text.edit(e.currentTarget.value))}
                  />
                {/if}
                {#if field.chips?.length}
                  <span class="chips">
                    {#each field.chips as chip (chip.label)}
                      <button
                        class="chip-button"
                        class:on={chip.on}
                        class:mono={chip.mono}
                        aria-pressed={chip.on}
                        disabled={!!chip.off}
                        title={chip.off}
                        onclick={() => !chip.on && confirm.change(chip.pick())}>{chip.label}</button
                      >
                    {/each}
                  </span>
                {/if}
                {#if field.error}<span class="field-note error">{field.error}</span>{:else if field.note}<span class="field-note">{field.note}</span>{/if}
              </div>
            </div>
          {/each}
        </div>
      {/if}

      {#if request.preview && confirm.phase === "ask"}
        <MergePreview preview={request.preview} />
      {/if}

      {#if request.options?.length && confirm.phase === "ask"}
        <div class="options">
          {#each request.options as option (option.label)}
            <button class="option" role="checkbox" aria-checked={option.on} onclick={() => confirm.change(option.toggle())}>
              <span class="box" class:on={option.on}><svg viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg></span>
              <span class="option-words"><span>{option.label}</span>{#if option.sub}<span class="sub">{option.sub}</span>{/if}</span>
            </button>
          {/each}
        </div>
      {/if}

      {#if confirm.notKept.length && confirm.phase === "ask"}
        <div class="gaps" aria-label="What Undo can’t bring back">
          {#each confirm.notKept as gap, i (i)}
            <div class="gap">
              <div class="gap-head">
                <svg class="icon" viewBox="0 0 16 16"><path d="M8 2.5l6 11H2zM8 6.5v3.5M8 12v.01" /></svg>
                <span class="gap-title">{gapTitle(gap)}</span>
              </div>
              <p class="gap-why">{gapWhy(gap)}</p>
              {#if gap.kind === "bigFiles" || gap.kind === "ignored"}
                <div class="gap-files">
                  {#each gap.files as file (file.path)}
                    <div class="gap-file"><span class="mono gap-path">{file.path}</span><span class="gap-size">{bytes(file.size)}</span></div>
                  {/each}
                  {#if gap.more}<div class="gap-file more">and {plural(gap.more, "more")}</div>{/if}
                </div>
              {:else if gap.kind === "pushed"}
                <div class="gap-files">
                  {#each gap.commits as commit (commit.id)}
                    <div class="gap-file"><span class="mono gap-sha">{commit.id.slice(0, 7)}</span><span class="gap-path">{commit.summary}</span></div>
                  {/each}
                  {#if gap.more}<div class="gap-file more">and {plural(gap.more, "more")}</div>{/if}
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}

      <!-- With "Show the git command" off, the commands stay out of the question; git's output
           still shows once it runs. -->
      {#if (confirm.phase !== "ask" || prefs.get("oxbow.confirm.showCommand")) && !(request.local && !request.local.commands.length)}
      <div class="term" aria-label="Git command">
        <div class="bar">
          <span class="light"></span><span class="light"></span><span class="light"></span>
          <span class="where">{request.local || !repo ? "zsh" : `zsh · ${repo}`}</span>
          <button class="copy" onclick={copy} disabled={!(failed ? confirm.recoveryCommands : confirm.commands).length}>
            <svg class="icon" viewBox="0 0 16 16"><path d="M5.5 5.5h7v8h-7zM3.5 10.5v-8h7" /></svg>
            {copied ? "Copied" : "Copy"}
          </button>
        </div>
        <div class="lines mono selectable" bind:this={linesBox} aria-live="polite">
          {#if confirm.phase === "ask"}
            {#each confirm.commands as command, i (i)}
              {#if command.before}{@render prompt(command.before)}{/if}
              {#if command.input}
                <div class="comment"># Oxbow writes a patch with just these lines and feeds it on stdin:</div>
                {#each patchLines(command.input) as line, k (k)}<div class="patch {line.kind}">{line.text}</div>{/each}
              {/if}
              {@render prompt(command.display)}
              {#if command.comment}<div class="comment"># {command.comment}</div>{/if}
              {#if command.todo}
                <div class="comment"># Oxbow hands git this todo list instead of opening an editor:</div>
                {#each todoLines(command.todo) as line, k (k)}<div class="todo {line.kind}"><span class="todo-word">{line.word}</span>{line.rest}</div>{/each}
              {/if}
            {/each}
          {:else}
            {#each confirm.lines as line, i (i)}
              {#if line.kind === "cmd"}{@render prompt(line.text)}{:else}<div class="output" style:color={OUTPUT[line.kind]}>{line.text}</div>{/if}
            {/each}
            {#if recovery?.checked?.length}
              {#if !recovery.secrets}<div class="comment"># Oxbow checked why:</div>{/if}
              {#each recovery.checked as line, i (i)}
                {#if line.kind === "cmd"}{@render prompt(line.text)}{:else if line.kind === "hint"}<div class="comment">{line.text}</div>{:else}<div class="output" style:color={OUTPUT[line.kind]}>{line.text}</div>{/if}
              {/each}
            {/if}
            {#if recovery?.button && confirm.recoveryCommands.length}
              <div class="comment gap"># {recovery.button.label} runs {confirm.recoveryCommands.length === 1 ? "this" : "these"}:</div>
              {#each confirm.recoveryCommands as command, i (i)}{@render prompt(command.display)}{/each}
            {/if}
          {/if}
        </div>
      </div>
      {/if}

      {#if running}
        <div class="run">
          <span class="track"><span class="fill" class:busy={confirm.progress === null} style:width="{confirm.progress ?? 30}%"></span></span>
          <button class="btn" onclick={() => confirm.stop()} title={withKeys("Stop", "Escape")}>Stop</button>
        </div>
      {:else}
        <div class="foot">
          {#if recovery?.alt}
            <button class="alt" class:danger={recovery.alt.danger} onclick={() => confirm.alternative()}>{recovery.alt.label}</button>
          {:else if !failed && request.alt}
            {@const alt = request.alt}
            <button class="alt" onclick={() => confirm.change(alt.request())}>{alt.label}</button>
          {/if}
          <span class="note">{failed ? (recovery?.note ?? "Nothing else was run.") : note}</span>
          {#if failed}
            <button class="btn" onclick={() => confirm.cancel()} title={withKeys(recovery?.close ?? "Close", "Escape")}>{recovery?.close ?? "Close"}</button>
            {#if recovery?.button}
              <button class="btn go" class:danger={recovery.button.danger} bind:this={goButton} onclick={() => confirm.recover()}>{recovery.button.label}</button>
            {:else if recovery?.act}
              <button class="btn go" bind:this={goButton} onclick={() => confirm.act()}>{recovery.act.label}</button>
            {/if}
          {:else}
            <button class="btn" onclick={() => confirm.cancel()} title={withKeys("Cancel", "Escape")}>Cancel</button>
            <button class="btn go" class:danger={request.danger} bind:this={goButton} disabled={!!request.invalid} title={request.invalid ?? withKeys(request.button, multiline ? "Mod+Enter" : "Enter")} onclick={() => confirm.go()}>{request.button}</button>
          {/if}
        </div>
      {/if}
    </div>
  </div>
{/if}

{#if confirm.toast}
  {@const toast = confirm.toast}
  {#key toast.id}
    <div
      class="toast {toast.tone}"
      role={toast.tone === "error" ? "alert" : "status"}
      onpointerenter={() => confirm.hold()}
      onpointerleave={() => confirm.release()}
    >
      <span class="mark" aria-hidden="true">
        <svg viewBox="0 0 16 16" width="14" height="14">
          {#if toast.tone === "done"}
            <path d="M3.5 8.4l3 3 6-6.6" />
          {:else if toast.tone === "info"}
            <path d="M8 7.2v4.6" /><circle cx="8" cy="4.6" r="0.4" />
          {:else}
            <path d="M8 3.6v5.2" /><circle cx="8" cy="11.8" r="0.4" />
          {/if}
        </svg>
      </span>
      <span class="words">{toast.text}</span>
      {#if confirm.toastUndo}<button class="undo" onclick={() => confirm.undo()}>Undo</button>{/if}
      <button class="close" title="Close" aria-label="Close" onclick={() => confirm.dismiss()}>
        <svg viewBox="0 0 16 16" width="11" height="11"><path d="M4 4l8 8M12 4l-8 8" /></svg>
      </button>
      {#if confirm.toastTime}<span class="time" style:animation-duration="{confirm.toastTime}ms"></span>{/if}
    </div>
  {/key}
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
    max-height: calc(100vh - 48px);
    overflow-y: auto;
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
    border: 0.5px solid var(--term-border);
    box-shadow: var(--term-shadow);
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
    color: var(--term-dim);
  }
  .bold {
    font-weight: 600;
  }
  .patch {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--term-dim);
  }
  .patch.at {
    color: var(--term-hint);
  }
  .patch.add {
    color: var(--term-ok);
  }
  .patch.del {
    color: var(--term-err);
  }
  .comment {
    color: var(--term-comment);
    font-style: italic;
  }
  /* A rebase todo list, the way git's editor shows it: the command word colored by what it does. */
  .todo {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--term-text);
    padding-left: 16px;
  }
  .todo-word {
    color: var(--term-sub);
  }
  .todo.drop .todo-word {
    color: var(--term-err);
  }
  .todo.fixup .todo-word,
  .todo.squash .todo-word {
    color: var(--term-flag);
  }
  .todo.update-ref .todo-word,
  .todo.exec .todo-word {
    color: var(--term-sha);
  }
  .todo.more {
    color: var(--term-dim);
  }
  .output {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    padding-left: 16px;
  }
  .gap {
    padding-top: 6px;
  }
  .fields {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding-left: 58px;
  }
  .field {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }
  .label {
    width: 70px;
    flex-shrink: 0;
    font-size: 12px;
    line-height: 28px;
    color: var(--text2);
  }
  .inputs {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .text {
    height: 28px;
    padding: 0 10px;
    border-radius: 8px;
    background: var(--win);
    border: 1px solid var(--sep);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    outline: none;
    user-select: text;
    -webkit-user-select: text;
  }
  .text.multiline {
    height: auto;
    min-height: 84px;
    padding: 6px 10px;
    line-height: 1.4;
    resize: vertical;
  }
  .text:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
    padding: 2px;
    border-radius: 9px;
    background: var(--field);
    align-self: flex-start;
  }
  .chip-button {
    height: 24px;
    padding: 0 11px;
    border-radius: 7px;
    font-size: 12px;
  }
  .chip-button.on {
    background: var(--glass);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
    font-weight: 600;
  }
  .chip-button.mono {
    font-family: var(--mono);
  }
  .chip-button:disabled {
    opacity: 0.4;
  }
  .field-note {
    font-size: 12px;
    line-height: 1.4;
    color: var(--text2);
  }
  .field-note.error {
    color: var(--red);
  }
  .gaps {
    margin-left: 58px;
    padding: 4px 12px;
    border-radius: 12px;
    background: var(--orange-soft);
    display: flex;
    flex-direction: column;
  }
  .gap {
    padding: 8px 0;
  }
  .gap + .gap {
    border-top: 0.5px solid color-mix(in srgb, var(--orange) 25%, transparent);
  }
  .gap-head {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--orange);
  }
  .gap-head .icon {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
  }
  .gap-title {
    font-weight: 600;
    font-size: 12.5px;
  }
  .gap-why {
    margin: 3px 0 0 20px;
    font-size: 12px;
    line-height: 1.45;
    color: var(--text2);
  }
  .gap-files {
    margin: 6px 0 0 20px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .gap-file {
    display: flex;
    gap: 10px;
    font-size: 12px;
    min-width: 0;
  }
  .gap-file.more {
    color: var(--text2);
  }
  .gap-path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .gap-size,
  .gap-sha {
    flex-shrink: 0;
    color: var(--text2);
  }
  .secrets {
    display: flex;
    flex-direction: column;
    margin-left: 58px;
    border-radius: 12px;
    background: var(--field);
    overflow: hidden;
  }
  .secret {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 8px 12px;
  }
  .secret + .secret {
    border-top: 0.5px solid var(--sep);
  }
  .secret-head {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .lock {
    width: 13px;
    height: 13px;
    flex-shrink: 0;
    fill: none;
    stroke: var(--orange);
    stroke-width: 1.4;
    stroke-linejoin: round;
  }
  .where-line {
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .line-no {
    color: var(--text2);
  }
  .kind {
    margin-left: auto;
    flex-shrink: 0;
    padding: 1px 8px;
    border-radius: 8px;
    background: var(--orange-soft);
    color: var(--orange);
    font-size: 11px;
    font-weight: 500;
  }
  .masked {
    font-size: 12px;
    color: var(--text2);
    padding-left: 19px;
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .options {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-left: 58px;
  }
  .option {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    white-space: normal;
    text-align: left;
  }
  .box {
    width: 14px;
    height: 14px;
    margin-top: 2px;
    flex-shrink: 0;
    border-radius: 4px;
    border: 1px solid var(--text2);
    display: flex;
    align-items: center;
    justify-content: center;
    color: transparent;
  }
  .box.on {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  .box svg {
    width: 10px;
    height: 10px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2.4;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .option-words {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .sub {
    font-size: 11px;
    color: var(--text2);
  }
  .run {
    display: flex;
    align-items: center;
    gap: 14px;
    padding-top: 2px;
  }
  .track {
    flex-grow: 1;
    height: 4px;
    border-radius: 2px;
    background: var(--field);
    overflow: hidden;
  }
  .fill {
    display: block;
    height: 4px;
    border-radius: 2px;
    background: var(--accent);
    transition: width 0.25s;
  }
  /* Before git reports a percent, the bar slides back and forth. */
  .fill.busy {
    animation: slide 1.2s ease-in-out infinite alternate;
  }
  @keyframes slide {
    from {
      transform: translateX(-60%);
    }
    to {
      transform: translateX(300%);
    }
  }
  .alt {
    flex-shrink: 0;
    height: 30px;
    padding: 0 12px;
    margin-left: -12px;
    border-radius: 15px;
    font-weight: 500;
  }
  .alt.danger {
    color: var(--red);
  }
  /* Toasts come in at the top right, under the toolbar, where the eye already is after a click
     on it. A tinted mark says at a glance whether it went well; a thin line runs down the time. */
  .toast {
    position: fixed;
    top: 64px;
    right: 16px;
    z-index: 42;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    width: max-content;
    min-width: 300px;
    max-width: min(440px, calc(100vw - 32px));
    padding: 12px 10px 13px 12px;
    border-radius: 14px;
    overflow: hidden;
    background: var(--sheet);
    border: 0.5px solid var(--glass-border);
    box-shadow:
      0 12px 32px rgba(0, 0, 0, 0.18),
      0 2px 6px rgba(0, 0, 0, 0.08);
    animation: toast-in 0.22s cubic-bezier(0.2, 0.9, 0.3, 1.1);
    --tone: var(--text2);
    --tone-soft: var(--field);
  }
  .toast.done {
    --tone: var(--green);
    --tone-soft: var(--green-soft);
  }
  .toast.warn {
    --tone: var(--orange);
    --tone-soft: var(--orange-soft);
  }
  .toast.error {
    --tone: var(--red);
    --tone-soft: var(--red-soft);
  }
  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateX(24px);
    }
  }
  .toast .mark {
    flex-shrink: 0;
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 12px;
    background: var(--tone-soft);
    color: var(--tone);
  }
  .toast .mark svg {
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .toast .words {
    flex-grow: 1;
    min-width: 0;
    padding-top: 3px;
    line-height: 1.4;
    font-weight: 500;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .toast .undo {
    flex-shrink: 0;
    height: 24px;
    padding: 0 12px;
    border-radius: 12px;
    background: var(--field);
    font-weight: 600;
  }
  .toast .close {
    flex-shrink: 0;
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 12px;
    color: var(--text2);
  }
  .toast .close:hover {
    opacity: 1;
    background: var(--field);
  }
  .toast .close svg {
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
  }
  .toast .time {
    position: absolute;
    left: 0;
    bottom: 0;
    height: 2px;
    width: 100%;
    background: var(--tone);
    opacity: 0.5;
    transform-origin: left;
    animation: toast-time linear forwards;
  }
  .toast:hover .time {
    animation-play-state: paused;
  }
  @keyframes toast-time {
    to {
      transform: scaleX(0);
    }
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
