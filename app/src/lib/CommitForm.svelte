<script lang="ts">
  // The commit form under the staged files: the summary and description, Amend, and Commit with
  // why it is grey and the way out when there is one.

  import { api } from "./api";
  import { commitRecovery } from "./changes";
  import type { Draft } from "./commitDrafts.svelte";
  import { confirm, type Part } from "./confirm.svelte";
  import { lane, plate, tint } from "./format";
  import { shortcut } from "./keys";
  import { prefs } from "./prefs.svelte";
  import { pressed } from "./shortcuts";
  import { browsing, splitMessage, stepMessage } from "./messageHistory";
  import type { FileChange, HistoryRow } from "./types";
  import WhyNote from "./WhyNote.svelte";

  let {
    draft,
    branch,
    color,
    head,
    staged,
    unstaged,
    conflicted,
    amendedCount,
    stage,
    onChanged,
    onResolve,
  }: {
    /** The repository's commit message, which outlives the panel. */
    draft: Draft;
    branch: string | null;
    color: number;
    /** The checked-out commit, for Amend. */
    head: HistoryRow | null;
    staged: FileChange[];
    unstaged: FileChange[];
    conflicted: FileChange[];
    /** Files the amended commit will have: its own and the staged ones. */
    amendedCount: number;
    /** Stage All, when nothing is staged yet. */
    stage: (files: FileChange[]) => unknown;
    /** Called after the commit was made. */
    onChanged: () => void;
    /** Opens the Conflicts screen. */
    onResolve?: () => void;
  } = $props();

  const branchPart = $derived<Part>(branch ? { branch, color } : { code: "HEAD" });
  /** Edits to tracked files, and new files git does not track yet, for Stage All. */
  const tracked = $derived(unstaged.filter((f) => f.status !== "untracked"));
  const untracked = $derived(unstaged.filter((f) => f.status === "untracked"));

  /** Settings › Git › Subject line guide; 0 hides the counter. */
  const SUMMARY_LIMIT = $derived(prefs.get("oxbow.commit.subjectGuide"));

  // Amend replaces the commit it was turned on for. Once HEAD moves (another branch, a commit made
  // elsewhere), it turns off, with the old commit's message if it was not edited.
  $effect(() => {
    if (draft.amend && head && head.id !== draft.amendOn) turnOffAmend();
  });

  const message = $derived([draft.summary.trim(), draft.description.trim()].filter(Boolean).join("\n\n"));
  const canCommit = $derived(!!draft.summary.trim() && conflicted.length === 0 && (staged.length > 0 || (draft.amend && !!head)) && (!draft.amend || head?.id === draft.amendOn));
  let summaryBox = $state<HTMLInputElement>();
  /** Why Commit is grey, over it, with the way out when there is one. */
  const commitHint = $derived.by((): { text: string; warn?: boolean; fix?: { label: string; run: () => void } } | null => {
    if (conflicted.length) {
      return {
        text: `${conflicted.length} ${conflicted.length === 1 ? "file has" : "files have"} conflicts`,
        warn: true,
        fix: onResolve ? { label: "Resolve…", run: onResolve } : undefined,
      };
    }
    if (!staged.length && !draft.amend) {
      if (!unstaged.length) return { text: "No changes to commit" };
      // Edits first: new files, listed apart, are staged by their own list's Stage All.
      const pool = tracked.length ? tracked : untracked;
      return { text: "Nothing staged yet", fix: { label: pool.length === 1 ? "Stage 1 File" : `Stage All ${pool.length} Files`, run: () => stage(pool) } };
    }
    if (!draft.summary.trim()) return { text: "A commit needs a summary", fix: { label: "Write It", run: () => summaryBox?.focus() } };
    return null;
  });

  async function commit() {
    if (!canCommit) return;
    const files = staged.length;
    const pushed = draft.amend && head && !head.unpushed;
    const action = { kind: "commit" as const, message, amend: draft.amend, signOff: draft.signOff };
    const summary = draft.summary.trim();
    const where = branch ?? "HEAD";
    const done = await confirm.run({
      title: draft.amend ? `Amend the last commit on ${branch ?? "HEAD"}?` : `Commit ${files} ${files === 1 ? "file" : "files"} to ${branch ?? "HEAD"}?`,
      body: draft.amend ? amendBody(files) : ["Makes the commit ", { quote: draft.summary.trim() }, " on ", branchPart, " with the staged changes."],
      icon: "commit",
      button: draft.amend ? "Amend" : "Commit",
      note: pushed ? "The last commit is already pushed. After amending, pushing will need --force-with-lease." : undefined,
      status: `Committing to ${where}…`,
      past: draft.amend ? `Amended the last commit on ${where}` : `Committed ${files} ${files === 1 ? "file" : "files"} to ${where}`,
      recover: (failure) => commitRecovery(failure, action, summary, where),
      action,
    });
    if (done) {
      draft.summary = "";
      draft.description = "";
      draft.amend = false;
      draft.amendOn = null;
      draft.prefilled = "";
      // The new commit is the newest message.
      messages = null;
      shownAt = -1;
      onChanged();
    }
  }

  function amendBody(files: number): Part[] {
    const last = { quote: head?.summary ?? "" };
    const reworded = draft.summary.trim() !== (head?.summary ?? "");
    if (!files) return ["Rewords the last commit ", last, " on ", branchPart, " to ", { quote: draft.summary.trim() }, ". Its changes stay the same."];
    if (!reworded) return ["Adds the staged changes to the last commit ", last, " on ", branchPart, ". The message stays the same."];
    return ["Replaces the last commit ", last, " on ", branchPart, " with ", { quote: draft.summary.trim() }, ", adding the staged changes."];
  }

  /** Amend starts from the last commit's message; turning it off again drops that message if it was not edited. */
  async function toggleAmend() {
    if (draft.amend) return turnOffAmend();
    if (!head) return;
    const own = draft;
    own.amend = true;
    own.amendOn = head.id;
    if (!own.summary.trim() && !own.description.trim()) {
      const detail = await api.commitDetail(head.id).catch(() => null);
      if (detail && own.amend && own.amendOn === detail.id && !own.summary.trim()) {
        own.summary = detail.summary;
        own.description = detail.body;
        own.prefilled = [own.summary.trim(), own.description.trim()].filter(Boolean).join("\n\n");
      }
    }
  }

  function turnOffAmend() {
    const prefilled = draft.prefilled && message === draft.prefilled;
    draft.amend = false;
    draft.amendOn = null;
    if (prefilled) {
      draft.summary = "";
      draft.description = "";
    }
    draft.prefilled = "";
  }

  /** The description grows with its text while it is being written, up to DESC_LINES, and goes
   *  back to two lines after, so the lists keep their room. Empty, it is one line, as tall as the
   *  summary, until it is clicked or tabbed into; Amend keeps it open. */
  const DESC_LINES = 10;
  let descBox = $state<HTMLTextAreaElement>();
  let descOpen = $state(false);
  const descFolded = $derived(!draft.description && !descOpen && !draft.amend);
  $effect(() => {
    draft.description;
    const open = descOpen;
    const box = descBox;
    const one = descFolded;
    if (!box) return;
    const style = getComputedStyle(box);
    const line = parseFloat(style.lineHeight);
    const frame = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom) + parseFloat(style.borderTopWidth) + parseFloat(style.borderBottomWidth);
    if (one) {
      box.style.height = `${SUMMARY_HEIGHT}px`;
      return;
    }
    const least = 2 * line + frame;
    box.style.height = `${least}px`;
    const wanted = Math.min(box.scrollHeight + parseFloat(style.borderTopWidth) + parseFloat(style.borderBottomWidth), DESC_LINES * line + frame);
    box.style.height = `${open ? Math.max(least, wanted) : least}px`;
  });
  /** The height of the summary box, which a folded description matches. */
  const SUMMARY_HEIGHT = 34;

  /** The room for the buttons under the commit form, and what the fix and Commit side by side take. */
  let actionsWidth = $state(0);
  let buttonsWidth = $state(0);

  function onCommitKey(event: KeyboardEvent) {
    if (pressed("commitNow", event)) {
      event.preventDefault();
      commit();
    }
  }

  /** Your latest commit messages, read on the first ↑, and which one the box shows (-1: none). */
  let messages: string[] | null = null;
  let shownAt = -1;
  function onSummaryKey(event: KeyboardEvent) {
    onCommitKey(event);
    const by = pressed("previousMessage", event) ? 1 : pressed("nextMessage", event) ? -1 : 0;
    // Amend starts from the last commit's message, which these keys would replace.
    if (!by || event.isComposing || draft.amend || !browsing(messages ?? [], shownAt, draft)) return;
    event.preventDefault();
    const own = draft;
    const step = (list: string[]) => {
      const next = stepMessage(list, shownAt, by, own);
      if (next === null || own !== draft) return;
      shownAt = next;
      const shown = next === -1 ? { summary: "", description: "" } : splitMessage(list[next]);
      own.summary = shown.summary;
      own.description = shown.description;
    };
    if (messages) step(messages);
    else
      api.recentMessages().then(
        (list) => {
          messages = list;
          step(list);
        },
        () => {},
      );
  }

</script>

<div class="commit" role="group" aria-label="Commit" style:background={tint(color)} style:border-color={`color-mix(in srgb, ${lane(color)} 40%, transparent)`}>
  <div class="row top">
    <span class="grow to">Commit to <b style:color={plate(color)}>{branch ?? "detached HEAD"}</b></span>
    <span class="switches">
      <button class="amend" role="switch" aria-checked={draft.signOff} onclick={() => (draft.signOff = !draft.signOff)} title="End the message with Signed-off-by and your name and email (git commit --signoff); stays on for this repository">
        Sign-off
        <span class="switch" class:on={draft.signOff}><span class="knob"></span></span>
      </button>
      <button class="amend" role="switch" aria-checked={draft.amend} onclick={toggleAmend} disabled={!head} title="Replace the last commit instead of adding a new one">
        Amend
        <span class="switch" class:on={draft.amend}><span class="knob"></span></span>
      </button>
    </span>
  </div>
  <div class="summary">
    <input type="text" aria-label="Commit summary" placeholder={`Summary (${shortcut("previousMessage") || "↑"} history)`} title="{shortcut("previousMessage") || "↑"} and {shortcut("nextMessage") || "↓"} bring back your past messages" bind:this={summaryBox} bind:value={draft.summary} onkeydown={onSummaryKey} spellcheck="true" />
    {#if SUMMARY_LIMIT}<span class="counter" class:over={draft.summary.length > SUMMARY_LIMIT}>{draft.summary.length}/{SUMMARY_LIMIT}</span>{/if}
  </div>
  <textarea
    aria-label="Commit description"
    placeholder="Description"
    rows="1"
    class:folded={descFolded}
    bind:this={descBox}
    bind:value={draft.description}
    onkeydown={onCommitKey}
    onfocus={() => (descOpen = true)}
    onblur={() => (descOpen = false)}
  ></textarea>
  {#if commitHint}<WhyNote text={commitHint.text} warn={commitHint.warn} />{/if}
  {#snippet goLabel()}{draft.amend ? "Amend" : "Commit"} {#if shortcut("commitNow")}<span class="keys">{shortcut("commitNow")}</span>{/if}{/snippet}
  <div class="row actions" bind:clientWidth={actionsWidth}>
    {#if commitHint?.fix}
      <!-- The fix and Commit side by side, unseen, to tell whether they fit on one line. -->
      <div class="buttons measure" aria-hidden="true" inert bind:offsetWidth={buttonsWidth}>
        <span class="button">{commitHint.fix.label}</span><span class="button primary go">{@render goLabel()}</span>
      </div>
      <div class="buttons" class:column={buttonsWidth > actionsWidth}>
        <button class="button" onclick={commitHint.fix.run}>{commitHint.fix.label}</button>
        <button class="button primary go" onclick={commit} disabled={!canCommit} title={commitHint.text}>{@render goLabel()}</button>
      </div>
    {:else}
      {#if !commitHint}
        <span class="staged-count"
          >{draft.amend
            ? staged.length
              ? `${amendedCount} ${amendedCount === 1 ? "file" : "files"}`
              : "Message only"
            : `${staged.length} ${staged.length === 1 ? "file" : "files"} staged`}</span
        >
      {/if}
      <button class="button primary go" onclick={commit} disabled={!canCommit} title={commitHint?.text}>{@render goLabel()}</button>
    {/if}
  </div>
</div>

<style>
  /* The commit, as a card in the branch color, as «this commit» is in a commit's chain. */
  .commit {
    flex-shrink: 0;
    margin: var(--space-2) var(--space-3) var(--space-3);
    padding: 12px;
    border: 1px solid transparent;
    border-radius: var(--radius-xl);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  /* Where the commit goes keeps its line: the switches move under it when both don't fit. */
  .top {
    flex-wrap: wrap;
    justify-content: flex-end;
    row-gap: 6px;
  }
  .top .to {
    flex: 1 0 auto;
    max-width: 100%;
  }
  .switches {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .to {
    font-size: var(--text-caption);
    color: var(--text2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .to b {
    font-weight: 600;
  }
  .staged-count {
    font-size: var(--text-small);
    color: var(--text2);
    white-space: nowrap;
  }
  .amend {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-small);
    flex-shrink: 0;
  }
  .amend:disabled {
    opacity: 0.5;
  }
  .switch {
    position: relative;
    width: 28px;
    height: 16px;
    border-radius: var(--radius-pill);
    background: var(--switch-off);
    transition: background-color 0.15s;
  }
  .switch.on {
    background: var(--accent);
  }
  .knob {
    position: absolute;
    left: 2px;
    top: 2px;
    width: 12px;
    height: 12px;
    border-radius: var(--radius-pill);
    background: #fff;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.25);
    transition: transform 0.15s;
  }
  .switch.on .knob {
    transform: translateX(12px);
  }
  .summary {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 var(--space-3);
    border-radius: var(--radius-m);
    background: var(--win);
    border: 0.5px solid var(--glass-border);
  }
  .summary:focus-within {
    box-shadow: var(--focus-ring);
  }
  .summary input {
    flex-grow: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    font: inherit;
    font-weight: 600;
    color: var(--text);
    box-shadow: none;
  }
  .counter {
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .counter.over {
    color: var(--orange);
  }
  textarea {
    resize: none;
    border-radius: var(--radius-m);
    padding: var(--space-2) var(--space-3);
    background: var(--win);
    border: 0.5px solid var(--glass-border);
    outline: 0;
    font: inherit;
    line-height: 1.45;
    color: var(--text);
    transition: height 0.15s ease;
  }
  /* Empty and not written in: one line, as tall as the summary. */
  textarea.folded {
    padding-top: 7px;
    padding-bottom: 7px;
    overflow: hidden;
  }
  input::placeholder,
  textarea::placeholder {
    color: var(--text2);
    font-weight: 400;
  }
  .actions {
    position: relative;
  }
  /* A fix, such as Stage All 10 Files, is as big as Commit beside it. */
  .actions .button {
    height: 32px;
    padding: 0 var(--space-4);
    min-width: 96px;
  }
  .go {
    margin-left: auto;
    gap: 8px;
  }
  .buttons {
    display: flex;
    gap: 8px;
    margin-left: auto;
  }
  .buttons .go {
    margin-left: 0;
  }
  /* Too wide for one line, a long fix stands over Commit, both as wide as the wider one, on the right. */
  .buttons.column {
    display: grid;
    grid-template-columns: max-content;
    justify-items: stretch;
  }
  .buttons.measure {
    position: absolute;
    top: 0;
    right: 0;
    width: max-content;
    visibility: hidden;
    pointer-events: none;
  }

  .keys {
    font-size: var(--text-caption);
    font-weight: 500;
    opacity: 0.85;
  }
  .grow {
    flex-grow: 1;
    min-width: 0;
  }
</style>
