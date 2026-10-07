<script lang="ts">
  import { confirm, type Icon, type Part } from "./confirm.svelte";
  import { plate, tint } from "./format";
  import MergePreview from "./MergePreview.svelte";
  import { prefs } from "./prefs.svelte";
  import { tokens } from "./term";

  let { repo, branch, color }: { repo: string; branch: string | null; color: number } = $props();

  let copied = $state(false);
  let goButton = $state<HTMLButtonElement>();
  let fieldsBox = $state<HTMLDivElement>();
  let linesBox = $state<HTMLDivElement>();

  const request = $derived(confirm.request);
  const failed = $derived(confirm.phase === "failed");
  const running = $derived(confirm.phase === "running");
  const recovery = $derived(failed ? confirm.recovery : null);
  // What the sheet says: the request, or after a failure the way out of it.
  const shown = $derived.by(() => {
    if (!request) return null;
    if (running) return { title: confirm.status, body: [] as Part[], icon: request.icon, tone: request.danger ? "err" : "" };
    if (recovery) return { title: recovery.title, body: recovery.body, icon: recovery.icon, tone: recovery.tone };
    if (failed) return { title: "Git stopped with an error", body: [] as Part[], icon: request.icon, tone: "err" };
    return { title: request.title, body: request.body, icon: request.icon, tone: request.danger ? "err" : (request.tone ?? "") };
  });

  const icons: Record<Icon, string> = {
    stage: "M8 12.5V4M4.5 7.5 8 4l3.5 3.5M3 1.5h10",
    unstage: "M8 3.5V12M4.5 8.5 8 12l3.5-3.5M3 14.5h10",
    discard: "M2.5 4.5h11M6 4.5V3a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1v1.5M4 4.5l.7 9a1 1 0 0 0 1 .9h4.6a1 1 0 0 0 1-.9l.7-9",
    commit: "M1.5 8h3.5M11 8h3.5M8 5a3 3 0 1 1 0 6 3 3 0 0 1 0-6z",
    fetch: "M13 8a5 5 0 0 1-8.6 3.5M3 8a5 5 0 0 1 8.6-3.5M11.8 1.8v2.9H8.9M4.2 14.2v-2.9h2.9",
    pull: "M8 2v9M4.5 7.5 8 11l3.5-3.5M3 14h10",
    push: "M8 12V3M4.5 6.5 8 3l3.5 3.5M3 14h10",
    warn: "M8 2 1.5 13.5h13zM8 6.5v3.5M8 12v.2",
    key: "M10 2.5a3.5 3.5 0 1 1-2.8 5.6L2.5 12.8V14.5h2v-1.5h1.5v-1.5h1.5l1.4-1.4M11 5.5v.1",
    offline: "M2 2l12 12M4.5 12.5a3 3 0 0 1-.4-6 4 4 0 0 1 1.3-2.4M8 3.5a4 4 0 0 1 3.8 2.5 3.2 3.2 0 0 1 1.4 5.8",
    hook: "M5 2v6.5a3.5 3.5 0 0 0 7 0V7M10 9l2-2 2 2",
    checkout: "M2.5 8h8M7.5 4.5 11 8l-3.5 3.5M13.5 3v10",
    branch: "M3 3.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0-3 0M3 12.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0-3 0M10 5.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0-3 0M4.5 5v6M11.5 7c0 3-7 2-7 4",
    edit: "M10.5 2.5l3 3L6 13H3v-3z",
    drop: "M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.7 9h5.6l.7-9",
    merge: "M4.5 2v12M4.5 4.5c0 3.5 7 2.5 7 6v3.5M2.5 12 4.5 14l2-2",
    rebase: "M4.5 14V7M4.5 7c0-3 7-1 7-5M11.5 2v12M9.5 4 11.5 2l2 2",
    cherry: "M3 12a2 2 0 1 0 4 0a2 2 0 1 0-4 0M9 11a2 2 0 1 0 4 0a2 2 0 1 0-4 0M5 10c.5-4 3-6.5 6-8M11 9c-.5-2.5-.5-5 0-7",
    undo: "M3 6.5h7a3.5 3.5 0 0 1 0 7H6M5.5 4 3 6.5 5.5 9",
    revert: "M12.5 8a4.5 4.5 0 1 1-1.3-3.2M11.5 2v3h-3",
    reset: "M3.5 8a4.5 4.5 0 1 0 1.3-3.2M4.5 2v3h3",
    stash: "M2.5 3.5h11v3h-11zM3.5 6.5v6h9v-6M6.5 9h3",
    pop: "M2.5 8.5v4.5h11V8.5M8 10.5V2.5M5.5 5 8 2.5 10.5 5",
    remote: "M8 1.8a6.2 6.2 0 1 0 0 12.4a6.2 6.2 0 1 0 0-12.4M1.8 8h12.4M8 1.8c-2.2 2.2-2.2 10.2 0 12.4M8 1.8c2.2 2.2 2.2 10.2 0 12.4",
    tag: "M2.5 2.5h5l6 6-5 5-6-6zM5.5 4.7a.8.8 0 1 0 0 1.6a.8.8 0 1 0 0-1.6",
    box: "M2.5 5 8 2l5.5 3v6L8 14l-5.5-3zM2.5 5 8 8l5.5-3M8 8v6",
    ignore: "M3 3l10 10M5.2 5.2C3.8 6 2.8 7 2 8c1.5 2.5 3.7 4 6 4 1 0 2-.3 2.8-.8M7 4.1c.3 0 .7-.1 1-.1 2.3 0 4.5 1.5 6 4-.4.7-.9 1.3-1.4 1.8",
    lines: "M2.5 4h11M2.5 8h7M2.5 12h9",
  };

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
  $effect(() => {
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
    <span class="prompt">{`${repo} `}</span><span style:color="var(--lane-{color})">({branch ?? "HEAD"})</span><span class="prompt">{" %"}</span>
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

      <!-- With "Show the git command" off, the commands stay out of the question; git's output
           still shows once it runs. -->
      {#if confirm.phase !== "ask" || prefs.get("oxbow.confirm.showCommand")}
      <div class="term" aria-label="Git command">
        <div class="bar">
          <span class="light"></span><span class="light"></span><span class="light"></span>
          <span class="where">zsh · {repo}</span>
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
            {/each}
          {:else}
            {#each confirm.lines as line, i (i)}
              {#if line.kind === "cmd"}{@render prompt(line.text)}{:else}<div class="output" style:color={OUTPUT[line.kind]}>{line.text}</div>{/if}
            {/each}
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
          <button class="btn" onclick={() => confirm.stop()}>Stop</button>
        </div>
      {:else}
        <div class="foot">
          {#if recovery?.alt}
            <button class="alt" class:danger={recovery.alt.danger} onclick={() => confirm.alternative()}>{recovery.alt.label}</button>
          {:else if !failed && request.alt}
            {@const alt = request.alt}
            <button class="alt" onclick={() => confirm.change(alt.request())}>{alt.label}</button>
          {/if}
          <span class="note">{failed ? (recovery?.note ?? "Nothing else was run.") : (request.note ?? "")}</span>
          {#if failed}
            <button class="btn" onclick={() => confirm.cancel()}>{recovery?.close ?? "Close"}</button>
            {#if recovery?.button}
              <button class="btn go" class:danger={recovery.button.danger} bind:this={goButton} onclick={() => confirm.recover()}>{recovery.button.label}</button>
            {/if}
          {:else}
            <button class="btn" onclick={() => confirm.cancel()}>Cancel</button>
            <button class="btn go" class:danger={request.danger} bind:this={goButton} disabled={!!request.invalid} title={request.invalid ?? undefined} onclick={() => confirm.go()}>{request.button}</button>
          {/if}
        </div>
      {/if}
    </div>
  </div>
{/if}

{#if confirm.toast}
  <div class="toast" class:with-undo={confirm.toastUndo} role="status">
    <span>{confirm.toast}</span>
    {#if confirm.toastUndo}<button class="undo" onclick={() => confirm.undo()}>Undo</button>{/if}
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
  .toast {
    position: fixed;
    left: 50%;
    bottom: 24px;
    transform: translateX(-50%);
    z-index: 42;
    max-width: min(560px, calc(100vw - 48px));
    padding: 9px 16px;
    border-radius: 18px;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    -webkit-backdrop-filter: blur(20px);
    backdrop-filter: blur(20px);
    font-weight: 500;
  }
  .toast.with-undo {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 5px 5px 5px 16px;
  }
  .toast .undo {
    height: 26px;
    padding: 0 12px;
    border-radius: 13px;
    background: var(--field);
    font-weight: 600;
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
