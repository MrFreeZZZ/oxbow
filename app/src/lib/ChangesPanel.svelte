<script lang="ts" module>
  interface Draft {
    summary: string;
    description: string;
    amend: boolean;
    /** The commit Amend was turned on for: it replaces that one only. */
    amendOn: string | null;
    prefilled: string;
  }

  // The commit message outlives the panel, so picking another commit and coming back keeps the
  // draft. Each repository has its own: a message or Amend from one never lands in another.
  const drafts = new Map<string, Draft>();

  function draftFor(repo: string): Draft {
    const known = drafts.get(repo);
    if (known) return known;
    const draft = $state<Draft>({ summary: "", description: "", amend: false, amendOn: null, prefilled: "" });
    drafts.set(repo, draft);
    return draft;
  }
</script>

<script lang="ts">
  import TipCard from "./TipCard.svelte";
  import { keys } from "./keys";
  import { api } from "./api";
  import { diffSettings, prefs, wholeByDefault } from "./prefs.svelte";
  import { confirm, type Part, type Request } from "./confirm.svelte";
  import type { FileChange, FileDiff, Hunk, HistoryRow, Side, WorkingTree } from "./types";
  import { lane, plate, splitPath, tabLabels, tint } from "./format";
  import DiffView, { type LinePicker } from "./DiffView.svelte";
  import FoldAll from "./FoldAll.svelte";
  import WhyNote from "./WhyNote.svelte";
  import Menu, { menuIcons, type MenuEntry } from "./Menu.svelte";
  import { nav } from "./nav.svelte";
  import { BIG_FILE, bytes } from "./unusual";
  import { commitRecovery, ignoreChoices, ignoreRequest, linesRequest, stashFilesRequest, stashMessageFor, type LinesKind } from "./changes";
  import { updateFolderRequest } from "./submodules";

  let {
    repo,
    branch,
    color,
    head,
    version,
    onChanged,
    onResolve,
  }: {
    /** Path of the repository, whose commit message draft the panel shows. */
    repo: string;
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
  /** The tab shown: null is Summary, otherwise a file's path. */
  let tab = $state<string | null>(null);
  /** The diffs shown, on each side: every file in Summary, one file on its tab. */
  let shown = $state<{ unstaged: FileDiff[]; staged: FileDiff[] }>({ unstaged: [], staged: [] });
  /** Files ticked in each list, for Stage Selected and Unstage Selected. */
  let ticked = $state<{ unstaged: string[]; staged: string[] }>({ unstaged: [], staged: [] });
  let whole = $state(wholeByDefault());
  // Diff & Text's default view applies again when it changes.
  $effect(() => {
    whole = wholeByDefault();
  });
  let error = $state<string | null>(null);
  let loads = $state(0);
  /** Lines picked in one hunk, with the one clicked last for Shift-click. */
  let sel = $state<{ hunk: Hunk; lines: number[]; anchor: number } | null>(null);
  let menu = $state<{ x: number; y: number; entries: MenuEntry[]; label: string } | null>(null);
  let scroller = $state<HTMLDivElement>();
  let tabBar = $state<HTMLDivElement>();
  let unstagedView = $state<DiffView>();
  let stagedView = $state<DiffView>();

  const staged = $derived(tree?.staged ?? []);
  const unstaged = $derived(tree?.unstaged ?? []);
  const conflicted = $derived(tree?.conflicted ?? []);
  const stagedPaths = $derived(new Set(staged.map((f) => f.path)));
  const unstagedPaths = $derived(new Set(unstaged.map((f) => f.path)));
  /** Every changed file once, for the tabs: staged first, then the rest, each in list order. A file
   *  staged in part sits with the staged ones. */
  const stagedTabs = $derived(staged.map((f) => f.path));
  const otherTabs = $derived([...new Set([...conflicted, ...unstaged].map((f) => f.path))].filter((path) => !stagedPaths.has(path)));
  const paths = $derived([...stagedTabs, ...otherTabs]);
  const labels = $derived(tabLabels(paths));
  const fileCount = $derived(paths.length);
  const branchPart = $derived<Part>(branch ? { branch, color } : { code: "HEAD" });

  /** Summary loads the diffs of this many files on each side; the rest are a tab away. */
  const SUMMARY_FILES = 300;

  const draft = $derived(draftFor(repo));

  // Amend replaces the commit it was turned on for. Once HEAD moves (another branch, a commit made
  // elsewhere), it turns off, with the old commit's message if it was not edited.
  $effect(() => {
    if (draft.amend && head && head.id !== draft.amendOn) turnOffAmend();
  });

  const message = $derived([draft.summary.trim(), draft.description.trim()].filter(Boolean).join("\n\n"));
  const canCommit = $derived(!!draft.summary.trim() && conflicted.length === 0 && (staged.length > 0 || (draft.amend && !!head)) && (!draft.amend || head?.id === draft.amendOn));
  let summaryBox = $state<HTMLInputElement>();
  /** Why Commit is grey, beside it, with the way out when there is one. */
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
      return { text: "Nothing staged yet", fix: { label: unstaged.length === 1 ? "Stage 1 File" : `Stage All ${unstaged.length} Files`, run: () => stage(unstaged) } };
    }
    if (!draft.summary.trim()) return { text: "A commit needs a summary", fix: { label: "Write It", run: () => summaryBox?.focus() } };
    return null;
  });

  async function load() {
    try {
      const next = await api.workingTree();
      tree = next;
      error = null;
      const unstagedNow = new Set([...next.unstaged, ...next.conflicted].map((f) => f.path));
      const stagedNow = new Set(next.staged.map((f) => f.path));
      // A file's tab closes once it has no changes left; ticks go with the files that moved.
      if (tab !== null && !unstagedNow.has(tab) && !stagedNow.has(tab)) tab = null;
      ticked = { unstaged: ticked.unstaged.filter((p) => unstagedNow.has(p)), staged: ticked.staged.filter((p) => stagedNow.has(p)) };
      loads++;
    } catch (err) {
      error = String(err);
    }
  }

  $effect(() => {
    version;
    load();
  });

  // The selected tab may be scrolled out of the tab bar.
  $effect(() => {
    tab;
    requestAnimationFrame(() => tabBar?.querySelector<HTMLElement>('[aria-selected="true"]')?.scrollIntoView({ inline: "nearest", block: "nearest" }));
  });

  $effect(() => {
    const current = tab;
    const wholeFile = whole;
    diffSettings();
    loads;
    const t = tree;
    if (!t) return;
    const side = (list: FileChange[]) =>
      list
        .map((f) => f.path)
        .filter((p) => current === null || p === current)
        .slice(0, SUMMARY_FILES);
    const unstagedList = side([...t.conflicted, ...t.unstaged]);
    const stagedList = side(t.staged);
    Promise.all([
      unstagedList.length ? api.workingDiffs(unstagedList, "unstaged", wholeFile) : [],
      stagedList.length ? api.workingDiffs(stagedList, "staged", wholeFile) : [],
    ]).then(
      ([u, st]) => {
        if (tab === current && whole === wholeFile && tree === t) {
          shown = { unstaged: u, staged: st };
          // Picked lines belong to the hunks they were picked in.
          sel = null;
        }
      },
      (err) => {
        if (tab === current) error = String(err);
      },
    );
  });

  function openTab(path: string | null) {
    tab = path;
    if (scroller) scroller.scrollTop = 0;
  }

  /** The side and file each hunk shown belongs to. */
  const hunkFile = $derived.by(() => {
    const map = new Map<Hunk, { file: FileChange; side: Side }>();
    for (const [side, list] of [["unstaged", shown.unstaged], ["staged", shown.staged]] as const)
      for (const d of list) for (const h of d.hunks) map.set(h, { file: d.file, side });
    return map;
  });

  function tick(group: Side, path: string) {
    const list = ticked[group];
    ticked = { ...ticked, [group]: list.includes(path) ? list.filter((p) => p !== path) : [...list, path] };
  }

  const pickedFiles = (group: Side) => (group === "staged" ? staged : unstaged).filter((f) => ticked[group].includes(f.path));

  async function act(request: Request): Promise<boolean> {
    const done = await confirm.run(request);
    if (done) onChanged();
    return done;
  }

  /** Stage Selected or Unstage Selected: the ticked files of one list, then their ticks go. */
  async function actOnTicked(group: Side) {
    const files = pickedFiles(group);
    if (!files.length) return;
    const done = await (group === "staged" ? unstage(files) : stage(files));
    if (done) ticked = { ...ticked, [group]: ticked[group].filter((p) => !files.some((f) => f.path === p)) };
  }

  const name = (files: FileChange[]): Part[] =>
    files.length === 1 ? [{ code: files[0].path }] : [`${files.length} files`];
  const count = (files: FileChange[]) => (files.length === 1 ? splitPath(files[0].path).name : `${files.length} files`);

  function stage(files: FileChange[]) {
    return act({
      title: `Stage ${count(files)}?`,
      body: ["Adds the changes in ", ...name(files), " to the next commit on ", branchPart, "."],
      icon: "stage",
      button: "Stage",
      past: `Staged ${count(files)}`,
      action: { kind: "stage", paths: files.map((f) => f.path) },
    });
  }

  function unstage(files: FileChange[]) {
    return act({
      title: `Unstage ${count(files)}?`,
      body: ["Takes ", ...name(files), " out of the next commit. Your edits stay in the files."],
      icon: "unstage",
      button: "Unstage",
      past: `Unstaged ${count(files)}`,
      action: { kind: "unstage", paths: files.map((f) => f.path) },
    });
  }

  function discard(files: FileChange[]) {
    const fresh = files.filter((f) => f.status === "untracked");
    const body: Part[] =
      fresh.length === files.length
        ? ["Deletes ", ...name(files), ". Git does not track ", files.length === 1 ? "it" : "them", " yet."]
        : ["Throws away your edits in ", ...name(files), " and puts back the staged or committed version."];
    return act({
      title: `Discard changes in ${count(files)}?`,
      body,
      icon: "discard",
      button: "Discard",
      danger: true,
      note: "Undo brings it back, from the toast or the Operation Log.",
      done: `Discarded changes in ${count(files)}.`,
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
    const file = path.slice(path.lastIndexOf("/") + 1);
    const request: Record<typeof kind, Omit<Request, "action">> = {
      stageHunk: { title: "Stage this hunk?", body: ["Adds ", ...where, " to the next commit. The rest of the file stays unstaged."], icon: "stage", button: "Stage Hunk", past: `Staged ${lines(hunk)} of ${file}` },
      unstageHunk: { title: "Unstage this hunk?", body: ["Takes ", ...where, " out of the next commit. Your edits stay in the file."], icon: "unstage", button: "Unstage Hunk", past: `Unstaged ${lines(hunk)} of ${file}` },
      discardHunk: {
        title: "Discard this hunk?",
        body: ["Throws away your edits in ", ...where, ". The rest of the file stays as it is."],
        icon: "discard",
        button: "Discard",
        danger: true,
        note: "Undo brings it back, from the toast or the Operation Log.",
        done: `Discarded ${lines(hunk)} of ${file}.`,
      },
    };
    act({ ...request[kind], action: { kind, path, header: hunk.header, check: hunk.check ?? "" } });
  }

  /** Changed lines can be picked in the hunks of a tracked file, not in the whole-file view. */
  const picker: LinePicker = {
    enabled: (hunk) => {
      const f = hunkFile.get(hunk)?.file;
      return !whole && !!f && f.status !== "untracked" && f.status !== "conflicted";
    },
    picked: (hunk) => (sel?.hunk === hunk ? sel.lines : []),
    toggle: (hunk, index, range) => {
      const current = sel?.hunk === hunk ? sel : null;
      let lines = current?.lines ?? [];
      if (range && current) {
        const [from, to] = current.anchor < index ? [current.anchor, index] : [index, current.anchor];
        const span = hunk.lines.flatMap((l, i) => (i >= from && i <= to && l.kind !== "context" ? [i] : []));
        lines = [...new Set([...lines, ...span])];
      } else lines = lines.includes(index) ? lines.filter((i) => i !== index) : [...lines, index];
      sel = lines.length ? { hunk, lines, anchor: index } : null;
    },
  };

  function linesAction(kind: LinesKind, path: string, hunk: Hunk) {
    if (sel?.hunk === hunk) act(linesRequest(kind, path, hunk, [...sel.lines].sort((a, b) => a - b)));
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
      entries.push({ kind: "item", label: "Open in Editor", icon: menuIcons.open, run: () => api.openInEditor(file.path, null).catch((err) => confirm.fail(err)) });
    if (file.status !== "deleted")
      entries.push({ kind: "item", label: revealLabel, icon: menuIcons.reveal, run: () => api.revealFile(file.path).catch((err) => confirm.fail(err)) });
    entries.push({
      kind: "item",
      label: "Copy Path",
      icon: menuIcons.copy,
      run: () => navigator.clipboard.writeText(file.path).then(() => confirm.say(`Copied ${file.path}.`, "done"), () => {}),
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

  /** The description grows with its text while it is being written, up to DESC_LINES, and
   *  goes back to two lines after, so the diffs keep their room. */
  const DESC_LINES = 10;
  let descBox = $state<HTMLTextAreaElement>();
  let descOpen = $state(false);
  $effect(() => {
    draft.description;
    const open = descOpen;
    const box = descBox;
    if (!box) return;
    const style = getComputedStyle(box);
    const line = parseFloat(style.lineHeight);
    const frame = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom) + parseFloat(style.borderTopWidth) + parseFloat(style.borderBottomWidth);
    const least = 2 * line + frame;
    box.style.height = `${least}px`;
    const wanted = Math.min(box.scrollHeight + parseFloat(style.borderTopWidth) + parseFloat(style.borderBottomWidth), DESC_LINES * line + frame);
    box.style.height = `${open ? Math.max(least, wanted) : least}px`;
  });

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

{#snippet fileRow(file: FileChange, group: "staged" | "unstaged" | "conflicted")}
  {@const p = splitPath(file.path)}
  {@const b = badge[file.status]}
  {@const on = group !== "conflicted" && ticked[group].includes(file.path)}
  {@const partly = group === "staged" ? unstagedPaths.has(file.path) : group === "unstaged" && stagedPaths.has(file.path)}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="file" class:ticked={on} oncontextmenu={(e) => fileMenu(e, file, group)}>
    {#if group !== "conflicted"}
      <input
        type="checkbox"
        checked={on}
        aria-label="Select {file.path}"
        title={group === "staged" ? "Select for Unstage Selected" : "Select for Stage Selected"}
        onchange={() => tick(group, file.path)}
      />
    {/if}
    <button class="name" onclick={() => openTab(file.path)} title={file.oldPath ? `${file.oldPath} → ${file.path}` : file.path}>
      <span class="badge {b.tone}" title={b.title}>{b.text}</span>
      <span class="path"><span class="dir">{p.dir}</span>{p.name}</span>
      {#if partly}<span class="note">partly staged</span>{/if}
      {#if file.submodule}
        <span class="note" title="Another repository in a folder here">Submodule{file.submodule.changed && !file.submodule.moved ? " · changed inside" : ""}</span>
      {:else if file.lfs}
        <span class="note" title="Kept in Git LFS: git stores a small pointer">LFS{file.newSize !== undefined ? ` · ${bytes(file.newSize)}` : ""}</span>
      {:else if (file.newSize ?? 0) >= BIG_FILE}
        <span class="note warn" title="Too big for a git repository: GitHub refuses files over 100 MB">{bytes(file.newSize!)}</span>
      {/if}
    </button>
    {#if group === "unstaged"}
      <button class="discard" onclick={() => discard([file])} aria-label="Discard changes in {file.path}" title="Discard changes">
        <svg class="icon" viewBox="0 0 16 16"><path d="M2.5 4.5h11M6 4.5V3a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1v1.5M4 4.5l.7 9a1 1 0 0 0 1 .9h4.6a1 1 0 0 0 1-.9l.7-9" /></svg>
      </button>
    {/if}
  </div>
{/snippet}

{#snippet hunkBar(d: FileDiff, hunk: Hunk, side: Side)}
  {#if !whole && d.file.status !== "untracked" && d.file.status !== "conflicted"}
    {@const n = sel?.hunk === hunk ? sel.lines.length : 0}
    {@const lineWord = n === 1 ? "Line" : "Lines"}
    <div class="hunk">
      <span class="header">{hunk.header}</span>
      {#if n}
        <button class="state picked" onclick={() => (sel = null)} title="Clear the picked lines">{n} of {hunk.lines.filter((l) => l.kind !== "context").length} lines</button>
        {#if side === "unstaged"}
          <button class="hbtn" onclick={() => linesAction("discardHunk", d.file.path, hunk)}>Discard {n} {lineWord}</button>
          <button class="hbtn primary" onclick={() => linesAction("stageHunk", d.file.path, hunk)}>Stage {n} {lineWord}</button>
        {:else}
          <button class="hbtn" onclick={() => linesAction("unstageHunk", d.file.path, hunk)}>Unstage {n} {lineWord}</button>
        {/if}
      {:else}
        <span class="state" class:staged={side === "staged"}>{side === "staged" ? "Staged" : "Unstaged"}</span>
        {#if side === "unstaged"}
          <button class="hbtn" onclick={() => hunkAction("discardHunk", d.file.path, hunk)}>Discard</button>
          <button class="hbtn primary" onclick={() => hunkAction("stageHunk", d.file.path, hunk)}>Stage Hunk</button>
        {:else}
          <button class="hbtn" onclick={() => hunkAction("unstageHunk", d.file.path, hunk)}>Unstage Hunk</button>
        {/if}
      {/if}
    </div>
  {/if}
{/snippet}
{#snippet unstagedHunk(d: FileDiff, hunk: Hunk)}{@render hunkBar(d, hunk, "unstaged")}{/snippet}
{#snippet stagedHunk(d: FileDiff, hunk: Hunk)}{@render hunkBar(d, hunk, "staged")}{/snippet}

<!-- The whole file's actions on its header bar. -->
{#snippet unstagedFile(d: FileDiff)}
  {#if d.submodule}
    <!-- A submodule's own files are committed in it; here only the commit it is at can be staged. -->
    {#if d.file.submodule?.moved}
      {@const move = d.submodule}
      {#if move.old}<button class="hbtn" onclick={() => act(updateFolderRequest(move))} title="Check out the commit recorded here again">Update to {move.old.slice(0, 7)}</button>{/if}
      <button class="hbtn primary" onclick={() => stage([d.file])} title="Record the commit it is at in the next commit">Stage</button>
    {:else}
      <button class="hbtn" onclick={() => api.openWindow(d.file.path).catch((err) => confirm.fail(err))} title="Its changes are committed in it">Open in New Window</button>
    {/if}
  {:else if d.file.status === "conflicted"}
    {#if onResolve}<button class="hbtn" onclick={onResolve}>Resolve Conflicts…</button>{/if}
  {:else}
    <button class="hbtn" onclick={() => discard([d.file])} title={d.file.status === "untracked" ? "Delete this new file" : "Throw away the edits in this file"}>Discard</button>
    <button class="hbtn primary" onclick={() => stage([d.file])} title="Add the whole file to the next commit">Stage</button>
  {/if}
{/snippet}
{#snippet stagedFile(d: FileDiff)}
  <button class="hbtn" onclick={() => unstage([d.file])} title="Take the whole file out of the next commit">Unstage</button>
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

  {#if fileCount}
    <div class="tabs" role="tablist" aria-label="Uncommitted changes">
      <!-- Summary stays pinned first; only the file tabs scroll. -->
      <button
        role="tab"
        aria-selected={tab === null}
        class="tab"
        class:on={tab === null}
        style:background={tab === null ? tint(color) : undefined}
        style:border-color={tab === null ? `color-mix(in srgb, ${lane(color)} 40%, transparent)` : undefined}
        style:color={tab === null ? plate(color) : undefined}
        onclick={() => openTab(null)}>Summary</button
      >
      <span class="divider" aria-hidden="true"></span>
      <div class="file-tabs" bind:this={tabBar}>
        {#each stagedTabs as path (path)}
          <button role="tab" aria-selected={tab === path} class="tab staged" class:on={tab === path} onclick={() => openTab(path)} title="Staged · {path}"
            >{labels.get(path)}</button
          >
        {/each}
        {#if stagedTabs.length && otherTabs.length}<span class="divider" aria-hidden="true"></span>{/if}
        {#each otherTabs as path (path)}
          <button role="tab" aria-selected={tab === path} class="tab neutral" class:on={tab === path} onclick={() => openTab(path)} title={path}
            >{labels.get(path)}</button
          >
        {/each}
      </div>
    </div>
  {/if}

  <div class="scroll" bind:this={scroller}>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if tab === null}
      <div class="lists" aria-label="Working copy">
        {#if conflicted.length}
          <div class="group">
            <div class="ghead"><span>Conflicted</span><span class="n">{conflicted.length}</span></div>
            {#each conflicted as file (file.path)}{@render fileRow(file, "conflicted")}{/each}
          </div>
        {/if}
        <div class="group">
          <div class="ghead">
            <span>Unstaged</span><span class="n">{unstaged.length}</span><span class="grow"></span>
            {#if ticked.unstaged.length}<button class="gbtn primary" onclick={() => actOnTicked("unstaged")}>Stage Selected ({ticked.unstaged.length})</button>{/if}
            {#if unstaged.length}<button class="gbtn" onclick={() => stage(unstaged)}>Stage All</button>{/if}
          </div>
          {#each unstaged as file (file.path)}{@render fileRow(file, "unstaged")}{/each}
          {#if !unstaged.length}<p class="empty">Nothing left to stage.</p>{/if}
        </div>
        <div class="group">
          <div class="ghead">
            <span>Staged</span><span class="n">{staged.length}</span><span class="grow"></span>
            {#if ticked.staged.length}<button class="gbtn primary" onclick={() => actOnTicked("staged")}>Unstage Selected ({ticked.staged.length})</button>{/if}
            {#if staged.length}<button class="gbtn" onclick={() => unstage(staged)}>Unstage All</button>{/if}
          </div>
          {#each staged as file (file.path)}{@render fileRow(file, "staged")}{/each}
          {#if !staged.length}<p class="empty">Tick files above and press Stage Selected to add them to the next commit.</p>{/if}
        </div>
      </div>
      {#if staged.some((f) => unstagedPaths.has(f.path))}<div class="tip"><TipCard id="partlyStaged" /></div>{/if}
    {/if}
    {#each [{ side: "unstaged" as Side, diffs: shown.unstaged }, { side: "staged" as Side, diffs: shown.staged }] as part (part.side)}
      {#if part.diffs.length}
        {@const total = tab === null ? (part.side === "staged" ? staged.length : unstaged.length + conflicted.length) : 1}
        {#if tab === null || (shown.unstaged.length && shown.staged.length)}
          <div class="part">
            <span class="grow">{part.side === "staged" ? "Staged" : "Unstaged"} changes</span>
            {#if total > part.diffs.length}<span class="cut">first {part.diffs.length} of {total} files; the rest are on their tabs</span>{/if}
            {#if tab === null}<FoldAll onFold={(fold) => (part.side === "staged" ? stagedView : unstagedView)?.setAll(fold)} />{/if}
          </div>
        {/if}
        {#if part.side === "unstaged"}
          <DiffView
            bind:this={unstagedView}
            foldable
            diffs={part.diffs}
            {color}
            {whole}
            onToggleWhole={() => (whole = !whole)}
            hunkBar={unstagedHunk}
            fileActions={unstagedFile}
            {picker}
            loadFull={(d) => api.workingDiff(d.file.path, "unstaged", whole, true)}
            command={(d) => `git diff -- ${d.file.path}`}
            working
            side="unstaged"
          />
        {:else}
          <DiffView
            bind:this={stagedView}
            foldable
            diffs={part.diffs}
            {color}
            {whole}
            onToggleWhole={() => (whole = !whole)}
            hunkBar={stagedHunk}
            fileActions={stagedFile}
            {picker}
            loadFull={(d) => api.workingDiff(d.file.path, "staged", whole, true)}
            command={(d) => `git diff --cached -- ${d.file.path}`}
            working
            side="staged"
          />
        {/if}
      {/if}
    {/each}
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
      <input type="text" aria-label="Commit summary" placeholder="Summary" bind:this={summaryBox} bind:value={draft.summary} onkeydown={onCommitKey} spellcheck="true" />
      {#if SUMMARY_LIMIT}<span class="counter" class:over={draft.summary.length > SUMMARY_LIMIT}>{draft.summary.length}/{SUMMARY_LIMIT}</span>{/if}
    </div>
    <textarea
      aria-label="Commit description"
      placeholder="Description"
      rows="2"
      bind:this={descBox}
      bind:value={draft.description}
      onkeydown={onCommitKey}
      onfocus={() => (descOpen = true)}
      onblur={() => (descOpen = false)}
    ></textarea>
    <div class="row">
      {#if commitHint}
        <WhyNote text={commitHint.text} warn={commitHint.warn} />
        {#if commitHint.fix}<button class="gbtn" onclick={commitHint.fix.run}>{commitHint.fix.label}</button>{/if}
      {/if}
      <span class="grow"></span>
      <button class="go" onclick={commit} disabled={!canCommit} title={commitHint?.text}>
        {draft.amend ? "Amend" : "Commit"} <span class="keys">{keys("Mod+Enter")}</span>
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
  /* Tabs, as in a commit's details: Summary in the branch color, then a tab per file. */
  .tabs {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 10px 0 0 12px;
    flex-shrink: 0;
  }
  .divider {
    width: 1px;
    height: 16px;
    background: var(--sep);
    flex-shrink: 0;
  }
  .file-tabs {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    padding-right: 12px;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .file-tabs::-webkit-scrollbar {
    display: none;
  }
  .tab {
    display: flex;
    align-items: center;
    height: 26px;
    padding: 0 11px;
    border-radius: 13px;
    border: 1px solid var(--sep);
    font-size: 12px;
    font-weight: 500;
    color: var(--text2);
    white-space: nowrap;
    flex-shrink: 0;
  }
  .tab.on {
    font-weight: 600;
  }
  /* Staged files: Steel, the color of Commit, which takes them. */
  .tab.staged {
    background: color-mix(in srgb, var(--accent) 9%, transparent);
    border-color: color-mix(in srgb, var(--accent) 26%, transparent);
    color: color-mix(in srgb, var(--accent) 75%, var(--text));
  }
  .tab.staged.on {
    background: color-mix(in srgb, var(--accent) 20%, transparent);
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
  }
  .tab.neutral.on {
    background: var(--side-sel);
    border-color: transparent;
    color: var(--text);
  }
  .tip {
    margin: 0 12px 6px;
    display: flex;
    flex-direction: column;
  }
  .lists {
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
  /* Stage All, Stage Selected and the like: small capsules, the blue one does what was ticked. */
  .gbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 22px;
    padding: 0 10px;
    border-radius: 11px;
    font-family: var(--font);
    font-size: 12px;
    font-weight: 500;
    line-height: 1;
    white-space: nowrap;
    flex-shrink: 0;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    color: var(--text);
  }
  .gbtn.primary {
    background: var(--accent);
    border-color: transparent;
    color: #fff;
    font-weight: 600;
  }
  /* A heading over each side's diffs in Summary. */
  .part {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 10px 12px 0;
    padding: 0 10px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
  }
  .cut {
    font-weight: 400;
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
  .file.ticked {
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
  .note.warn {
    color: var(--orange);
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
  .error {
    margin: 12px 20px 0;
    font-size: 12px;
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
    line-height: normal;
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
  /* Buttons over the code: their own line height, or the code's would push the label down. */
  .hbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 24px;
    padding: 0 10px;
    line-height: 1;
    white-space: nowrap;
    flex-shrink: 0;
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
    transition: height 0.15s ease;
  }
  input::placeholder,
  textarea::placeholder {
    color: var(--text2);
    font-weight: 400;
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
</style>
