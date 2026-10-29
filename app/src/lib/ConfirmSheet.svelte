<script lang="ts">
  import { modal } from "./modal";
  import { confirm, type Part } from "./confirm.svelte";
  import { actionIcons as icons } from "./icons";
  import { withKeys } from "./keys";
  import { plate, tint } from "./format";
  import MergePreview from "./MergePreview.svelte";
  import WhyNote from "./WhyNote.svelte";
  import PrefixField from "./PrefixField.svelte";
  import { splitPrefix, type BranchPrefix } from "./branchPrefixes";
  import { prefs } from "./prefs.svelte";
  import CommandWords from "./CommandWords.svelte";
  import TipCard from "./TipCard.svelte";
  import { tips, type TipId } from "./tips.svelte";
  import { bytes } from "./unusual";
  import { errorLine } from "./reads";
  import type { NotKept } from "./types";

  let { repo, branch, color }: { repo: string; branch: string | null; color: number } = $props();

  let copied = $state(false);
  let goButton = $state<HTMLButtonElement>();
  let fieldsBox = $state<HTMLDivElement>();
  let linesBox = $state<HTMLDivElement>();

  const request = $derived(confirm.request);
  /** The prefix a new branch's name has, for the words under its box. */
  const prefixOf = (name: string, list: BranchPrefix[]) => list.find((p) => p.prefix === splitPrefix(name, list).prefix);
  const failed = $derived(confirm.phase === "failed");
  const running = $derived(confirm.phase === "running");
  /** A commit message: Return makes a new line, so the action runs with ⌘Return. */
  const multiline = $derived(!!request?.fields?.some((f) => f.text?.multiline));
  const recovery = $derived(failed ? confirm.recovery : null);
  // What the sheet says: the request, or after a failure the way out of it.
  const shown = $derived.by(() => {
    if (!request) return null;
    if (running) return { title: confirm.status + partsAt, body: [] as Part[], icon: request.icon, tone: request.danger ? "err" : "" };
    if (recovery) return { title: recovery.title, body: recovery.body, icon: recovery.icon, tone: recovery.tone };
    if (failed) return { title: "Git stopped with an error", body: [] as Part[], icon: request.icon, tone: "err" };
    return { title: request.title, body: request.body, icon: request.icon, tone: request.danger ? "err" : (request.tone ?? "") };
  });

  /** Which of the parts runs now, e.g. " 2 of 5", for an action that goes over several one at a time. */
  const partsAt = $derived.by(() => {
    const n = confirm.parts.length;
    if (n < 2) return "";
    // Parts that run side by side, like the files of a download, count what is done.
    if (confirm.parts.some((p) => p.bytes)) return ` ${confirm.parts.filter((p) => p.state === "done").length} of ${n} done`;
    const at = confirm.parts.findIndex((p) => p.state === "started");
    return at < 0 ? "" : ` ${at + 1} of ${n}`;
  });
  const PART_WORDS = { waiting: "Waiting", started: "Running…", done: "Done", failed: "Failed" };

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
      case "ignoredInTheWay":
        return `Files git ignores are written over for good`;
      case "unchecked":
        return `Files in ${gap.folders.length + gap.more === 1 ? gap.folders[0] : "these folders"} may be lost for good`;
      case "ignored":
        return `Files git ignores in ${gap.folder} are deleted for good`;
      case "worktree":
        if (gap.removed && gap.changed === null) return `Uncommitted files in ${gap.folder}, if any, are lost: git status failed there`;
        return gap.removed ? `${plural(gap.changed ?? 0, "uncommitted file")} in ${gap.folder} ${gap.changed === 1 ? "is" : "are"} lost` : `${gap.branch} is checked out in ${gap.folder}`;
      case "notLogged":
        return "Undo can’t put the folder back";
      case "submoduleFolder":
        return `Undo can’t put ${gap.folder} back`;
      case "pushed":
        return `${gap.remote} already has ${gap.commits.length + gap.more === 1 ? "this commit" : `${plural(gap.commits.length + gap.more, "commit")}`}`;
    }
  }

  /** The line under the headline: why, and what to do instead. */
  function gapWhy(gap: NotKept): string {
    switch (gap.kind) {
      case "bigFiles":
        return `Untracked files over ${Math.round(gap.limit / 1048576)} MB stay out of snapshots, so the Operation Log has no copy.`;
      case "ignoredInTheWay":
        return `The commit has files where these are. They are not in git or in the Operation Log, ${bytes(gap.bytes)} in all. Copy anything you need, like a .env file, first.`;
      case "unchecked":
        return `Oxbow couldn’t look through all of ${gap.folders.length + gap.more === 1 ? "it" : "them"}: too many files, or some it can’t read. Files over ${Math.round(gap.limit / 1048576)} MB and files git ignores there have no copy in the Operation Log. Copy anything you need first.`;
      case "ignored":
        return `They are not in git or in the Operation Log, ${bytes(gap.bytes)} in all. Copy anything you need, like a .env file, first.`;
      case "worktree":
        return gap.removed
          ? "Another folder’s changes are not in this repository’s Operation Log. Stash them first to keep them."
          : "Undo moves the branch, but the files in that folder stay as they are and show up there as changes.";
      case "notLogged":
        return `Worktree steps are not in the Operation Log. The branch and its commits stay, so you can make the folder again.`;
      case "submoduleFolder":
        return "Removing a submodule is not in the Operation Log. Its commits stay, so you can add it back, but files in its folder that aren’t in a commit are gone.";
      case "pushed":
        return `Undo changes only your copy of ${gap.branch}: ${gap.remote.split("/")[0]} keeps ${gap.commits.length + gap.more === 1 ? "it" : "them"}, and a later pull brings ${gap.commits.length + gap.more === 1 ? "it" : "them"} back. To take ${gap.commits.length + gap.more === 1 ? "it" : "them"} back for everyone, revert instead.`;
    }
  }

  /** A fix's steps in words, numbered above the commands. */
  const stepLines = $derived(request?.stepWords && confirm.steps.length && confirm.phase === "ask" ? request.stepWords(confirm.steps) : []);
  /** Where each step's commands start, for its title in the terminal, and the branch checked
   *  out when each command runs, for its prompt: a fix switches branches along the way. */
  const stepPlaces = $derived.by(() => {
    const starts = new Map<number, number>();
    const branches: (string | null)[] = [];
    let at = 0;
    let here = branch;
    confirm.steps.forEach((step, n) => {
      starts.set(at, n);
      for (let i = 0; i < step.commands; i++) branches[at + i] = here;
      at += step.commands;
      if (step.kind === "switch") here = step.branch;
      else if (step.kind === "createBranch" && step.switch) here = step.name || "…";
    });
    return { starts, branches };
  });

  /** The branch each command that ran was run on, by its line: a fix's steps switch branches. */
  const ranOn = $derived.by(() => {
    const out: (string | null)[] = [];
    if (request?.action?.kind !== "fix") return out;
    let n = 0;
    confirm.lines.forEach((line, i) => {
      if (line.kind === "cmd") out[i] = stepPlaces.branches[n++] ?? null;
    });
    return out;
  });

  /** Why the action button is grey, beside it, unless a field already says so under itself. */
  const blocked = $derived(request?.invalid && !request.fields?.some((f) => f.error === request.invalid) ? request.invalid : null);

  /** How to get it back, above the commands; "Undo brings it back" isn't true of what the gaps
   *  above it name. */
  const note = $derived.by(() => {
    const text = request?.note ?? "";
    const lost = confirm.notKept.some((g) => g.kind !== "pushed" && !(g.kind === "worktree" && !g.removed));
    return lost && /^Undo brings/.test(text) ? "Undo brings back everything else." : text;
  });

  /** A one-time tip about what the commands do, the first that hasn't been closed. */
  const tip = $derived.by((): TipId | undefined => {
    if (confirm.phase !== "ask") return undefined;
    const all = confirm.commands.map((c) => c.args);
    const found: TipId[] = [];
    if (all.some((a) => a[0] === "push" && a.some((x) => x.startsWith("--force-with-lease")))) found.push("forceLease");
    if (all.some((a) => a[0] === "commit" && a.includes("--amend"))) found.push("amend");
    if (all.some((a) => (a[0] === "revert" && !a.includes("--abort") && !a.includes("--continue")) || (a[0] === "reset" && a.some((x) => x === "--soft" || x === "--mixed" || x === "--hard") && a.length > 2 && a[a.length - 1] !== "HEAD")))
      found.push("revertReset");
    return found.find((id) => tips.shows(id));
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

{#snippet prompt(display: string, at: string | null = branch)}
  <div class="cmd">
    <!-- A step Oxbow does itself runs outside the repository, from the home folder. -->
    {#if request?.local}<span class="prompt">~ %</span>{:else}<span class="prompt">{`${repo} `}</span><span style:color={at === branch ? `var(--plate-${color})` : "var(--term-sub)"}>({at ?? "HEAD"})</span><span class="prompt">{" %"}</span>{/if}
    <CommandWords text={display} />
  </div>
{/snippet}

{#if request && shown}
  <!-- Run without asking: out of sight unless it takes a while, so a quick stage never flashes. -->
  <div class="dim" class:quiet={confirm.quiet && running}>
    <div class="sheet" use:modal role="alertdialog" aria-modal="true" aria-labelledby="confirm-title" aria-busy={running}>
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
                {:else if field.text?.prefixes}
                  {@const text = field.text}
                  <PrefixField value={text.value} list={field.text.prefixes} placeholder={text.placeholder} label={field.label} focus={text.focus} onedit={(value) => confirm.edit(text.edit(value))} />
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
                {#if field.error}<span class="field-note error">{field.error}</span>{:else if field.note}<span class="field-note">{field.note}</span
                  >{:else if field.text?.prefixes && prefixOf(field.text.value, field.text.prefixes)}{@const p = prefixOf(field.text.value, field.text.prefixes)}<span
                    class="field-note"><span class="mono">{p?.prefix}</span> · {p?.words}</span
                  >{/if}
              </div>
            </div>
          {/each}
        </div>
      {/if}

      {#if request.preview && confirm.phase === "ask"}
        <MergePreview preview={request.preview} />
      {/if}

      {#if request.warning && confirm.phase === "ask"}
        <div class="gaps" role="note">
          <div class="gap">
            <div class="gap-head">
              <svg class="icon" viewBox="0 0 16 16"><path d="M8 2.5l6 11H2zM8 6.5v3.5M8 12v.01" /></svg>
              <span class="gap-title">{request.warning.title}</span>
            </div>
            <p class="gap-why">
              {#each request.warning.body as part, i (i)}
                {#if typeof part === "string"}{part}{:else if "branch" in part}<span class="chip" style:background={tint(part.color, "label")} style:color={plate(part.color)}>{part.branch}</span
                  >{:else if "code" in part}<span class="chip mono">{part.code}</span>{:else}<span class="quote">“{part.quote}”</span>{/if}
              {/each}
            </p>
          </div>
        </div>
      {/if}

      {#if request.list?.length && confirm.phase === "ask"}
        <div class="parts" aria-label={request.list.length === 1 ? "The one" : "Each one"}>
          {#each request.list as item, i (i)}
            {#if item.group && item.group !== request.list[i - 1]?.group}
              <div class="part-group">{item.group}</div>
            {/if}
            <div class="part">
              <span class="part-words"><span class="part-name" class:mono={!item.words} title={item.name}>{item.name}</span></span>
              <span class="part-state">{item.meta}</span>
            </div>
          {/each}
        </div>
      {/if}

      {#if confirm.parts.length > 1 && confirm.phase !== "ask"}
        <div class="parts" aria-label="Each one">
          {#each confirm.parts as part (part.name)}
            <div class="part {part.state}">
              <span class="part-mark">
                {#if part.state === "done"}
                  <svg class="icon" viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg>
                {:else if part.state === "failed"}
                  <svg class="icon" viewBox="0 0 16 16"><path d="M8 2.5l6 11H2zM8 6.5v3.5M8 12v.01" /></svg>
                {:else}
                  <span class="dot"></span>
                {/if}
              </span>
              <span class="part-words">
                <span class="mono part-name">{part.name}</span>
                {#if part.state === "failed"}
                  <span class="part-why" title={part.error ?? ""}>{#if part.inner}In <span class="mono">{part.inner.slice(part.name.length + 1)}</span>, its own submodule: {/if}{(request.partWhy ?? errorLine)(part.error ?? "")}</span>
                {/if}
              </span>
              {#if part.state === "started" && part.bytes}
                <span class="part-state part-bytes" title="{bytes(part.bytes.done)} of {bytes(part.bytes.total)}">
                  <span class="part-bar"><span style:width="{part.bytes.total ? Math.min(100, (100 * part.bytes.done) / part.bytes.total) : 0}%"></span></span>
                  {bytes(part.bytes.done)} of {bytes(part.bytes.total)}
                </span>
              {:else}
                <span class="part-state">{PART_WORDS[part.state]}</span>
              {/if}
            </div>
          {/each}
        </div>
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

      {#if stepLines.length}
        <ol class="steps" aria-label="Steps">
          {#each stepLines as line, n (n)}
            <li class="step">
              <span class="num">{n + 1}</span>
              <span class="step-words">
                <span>
                  {#each line.parts as part, i (i)}
                    {#if typeof part === "string"}{part}{:else if "branch" in part}<span class="chip" style:background={tint(part.color, "label")} style:color={plate(part.color)}>{part.branch}</span
                      >{:else if "code" in part}<span class="chip mono">{part.code}</span>{:else}<span class="quote">“{part.quote}”</span>{/if}
                  {/each}
                </span>
                {#if line.sub}<span class="step-sub">{line.sub}</span>{/if}
              </span>
            </li>
          {/each}
        </ol>
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
              {#if gap.kind === "bigFiles" || gap.kind === "ignored" || gap.kind === "ignoredInTheWay"}
                <div class="gap-files">
                  {#each gap.files as file (file.path)}
                    <div class="gap-file"><span class="mono gap-path">{file.path}</span><span class="gap-size">{bytes(file.size)}</span></div>
                  {/each}
                  {#if gap.more}<div class="gap-file more">and {plural(gap.more, "more")}</div>{/if}
                </div>
              {:else if gap.kind === "unchecked" && gap.folders.length + gap.more > 1}
                <div class="gap-files">
                  {#each gap.folders as folder (folder)}
                    <div class="gap-file"><span class="mono gap-path">{folder}</span></div>
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

      {#if note && confirm.phase === "ask"}
        <div class="back">
          {#if /\bUndo\b|\bback\b/.test(note)}
            <svg class="icon" viewBox="0 0 16 16"><path d={icons.undo} /></svg>
          {:else}
            <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="5.75" /><path d="M8 7.2v3.6" /><path d="M8 5.2v.01" /></svg>
          {/if}
          <span>{note}</span>
        </div>
      {/if}

      {#if tip}<TipCard id={tip} />{/if}

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
              {#if stepPlaces.starts.has(i) && stepLines.length}
                {@const n = stepPlaces.starts.get(i)!}
                <div class="comment step-head">{`# ${n + 1} · ${stepLines[n]?.head ?? ""}`}</div>
              {/if}
              {#if command.before}{@render prompt(command.before)}{/if}
              {#if command.input}
                <div class="comment"># Oxbow writes a patch with just these lines and feeds it on stdin:</div>
                {#each patchLines(command.input) as line, k (k)}<div class="patch {line.kind}">{line.text}</div>{/each}
              {/if}
              {@render prompt(command.display, stepPlaces.branches[i] ?? branch)}
              {#if command.comment}<div class="comment"># {command.comment}</div>{/if}
              {#if command.todo}
                <div class="comment"># Oxbow hands git this todo list instead of opening an editor:</div>
                {#each todoLines(command.todo) as line, k (k)}<div class="todo {line.kind}"><span class="todo-word">{line.word}</span>{line.rest}</div>{/each}
              {/if}
            {/each}
          {:else}
            {#each confirm.lines as line, i (i)}
              {#if line.kind === "cmd"}{@render prompt(line.text, ranOn[i] ?? branch)}{:else}<div class="output" style:color={OUTPUT[line.kind]}>{line.text}</div>{/if}
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
          <button class="button" onclick={() => confirm.stop()} title={withKeys("Stop", "Escape")}>Stop</button>
        </div>
      {:else}
        <div class="foot">
          {#if !failed && request.back}
            {@const back = request.back}
            <button
              class="alt back-button"
              onclick={() => {
                confirm.cancel();
                back.run();
              }}><svg class="icon" viewBox="0 0 16 16"><path d="M10 3.5 5.5 8l4.5 4.5" /></svg>{back.label}</button
            >
          {/if}
          {#if recovery?.alt}
            <button class="alt" class:danger={recovery.alt.danger} onclick={() => confirm.alternative()}>{recovery.alt.label}</button>
          {:else if !failed && request.alt}
            {@const alt = request.alt}
            <button class="alt" onclick={() => confirm.change(alt.request())}>{alt.label}</button>
          {/if}
          {#if !failed && blocked}
            <WhyNote text={blocked} grow />
          {:else}
            <span class="note">{failed ? (recovery?.note ?? "Nothing else was run.") : ""}</span>
          {/if}
          {#if failed}
            <button class="button" onclick={() => confirm.cancel()} title={withKeys(recovery?.close ?? "Close", "Escape")}>{recovery?.close ?? "Close"}</button>
            {#if recovery?.button}
              <button class="button primary" class:danger={recovery.button.danger} bind:this={goButton} onclick={() => confirm.recover()}>{recovery.button.label}</button>
            {:else if recovery?.act}
              <button class="button primary" bind:this={goButton} onclick={() => confirm.act()}>{recovery.act.label}</button>
            {/if}
          {:else}
            <button class="button" onclick={() => confirm.cancel()} title={withKeys("Cancel", "Escape")}>Cancel</button>
            <button class="button primary" class:danger={request.danger} bind:this={goButton} disabled={!!request.invalid} title={request.invalid ?? withKeys(request.button, multiline ? "Mod+Enter" : "Enter")} onclick={() => confirm.go()}>{request.button}</button>
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
  .dim.quiet {
    opacity: 0;
    animation: appear 0.15s 0.6s forwards;
  }
  @keyframes appear {
    to {
      opacity: 1;
    }
  }
  .sheet {
    width: min(620px, calc(100vw - 48px));
    max-height: calc(100vh - 48px);
    overflow-y: auto;
    padding: 22px 22px 18px;
    border-radius: var(--radius-sheet);
    background: var(--sheet);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--shadow-sheet);
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
    border-radius: var(--radius-l);
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
    font-size: var(--text-heading);
    font-weight: 700;
    line-height: 1.3;
  }
  .body {
    line-height: 1.75;
    overflow-wrap: anywhere;
  }
  .chip {
    padding: 1px 6px;
    border-radius: var(--radius-s);
    background: var(--field);
    font-weight: 500;
    white-space: pre-wrap;
  }
  .chip.mono {
    padding: 1px 5px;
    font-weight: 400;
    font-size: var(--text-small);
  }
  .quote {
    font-weight: 600;
  }
  .term {
    border-radius: var(--radius-l);
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
    border-radius: var(--radius-pill);
    background: var(--term-dot);
  }
  .where {
    flex-grow: 1;
    text-align: center;
    font-size: var(--text-caption);
    color: var(--term-dim);
  }
  .copy {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 20px;
    padding: 0 8px;
    border-radius: var(--radius-pill);
    font-size: var(--text-caption);
    color: var(--term-dim);
    background: var(--term-button);
  }
  .copy .icon {
    width: 11px;
    height: 11px;
  }
  .lines {
    padding: 10px 14px 12px;
    font-family: var(--term-font);
    font-size: var(--term-size);
    line-height: var(--term-line);
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
    font-size: var(--text-small);
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
    border-radius: var(--radius-m);
    background: var(--win);
    border: 1px solid var(--sep);
    color: var(--text);
    font: inherit;
    font-size: var(--text-body);
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
    box-shadow: var(--focus-ring);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
    padding: 2px;
    border-radius: var(--radius-m);
    background: var(--field);
    align-self: flex-start;
  }
  .chip-button {
    height: 24px;
    padding: 0 11px;
    border-radius: var(--radius-s);
    font-size: var(--text-small);
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
    font-size: var(--text-small);
    line-height: 1.4;
    color: var(--text2);
  }
  .field-note.error {
    color: var(--red);
  }
  .parts {
    margin-left: 58px;
    border: 1px solid var(--sep);
    border-radius: var(--radius-l);
    max-height: 180px;
    overflow-y: auto;
  }
  .part {
    display: flex;
    align-items: flex-start;
    gap: 9px;
    padding: 6px 12px;
    font-size: var(--text-small);
  }
  .part + .part,
  .part + .part-group {
    border-top: 0.5px solid var(--sep);
  }
  /* "3 commits", then "4 files": a heading over each group of the list. */
  .part-group {
    padding: 6px 12px 3px;
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .part-mark {
    width: 14px;
    height: 16px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text2);
  }
  .part-mark .icon {
    width: 14px;
    height: 14px;
  }
  .part.done .part-mark {
    color: var(--accent-text);
  }
  .part.failed .part-mark,
  .part.failed .part-state {
    color: var(--orange);
  }
  .part-mark .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    border: 1.5px solid currentColor;
  }
  .part.started .dot {
    border-color: var(--accent);
    background: var(--accent);
    animation: pulse 1s ease-in-out infinite alternate;
  }
  @keyframes pulse {
    to {
      opacity: 0.35;
    }
  }
  .part-words {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .part-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .part-why {
    color: var(--text2);
    line-height: 1.4;
  }
  .part-state {
    flex-shrink: 0;
    color: var(--text2);
  }
  .part.waiting .part-name {
    color: var(--text2);
  }
  .part-bytes {
    display: flex;
    align-items: center;
    gap: 8px;
    font-variant-numeric: tabular-nums;
  }
  .part-bar {
    width: 64px;
    height: 4px;
    border-radius: 2px;
    background: var(--sep);
    overflow: hidden;
  }
  .part-bar span {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  .gaps {
    margin-left: 58px;
    padding: 4px 12px;
    border-radius: var(--radius-l);
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
    font-size: var(--text-body);
  }
  .gap-why {
    margin: 3px 0 0 20px;
    font-size: var(--text-small);
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
    font-size: var(--text-small);
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
    border-radius: var(--radius-l);
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
    font-size: var(--text-small);
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
    border-radius: var(--radius-m);
    background: var(--orange-soft);
    color: var(--orange);
    font-size: var(--text-caption);
    font-weight: 500;
  }
  .masked {
    font-size: var(--text-small);
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
    border-radius: var(--radius-xs);
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
    font-size: var(--text-caption);
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
    border-radius: var(--radius-pill);
    background: var(--field);
    overflow: hidden;
  }
  .fill {
    display: block;
    height: 4px;
    border-radius: var(--radius-pill);
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
    border-radius: var(--radius-pill);
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
    border-radius: var(--radius-l);
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
    border-radius: var(--radius-pill);
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
    border-radius: var(--radius-pill);
    background: var(--field);
    font-weight: 600;
  }
  .toast .close {
    flex-shrink: 0;
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: var(--radius-pill);
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
  .back {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 0 2px;
    font-size: var(--text-small);
    line-height: 1.4;
    color: var(--text2);
  }
  .back .icon {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    margin-top: 1px;
  }
  .note {
    flex-grow: 1;
    min-width: 0;
    font-size: var(--text-caption);
    line-height: 1.35;
    color: var(--text2);
  }
  /* A fix's plan: numbered steps in words, above the commands they run. */
  .steps {
    margin: 0 0 0 58px;
    padding: 4px 12px;
    list-style: none;
    border-radius: var(--radius-l);
    background: var(--panel);
    border: 0.5px solid var(--panel-border);
    display: flex;
    flex-direction: column;
  }
  .step {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 6px 0;
    line-height: 1.6;
  }
  .step + .step {
    border-top: 0.5px solid var(--sep);
  }
  .num {
    width: 20px;
    height: 20px;
    margin-top: 1px;
    flex-shrink: 0;
    border-radius: var(--radius-pill);
    display: grid;
    place-items: center;
    font-size: var(--text-caption);
    font-weight: 700;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--plate-0);
  }
  .step-words {
    min-width: 0;
    display: flex;
    flex-direction: column;
    overflow-wrap: anywhere;
  }
  .step-sub {
    font-size: var(--text-small);
    line-height: 1.4;
    color: var(--text2);
  }
  .step-head {
    font-weight: 600;
    font-style: normal;
  }
  .step-head:not(:first-child) {
    margin-top: 6px;
  }
  .back-button {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .back-button .icon {
    width: 14px;
    height: 14px;
  }

  .button {
    min-width: 96px;
    padding: 0 16px;
  }
</style>
