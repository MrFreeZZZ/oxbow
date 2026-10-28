<script lang="ts">
  // settings.json as text, like VS Code: the text box draws only the caret and selection, and a
  // colored copy of its text under it shows what is wrong as it is typed.

  import { api } from "./api";
  import { check, lineOf, segments } from "./jsonCheck";
  import { keys } from "./keys";
  import { knownSetting, prefs, problemOf } from "./prefs.svelte";
  import { load } from "./settingsModel.svelte";

  let { text = $bindable(), saved = $bindable() }: { text: string; saved: string } = $props();

  let error = $state<string | null>(null);

  // What is wrong in the text, checked as it is typed: errors keep it from being saved, so a
  // value Oxbow can't use never reaches the other windows.
  const checked = $derived(check(text, knownSetting, problemOf));
  const pieces = $derived(segments(text, checked));
  const errors = $derived(checked.problems.filter((p) => p.level === "error"));
  let editor = $state<HTMLTextAreaElement>();
  let painted = $state<HTMLPreElement>();
  let caret = $state(0);
  /** The problem under the pointer, with where to show it. */
  let hover = $state<{ index: number; x: number; y: number } | null>(null);
  /** The problem the bar describes: the one at the caret, else the first. */
  const described = $derived(checked.problems.find((p) => p.from <= caret && caret <= p.to) ?? errors[0] ?? checked.problems[0] ?? null);

  function followScroll() {
    if (!editor || !painted) return;
    painted.scrollTop = editor.scrollTop;
    painted.scrollLeft = editor.scrollLeft;
  }

  function pointAt(event: MouseEvent) {
    // The painted text lies under the text box, so ask what is under the pointer at every depth.
    const under = document.elementsFromPoint(event.clientX, event.clientY).find((el) => el instanceof HTMLElement && el.dataset.problem);
    const index = under instanceof HTMLElement ? Number(under.dataset.problem) : null;
    hover = index === null ? null : { index, x: event.clientX, y: event.clientY };
  }

  /** Put the caret on a problem. */
  function goTo(from: number, to: number) {
    if (!editor) return;
    editor.focus();
    editor.setSelectionRange(from, to);
    caret = from;
  }

  async function saveJson() {
    if (errors.length) {
      error = `Fix ${errors.length === 1 ? "the error" : `${errors.length} errors`} to save`;
      return;
    }
    try {
      await api.saveSettingsText(text);
      saved = text;
      error = null;
      load();
      prefs.reload();
    } catch (err) {
      error = String(err);
    }
  }
</script>

<div class="json">
  <p class="banner">
    Only values that differ from the defaults are listed. Git’s own settings (identity, pull, default branch, signing) stay in ~/.gitconfig, where Settings writes them with
    <code>git config</code>.
  </p>
  <div class="code">
    <pre class="painted" bind:this={painted} aria-hidden="true">{#each pieces as piece, i (i)}<span
          class="t-{piece.kind}"
          class:bad={piece.problem !== null && checked.problems[piece.problem].level === "error"}
          class:odd={piece.problem !== null && checked.problems[piece.problem].level === "warn"}
          data-problem={piece.problem ?? undefined}>{piece.text}</span
        >{/each}{"\n"}</pre>
    <textarea
    class="editor"
    bind:this={editor}
    bind:value={text}
    spellcheck="false"
    wrap="off"
    autocapitalize="off"
    autocomplete="off"
    aria-label="settings.json"
    aria-invalid={errors.length > 0}
    onscroll={followScroll}
    oninput={() => (error = null)}
    onselect={(event) => (caret = event.currentTarget.selectionStart)}
    onkeyup={(event) => (caret = event.currentTarget.selectionStart)}
    onclick={(event) => (caret = event.currentTarget.selectionStart)}
    onmousemove={pointAt}
    onmouseleave={() => (hover = null)}
    onkeydown={(event) => {
      if ((event.metaKey || event.ctrlKey) && event.key === "s") {
        event.preventDefault();
        saveJson();
      } else if (event.key === "Tab") {
        event.preventDefault();
        const box = event.currentTarget;
        box.setRangeText("  ", box.selectionStart, box.selectionEnd, "end");
        text = box.value;
      }
    }}
    ></textarea>
    {#if hover && checked.problems[hover.index]}
      {@const problem = checked.problems[hover.index]}
      <div class="tip" class:warn={problem.level === "warn"} role="tooltip" style:left="{hover.x + 12}px" style:top="{hover.y + 16}px">{problem.message}</div>
    {/if}
  </div>
  <div class="json-bar">
    {#if error}
      <span class="error">{error}</span>
    {:else if described}
      <button class="problem-link" class:warn={described.level === "warn"} onclick={() => goTo(described.from, described.to)} title="Go to it">
        Line {lineOf(text, described.from)}: {described.message}{#if checked.problems.length > 1}<span class="more">&nbsp;· {checked.problems.length - 1} more</span>{/if}
      </button>
    {:else}
      <span class="sub">{text === saved ? "Saved" : "Not saved yet"}</span>
    {/if}
    <span class="spacer"></span>
    <button class="button small" disabled={text === saved} onclick={() => ((text = saved), (error = null))}>Revert</button>
    <button class="button small primary" disabled={text === saved || errors.length > 0} onclick={saveJson} title={errors.length ? "Fix the errors underlined in red to save" : undefined}>Save <span class="keys">{keys("Mod+S")}</span></button>
  </div>
</div>

<style>
  .json {
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

  .banner code {
    font-family: var(--mono);
    font-size: var(--text-caption);
  }

  /* The text box draws only the caret and selection; the colored copy of its text lies under it,
     in the same font and place, and scrolls with it. */
  .code {
    position: relative;
    flex-grow: 1;
    min-height: 0;
    border-radius: var(--radius-l);
    border: 0.5px solid var(--panel-border);
    background: var(--code-bg, var(--win));
  }

  .code:focus-within {
    border-color: var(--accent);
    box-shadow: var(--focus-ring);
  }

  .painted,
  .editor {
    position: absolute;
    inset: 0;
    margin: 0;
    padding: 10px 12px;
    border: 0;
    font-family: var(--code-font);
    font-size: var(--code-size);
    line-height: var(--code-line);
    letter-spacing: 0;
    tab-size: 2;
    white-space: pre;
    overflow: auto;
  }

  .painted {
    color: var(--code-fg, var(--code));
    overflow: hidden;
    pointer-events: auto;
  }

  .editor {
    resize: none;
    background: transparent;
    color: transparent;
    caret-color: var(--code-fg, var(--code));
    outline: 0;
    user-select: text;
    -webkit-user-select: text;
    box-shadow: none;
  }

  .editor::selection {
    background: color-mix(in srgb, var(--accent) 28%, transparent);
    color: transparent;
  }

  .t-key {
    color: var(--syn-type, var(--code-fg));
  }

  .t-str {
    color: var(--syn-str, var(--code-fg));
  }

  .t-num {
    color: var(--syn-num, var(--code-fg));
  }

  .t-lit {
    color: var(--syn-kw, var(--code-fg));
  }

  .t-com {
    color: var(--syn-com, var(--code-fg));
  }

  .t-punct {
    opacity: 0.75;
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

  .tip {
    position: fixed;
    z-index: 20;
    max-width: 360px;
    padding: 6px 10px;
    border-radius: var(--radius-m);
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    -webkit-backdrop-filter: blur(20px);
    backdrop-filter: blur(20px);
    font-size: var(--text-small);
    line-height: 1.35;
    color: var(--danger);
    pointer-events: none;
  }

  .tip.warn {
    color: var(--orange);
  }

  .problem-link {
    min-width: 0;
    padding: 0;
    background: none;
    font-size: var(--text-small);
    color: var(--danger);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: left;
  }

  .problem-link.warn {
    color: var(--orange);
  }

  .problem-link .more {
    color: var(--text2);
  }

  .json-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--text-small);
  }

  .json-bar .error {
    color: var(--danger);
  }

  .keys {
    opacity: 0.7;
    font-weight: 400;
    margin-left: 4px;
  }

  .spacer {
    flex-grow: 1;
    align-self: stretch;
  }

  .sub {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: var(--text-caption);
    color: var(--text2);
  }

  .button.small {
    min-width: 72px;
  }
</style>
