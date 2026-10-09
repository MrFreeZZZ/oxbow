<script lang="ts">
  // What changed about a file whose diff isn't text worth reading: a binary file, a diff too long
  // to load at once, a mode bit, or line endings alone.

  import { api } from "./api";
  import { confirm } from "./confirm.svelte";
  import { mac } from "./keys";
  import TermBlock, { type TermLine } from "./TermBlock.svelte";
  import type { FileDiff, Source } from "./types";
  import { bytes, bytesDelta, count, maxLines, modeWord } from "./unusual";

  let {
    diff,
    kind,
    command = null,
    openable = true,
    onShowAnyway,
  }: {
    diff: FileDiff;
    kind: "binary" | "limited" | "tooLarge" | "mode" | "eol";
    /** The `git diff` that shows this file's change, for the binary card. */
    command?: string | null;
    openable?: boolean;
    onShowAnyway?: () => void;
  } = $props();

  const file = $derived(diff.file);
  const name = $derived(file.path.slice(file.path.lastIndexOf("/") + 1));
  const short = (s: Source | null) => (s?.kind === "blob" ? s.id.slice(0, 7) : s ? "working copy" : "—");
  let busy = $state(false);

  function open(source: Source, label: string) {
    api.openSource(source, file.path, label).catch((err) => confirm.say(String(err)));
  }

  async function showAnyway() {
    busy = true;
    onShowAnyway?.();
  }

  function openInEditor() {
    api.openInEditor(file.path, null).catch((err) => confirm.say(String(err)));
  }

  const binaryLines = $derived<TermLine[]>([
    ...(command ? [{ kind: "cmd" as const, text: command }] : []),
    { kind: "out", text: `Binary files ${file.oldSize === undefined ? "/dev/null" : `a/${file.oldPath ?? file.path}`} and ${file.newSize === undefined ? "/dev/null" : `b/${file.path}`} differ` },
  ]);

  const modeTitle = $derived.by(() => {
    if (!file.mode) return "";
    switch (modeWord(file.mode)) {
      case "executable":
        return "Now executable";
      case "notExecutable":
        return "No longer executable";
      case "link":
        return "Now a symbolic link";
      default:
        return "File mode changed";
    }
  });
  const modeText = $derived.by(() => {
    if (!file.mode) return "";
    switch (modeWord(file.mode)) {
      case "executable":
        return `Only the file mode changed: ${name} can be run as a program now. The content is the same.`;
      case "notExecutable":
        return `Only the file mode changed: ${name} can no longer be run as a program. The content is the same.`;
      default:
        return "Only the file mode changed. The content is the same.";
    }
  });

  const style = (eol: string) => (eol === "CRLF" ? "CRLF (the Windows style)" : eol === "LF" ? "LF" : "a mix of LF and CRLF");
</script>

<div class="card">
  {#if kind === "binary"}
    <div class="top">
      <span class="badge"><svg class="icon" viewBox="0 0 16 16"><path d="M4 1.5h5.5l3 3v10H4zM9.5 1.5v3h3M6.5 8v4M9.5 8v4M6 8h1M9 8h1" /></svg></span>
      <div>
        <h3>Binary file</h3>
        <p>Git cannot show it as text, so Oxbow shows what changed about it.</p>
      </div>
    </div>
    <dl class="facts">
      <dt>Size</dt>
      <dd class="mono">
        {file.oldSize !== undefined ? bytes(file.oldSize) : "—"}
        <span class="arrow">→</span>
        <b>{file.newSize !== undefined ? bytes(file.newSize) : "—"}</b>
        {#if file.oldSize !== undefined && file.newSize !== undefined && file.oldSize !== file.newSize}
          <span class="delta">{bytesDelta(file.newSize - file.oldSize)}</span>
        {/if}
      </dd>
      <dt>Blob</dt>
      <dd class="mono">{short(diff.old)} <span class="arrow">→</span> <b>{short(diff.new)}</b></dd>
    </dl>
    <TermBlock where={name} lines={binaryLines} copy={command ? [command] : []} />
    <div class="actions">
      {#if diff.old}<button class="button" onclick={() => open(diff.old!, "old")}>Open Old Version</button>{/if}
      {#if diff.new}<button class="button" onclick={() => open(diff.new!, "new")}>Open New Version</button>{/if}
      {#if openable && file.status !== "deleted"}
        <button class="button" onclick={() => api.revealFile(file.path).catch((err) => confirm.say(String(err)))}>{mac ? "Show in Finder" : "Show in Folder"}</button>
      {/if}
    </div>
    <p class="tip">Tip: a <code>textconv</code> rule in .gitattributes lets git diff some binary formats as text.</p>
  {:else if kind === "limited" || kind === "tooLarge"}
    <div class="top">
      <span class="badge"><svg class="icon" viewBox="0 0 16 16"><path d="M4 1.5h5.5l3 3v10H4zM9.5 1.5v3h3M6 8h4M6 10.5h4" /></svg></span>
      <div>
        {#if kind === "limited"}
          <h3>Large diff hidden</h3>
          <p>
            {count(file.additions + file.deletions)} changed lines{file.newSize !== undefined ? ` in a ${bytes(file.newSize)} file` : ""}. Oxbow hides diffs over
            {count(maxLines())} lines so History stays fast.
          </p>
        {:else}
          <h3>File too large to diff</h3>
          <p>{file.newSize !== undefined ? `At ${bytes(file.newSize)}, it` : "It"} is too big to compare line by line here.</p>
        {/if}
      </div>
    </div>
    {#if kind === "limited"}
      <dl class="facts">
        <dt>Lines</dt>
        <dd class="mono"><span class="del">−{count(file.deletions)}</span> <span class="arrow">→</span> <b class="add">+{count(file.additions)}</b></dd>
      </dl>
    {/if}
    <div class="actions">
      {#if kind === "limited" && onShowAnyway}
        <button class="button primary" disabled={busy} onclick={showAnyway}>{busy ? "Loading…" : "Show Diff Anyway"}</button>
      {/if}
      {#if openable && file.status !== "deleted"}<button class="button" onclick={openInEditor}>Open in Editor</button>{/if}
    </div>
    {#if kind === "limited"}
      <p class="tip">Change the limit in <button class="link" onclick={() => api.openSettings()}>Settings › Diff & Text</button>.</p>
    {/if}
  {:else if kind === "mode" && file.mode}
    <div class="top">
      <span class="badge"><svg class="icon" viewBox="0 0 16 16"><rect x="1.5" y="2.5" width="13" height="11" rx="2" /><path d="M4.5 6l2 2-2 2M8 10.5h3" /></svg></span>
      <div>
        <h3>{modeTitle}</h3>
        <p>{modeText}</p>
      </div>
    </div>
    <dl class="facts">
      <dt>Mode</dt>
      <dd class="mono">
        {file.mode.old} <span class="arrow">→</span> <b>{file.mode.new}</b>
        {#if modeWord(file.mode) === "executable"}<span class="delta">+x</span>{:else if modeWord(file.mode) === "notExecutable"}<span class="delta">−x</span>{/if}
      </dd>
    </dl>
    <TermBlock
      where={name}
      lines={[
        { kind: "out", text: `old mode ${file.mode.old}` },
        { kind: "out", text: `new mode ${file.mode.new}` },
      ]}
      copy={[]}
    />
    {#if modeWord(file.mode) === "executable" || modeWord(file.mode) === "notExecutable"}
      <p class="tip">This is what <code>chmod {modeWord(file.mode) === "executable" ? "+x" : "-x"}</code> does. Git tracks only this one permission bit.</p>
    {/if}
  {:else if kind === "eol" && file.eol}
    <div class="top">
      <span class="badge"><svg class="icon" viewBox="0 0 16 16"><path d="M12.5 3.5v5a2 2 0 0 1-2 2h-7M6 8l-2.5 2.5L6 13" /></svg></span>
      <div>
        <h3>No visible changes</h3>
        <p>Every line ends with {style(file.eol.to)} now instead of {style(file.eol.from)}. The code itself is the same.</p>
      </div>
    </div>
    <TermBlock
      where=".gitattributes"
      lines={[
        { kind: "comment", text: ".gitattributes" },
        { kind: "out", text: "* text=auto eol=lf" },
      ]}
      copy={["* text=auto eol=lf"]}
    />
    <p class="tip">With this rule in .gitattributes, git keeps LF in the repository on every OS.</p>
  {/if}
</div>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 8px 6px 4px;
  }
  .top {
    display: flex;
    gap: 14px;
    align-items: flex-start;
  }
  .badge {
    width: 40px;
    height: 40px;
    border-radius: 10px;
    background: var(--field);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    color: var(--text);
  }
  .badge .icon {
    width: 18px;
    height: 18px;
  }
  h3 {
    margin: 2px 0 4px;
    font-size: 13px;
    font-weight: 600;
  }
  p {
    margin: 0;
    color: var(--text2);
    line-height: 1.4;
  }
  .facts {
    display: grid;
    grid-template-columns: 100px 1fr;
    row-gap: 6px;
    margin: 0;
    padding: 9px 12px;
    border-radius: 10px;
    background: var(--field);
    font-size: 12px;
  }
  dt {
    color: var(--text2);
  }
  dd {
    margin: 0;
  }
  .arrow {
    color: var(--text2);
    margin: 0 6px;
  }
  .delta {
    margin-left: 6px;
    color: var(--text2);
  }
  .add {
    color: var(--green);
  }
  .del {
    color: var(--red);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .button {
    height: 28px;
    min-width: 120px;
    flex-shrink: 0;
    white-space: nowrap;
    padding: 0 14px;
    border-radius: 14px;
    border: 1px solid var(--sep);
    font-size: 12px;
  }
  .button.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
    font-weight: 600;
  }
  .button:disabled {
    opacity: 0.6;
  }
  .tip {
    font-size: 11px;
  }
  .link {
    padding: 0;
    font-size: 11px;
    color: var(--accent);
  }
  code {
    font-family: var(--code-font);
    font-size: 11px;
  }
</style>
