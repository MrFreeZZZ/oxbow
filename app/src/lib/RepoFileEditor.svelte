<script lang="ts">
  // One shared file of the repository, in Settings › Repository Files. As in settings.json, the
  // text box draws only the caret and selection and a painted copy under it underlines what is
  // wrong; the gray notes beside each line say what it matches. The core checks the text as it
  // is typed, and errors keep it from being saved.

  import { onDestroy } from "svelte";
  import { api } from "./api";
  import { keys } from "./keys";
  import { applyFix, banner, lineStart, messageParts, notesByLine, paintLine, status } from "./repoFiles";
  import { loadRepoFiles, repoFiles, untrack } from "./repoFileState.svelte";
  import type { FileCheck, FileProblem, RepoFileKind, RepoFileText } from "./types";

  let { kind, gitlab }: { kind: RepoFileKind; gitlab: boolean } = $props();

  let file = $state<RepoFileText | null>(null);
  let text = $state("");
  /** What is on disk; nothing for a new file. */
  let saved = $state("");
  let checked = $state<FileCheck>({ problems: [], notes: [] });
  let error = $state<string | null>(null);
  let editor = $state<HTMLTextAreaElement>();
  let scroll = $state({ top: 0, left: 0 });
  /** The problem last gone to, highlighted in the list. */
  let current = $state<FileProblem | null>(null);
  /** Something else changed the file since it was opened here: what is on disk now, nothing
   *  when it was removed. The text typed stays until one of the two is picked. */
  let changed = $state<{ text: string | null } | null>(null);
  let saving = $state(false);
  /** Save was asked again while one was being written. */
  let again = false;
  /** Settings left the file, or the window closed: answers that come after are dropped. */
  let gone = false;

  const readOnly = $derived(file?.readOnly ?? null);
  const exists = $derived(file?.exists ?? false);
  const dirty = $derived(!!file && (text !== saved || !exists));
  const errors = $derived(checked.problems.filter((p) => p.level === "error").length);
  const lines = $derived(text.split("\n").map((line) => line.replace(/\r$/, "")));
  const notes = $derived(notesByLine(checked.notes));
  const uncommitted = $derived(repoFiles.list?.files.find((f) => f.kind === kind)?.uncommitted ?? false);
  const bar = $derived(readOnly ? null : status(checked.problems, dirty, exists, uncommitted));
  /** A new file whose starter brings lines of its own, not only comments. */
  const suggested = $derived(!exists && kind === "mailmap" && lines.some((l) => l.trim() && !l.startsWith("#")));

  open();

  onDestroy(() => {
    gone = true;
    clearTimeout(timer);
  });

  // Back from another app: a file changed there is taken in when nothing is typed here, and
  // said otherwise, before Save would write over it.
  function cameBack() {
    if (file && !readOnly && !saving && !changed) reread();
  }

  async function reread() {
    try {
      const read = await api.repoFileText(kind, gitlab);
      if (gone || saving) return;
      const there = read.exists ? read.text : null;
      if (there === (exists ? saved : null)) return;
      if (dirty) {
        changed = { text: there };
        return;
      }
      take(read);
    } catch {
      // Read again at the next return.
    }
  }

  /** The file as it is on disk, over what was typed. */
  function take(read: RepoFileText) {
    file = read;
    saved = read.exists ? read.text : "";
    text = read.text;
    changed = null;
    error = null;
    const drafts = { ...repoFiles.drafts };
    delete drafts[kind];
    repoFiles.drafts = drafts;
    check(true);
  }

  async function reload() {
    try {
      const read = await api.repoFileText(kind, gitlab);
      if (!gone) take(read);
    } catch (err) {
      if (!gone) error = String(err);
    }
  }

  async function open() {
    try {
      const read = await api.repoFileText(kind, gitlab);
      if (gone) return;
      file = read;
      saved = read.exists ? read.text : "";
      text = repoFiles.drafts[kind] ?? read.text;
      await check(true);
    } catch (err) {
      error = String(err);
    }
  }

  // The check of the text as it was typed last; an answer to an older text is dropped.
  let asked = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function check(fresh: boolean) {
    const n = ++asked;
    try {
      const answer = await api.checkRepoFile(kind, gitlab, text, fresh);
      if (n === asked && !gone) checked = answer;
    } catch (err) {
      if (n === asked && !gone) error = String(err);
    }
  }

  function edited() {
    error = null;
    const drafts = { ...repoFiles.drafts };
    if (text !== saved || !exists) drafts[kind] = text;
    else delete drafts[kind];
    repoFiles.drafts = drafts;
    clearTimeout(timer);
    timer = setTimeout(() => check(false), 250);
  }

  /** Select the part a problem is about. */
  function goTo(problem: FileProblem) {
    current = problem;
    if (!editor || problem.line === 0) return;
    const start = lineStart(text, problem.line);
    editor.focus();
    editor.setSelectionRange(start + problem.from, start + Math.max(problem.from, problem.to));
    const height = parseFloat(getComputedStyle(editor).lineHeight) || 20;
    const y = (problem.line - 1) * height;
    if (y < editor.scrollTop || y > editor.scrollTop + editor.clientHeight - 2 * height) editor.scrollTop = Math.max(0, y - 3 * height);
  }

  async function fix(problem: FileProblem) {
    const how = problem.fix;
    if (!how) return;
    if (how.kind === "replace") {
      text = applyFix(text, how);
      edited();
      return;
    }
    await untrack(how.paths);
    await loadRepoFiles();
    await check(true);
  }

  /** Write the text as it is now. Only that text counts as saved: what is typed while it is
   *  written stays not saved, and a Save asked meanwhile comes after this one. `anyway` writes
   *  over what something else saved since the file was opened. */
  async function save(anyway = false) {
    if (errors || readOnly || !dirty || (changed && !anyway)) return;
    if (saving) {
      again = true;
      return;
    }
    saving = true;
    const sent = text;
    const over = anyway && changed ? changed.text : exists ? saved : null;
    try {
      const answer = await api.saveRepoFile(kind, gitlab, sent, over);
      if (gone) return;
      if (answer.kind === "changed") {
        changed = { text: answer.text };
        return;
      }
      changed = null;
      saved = sent;
      if (file) file = { ...file, exists: true };
      if (text === sent) {
        const drafts = { ...repoFiles.drafts };
        delete drafts[kind];
        repoFiles.drafts = drafts;
      }
      error = null;
      await loadRepoFiles();
      await check(true);
    } catch (err) {
      if (!gone) error = String(err);
    } finally {
      saving = false;
      if (again && !gone) {
        again = false;
        save();
      }
    }
  }

  function revert() {
    if (!file) return;
    text = exists ? saved : file.text;
    edited();
  }
</script>

<svelte:window onfocus={cameBack} />

<div class="file-editor">
  {#if changed}
    <div class="banner changed" role="alert">
      <span class="what"
        >{#if changed.text === null}<code>{file?.path}</code> was removed from disk since you opened it here.{:else}<code>{file?.path}</code> was changed
          on disk since you opened it here.{/if} Your text isn't saved.</span
      >
      <button class="button mini" onclick={reload}>{changed.text === null ? "Discard My Text" : "Reload from Disk"}</button>
      <button class="button mini" disabled={errors > 0} onclick={() => save(true)}>Save Anyway</button>
    </div>
  {:else if readOnly}
    <p class="banner">{readOnly}</p>
  {:else if file && !exists}
    <p class="banner new">
      New file. Create writes <code>{file.path}</code>, and it shows up in Changes.{#if suggested}{" "}The lines below come from History: the same name
        with different e-mails.{/if}
    </p>
  {:else}
    <p class="banner">{banner(kind, gitlab)}</p>
  {/if}

  <div class="code">
    <div class="gutter" aria-hidden="true">
      <div style:transform="translateY({-scroll.top}px)">
        {#each lines as _, i (i)}
          <div class="no">{i + 1}</div>
        {/each}
      </div>
    </div>
    <div class="painted" aria-hidden="true">
      <div style:transform="translate({-scroll.left}px, {-scroll.top}px)">
        {#each lines as line, i (i)}
          {@const here = checked.problems.filter((p) => p.line === i + 1)}
          <div class="ln">
            {#if line === ""}{" "}{:else}{#each paintLine(kind, line, here) as piece, j (j)}<span class:com={piece.comment} class:bad={piece.level === "error"} class:odd={piece.level === "warn"}>{piece.text}</span>{/each}{/if}
          </div>
        {/each}
      </div>
    </div>
    <div class="notes" aria-hidden="true">
      <div style:transform="translateY({-scroll.top}px)">
        {#each lines as _, i (i)}
          {@const note = notes.get(i + 1)}
          <div class="note {note?.level ?? ''}">{note?.text ?? ""}</div>
        {/each}
      </div>
    </div>
    <textarea
      class="editor"
      bind:this={editor}
      bind:value={text}
      readonly={!!readOnly}
      spellcheck="false"
      wrap="off"
      autocapitalize="off"
      autocomplete="off"
      aria-label={file?.path ?? "File"}
      aria-invalid={errors > 0}
      oninput={edited}
      onscroll={(event) => (scroll = { top: event.currentTarget.scrollTop, left: event.currentTarget.scrollLeft })}
      onkeydown={(event) => {
        if ((event.metaKey || event.ctrlKey) && event.key === "s") {
          event.preventDefault();
          save();
        }
      }}
    ></textarea>
  </div>

  {#if checked.problems.length}
    <div class="problems" role="list" aria-label="Problems">
      {#each checked.problems as problem, i (i)}
        <div class="problem {problem.level}" class:current={current === problem} role="listitem">
          <button class="where" onclick={() => goTo(problem)} title={problem.line ? "Go to it" : undefined}>
            <svg class="icon mark" viewBox="0 0 16 16"
              ><path
                d={problem.level === "error" ? "M8 1.8a6.2 6.2 0 1 0 0 12.4A6.2 6.2 0 0 0 8 1.8zM8 4.8v3.8M8 11h.01" : "M8 2.2 14.3 13.3H1.7zM8 6.3v3.3M8 11.4h.01"}
              /></svg
            >
            <span class="line-no">{problem.line ? `Line ${problem.line}` : "File"}</span>
            <span class="msg">{#each messageParts(problem.message) as part, j (j)}{#if part.code}<code>{part.text}</code>{:else}{part.text}{/if}{/each}</span>
          </button>
          {#if problem.fix && !readOnly}
            <button class="button mini" onclick={() => fix(problem)}>{problem.fix.label}</button>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  {#if !readOnly}
    <div class="bar">
      {#if error}
        <span class="failed">{error}</span>
      {:else if bar}
        <span class:failed={bar.error} class:sub={!bar.error}>{bar.text}</span>
      {/if}
      <span class="spacer"></span>
      <button class="button small" disabled={!dirty || (!exists && text === file?.text)} onclick={revert}>Revert</button>
      <button class="button small primary" disabled={!dirty || errors > 0 || !!changed} onclick={() => save()} title={errors ? "Fix the errors underlined in red to save" : undefined}
        >{exists ? "Save" : "Create"} <span class="keys">{keys("Mod+S")}</span></button
      >
    </div>
  {/if}
</div>

<style>
  .file-editor {
    flex-grow: 1;
    min-height: 0;
    padding: 6px 30px 20px 28px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .banner {
    margin: 0;
    padding: 9px 12px;
    border-radius: var(--radius-l);
    background: var(--panel);
    border: 0.5px solid var(--panel-border);
    font-size: var(--text-small);
    line-height: 1.4;
    color: var(--text2);
  }

  .banner.changed {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px 6px 12px;
    background: var(--accent-quiet);
    border-color: transparent;
    color: var(--accent-ink);
  }

  .banner.changed .what {
    flex-grow: 1;
    min-width: 0;
  }

  .banner.new {
    background: var(--accent-quiet);
    border-color: transparent;
    color: var(--accent-ink);
  }

  code {
    font-family: var(--mono);
    font-size: var(--text-caption);
  }

  /* The text box over three painted layers: line numbers, the text with its underlines, and
     the notes, each moved with the box's scroll. */
  .code {
    --gutter: 44px;
    position: relative;
    flex-grow: 1;
    min-height: 120px;
    border-radius: var(--radius-l);
    border: 0.5px solid var(--panel-border);
    background: var(--code-bg, var(--win));
    overflow: hidden;
    font-family: var(--code-font);
    font-size: var(--code-size);
    line-height: var(--code-line);
  }

  .code:focus-within {
    border-color: var(--accent);
    box-shadow: var(--focus-ring);
  }

  .gutter,
  .painted,
  .notes {
    position: absolute;
    top: 0;
    bottom: 0;
    overflow: hidden;
    padding: 10px 0;
    pointer-events: none;
  }

  .gutter {
    left: 0;
    width: var(--gutter);
  }

  .no {
    padding-right: 12px;
    text-align: right;
    color: var(--code-ln, var(--text2));
    opacity: 0.8;
  }

  .painted {
    left: var(--gutter);
    right: 0;
    padding-left: 0;
    color: var(--code-fg, var(--code));
  }

  .ln {
    white-space: pre;
    tab-size: 4;
  }

  .notes {
    right: 0;
    left: 50%;
    text-align: right;
  }

  .note {
    height: var(--code-line);
    padding: 0 12px 0 16px;
    font-family: var(--font);
    font-size: var(--text-caption);
    color: var(--code-ln, var(--text2));
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .note.warn {
    color: var(--orange);
  }

  .note.error {
    color: var(--danger);
  }

  .editor {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    padding: 10px 12px 10px var(--gutter);
    border: 0;
    resize: none;
    background: transparent;
    color: transparent;
    caret-color: var(--code-fg, var(--code));
    font: inherit;
    line-height: inherit;
    letter-spacing: 0;
    tab-size: 4;
    white-space: pre;
    overflow: auto;
    outline: 0;
    box-shadow: none;
    user-select: text;
    -webkit-user-select: text;
  }

  .editor::selection {
    background: color-mix(in srgb, var(--accent) 28%, transparent);
    color: transparent;
  }

  .com {
    color: var(--syn-com, var(--text2));
    font-style: italic;
  }

  .bad,
  .odd {
    text-decoration-line: underline;
    text-decoration-style: wavy;
    text-decoration-thickness: 1px;
    text-underline-offset: 3px;
    text-decoration-skip-ink: none;
  }

  .bad {
    text-decoration-color: var(--danger);
  }

  .odd {
    text-decoration-color: var(--orange);
  }

  .problems {
    flex-shrink: 0;
    max-height: 168px;
    overflow-y: auto;
    border-radius: var(--radius-l);
    background: var(--panel);
    border: 0.5px solid var(--panel-border);
    font-size: var(--text-small);
  }

  .problem {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 32px;
    padding: 3px 6px 3px 0;
  }

  .problem + .problem {
    border-top: 0.5px solid var(--sep);
  }

  .problem.current {
    background: var(--side-sel);
  }

  .where {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 9px;
    align-self: stretch;
    padding-left: 12px;
  }

  .mark {
    width: 14px;
    height: 14px;
  }

  .error .mark {
    color: var(--danger);
  }

  .warn .mark {
    color: var(--orange);
  }

  .line-no {
    width: 50px;
    flex-shrink: 0;
    color: var(--text2);
    font-variant-numeric: tabular-nums;
  }

  .msg {
    min-width: 0;
    padding: 4px 0;
    line-height: 1.35;
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--text-small);
    flex-shrink: 0;
  }

  .failed {
    color: var(--danger);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sub {
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .spacer {
    flex-grow: 1;
  }

  .keys {
    opacity: 0.7;
    font-weight: 400;
    margin-left: 4px;
  }

  .button.small {
    min-width: 72px;
  }
</style>
