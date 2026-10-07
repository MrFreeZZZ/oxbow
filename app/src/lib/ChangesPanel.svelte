<script lang="ts" module>
  // The commit message outlives the panel, so picking another commit and coming back keeps the draft.
  const draft = $state({ summary: "", description: "", amend: false, prefilled: "" });
</script>

<script lang="ts">
  import { api } from "./api";
  import { diffSettings, prefs, wholeByDefault } from "./prefs.svelte";
  import { confirm, type Part, type Request } from "./confirm.svelte";
  import type { FileChange, FileDiff, Hunk, HistoryRow, Side, WorkingTree } from "./types";
  import { plate, splitPath, tint } from "./format";
  import DiffView, { type LinePicker } from "./DiffView.svelte";
  import Menu, { menuIcons, type MenuEntry } from "./Menu.svelte";
  import { nav } from "./nav.svelte";
  import { commitRecovery, ignoreChoices, ignoreRequest, linesRequest, stashFilesRequest, stashMessageFor, type LinesKind } from "./changes";

  let {
    branch,
    color,
    head,
    version,
    onChanged,
    onResolve,
  }: {
    branch: string | null;
    color: number;
    /** The checked-out commit, for Amend. */
    head: HistoryRow | null;
    /** Goes up whenever the history is reloaded; the panel reloads with it. */
    version: number;
    /** Called after an action changed the repository. */
    onChanged: () => void;
    /** Opens the Conflicts screen. */
    onResolve?: () => void;
  } = $props();

  /** Settings › Git › Subject line guide; 0 hides the counter. */
  const SUMMARY_LIMIT = $derived(prefs.get("oxbow.commit.subjectGuide"));

  let tree = $state<WorkingTree | null>(null);
  let pick = $state<{ path: string; side: Side } | null>(null);
  let diff = $state<FileDiff | null>(null);
  let whole = $state(wholeByDefault());
  // Diff & Text's default view applies again when it changes.
  $effect(() => {
    whole = wholeByDefault();
  });
  let error = $state<string | null>(null);
  let loads = $state(0);
  /** Lines picked in one hunk of the diff shown, with the one clicked last for Shift-click. */
  let sel = $state<{ header: string; lines: number[]; anchor: number } | null>(null);
  let menu = $state<{ x: number; y: number; entries: MenuEntry[]; label: string } | null>(null);

  const staged = $derived(tree?.staged ?? []);
  const unstaged = $derived(tree?.unstaged ?? []);
  const conflicted = $derived(tree?.conflicted ?? []);
  const stagedPaths = $derived(new Set(staged.map((f) => f.path)));
  const unstagedPaths = $derived(new Set(unstaged.map((f) => f.path)));
  const fileCount = $derived(new Set([...stagedPaths, ...unstagedPaths, ...conflicted.map((f) => f.path)]).size);
  const branchPart = $derived<Part>(branch ? { branch, color } : { code: "HEAD" });

  const message = $derived([draft.summary.trim(), draft.description.trim()].filter(Boolean).join("\n\n"));
  const canCommit = $derived(!!draft.summary.trim() && conflicted.length === 0 && (staged.length > 0 || (draft.amend && !!head)));
  const commitHint = $derived(
    conflicted.length
      ? "Resolve the conflicts first"
      : !staged.length && !draft.amend
        ? "Stage the changes to commit"
        : !draft.summary.trim()
          ? "Write a summary"
          : "",
  );

  async function load() {
    try {
      const next = await api.workingTree();
      tree = next;
      error = null;
      const has = (p: { path: string; side: Side } | null) =>
        !!p && (p.side === "staged" ? next.staged : [...next.unstaged, ...next.conflicted]).some((f) => f.path === p.path);
      if (!has(pick)) {
        // The file moved to the other side (it was just staged or unstaged): follow it there.
        const other = pick && { path: pick.path, side: (pick.side === "staged" ? "unstaged" : "staged") as Side };
        if (has(other)) pick = other;
        else {
          const first = next.conflicted[0] ?? next.unstaged[0];
          pick = first ? { path: first.path, side: "unstaged" } : next.staged[0] ? { path: next.staged[0].path, side: "staged" } : null;
        }
      }
      loads++;
    } catch (err) {
      error = String(err);
    }
  }

  $effect(() => {
    version;
    load();
  });

  $effect(() => {
    const current = pick;
    const wholeFile = whole;
    diffSettings();
    loads;
    if (!current) {
      diff = null;
      return;
    }
    api.workingDiff(current.path, current.side, wholeFile).then(
      (d) => {
        if (pick === current && whole === wholeFile) {
          diff = d;
          // Picked lines belong to the hunks they were picked in.
          sel = null;
        }
      },
      (err) => {
        if (pick === current) error = String(err);
      },
    );
  });

  async function act(request: Request) {
    if (await confirm.run(request)) onChanged();
  }

  const name = (files: FileChange[]): Part[] =>
    files.length === 1 ? [{ code: files[0].path }] : [`${files.length} files`];
  const count = (files: FileChange[]) => (files.length === 1 ? splitPath(files[0].path).name : `${files.length} files`);

  function stage(files: FileChange[]) {
    act({
      title: `Stage ${count(files)}?`,
      body: ["Adds the changes in ", ...name(files), " to the next commit on ", branchPart, "."],
      icon: "stage",
      button: "Stage",
      action: { kind: "stage", paths: files.map((f) => f.path) },
    });
  }

  function unstage(files: FileChange[]) {
    act({
      title: `Unstage ${count(files)}?`,
      body: ["Takes ", ...name(files), " out of the next commit. Your edits stay in the files."],
      icon: "unstage",
      button: "Unstage",
      action: { kind: "unstage", paths: files.map((f) => f.path) },
    });
  }

  function discard(files: FileChange[]) {
    const fresh = files.filter((f) => f.status === "untracked");
    const body: Part[] =
      fresh.length === files.length
        ? ["Deletes ", ...name(files), ". Git does not track ", files.length === 1 ? "it" : "them", " yet."]
        : ["Throws away your edits in ", ...name(files), " and puts back the staged or committed version."];
    act({
      title: `Discard changes in ${count(files)}?`,
      body,
      icon: "discard",
      button: "Discard",
      danger: true,
      note: "This can't be undone.",
      action: { kind: "discard", paths: files.map((f) => f.path) },
    });
  }

  /** The changed lines of a hunk, without its context: new line numbers, or old ones when it only removes. */
  function lines(hunk: Hunk): string {
    const added = hunk.lines.filter((l) => l.kind === "added").map((l) => l.newLine ?? 0);
    const numbers = added.length ? added : hunk.lines.filter((l) => l.kind === "removed").map((l) => l.oldLine ?? 0);
    if (!numbers.length) return "a hunk";
    const first = Math.min(...numbers);
    const last = Math.max(...numbers);
    return first === last ? `line ${first}` : `lines ${first}–${last}`;
  }

  function hunkAction(kind: "stageHunk" | "unstageHunk" | "discardHunk", path: string, hunk: Hunk) {
    const where: Part[] = [`${lines(hunk)} of `, { code: path }];
    const request: Record<typeof kind, Omit<Request, "action">> = {
      stageHunk: { title: "Stage this hunk?", body: ["Adds ", ...where, " to the next commit. The rest of the file stays unstaged."], icon: "stage", button: "Stage Hunk" },
      unstageHunk: { title: "Unstage this hunk?", body: ["Takes ", ...where, " out of the next commit. Your edits stay in the file."], icon: "unstage", button: "Unstage Hunk" },
      discardHunk: {
        title: "Discard this hunk?",
        body: ["Throws away your edits in ", ...where, ". The rest of the file stays as it is."],
        icon: "discard",
        button: "Discard",
        danger: true,
        note: "This can't be undone.",
      },
    };
    act({ ...request[kind], action: { kind, path, header: hunk.header } });
  }

  /** Changed lines can be picked in the hunks of a tracked file, not in the whole-file view. */
  const picker: LinePicker = {
    enabled: () => !whole && !!pick && !!diff && diff.file.status !== "untracked" && diff.file.status !== "conflicted",
    picked: (hunk) => (sel?.header === hunk.header ? sel.lines : []),
    toggle: (hunk, index, range) => {
      const current = sel?.header === hunk.header ? sel : null;
      let lines = current?.lines ?? [];
      if (range && current) {
        const [from, to] = current.anchor < index ? [current.anchor, index] : [index, current.anchor];
        const span = hunk.lines.flatMap((l, i) => (i >= from && i <= to && l.kind !== "context" ? [i] : []));
        lines = [...new Set([...lines, ...span])];
      } else lines = lines.includes(index) ? lines.filter((i) => i !== index) : [...lines, index];
      sel = lines.length ? { header: hunk.header, lines, anchor: index } : null;
    },
  };

  function linesAction(kind: LinesKind, path: string, hunk: Hunk) {
    if (sel?.header === hunk.header) act(linesRequest(kind, path, hunk, [...sel.lines].sort((a, b) => a - b)));
  }

  /** Every file with changes once, for a stash of a few of them. */
  function stashCandidates() {
    const seen = new Map<string, { path: string; untracked: boolean; staged: boolean; unstaged: boolean }>();
    for (const f of unstaged) seen.set(f.path, { path: f.path, untracked: f.status === "untracked", staged: stagedPaths.has(f.path), unstaged: true });
    for (const f of staged) if (!seen.has(f.path)) seen.set(f.path, { path: f.path, untracked: false, staged: true, unstaged: false });
    return [...seen.values()];
  }

  const revealLabel = navigator.platform.startsWith("Mac") ? "Reveal in Finder" : navigator.platform.startsWith("Win") ? "Show in Explorer" : "Open Containing Folder";

  function fileMenu(event: MouseEvent, file: FileChange, group: "staged" | "unstaged" | "conflicted") {
    event.preventDefault();
    const name = splitPath(file.path).name;
    const what = { modified: "modified", added: "new file", untracked: "untracked", deleted: "deleted", renamed: "renamed", copied: "copied", conflicted: "conflicted" }[file.status];
    const entries: MenuEntry[] = [{ kind: "header", label: `${name} · ${what}${group === "staged" ? ", staged" : ""}` }];
    if (group === "staged") entries.push({ kind: "item", label: "Unstage File", icon: menuIcons.unstage, run: () => unstage([file]) });
    else if (group === "unstaged") entries.push({ kind: "item", label: "Stage File", icon: menuIcons.stage, run: () => stage([file]) });
    if (group !== "conflicted")
      entries.push({ kind: "item", label: "Stash This File…", icon: menuIcons.stash, run: () => act(stashFilesRequest(stashCandidates(), [file.path], stashMessageFor(file.path))) });
    if (file.status === "untracked")
      entries.push({
        kind: "sub",
        label: "Ignore",
        icon: menuIcons.ignore,
        entries: ignoreChoices(file.path).map((choice) => ({ label: choice.label, hint: choice.pattern, run: () => act(ignoreRequest(file.path, choice)) })),
      });
    entries.push({ kind: "sep" });
    if (file.status !== "deleted")
      entries.push({ kind: "item", label: "Open in Editor", icon: menuIcons.open, run: () => api.openInEditor(file.path, null).catch((err) => confirm.say(String(err))) });
    if (file.status !== "deleted")
      entries.push({ kind: "item", label: revealLabel, icon: menuIcons.reveal, run: () => api.revealFile(file.path).catch((err) => confirm.say(String(err))) });
    entries.push({
      kind: "item",
      label: "Copy Path",
      icon: menuIcons.copy,
      run: () => navigator.clipboard.writeText(file.path).then(() => confirm.say(`Copied ${file.path}.`), () => {}),
    });
    if (file.status !== "untracked" && file.status !== "added") entries.push({ kind: "item", label: "File History", icon: menuIcons.history, run: () => nav.openFile(file.path) });
    if (group === "unstaged") {
      entries.push({ kind: "sep" });
      entries.push({ kind: "item", label: file.status === "untracked" ? "Delete File…" : "Discard Changes…", icon: menuIcons.drop, danger: true, run: () => discard([file]) });
    }
    menu = { x: event.clientX, y: event.clientY, entries, label: `${name} menu` };
  }

  async function commit() {
    if (!canCommit) return;
    const files = staged.length;
    const pushed = draft.amend && head && !head.unpushed;
    const action = { kind: "commit" as const, message, amend: draft.amend };
    const summary = draft.summary.trim();
    const where = branch ?? "HEAD";
    const done = await confirm.run({
      title: draft.amend ? `Amend the last commit on ${branch ?? "HEAD"}?` : `Commit ${files} ${files === 1 ? "file" : "files"} to ${branch ?? "HEAD"}?`,
      body: draft.amend ? amendBody(files) : ["Makes the commit ", { quote: draft.summary.trim() }, " on ", branchPart, " with the staged changes."],
      icon: "commit",
      button: draft.amend ? "Amend" : "Commit",
      note: pushed ? "The last commit is already pushed. After amending, pushing will need --force-with-lease." : undefined,
      status: `Committing to ${where}…`,
      recover: (failure) => commitRecovery(failure, action, summary, where),
      action,
    });
    if (done) {
      draft.summary = "";
      draft.description = "";
      draft.amend = false;
      draft.prefilled = "";
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
    draft.amend = !draft.amend;
    if (draft.amend && !draft.summary.trim() && !draft.description.trim() && head) {
      const detail = await api.commitDetail(head.id).catch(() => null);
      if (detail && draft.amend && !draft.summary.trim()) {
        draft.summary = detail.summary;
        draft.description = detail.body;
        draft.prefilled = message;
      }
    } else if (!draft.amend && draft.prefilled && message === draft.prefilled) {
      draft.summary = "";
      draft.description = "";
      draft.prefilled = "";
    }
  }

  function onCommitKey(event: KeyboardEvent) {
    if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      commit();
    }
  }

  const badge: Record<string, { text: string; tone: string; title: string }> = {
    modified: { text: "M", tone: "orange", title: "Modified" },
    added: { text: "A", tone: "green", title: "Added" },
    untracked: { text: "U", tone: "green", title: "New, not tracked yet" },
    deleted: { text: "D", tone: "red", title: "Deleted" },
    renamed: { text: "R", tone: "orange", title: "Renamed" },
    copied: { text: "C", tone: "orange", title: "Copied" },
    conflicted: { text: "!", tone: "red", title: "Conflicted" },
  };
</script>

{#snippet fileRow(file: FileChange, side: Side, group: "staged" | "unstaged" | "conflicted")}
  {@const p = splitPath(file.path)}
  {@const b = badge[file.status]}
  {@const isPicked = pick?.path === file.path && pick.side === side}
  {@const partly = group === "staged" ? unstagedPaths.has(file.path) : group === "unstaged" && stagedPaths.has(file.path)}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="file" class:picked={isPicked} oncontextmenu={(e) => fileMenu(e, file, group)}>
    {#if group !== "conflicted"}
      <input
        type="checkbox"
        checked={group === "staged"}
        aria-label="{group === 'staged' ? 'Unstage' : 'Stage'} {file.path}"
        title={group === "staged" ? "Unstage" : "Stage"}
        onclick={(e) => {
          e.preventDefault();
          if (group === "staged") unstage([file]);
          else stage([file]);
        }}
      />
    {/if}
    <button class="name" onclick={() => (pick = { path: file.path, side })} title={file.oldPath ? `${file.oldPath} → ${file.path}` : file.path}>
      <span class="badge {b.tone}" title={b.title}>{b.text}</span>
      <span class="path"><span class="dir">{p.dir}</span>{p.name}</span>
      {#if partly}<span class="note">partly staged</span>{/if}
    </button>
    {#if group === "unstaged"}
      <button class="discard" onclick={() => discard([file])} aria-label="Discard changes in {file.path}" title="Discard changes">
        <svg class="icon" viewBox="0 0 16 16"><path d="M2.5 4.5h11M6 4.5V3a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1v1.5M4 4.5l.7 9a1 1 0 0 0 1 .9h4.6a1 1 0 0 0 1-.9l.7-9" /></svg>
      </button>
    {/if}
  </div>
{/snippet}

{#snippet hunkBar(d: FileDiff, hunk: Hunk)}
  {#if !whole && pick && d.file.status !== "untracked" && d.file.status !== "conflicted"}
    {@const current = pick}
    {@const n = sel?.header === hunk.header ? sel.lines.length : 0}
    {@const lineWord = n === 1 ? "Line" : "Lines"}
    <div class="hunk">
      <span class="header">{hunk.header}</span>
      {#if n}
        <button class="state picked" onclick={() => (sel = null)} title="Clear the picked lines">{n} of {hunk.lines.filter((l) => l.kind !== "context").length} lines</button>
        {#if current.side === "unstaged"}
          <button class="hbtn" onclick={() => linesAction("discardHunk", current.path, hunk)}>Discard {n} {lineWord}</button>
          <button class="hbtn primary" onclick={() => linesAction("stageHunk", current.path, hunk)}>Stage {n} {lineWord}</button>
        {:else}
          <button class="hbtn" onclick={() => linesAction("unstageHunk", current.path, hunk)}>Unstage {n} {lineWord}</button>
        {/if}
      {:else}
      <span class="state" class:staged={current.side === "staged"}>{current.side === "staged" ? "Staged" : "Unstaged"}</span>
      {#if current.side === "unstaged"}
        <button class="hbtn" onclick={() => hunkAction("discardHunk", current.path, hunk)}>Discard</button>
        <button class="hbtn primary" onclick={() => hunkAction("stageHunk", current.path, hunk)}>Stage Hunk</button>
      {:else}
        <button class="hbtn" onclick={() => hunkAction("unstageHunk", current.path, hunk)}>Unstage Hunk</button>
      {/if}
      {/if}
    </div>
  {/if}
{/snippet}

<div class="panel">
  <div class="head">
    <h1>
      Uncommitted changes
      {#if branch}
        <span class="branch" style:color={plate(color)} style:background={tint(color, "label")} title="Checked-out branch">
          <svg class="icon tiny" viewBox="0 0 16 16"><circle cx="4.5" cy="3.5" r="1.5" /><circle cx="4.5" cy="12.5" r="1.5" /><circle cx="11.5" cy="5.5" r="1.5" /><path d="M4.5 5v6M11.5 7c0 3-7 2-7 4" /></svg>
          {branch}
        </span>
      {/if}
    </h1>
    <span class="sub">
      {fileCount}
      {fileCount === 1 ? "file" : "files"}{#if conflicted.length} · {conflicted.length} conflicted{/if} · {staged.length} staged · {unstaged.length} unstaged
    </span>
  </div>

  <div class="lists" aria-label="Working copy">
    {#if conflicted.length}
      <div class="group">
        <div class="ghead"><span>Conflicted</span><span class="n">{conflicted.length}</span></div>
        {#each conflicted as file (file.path)}{@render fileRow(file, "unstaged", "conflicted")}{/each}
      </div>
    {/if}
    <div class="group">
      <div class="ghead">
        <span>Unstaged</span><span class="n">{unstaged.length}</span><span class="grow"></span>
        {#if unstaged.length}<button class="link" onclick={() => stage(unstaged)}>Stage All</button>{/if}
      </div>
      {#each unstaged as file (file.path)}{@render fileRow(file, "unstaged", "unstaged")}{/each}
      {#if !unstaged.length}<p class="empty">Nothing left to stage.</p>{/if}
    </div>
    <div class="group">
      <div class="ghead">
        <span>Staged</span><span class="n">{staged.length}</span><span class="grow"></span>
        {#if staged.length}<button class="link" onclick={() => unstage(staged)}>Unstage All</button>{/if}
      </div>
      {#each staged as file (file.path)}{@render fileRow(file, "staged", "staged")}{/each}
      {#if !staged.length}<p class="empty">Tick a file to add it to the next commit.</p>{/if}
    </div>
  </div>

  <div class="scroll">
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if diff}
      {#if diff.file.status === "conflicted"}
        <p class="hint">
          This file has conflicts: both sides changed the same lines.
          {#if onResolve}<button class="resolve" onclick={onResolve}>Resolve Conflicts…</button>{:else}Fix them in your editor, then stage the file.{/if}
        </p>
      {/if}
      <DiffView diffs={[diff]} {color} {whole} onToggleWhole={() => (whole = !whole)} {hunkBar} {picker} />
    {/if}
  </div>

  <div class="commit" role="group" aria-label="Commit">
    <div class="row">
      <span class="grow to">Commit to <b>{branch ?? "detached HEAD"}</b></span>
      <button class="amend" role="switch" aria-checked={draft.amend} onclick={toggleAmend} disabled={!head} title="Replace the last commit instead of adding a new one">
        Amend
        <span class="switch" class:on={draft.amend}><span class="knob"></span></span>
      </button>
    </div>
    <div class="summary">
      <input type="text" aria-label="Commit summary" placeholder="Summary" bind:value={draft.summary} onkeydown={onCommitKey} spellcheck="true" />
      {#if SUMMARY_LIMIT}<span class="counter" class:over={draft.summary.length > SUMMARY_LIMIT}>{draft.summary.length}/{SUMMARY_LIMIT}</span>{/if}
    </div>
    <textarea aria-label="Commit description" placeholder="Description" rows="2" bind:value={draft.description} onkeydown={onCommitKey}></textarea>
    <div class="row">
      <span class="grow hint-line">{commitHint}</span>
      <button class="go" onclick={commit} disabled={!canCommit}>
        {draft.amend ? "Amend" : "Commit"} <span class="keys">{navigator.platform.startsWith("Mac") ? "⌘↩" : "Ctrl+↩"}</span>
      </button>
    </div>
  </div>
</div>

{#if menu}
  <Menu x={menu.x} y={menu.y} label={menu.label} entries={menu.entries} onClose={() => (menu = null)} />
{/if}

<style>
  .panel {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  /* A card, like the commit details and the graph. */
  .head {
    margin: 4px 12px 0;
    padding: 14px 16px 12px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    border: 1px solid var(--sep);
    border-radius: 12px;
  }
  h1 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
    line-height: 1.3;
  }
  .branch {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 20px;
    margin-left: 6px;
    padding: 0 8px;
    border-radius: 10px;
    font-size: 12px;
    font-weight: 500;
    line-height: 1;
    white-space: nowrap;
    vertical-align: 2px;
  }
  .tiny {
    width: 12px;
    height: 12px;
  }
  .sub {
    font-size: 11px;
    color: var(--text2);
  }
  /* The file lists are a card too, like the file list in a commit's Summary. */
  .lists {
    flex-shrink: 0;
    max-height: 38%;
    overflow-y: auto;
    margin: 12px 12px 6px;
    padding: 6px 0;
    border: 1px solid var(--sep);
    border-radius: 12px;
  }
  .group {
    display: flex;
    flex-direction: column;
    padding: 2px 8px 4px;
  }
  .ghead {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
  }
  .n {
    font-weight: 400;
  }
  .grow {
    flex-grow: 1;
    min-width: 0;
  }
  .link {
    font-size: 12px;
    font-weight: 500;
    color: var(--accent-text);
  }
  .empty {
    margin: 0;
    padding: 2px 10px 4px;
    font-size: 12px;
    color: var(--text2);
  }
  .file {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 28px;
    padding: 0 4px 0 10px;
    border-radius: 7px;
  }
  .file:hover {
    background: var(--field);
  }
  .file.picked {
    background: var(--side-sel);
  }
  input[type="checkbox"] {
    margin: 0;
    width: 14px;
    height: 14px;
    accent-color: var(--accent);
    flex-shrink: 0;
  }
  .name {
    flex-grow: 1;
    min-width: 0;
    height: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .badge {
    width: 16px;
    height: 16px;
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 10px;
    font-weight: 700;
    flex-shrink: 0;
  }
  .badge.orange {
    background: var(--orange-soft);
    color: var(--orange);
  }
  .badge.green {
    background: var(--green-soft);
    color: var(--green);
  }
  .badge.red {
    background: var(--red-soft);
    color: var(--red);
  }
  .path {
    flex-grow: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dir {
    color: var(--text2);
  }
  .note {
    font-size: 11px;
    color: var(--text2);
    flex-shrink: 0;
  }
  .discard {
    width: 24px;
    height: 24px;
    border-radius: 12px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text2);
    opacity: 0;
    flex-shrink: 0;
  }
  .discard .icon {
    width: 14px;
    height: 14px;
  }
  .file:hover .discard,
  .file.picked .discard,
  .discard:focus-visible {
    opacity: 1;
  }
  .discard:hover {
    color: var(--red);
    background: var(--red-soft);
  }
  .scroll {
    flex-grow: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .hint,
  .error {
    margin: 12px 20px 0;
    font-size: 12px;
  }
  .hint {
    color: var(--orange);
  }
  .error {
    color: var(--red);
  }
  /* Hunk bar, as in the design: the @@ line, its state and the actions over it. */
  .hunk {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    margin: 6px 0 4px;
    padding: 0 6px 0 12px;
    border-radius: 10px;
    background: var(--hunk-bg);
    color: var(--hunk-text);
    font-family: var(--mono);
    font-size: 11.5px;
  }
  .hunk .header {
    flex-grow: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .state,
  .hbtn {
    font-family: var(--font);
  }
  .state {
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
  }
  .state.staged {
    color: var(--green);
  }
  .state.picked {
    color: var(--text);
    padding: 0 4px;
  }
  .hbtn {
    height: 24px;
    padding: 0 10px;
    border-radius: 12px;
    font-size: 12px;
    font-weight: 500;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    color: var(--text);
  }
  .hbtn.primary {
    background: var(--accent);
    border-color: transparent;
    color: #fff;
    font-weight: 600;
  }
  .commit {
    flex-shrink: 0;
    margin: 10px 12px 12px;
    padding: 12px;
    border-radius: 16px;
    background: var(--field);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .to {
    font-size: 11px;
    color: var(--text2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .to b {
    font-weight: 600;
    color: var(--text);
  }
  .amend {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
  }
  .amend:disabled {
    opacity: 0.5;
  }
  .switch {
    position: relative;
    width: 28px;
    height: 16px;
    border-radius: 8px;
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
    border-radius: 6px;
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
    padding: 0 10px;
    border-radius: 9px;
    background: var(--win);
    border: 0.5px solid var(--glass-border);
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
  }
  .counter {
    font-size: 11px;
    color: var(--text2);
  }
  .counter.over {
    color: var(--orange);
  }
  textarea {
    resize: none;
    border-radius: 9px;
    padding: 8px 10px;
    background: var(--win);
    border: 0.5px solid var(--glass-border);
    outline: 0;
    font: inherit;
    line-height: 1.45;
    color: var(--text);
  }
  input::placeholder,
  textarea::placeholder {
    color: var(--text2);
    font-weight: 400;
  }
  .hint-line {
    font-size: 11px;
    color: var(--text2);
  }
  .go {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 14px;
    border-radius: 16px;
    background: var(--accent);
    color: #fff;
    font-weight: 600;
  }
  .go:disabled {
    opacity: 0.45;
  }
  .keys {
    font-size: 11px;
    font-weight: 500;
    opacity: 0.85;
  }
  .resolve {
    margin-left: 6px;
    color: var(--accent-text);
    font-weight: 600;
  }
</style>
