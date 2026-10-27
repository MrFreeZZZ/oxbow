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
  // It outlives the app too. Each folder has its own key, so a worktree keeps its own draft and
  // two windows never write over each other's.
  const DRAFT_KEY = "oxbow.commitDraft:";

  function savedDraft(repo: string): Partial<Draft> {
    try {
      const saved = JSON.parse(localStorage.getItem(DRAFT_KEY + repo) ?? "null");
      return saved && typeof saved === "object" ? saved : {};
    } catch {
      return {};
    }
  }

  function saveDraft(repo: string, draft: Draft) {
    try {
      if (!draft.summary.trim() && !draft.description.trim() && !draft.amend) localStorage.removeItem(DRAFT_KEY + repo);
      else localStorage.setItem(DRAFT_KEY + repo, JSON.stringify(draft));
    } catch {
      // Private mode or full storage: the draft lasts until the app quits.
    }
  }

  /** The commit message draft saved for the folder `repo`, as it was saved. */
  export function savedMessage(repo: string): unknown {
    return drafts.get(repo) ?? savedDraft(repo);
  }

  /** The folder `from` was moved to `to`: its commit message goes along, unless `to` has one of
   *  its own. True when it went. */
  export function moveMessage(from: string, to: string): boolean {
    const draft = draftFor(from);
    if (!draft.summary.trim() && !draft.description.trim() && !draft.amend) return false;
    const there = draftFor(to);
    if (there.summary.trim() || there.description.trim() || there.amend) return false;
    Object.assign(there, $state.snapshot(draft));
    saveDraft(to, there);
    drafts.delete(from);
    saveDraft(from, { summary: "", description: "", amend: false, amendOn: null, prefilled: "" });
    return true;
  }

  function draftFor(repo: string): Draft {
    const known = drafts.get(repo);
    if (known) return known;
    const saved = savedDraft(repo);
    const text = (value: unknown) => (typeof value === "string" ? value : "");
    const draft = $state<Draft>({
      summary: text(saved.summary),
      description: text(saved.description),
      amend: saved.amend === true,
      amendOn: typeof saved.amendOn === "string" ? saved.amendOn : null,
      prefilled: text(saved.prefilled),
    });
    drafts.set(repo, draft);
    return draft;
  }

  /** Untracked files start folded away, apart from the edits, and stay open once opened until
   *  the app quits; every window and repository shares that. */
  const view = $state({ untracked: false });
</script>

<script lang="ts">
  import { tick, untrack } from "svelte";
  import TipCard from "./TipCard.svelte";
  import { shortcut } from "./keys";
  import { pressed } from "./shortcuts";
  import { api, stopped } from "./api";
  import { diffSettings, prefs, wholeByDefault } from "./prefs.svelte";
  import { confirm, type Part, type Request } from "./confirm.svelte";
  import type { FileChange, FileDiff, Hunk, HistoryRow, Side, WorkingTree } from "./types";
  import { lane, plate, splitPath, tint } from "./format";
  import { tabStepper } from "./tabs";
  import DiffView, { type LinePicker } from "./DiffView.svelte";
  import DiffTools from "./DiffTools.svelte";
  import FoldAll from "./FoldAll.svelte";
  import PathName from "./PathName.svelte";
  import WhyNote from "./WhyNote.svelte";
  import Menu, { menuIcons, type MenuEntry } from "./Menu.svelte";
  import { nav } from "./nav.svelte";
  import { BIG_FILE, bytes } from "./unusual";
  import { commitRecovery, ignoreChoices, ignoreRequest, linesRequest, stashFilesRequest, stashMessageFor, type LinesKind } from "./changes";
  import { keepShown, nextAfter, pickRange, sameShown, stepShown, type Shown } from "./changesView";
  import { updateFolderRequest } from "./submodules";
  import { byFolder, filesWidth, listView, resizeFiles, setFolders } from "./listView.svelte";

  let {
    repo,
    branch,
    color,
    head,
    version,
    changes,
    snapshot,
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
    /** The files the reload read, and which reading that was: the panel shows those, so git
     *  lists them once per reload. Null when git could not list them; the panel then asks itself
     *  to show why. */
    changes: WorkingTree | null;
    snapshot: number;
    /** Called after an action changed the repository. */
    onChanged: () => void;
    /** Opens the Conflicts screen. */
    onResolve?: () => void;
  } = $props();

  /** Settings › Git › Subject line guide; 0 hides the counter. */
  const SUMMARY_LIMIT = $derived(prefs.get("oxbow.commit.subjectGuide"));

  let tree = $state<WorkingTree | null>(null);
  /** What the diff column shows: a file on one side, or every file of a side. */
  let shown = $state<Shown | null>(null);
  /** The diffs in the diff column, and what they were read for: until a new read is in, the
   *  column keeps the old diffs, with their own side's actions. */
  let diffs = $state<FileDiff[]>([]);
  let diffsOf = $state<Shown | null>(null);
  /** The lists of files: untracked files are unstaged ones listed apart. */
  type List = Side | "untracked";
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
  let diffScroller = $state<HTMLDivElement>();
  let diffView = $state<DiffView>();

  const staged = $derived(tree?.staged ?? []);
  const unstaged = $derived(tree?.unstaged ?? []);
  const conflicted = $derived(tree?.conflicted ?? []);
  /** Unstaged files split as Sublime Merge does: edits to tracked files, and new files git does not track yet. */
  const tracked = $derived(unstaged.filter((f) => f.status !== "untracked"));
  const untracked = $derived(unstaged.filter((f) => f.status === "untracked"));
  /** Only files whose path has this text are listed, and their diffs shown with every file of a side. */
  let filter = $state("");
  const needle = $derived(filter.trim().toLowerCase());
  const matches = (file: FileChange) => !needle || file.path.toLowerCase().includes(needle) || !!file.oldPath?.toLowerCase().includes(needle);
  /** Untracked files show while a filter is typed, so it finds them too. */
  const untrackedOpen = $derived(view.untracked || !!needle);
  /** Unstaged and Staged fold to their heading, until the app quits. */
  let shut = $state({ unstaged: false, staged: false });
  /** Each list's files by path, for the other side of a file staged in part. */
  const stagedByPath = $derived(new Map(staged.map((f) => [f.path, f])));
  const unstagedByPath = $derived(new Map(unstaged.map((f) => [f.path, f])));
  const fileCount = $derived(new Set([...conflicted, ...unstaged, ...staged].map((f) => f.path)).size);
  const branchPart = $derived<Part>(branch ? { branch, color } : { code: "HEAD" });

  /** Lines of diff a page of every file of a side brings, about what the diff shows at first. */
  const PAGE_LINES = 4000;
  /** Lanes of the diff reads of each side: a newer read in a lane stops the older one. */
  const LANES: Record<Side, string> = { unstaged: "changes-unstaged", staged: "changes-staged" };
  /** Where the next page of diffs starts (null once all are loaded), and how many files are still to come. */
  let more = $state<number | null>(null);
  let pending = $state(0);
  let loadingMore = false;
  /** What the diffs shown were asked for: later pages must be of the same. */
  let asked: { side: Side; paths: string[]; whole: boolean; reading: number | null } | null = null;

  const draft = $derived(draftFor(repo));

  $effect(() => {
    const { summary, description, amend, amendOn, prefilled } = draft;
    saveDraft(repo, { summary, description, amend, amendOn, prefilled });
  });

  const shownConflicted = $derived(conflicted.filter(matches));
  const shownTracked = $derived(tracked.filter(matches));
  const shownUntracked = $derived(untracked.filter(matches));
  const shownStaged = $derived(staged.filter(matches));
  /** The filter the diffs of a whole side follow, a moment after typing stops. */
  let diffNeedle = $state("");
  $effect(() => {
    const next = needle;
    const timer = setTimeout(() => (diffNeedle = next), 200);
    return () => clearTimeout(timer);
  });

  /** Rows drawn at first in each list, and more on each request: thousands of files draw the
   *  rest when asked. */
  const LIST_STEP = 300;
  let listed = $state<Record<List | "conflicted", number>>({ unstaged: LIST_STEP, staged: LIST_STEP, untracked: LIST_STEP, conflicted: LIST_STEP });

  /** Each folder with how many of its files' rows are drawn: the files of an open folder count
   *  toward `limit`, a closed one as one row, and a folder past the limit waits with its files. */
  function foldersWithin(folders: { dir: string; files: FileChange[] }[], group: List | "conflicted", limit: number) {
    let used = 0;
    const out = folders.map((folder) => {
      const room = Math.max(0, limit - used);
      used += folded.has(`${group}:${folder.dir}`) ? 1 : folder.files.length;
      return { folder, room };
    });
    return { folders: out, hidden: used - limit };
  }

  /** Folders closed in each list, as "list:folder/". */
  let folded = $state(new Set<string>());
  function fold(key: string) {
    const next = new Set(folded);
    if (!next.delete(key)) next.add(key);
    folded = next;
  }

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

  /** Which reading of the files `tree` is, for diffs of the same one; null for one of its own. */
  let reading: number | null = null;

  async function load() {
    try {
      const given = changes;
      const next = given ?? (await api.workingTree());
      reading = given ? snapshot : null;
      tree = next;
      error = null;
      // A file staged or unstaged whole is shown where it went; a selection keeps the files still there.
      shown = keepShown(shown, next);
      loads++;
    } catch (err) {
      error = String(err);
    }
  }

  $effect(() => {
    version;
    // The files often come with the reload, so `load` runs at once: what it reads must not tie
    // this effect to it.
    untrack(load);
  });

  $effect(() => {
    const target = shown;
    const wholeFile = whole;
    diffSettings();
    loads;
    const t = tree;
    if (!t || !target) {
      asked = null;
      diffs = [];
      diffsOf = target;
      return;
    }
    const n = target.kind === "all" ? diffNeedle : "";
    const pool =
      target.side === "staged"
        ? t.staged
        : target.kind !== "all"
          ? [...t.conflicted, ...t.unstaged]
          : target.untracked
            ? t.unstaged.filter((f) => f.status === "untracked")
            : [...t.conflicted, ...t.unstaged.filter((f) => f.status !== "untracked")];
    const chosen = target.kind === "files" ? new Set(target.paths) : null;
    const paths = pool
      .filter((f) => (target.kind === "file" ? f.path === target.path : chosen ? chosen.has(f.path) : !n || f.path.toLowerCase().includes(n) || !!f.oldPath?.toLowerCase().includes(n)))
      .map((f) => f.path);
    const ask = { side: target.side, paths, whole: wholeFile, reading };
    asked = ask;
    const first = paths.length ? api.workingDiffPage(paths, target.side, wholeFile, reading, 0, PAGE_LINES, LANES[target.side]) : Promise.resolve({ diffs: [], next: null, files: 0 });
    first.then(
      (page) => {
        if (asked !== ask) return;
        diffs = page.diffs;
        diffsOf = target;
        more = page.next;
        pending = page.files - page.diffs.length;
        // Picked lines belong to the hunks they were picked in.
        sel = null;
      },
      (err) => {
        if (asked === ask && !stopped(err)) error = String(err);
      },
    );
  });

  // The panel's reads stop when it closes: their answers would go nowhere.
  $effect(() => () => {
    api.stopReads(Object.values(LANES)).catch(() => {});
  });

  /** The next page of the diffs of every file of a side. */
  function loadMore() {
    const ask = asked;
    const from = more;
    if (!ask || from === null || loadingMore) return;
    loadingMore = true;
    api.workingDiffPage(ask.paths, ask.side, ask.whole, ask.reading, from, PAGE_LINES, LANES[ask.side]).then(
      (page) => {
        loadingMore = false;
        if (asked !== ask || more !== from) return;
        diffs = [...diffs, ...page.diffs];
        more = page.next;
        pending = Math.max(0, pending - page.diffs.length);
      },
      (err) => {
        loadingMore = false;
        if (asked === ask && !stopped(err)) error = String(err);
      },
    );
  }

  let filesBox = $state<HTMLDivElement>();

  function show(next: Shown) {
    if ((lastOpen || !sameShown(next, shown)) && diffScroller) diffScroller.scrollTop = 0;
    lastOpen = null;
    shown = next;
    if (next.kind === "file") {
      const group = groupOf(next.path, next.side);
      anchor = group ? { group, path: next.path } : null;
    }
    // Stepped to with the keys, the file may be scrolled out of its list.
    requestAnimationFrame(() => filesBox?.querySelector('[aria-current="true"]')?.scrollIntoView({ block: "nearest" }));
  }

  const sideOf = (group: List | "conflicted"): Side => (group === "staged" ? "staged" : "unstaged");
  /** The list a file on a side is in; null for a conflicted file, which is never selected with others. */
  function groupOf(path: string, side: Side): List | null {
    if (side === "staged") return "staged";
    const file = unstagedByPath.get(path);
    return !file || file.status === "conflicted" ? null : file.status === "untracked" ? "untracked" : "unstaged";
  }

  /** The files selected in each list, as in a Finder list: the one open, or those picked with
   *  ⌘-click or Shift-click, which the diff then shows together. */
  const picks = $derived.by((): Record<List, string[]> => {
    const none: Record<List, string[]> = { unstaged: [], staged: [], untracked: [] };
    if (lastOpen || !shown || shown.kind === "all") return none;
    if (shown.kind === "files") return { ...none, [shown.side === "staged" ? "staged" : shown.untracked ? "untracked" : "unstaged"]: shown.paths };
    const group = groupOf(shown.path, shown.side);
    return group ? { ...none, [group]: [shown.path] } : none;
  });
  /** The same picks as sets, so a list of thousands of files checks each one at once. */
  const pickSet = $derived({ unstaged: new Set(picks.unstaged), staged: new Set(picks.staged), untracked: new Set(picks.untracked) });

  /** Shows the files selected in a list: one file, several together, or the whole list once none is. */
  function showPicked(group: List, paths: string[]) {
    const side = sideOf(group);
    const fresh = group === "untracked" ? { untracked: true } : {};
    show(paths.length > 1 ? { kind: "files", paths, side, ...fresh } : paths.length ? { kind: "file", path: paths[0], side } : { kind: "all", side, ...fresh });
  }
  const isOpen = (file: FileChange, group: List | "conflicted") => !lastOpen && shown?.kind === "file" && shown.path === file.path && shown.side === sideOf(group);
  const isAll = (side: Side, fresh = false) => !lastOpen && shown?.kind === "all" && shown.side === side && !!shown.untracked === fresh;

  /** The side and file each hunk shown belongs to. */
  const hunkFile = $derived.by(() => {
    const map = new Map<Hunk, FileChange>();
    for (const d of diffs) for (const h of d.hunks) map.set(h, d.file);
    return map;
  });

  /** The file opened, picked or unpicked last, where Shift-click starts a range. */
  let anchor: { group: List; path: string } | null = null;

  /** The files a list shows, the filter applied. */
  const listOf = (group: List) => (group === "staged" ? shownStaged : group === "untracked" ? shownUntracked : shownTracked);

  /** The files of a list in the order they are shown, without those in closed folders. */
  function order(group: List | "conflicted"): string[] {
    const list = group === "conflicted" ? shownConflicted : listOf(group);
    if (!listView.folders) return list.map((f) => f.path);
    return byFolder(list).flatMap((g) => (folded.has(`${group}:${g.dir}`) ? [] : g.files.map((f) => f.path)));
  }

  /** A file's name selects it and opens its diff; ⌘-click (Ctrl) adds it to the selection or
   *  takes it out, Shift-click adds every file from the one clicked last. */
  function rowClick(event: MouseEvent, file: FileChange, group: List | "conflicted") {
    if (dragged) return;
    if (group !== "conflicted" && (event.metaKey || event.ctrlKey || event.shiftKey)) {
      event.preventDefault();
      const list = picks[group];
      if (event.shiftKey) return showPicked(group, pickRange(order(group), list, anchor?.group === group ? anchor.path : null, file.path));
      showPicked(group, pickSet[group].has(file.path) ? list.filter((p) => p !== file.path) : [...list, file.path]);
      anchor = { group, path: file.path };
      return;
    }
    show({ kind: "file", path: file.path, side: sideOf(group) });
  }

  /** Every file shown once, in the order of the lists, each side led by all of its files: the
   *  steps of Next Tab and Previous Tab. */
  const steps = $derived.by((): Shown[] => {
    const out: Shown[] = [];
    const add = (group: List | "conflicted") => out.push(...order(group).map((path): Shown => ({ kind: "file", path, side: sideOf(group) })));
    if (conflicted.length || tracked.length) out.push({ kind: "all", side: "unstaged" });
    add("conflicted");
    if (!shut.unstaged) add("unstaged");
    if (untracked.length) out.push({ kind: "all", side: "unstaged", untracked: true });
    if (untrackedOpen) add("untracked");
    if (staged.length) out.push({ kind: "all", side: "staged" });
    if (!shut.staged) add("staged");
    return out;
  });
  const stepper = $derived((by: 1 | -1, wrap: boolean) => {
    // From several files, the keys go on from the one clicked last.
    const from: Shown | null = shown?.kind === "files" && anchor ? { kind: "file", path: anchor.path, side: shown.side } : shown;
    const next = stepShown(steps, from, by, wrap);
    if (!next) return false;
    show(next);
    return true;
  });

  /** The files Stage & Next File goes through: the unstaged edits, then the new files. */
  const unstagedOrder = () => [...order("unstaged"), ...(untrackedOpen ? order("untracked") : [])];

  /** Stages the open unstaged file and opens the one after it; the next commit when none is left. */
  async function stageAndNext(file: FileChange) {
    const next = nextAfter(unstagedOrder(), file.path);
    if (!(await stage([file]))) return;
    show(next && next !== file.path ? { kind: "file", path: next, side: "unstaged" } : { kind: "all", side: "staged" });
  }

  /** Space: the picked files of a list, else the open file, else every file of the open side,
   *  go to the other side. */
  function stageOrUnstage() {
    for (const group of ["unstaged", "untracked", "staged"] as const) if (pickedFiles(group).length) return actOnPicked(group);
    if (shown?.kind === "all") {
      const files = shown.side === "staged" ? shownStaged : shown.untracked ? shownUntracked : shownTracked;
      return files.length && (shown.side === "staged" ? unstage(files) : stage(files));
    }
    if (shown?.kind !== "file") return;
    if (shown.side === "staged") return openStaged && unstage([openStaged]);
    if (openUnstaged && openUnstaged.status !== "conflicted") return stage([openUnstaged]);
  }

  /** The list's own keys, while a file or a heading of it has the focus: the filter field and
   *  the buttons keep theirs. */
  function onFilesKey(event: KeyboardEvent) {
    const target = event.target as HTMLElement;
    if (!target.closest(".name, .gname")) return;
    keysInList = true;
    const by = pressed("nextFile", event) ? 1 : pressed("previousFile", event) ? -1 : 0;
    if (by) {
      event.preventDefault();
      if (stepper(by, false)) requestAnimationFrame(() => filesBox?.querySelector<HTMLElement>('[aria-current="true"]')?.focus());
    } else if (pressed("stageAndNext", event)) {
      event.preventDefault();
      if (shown?.kind === "file" && shown.side === "unstaged" && openUnstaged && openUnstaged.status !== "conflicted") stageAndNext(openUnstaged);
    } else if (pressed("stageFile", event)) {
      event.preventDefault();
      stageOrUnstage();
    } else if (event.key === "Escape" && shown && shown.kind !== "all" && !lastOpen) {
      // As in a Finder list, the selection goes; its list is shown whole, with Stage All.
      event.preventDefault();
      const fresh = shown.kind === "files" ? !!shown.untracked : groupOf(shown.path, shown.side) === "untracked";
      show(fresh ? { kind: "all", side: shown.side, untracked: true } : { kind: "all", side: shown.side });
    }
  }

  /** Whether the list's keys were used: a file staged with them leaves its row, and the keys go
   *  on from the row of the file open next. */
  let keysInList = false;
  $effect(() => {
    shown;
    tree;
    tick().then(() => {
      const lost = !document.activeElement || document.activeElement === document.body;
      if (keysInList && lost) filesBox?.querySelector<HTMLElement>('[aria-current="true"]')?.focus({ preventScroll: true });
    });
  });

  /** The files being dragged to the other side, and where the pointer is. Pointer events, not the
   *  page's drag and drop, which the window's own file drop takes over on Windows. */
  let dragging = $state<{ group: List; files: FileChange[]; x: number; y: number } | null>(null);
  /** Whether the files would be dropped where the pointer is. */
  let dropping = $state(false);
  /** A drag ends with a click on the row it started from, which must not open the file. */
  let dragged = false;
  let listsBox = $state<HTMLDivElement>();
  let stagedBox = $state<HTMLDivElement>();

  function dragFile(event: PointerEvent, file: FileChange, group: List | "conflicted") {
    if (group === "conflicted" || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey) return;
    const from = { x: event.clientX, y: event.clientY };
    const files = () => (pickSet[group].has(file.path) ? pickedFiles(group) : [file]);
    const move = (e: PointerEvent) => {
      if (!dragging && Math.hypot(e.clientX - from.x, e.clientY - from.y) < 5) return;
      dragging = { group, files: dragging?.files ?? files(), x: e.clientX, y: e.clientY };
      const box = (group === "staged" ? listsBox : stagedBox)?.getBoundingClientRect();
      dropping = !!box && e.clientX >= box.left && e.clientX <= box.right && e.clientY >= box.top && e.clientY <= box.bottom;
    };
    const end = (drop: boolean) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("keydown", key, true);
      const done = dragging;
      dragging = null;
      const there = dropping;
      dropping = false;
      if (!done) return;
      dragged = true;
      setTimeout(() => (dragged = false));
      if (!drop || !there) return;
      if (done.group === "staged") unstage(done.files);
      else stage(done.files);
    };
    const up = () => end(true);
    const cancel = () => end(false);
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || !dragging) return;
      e.preventDefault();
      e.stopPropagation();
      end(false);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("keydown", key, true);
  }

  /** Only picked files that are listed: the filter can hide some. */
  const pickedFiles = (group: List) => listOf(group).filter((f) => pickSet[group].has(f.path));
  const pickedUnstaged = $derived(pickedFiles("unstaged"));
  const pickedUntracked = $derived(pickedFiles("untracked"));
  const pickedStaged = $derived(pickedFiles("staged"));
  /** The heading's button for the selected files of its list. */
  const pickedLabel = (verb: string, n: number) => (n === 1 ? `${verb} File` : `${verb} ${n} Files`);

  /** Changed lines, "+12 −3", or what the change is when it has none. */
  function counts(file: FileChange): string {
    if (file.binary) return "binary";
    if (file.uncounted) return "new";
    const parts = [file.additions ? `+${file.additions}` : "", file.deletions ? `−${file.deletions}` : ""].filter(Boolean);
    return parts.length ? parts.join(" ") : "no lines";
  }

  async function act(request: Request): Promise<boolean> {
    const done = await confirm.run(request);
    if (done) onChanged();
    return done;
  }

  /** Stage, Unstage or Discard the selected files of one list: once they leave it, the
   *  selection lets them go. */
  async function actOnPicked(group: List, how: "move" | "discard" = "move") {
    const files = pickedFiles(group);
    if (!files.length) return;
    await (how === "discard" ? discard(files) : group === "staged" ? unstage(files) : stage(files));
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
        : fresh.length
          ? ["Throws away your edits in ", ...name(files.filter((f) => !fresh.includes(f))), " and deletes ", ...name(fresh), ", which git does not track yet."]
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
      const f = hunkFile.get(hunk);
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
    for (const f of unstaged) seen.set(f.path, { path: f.path, untracked: f.status === "untracked", staged: stagedByPath.has(f.path), unstaged: true });
    for (const f of staged) if (!seen.has(f.path)) seen.set(f.path, { path: f.path, untracked: false, staged: true, unstaged: false });
    return [...seen.values()];
  }

  const modClick = navigator.platform.startsWith("Mac") ? "⌘-click" : "Ctrl-click";
  const revealLabel = navigator.platform.startsWith("Mac") ? "Reveal in Finder" : navigator.platform.startsWith("Win") ? "Show in Explorer" : "Open Containing Folder";

  /** Stage's menu on the bar of the open file: Stage & Next File first, as the way through a
   *  review, then Stage itself, and Discard the Rest for a file staged in part. */
  function stageMenu(event: MouseEvent, file: FileChange, label: string) {
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const entries: MenuEntry[] = [
      { kind: "item", label: "Stage & Next File", icon: menuIcons.skip, keys: shortcut("stageAndNext"), run: () => stageAndNext(file) },
      { kind: "item", label, icon: menuIcons.stage, keys: shortcut("stageFile"), run: () => stage([file]) },
    ];
    if (stagedByPath.has(file.path)) {
      entries.push({ kind: "sep" });
      entries.push({ kind: "item", label: "Discard the Rest…", icon: menuIcons.drop, danger: true, run: () => discard([file]) });
    }
    menu = { x: box.right - 240, y: box.bottom + 4, entries, label: "Stage menu" };
  }

  function fileMenu(event: MouseEvent, file: FileChange, group: List | "conflicted") {
    event.preventDefault();
    // A right-click on one of several picked files acts on all of them.
    const several = group !== "conflicted" && pickSet[group].has(file.path) ? pickedFiles(group) : [];
    if (several.length > 1 && group !== "conflicted") {
      const n = several.length;
      const fresh = several.every((f) => f.status === "untracked");
      const entries: MenuEntry[] = [{ kind: "header", label: `${n} selected files` }];
      if (group === "staged") entries.push({ kind: "item", label: `Unstage ${n} Files`, icon: menuIcons.unstage, run: () => actOnPicked(group) });
      else entries.push({ kind: "item", label: `Stage ${n} Files`, icon: menuIcons.stage, run: () => actOnPicked(group) });
      entries.push({ kind: "item", label: "Stash These Files…", icon: menuIcons.stash, run: () => act(stashFilesRequest(stashCandidates(), several.map((f) => f.path), stashMessageFor(several[0].path))) });
      entries.push({ kind: "sep" });
      entries.push({ kind: "item", label: "Copy Paths", icon: menuIcons.copy, run: () => navigator.clipboard.writeText(several.map((f) => f.path).join("\n")).then(() => confirm.say(`Copied ${n} paths.`, "done"), () => {}) });
      if (group !== "staged") {
        entries.push({ kind: "sep" });
        entries.push({ kind: "item", label: fresh ? `Delete ${n} Files…` : `Discard ${n} Files…`, icon: menuIcons.drop, danger: true, run: () => actOnPicked(group, "discard") });
      }
      menu = { x: event.clientX, y: event.clientY, entries, label: `${n} files menu` };
      return;
    }
    const side = group === "untracked" ? "unstaged" : group;
    const name = splitPath(file.path).name;
    const what = { modified: "modified", added: "new file", untracked: "untracked", deleted: "deleted", renamed: "renamed", copied: "copied", conflicted: "conflicted" }[file.status];
    const entries: MenuEntry[] = [{ kind: "header", label: `${name} · ${what}${side === "staged" ? ", staged" : ""}` }];
    if (side === "staged") entries.push({ kind: "item", label: "Unstage File", icon: menuIcons.unstage, run: () => unstage([file]) });
    else if (side === "unstaged") entries.push({ kind: "item", label: "Stage File", icon: menuIcons.stage, run: () => stage([file]) });
    if (side !== "conflicted")
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
    if (side === "unstaged") {
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

  /** The files of the commit Amend replaces, under Staged: they stay in it with the staged ones. */
  let lastFiles = $state<FileChange[]>([]);
  /** One of them, its diff in that commit shown instead of the working copy's. */
  let lastOpen = $state<string | null>(null);
  let lastDiffs = $state<FileDiff[]>([]);
  const amending = $derived(draft.amend && !!head && head.id === draft.amendOn ? head.id : null);
  $effect(() => {
    const id = amending;
    lastFiles = [];
    lastOpen = null;
    if (!id) return;
    let live = true;
    api.commitDetail(id, true).then(
      (detail) => {
        if (live) lastFiles = detail.files;
      },
      () => {},
    );
    return () => (live = false);
  });
  $effect(() => {
    const id = amending;
    const path = lastOpen;
    const wholeFile = whole;
    diffSettings();
    lastDiffs = [];
    if (!id || !path) return;
    let live = true;
    api.commitDiff(id, path, wholeFile).then(
      (d) => {
        if (live) lastDiffs = d;
      },
      (err) => {
        if (live && !stopped(err)) error = String(err);
      },
    );
    return () => (live = false);
  });
  function openLast(path: string) {
    if (lastOpen !== path && diffScroller) diffScroller.scrollTop = 0;
    lastOpen = path;
  }
  /** Amend's files get room of their own under the staged ones, up to three files. */
  const lastRoom = $derived.by(() => (lastFiles.length ? 24 + Math.min(lastFiles.length, 3) * ROW : 0));
  /** Files the amended commit will have: its own and the staged ones. */
  const amendedCount = $derived(new Set([...lastFiles, ...staged].map((f) => f.path)).size);

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

  function onCommitKey(event: KeyboardEvent) {
    if (pressed("commitNow", event)) {
      event.preventDefault();
      commit();
    }
  }

  /** Two columns, the files beside their diff, from this width of the panel; under it the files
   *  sit over the diff, with a border to drag between them. */
  const TWO_COLUMNS = 600;
  let bodyWidth = $state(0);
  let bodyHeight = $state(0);
  const stacked = $derived(bodyWidth > 0 && bodyWidth < TWO_COLUMNS);
  /** The most height the staged files take before they scroll: four rows at first, then as the
   *  border over them is dragged. Kept for every window and repository. */
  const STAGED_KEY = "oxbow.changes.stagedHeight";
  const ROW = 28;
  let stagedHeight = $state(readNumber(STAGED_KEY, 4 * ROW));
  /** In one column, the share of the height the files take over the diff. */
  const SHARE_KEY = "oxbow.changes.filesShare";
  let filesShare = $state(readNumber(SHARE_KEY, 0.6));

  function readNumber(key: string, fallback: number): number {
    try {
      const value = Number(localStorage.getItem(key));
      return localStorage.getItem(key) !== null && Number.isFinite(value) && value > 0 ? value : fallback;
    } catch {
      return fallback;
    }
  }

  function keepNumber(key: string, value: number) {
    try {
      localStorage.setItem(key, String(value));
    } catch {
      // Lasts until the app quits.
    }
  }

  /** A border dragged up or down: `onMove` gets how far the pointer went, and the value is kept once it is let go. */
  function drag(event: PointerEvent, onMove: (dy: number) => void, onDone: () => void) {
    const startY = event.clientY;
    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);
    const move = (e: PointerEvent) => onMove(e.clientY - startY);
    const up = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
      onDone();
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  }

  function dragStaged(event: PointerEvent) {
    const from = stagedHeight;
    drag(
      event,
      (dy) => (stagedHeight = Math.min(Math.max(from - dy, ROW), 20 * ROW)),
      () => keepNumber(STAGED_KEY, Math.round(stagedHeight)),
    );
  }

  function dragShare(event: PointerEvent) {
    const from = filesShare;
    const height = bodyHeight || 1;
    drag(
      event,
      (dy) => (filesShare = Math.min(Math.max(from + dy / height, 0.2), 0.8)),
      () => keepNumber(SHARE_KEY, Math.round(filesShare * 1000) / 1000),
    );
  }

  /** The part of the open file staged in part that the other side holds. */
  const openFile = $derived(shown?.kind === "file" ? shown : null);
  const openUnstaged = $derived(openFile ? ([...conflicted, ...unstaged].find((f) => f.path === openFile.path) ?? null) : null);
  const openStaged = $derived(openFile ? (stagedByPath.get(openFile.path) ?? null) : null);
  /** Files of every file of a side shown, with their lines, for the heading over them. */
  const sideFiles = $derived.by(() => {
    if (shown?.kind !== "all" && shown?.kind !== "files") return [];
    if (shown.kind === "files") {
      const chosen = new Set(shown.paths);
      return (shown.side === "staged" ? staged : shown.untracked ? untracked : tracked).filter((f) => chosen.has(f.path));
    }
    return shown.side === "staged" ? shownStaged : shown.untracked ? shownUntracked : [...shownConflicted, ...shownTracked];
  });
  const sideTotals = $derived(sideFiles.reduce((sum, f) => ({ add: sum.add + f.additions, del: sum.del + f.deletions }), { add: 0, del: 0 }));

  /** What the open file's side holds, under the switch. */
  function caption(file: Shown & { kind: "file" }): string {
    if (file.side === "staged") return "Goes into the commit";
    const f = openUnstaged;
    if (f?.status === "conflicted") return "Has conflicts to resolve";
    if (f?.status === "untracked") return "New file git does not track yet";
    return "Stays out of the commit";
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

{#snippet fileRow(file: FileChange, group: List | "conflicted", nested: boolean)}
  {@const b = badge[file.status]}
  {@const picked = group !== "conflicted" && pickSet[group].has(file.path)}
  {@const open = isOpen(file, group)}
  <!-- The part of a file staged in part that the other list holds. -->
  {@const other = group === "staged" ? unstagedByPath.get(file.path) : group === "unstaged" ? stagedByPath.get(file.path) : undefined}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="file"
    class:open
    class:nested
    class:bin={group === "unstaged" || group === "untracked"}
    class:moving={dragging?.group === group && dragging.files.includes(file)}
    style:background={open || picked ? tint(color) : undefined}
    oncontextmenu={(e) => fileMenu(e, file, group)}
  >
    <button
      class="name"
      onclick={(e) => rowClick(e, file, group)}
      onpointerdown={(e) => dragFile(e, file, group)}
      aria-current={open ? "true" : undefined}
      title={dragging ? undefined : `${file.oldPath ? `${file.oldPath} → ${file.path}` : file.path}${group === "conflicted" ? "" : ` · ${modClick} or Shift-click to select more, drag to ${group === "staged" ? "unstage" : "stage"}`}`}
    >
      <span class="badge {b.tone}" title={b.title}>{b.text}</span>
      <PathName path={file.path} folders={!nested} />
      {#if other}
        <span class="note partial" title={group === "staged" ? `Staged in part: ${counts(file)} go into the commit, ${counts(other)} stay out` : `Staged in part: ${counts(other)} go into the commit, ${counts(file)} stay out`}>
          <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="5.5" /><path class="fill" d="M8 2.5a5.5 5.5 0 0 0 0 11z" /></svg>Partial
        </span>
      {/if}
      {#if file.submodule}
        <span class="note" title="Another repository in a folder here">Submodule{file.submodule.changed && !file.submodule.moved ? " · changed inside" : ""}</span>
      {:else if file.lfs}
        <span class="note" title="Kept in Git LFS: git stores a small pointer">LFS{file.newSize !== undefined ? ` · ${bytes(file.newSize)}` : ""}</span>
      {:else if (file.newSize ?? 0) >= BIG_FILE}
        <span class="note warn" title="Too big for a git repository: GitHub refuses files over 100 MB">{bytes(file.newSize!)}</span>
      {:else if !file.binary && !file.uncounted && file.status !== "conflicted"}
        <span class="counts mono"><span class="add">+{file.additions}</span> <span class="del">−{file.deletions}</span></span>
      {/if}
    </button>
    {#if group === "unstaged" || group === "untracked"}
      <button class="discard" onclick={() => discard([file])} aria-label="{file.status === 'untracked' ? 'Delete' : 'Discard changes in'} {file.path}" title={file.status === "untracked" ? "Delete this new file" : "Discard changes"}>
        <svg class="icon" viewBox="0 0 16 16"><path d="M2.5 4.5h11M6 4.5V3a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1v1.5M4 4.5l.7 9a1 1 0 0 0 1 .9h4.6a1 1 0 0 0 1-.9l.7-9" /></svg>
      </button>
    {/if}
  </div>
{/snippet}

{#snippet fileList(files: FileChange[], group: List | "conflicted")}
  {@const limit = listed[group]}
  {#if listView.folders}
    {@const cut = foldersWithin(byFolder(files), group, limit)}
    {#each cut.folders as { folder, room } (folder.dir)}
      {@const key = `${group}:${folder.dir}`}
      {@const open = !folded.has(key)}
      {#if room > 0}
      <div class="folder">
        <button class="fold" aria-expanded={open} onclick={() => fold(key)} title={folder.dir || "The repository's top folder"}>
          <svg class="chevron" class:open viewBox="0 0 16 16"><path d="M6 4l4 4-4 4" /></svg>
          <svg class="icon folder-icon" viewBox="0 0 16 16"><path d="M1.5 4.5a1 1 0 0 1 1-1H6l1.5 1.5h6a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" /></svg>
          <span class="dirname">{folder.dir ? folder.dir.slice(0, -1) : "Top folder"}</span>
          <span class="n">{folder.files.length}</span>
        </button>
      </div>
      {#if open}{#each folder.files.slice(0, room) as file (file.path)}{@render fileRow(file, group, true)}{/each}{/if}
      {/if}
    {/each}
    {#if cut.hidden > 0}{@render moreRows(group, cut.hidden)}{/if}
  {:else}
    {#each files.slice(0, limit) as file (file.path)}{@render fileRow(file, group, false)}{/each}
    {#if files.length > limit}{@render moreRows(group, files.length - limit)}{/if}
  {/if}
{/snippet}

{#snippet dropRow(label: string)}
  <div class="drop-row" aria-hidden="true">
    <svg class="icon" viewBox="0 0 16 16"><path d="M8 3v9M4.5 8.5 8 12l3.5-3.5" /></svg>{label}
  </div>
{/snippet}

{#snippet moreRows(group: List | "conflicted", left: number)}
  <button class="more-rows" onclick={() => (listed = { ...listed, [group]: listed[group] + LIST_STEP })}
    >Show {Math.min(left, LIST_STEP).toLocaleString()} more {Math.min(left, LIST_STEP) === 1 ? "file" : "files"}</button
  >
{/snippet}

<!-- A list's heading: the chevron folds it, the name shows every file of the side. -->
{#snippet chevron(open: boolean, label: string, toggle: () => void, disabled = false)}
  <button class="chev" aria-expanded={open} aria-label={label} title={disabled ? "Shown while the filter is on" : label} {disabled} onclick={toggle}>
    <svg class="chevron" class:open viewBox="0 0 16 16"><path d="M6 4l4 4-4 4" /></svg>
  </button>
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
          <button class="button mini text" onclick={() => linesAction("discardHunk", d.file.path, hunk)}>Discard {n} {lineWord}</button>
          <button class="button mini quiet" onclick={() => linesAction("stageHunk", d.file.path, hunk)}>Stage {n} {lineWord}</button>
        {:else}
          <button class="button mini text" onclick={() => linesAction("unstageHunk", d.file.path, hunk)}>Unstage {n} {lineWord}</button>
        {/if}
      {:else}
        <span class="state" class:staged={side === "staged"}>{side === "staged" ? "Staged" : "Unstaged"}</span>
        {#if side === "unstaged"}
          <button class="button mini text" onclick={() => hunkAction("discardHunk", d.file.path, hunk)}>Discard</button>
          <button class="button mini quiet" onclick={() => hunkAction("stageHunk", d.file.path, hunk)}>Stage Hunk</button>
        {:else}
          <button class="button mini text" onclick={() => hunkAction("unstageHunk", d.file.path, hunk)}>Unstage Hunk</button>
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
      {#if move.old}<button class="button mini text" onclick={() => act(updateFolderRequest(move))} title="Check out the commit recorded here again">Update to {move.old.slice(0, 7)}</button>{/if}
      <button class="button mini quiet" onclick={() => stage([d.file])} title="Record the commit it is at in the next commit">Stage</button>
    {:else}
      <button class="button mini text" onclick={() => api.openWindow(d.file.path).catch((err) => confirm.fail(err))} title="Its changes are committed in it">Open in New Window</button>
    {/if}
  {:else if d.file.status === "conflicted"}
    {#if onResolve}<button class="button mini text" onclick={onResolve}>Resolve Conflicts…</button>{/if}
  {:else}
    {@const rest = stagedByPath.has(d.file.path)}
    {@const label = rest ? "Stage the Rest" : "Stage"}
    {@const keys = shortcut("stageFile")}
    <!-- A file staged in part has Discard the Rest in the Stage menu: Discard beside Stage the Rest would read as the whole file. -->
    {#if !rest || diffsOf?.kind !== "file"}
      <button class="button mini text" onclick={() => discard([d.file])} title={d.file.status === "untracked" ? "Delete this new file" : rest ? "Throw away the edits not staged; the staged part stays" : "Throw away the edits in this file"}>Discard</button>
    {/if}
    {#if diffsOf?.kind === "file"}
      <span class="split">
        <button class="button mini quiet" onclick={() => stage([d.file])} title="{rest ? 'Add the rest of the file to the next commit' : 'Add the whole file to the next commit'}{keys ? ` (${keys})` : ''}">{label}</button>
        <button class="button mini quiet more" onclick={(e) => stageMenu(e, d.file, label)} aria-label="More ways to stage" aria-haspopup="menu" title="More ways to stage">
          <svg class="icon" viewBox="0 0 16 16"><path d="M4.5 6.5 8 10l3.5-3.5" /></svg>
        </button>
      </span>
    {:else}
      <button class="button mini quiet" onclick={() => stage([d.file])} title={rest ? "Add the rest of the file to the next commit" : "Add the whole file to the next commit"}>{label}</button>
    {/if}
  {/if}
{/snippet}
{#snippet stagedFile(d: FileDiff)}
  <button class="button mini text" onclick={() => unstage([d.file])} title="Take the whole file out of the next commit">Unstage</button>
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
      {fileCount === 1 ? "file" : "files"}{#if conflicted.length} · {conflicted.length} conflicted{/if} · {staged.length} staged · {tracked.length} unstaged{#if untracked.length}{` · ${untracked.length} untracked`}{/if}
    </span>
  </div>

  <div class="body" class:stacked bind:clientWidth={bodyWidth} bind:clientHeight={bodyHeight}>
    <div class="files" bind:this={filesBox} aria-label="Working copy" data-steps use:tabStepper={stepper} onkeydown={onFilesKey} class:dragging={!!dragging} style:height={stacked ? `${Math.round(filesShare * 100)}%` : null} style:width={stacked ? null : `${filesWidth(bodyWidth)}px`}>
      <div class="tools">
        <label class="filter">
          <svg class="icon" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5" /><path d="m10.5 10.5 3 3" /></svg>
          <input
            bind:value={filter}
            placeholder="Filter files"
            aria-label="Filter files"
            spellcheck="false"
            autocomplete="off"
            onkeydown={(e) => {
              if (e.key === "Escape" && filter) {
                e.stopPropagation();
                filter = "";
              }
            }}
          />
          {#if filter}<button class="clear" onclick={() => (filter = "")} aria-label="Clear the filter" title="Clear the filter">×</button>{/if}
        </label>
        <div class="segmented icons" role="radiogroup" aria-label="Show files">
          <button role="radio" aria-checked={!listView.folders} class:on={!listView.folders} onclick={() => setFolders(false)} aria-label="List" title="One list of paths">
            <svg class="icon" viewBox="0 0 16 16"><path d="M5.5 4h8M5.5 8h8M5.5 12h8M2.5 4h.01M2.5 8h.01M2.5 12h.01" /></svg>
          </button>
          <button role="radio" aria-checked={listView.folders} class:on={listView.folders} onclick={() => setFolders(true)} aria-label="Folders" title="Files under their folders">
            <svg class="icon" viewBox="0 0 16 16"><path d="M1.5 4.5a1 1 0 0 1 1-1H6l1.5 1.5h6a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" /></svg>
          </button>
        </div>
      </div>

      <div class="lists" class:drop={dragging?.group === "staged"} class:over={dragging?.group === "staged" && dropping} bind:this={listsBox}>
        {#if dragging?.group === "staged"}{@render dropRow("Drop to Unstage")}{/if}
        {#if shownConflicted.length}
          <div class="group">
            <div class="ghead"><span class="gtitle conflicted">Conflicted</span><span class="n">{shownConflicted.length}</span></div>
            {@render fileList(shownConflicted, "conflicted")}
          </div>
        {/if}
        <div class="group">
          <div class="ghead" class:on={isAll("unstaged")} style:background={isAll("unstaged") ? tint(color) : undefined}>
            {@render chevron(!shut.unstaged, shut.unstaged ? "Show the unstaged files" : "Hide the unstaged files", () => (shut.unstaged = !shut.unstaged))}
            <button class="gname" onclick={() => show({ kind: "all", side: "unstaged" })} aria-current={isAll("unstaged") ? "true" : undefined} title="Show every unstaged change">
              <span class="gtitle" style:color={plate(color)}>Unstaged</span><span class="n">{needle ? `${shownTracked.length} of ${tracked.length}` : tracked.length}</span>
            </button>
            {#if pickedUnstaged.length}
              <button class="button mini primary" onclick={() => actOnPicked("unstaged")}>{pickedLabel("Stage", pickedUnstaged.length)}</button>
            {:else if shownTracked.length}
              <button class="button mini" onclick={() => stage(shownTracked)}>{needle && shownTracked.length < tracked.length ? "Stage Shown" : "Stage All"}</button>
            {/if}
          </div>
          {#if !shut.unstaged}
            {@render fileList(shownTracked, "unstaged")}
            {#if !shownTracked.length}
              <p class="empty">{needle && tracked.length ? "No unstaged files match the filter." : untracked.length ? "No edits to tracked files." : "Nothing left to stage."}</p>
            {/if}
          {/if}
        </div>
        {#if untracked.length}
          <!-- New files git does not track yet, apart from the edits and folded at first, as in Sublime Merge. -->
          <div class="group">
            <div class="ghead" class:on={isAll("unstaged", true)} style:background={isAll("unstaged", true) ? tint(color) : undefined}>
              {@render chevron(untrackedOpen, untrackedOpen ? "Hide the untracked files" : "Show the new files git does not track yet", () => (view.untracked = !view.untracked), !!needle)}
              <button class="gname" onclick={() => show({ kind: "all", side: "unstaged", untracked: true })} aria-current={isAll("unstaged", true) ? "true" : undefined} title="Show every new file git does not track yet">
                <span class="gtitle" style:color={plate(color)}>Untracked</span><span class="n">{needle ? `${shownUntracked.length} of ${untracked.length}` : untracked.length}</span>
              </button>
              {#if pickedUntracked.length}
                <button class="button mini primary" onclick={() => actOnPicked("untracked")}>{pickedLabel("Stage", pickedUntracked.length)}</button>
              {:else if shownUntracked.length}
                <button class="button mini" onclick={() => stage(shownUntracked)}>{needle && shownUntracked.length < untracked.length ? "Stage Shown" : "Stage All"}</button>
              {/if}
            </div>
            {#if untrackedOpen}
              {@render fileList(shownUntracked, "untracked")}
              {#if !shownUntracked.length}<p class="empty">No untracked files match the filter.</p>{/if}
            {/if}
          </div>
        {/if}
      </div>

      <!-- The staged files and the commit, held at the bottom; the border over them sets how tall the list gets. -->
      {#if !stacked}<button class="grip" tabindex="-1" onpointerdown={dragStaged} aria-label="Resize the staged files" title="Drag to resize"></button>{/if}
      <div class="group staged" class:drop={!!dragging && dragging.group !== "staged"} class:over={!!dragging && dragging.group !== "staged" && dropping} bind:this={stagedBox}>
        <div class="ghead" class:on={isAll("staged")} style:background={isAll("staged") ? tint(color) : undefined}>
          {@render chevron(!shut.staged, shut.staged ? "Show the staged files" : "Hide the staged files", () => (shut.staged = !shut.staged))}
          <button class="gname" onclick={() => show({ kind: "all", side: "staged" })} aria-current={isAll("staged") ? "true" : undefined} title="Show the next commit: every staged change">
            <span class="gtitle" style:color={plate(color)}>Staged</span><span class="n">{needle ? `${shownStaged.length} of ${staged.length}` : staged.length}</span>
          </button>
          {#if pickedStaged.length}
            <button class="button mini primary" onclick={() => actOnPicked("staged")}>{pickedLabel("Unstage", pickedStaged.length)}</button>
          {:else if shownStaged.length}
            <button class="button mini" onclick={() => unstage(shownStaged)}>{needle && shownStaged.length < staged.length ? "Unstage Shown" : "Unstage All"}</button>
          {/if}
        </div>
        {#if dragging && dragging.group !== "staged"}{@render dropRow("Drop to Stage")}{/if}
        {#if !shut.staged}
          <div class="staged-files" style:max-height={stacked ? null : `${stagedHeight + lastRoom}px`}>
            {@render fileList(shownStaged, "staged")}
            {#if !shownStaged.length && !lastFiles.length}
              <p class="empty">{needle && staged.length ? "No staged files match the filter." : "Stage files or lines to add them to the next commit."}</p>
            {/if}
            {#if lastFiles.length}
              <!-- Amend: the commit it replaces keeps its files, and the staged ones join them. -->
              <div class="from-last" title="Files of the commit Amend replaces: they stay in it, with the staged changes added">
                <span class="grow">From last commit</span><span class="n">{lastFiles.length}</span>
              </div>
              {#each lastFiles.filter(matches) as file (file.path)}
                {@const b = badge[file.status]}
                {@const open = lastOpen === file.path}
                <div class="file" class:open style:background={open ? tint(color) : undefined}>
                  <button class="name" onclick={() => openLast(file.path)} aria-current={open ? "true" : undefined} title="{file.path} · its changes in the last commit">
                    <span class="badge {b.tone}" title={b.title}>{b.text}</span>
                    <PathName path={file.path} />
                  </button>
                </div>
              {/each}
            {/if}
          </div>
        {/if}
      </div>

      <div class="commit" role="group" aria-label="Commit" style:background={tint(color)} style:border-color={`color-mix(in srgb, ${lane(color)} 40%, transparent)`}>
        <div class="row">
          <span class="grow to">Commit to <b style:color={plate(color)}>{branch ?? "detached HEAD"}</b></span>
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
          rows="1"
          class:folded={descFolded}
          bind:this={descBox}
          bind:value={draft.description}
          onkeydown={onCommitKey}
          onfocus={() => (descOpen = true)}
          onblur={() => (descOpen = false)}
        ></textarea>
        {#if commitHint}<WhyNote text={commitHint.text} warn={commitHint.warn} />{/if}
        <div class="row actions">
          {#if commitHint?.fix}
            <button class="button mini" onclick={commitHint.fix.run}>{commitHint.fix.label}</button>
          {:else if !commitHint}
            <span class="staged-count"
              >{draft.amend
                ? staged.length
                  ? `${amendedCount} ${amendedCount === 1 ? "file" : "files"}`
                  : "Message only"
                : `${staged.length} ${staged.length === 1 ? "file" : "files"} staged`}</span
            >
          {/if}
          <span class="grow"></span>
          <button class="button primary go" onclick={commit} disabled={!canCommit} title={commitHint?.text}>
            {draft.amend ? "Amend" : "Commit"} {#if shortcut("commitNow")}<span class="keys">{shortcut("commitNow")}</span>{/if}
          </button>
        </div>
      </div>
    </div>

    {#if stacked}
      <button class="grip across" tabindex="-1" onpointerdown={dragShare} aria-label="Resize the files and the diff" title="Drag to resize"></button>
    {/if}

    {#if !stacked}<button class="edge" tabindex="-1" style:left={`${filesWidth(bodyWidth) - 4}px`} onpointerdown={(e) => resizeFiles(e, bodyWidth)} aria-label="Resize the files" title="Drag to resize"></button>{/if}
    <div class="diff" aria-label="Diff">
      {#if lastOpen}
        <div class="dhead">
          <div class="grow">
            <div class="dtitle"><b>From last commit</b></div>
            <div class="caption">Amend keeps these changes in the commit</div>
          </div>
          <DiffTools {whole} onToggleWhole={() => (whole = !whole)} working />
        </div>
      {:else if shown}
        <div class="dhead">
          {#if shown.kind === "file"}
            {@const file = shown}
            <div class="segmented" role="radiogroup" aria-label="Which part of the file">
              <button role="radio" aria-checked={file.side === "unstaged"} class:on={file.side === "unstaged"} disabled={!openUnstaged} onclick={() => show({ ...file, side: "unstaged" })} title={openUnstaged ? "What stays out of the commit" : "Nothing of this file is left out of the commit"}>Unstaged</button>
              <button role="radio" aria-checked={file.side === "staged"} class:on={file.side === "staged"} disabled={!openStaged} onclick={() => show({ ...file, side: "staged" })} title={openStaged ? "What goes into the commit" : "Nothing of this file is staged yet"}>Staged</button>
            </div>
            <span class="caption grow">{caption(file)}</span>
          {:else}
            <div class="grow">
              <div class="dtitle">
                <b>{shown.kind === "files" ? "Selected files" : shown.side === "staged" ? "Next commit" : shown.untracked ? "Untracked files" : "Unstaged changes"}</b>
                <span class="n">{sideFiles.length} {sideFiles.length === 1 ? "file" : "files"}</span>
                {#if sideFiles.length}<span class="mono add">+{sideTotals.add}</span><span class="mono del">−{sideTotals.del}</span>{/if}
              </div>
              <div class="caption">{shown.side === "staged" ? "Only staged changes go into the commit" : shown.untracked ? "New files git does not track yet stay out of the commit until staged" : "They stay out of the commit until staged"}</div>
            </div>
            {#if diffs.length > 1}<FoldAll onFold={(fold) => diffView?.setAll(fold)} />{/if}
          {/if}
          <DiffTools {whole} onToggleWhole={() => (whole = !whole)} working />
        </div>
      {/if}
      <div class="scroll" bind:this={diffScroller}>
        {#if error}<p class="error" role="alert">{error}</p>{/if}
        {#if openFile && openUnstaged && openStaged && !lastOpen}<div class="tip"><TipCard id="partlyStaged" /></div>{/if}
        {#if lastOpen}
          <DiffView diffs={lastDiffs} {color} {whole} onToggleWhole={() => (whole = !whole)} tools={false} />
        {:else if !shown}
          <p class="nothing">No uncommitted changes.</p>
        {:else if shown.kind === "all" && !sideFiles.length && !needle}
          <!-- Nothing left on this side: a short line with the way on, not a big empty screen. -->
          <div class="done-strip">
            <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" /><path d="M5.5 8.2 7.2 10l3.3-3.8" /></svg>
            {#if shown.untracked}
              <span class="grow">{staged.length ? "All new files staged" : "No untracked files"}</span>
              {#if tracked.length || conflicted.length}<button class="button mini quiet" onclick={() => show({ kind: "all", side: "unstaged" })}>Review Unstaged</button>{/if}
              {#if staged.length}<button class="button mini quiet" onclick={() => show({ kind: "all", side: "staged" })}>Review Staged</button>{/if}
            {:else if shown.side === "unstaged"}
              <span class="grow">{staged.length ? "All changes staged" : "No edits to tracked files"}</span>
              {#if untracked.length}<button class="button mini" onclick={() => show({ kind: "file", path: untracked[0].path, side: "unstaged" })}>Next Unstaged File</button>{/if}
              {#if staged.length}<button class="button mini quiet" onclick={() => show({ kind: "all", side: "staged" })}>Review Staged</button>{/if}
            {:else}
              <span class="grow">Nothing staged yet</span>
              {#if tracked.length || conflicted.length}<button class="button mini quiet" onclick={() => show({ kind: "all", side: "unstaged" })}>Review Unstaged</button>{/if}
            {/if}
          </div>
        {:else if diffsOf}
          {@const side = diffsOf.side}
          <DiffView
            bind:this={diffView}
            foldable={diffsOf.kind !== "file"}
            {diffs}
            {color}
            {whole}
            onToggleWhole={() => (whole = !whole)}
            hunkBar={side === "staged" ? stagedHunk : unstagedHunk}
            fileActions={side === "staged" ? stagedFile : unstagedFile}
            {picker}
            pending={diffsOf.kind !== "file" ? pending : 0}
            onMore={loadMore}
            loadFull={(d) => api.workingDiff(d.file.path, side, whole, true)}
            command={(d) => (side === "staged" ? `git diff --cached -- ${d.file.path}` : `git diff -- ${d.file.path}`)}
            working
            {side}
            tools={false}
          />
        {/if}
      </div>
    </div>
  </div>
</div>

{#if dragging}
  {@const one = dragging.files.length === 1 ? splitPath(dragging.files[0].path) : null}
  <div class="ghost" aria-hidden="true" style:left="{dragging.x + 12}px" style:top="{dragging.y - 14}px">
    {#if one}
      {@const b = badge[dragging.files[0].status]}
      <span class="badge {b.tone}">{b.text}</span><span class="path"><span class="dir">{one.dir}</span>{one.name}</span>
    {:else}
      <span class="path">{dragging.files.length} files</span>
    {/if}
  </div>
{/if}

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
  /* No frame: the head sits on the panel itself, like the commit details. */
  .head {
    padding: var(--space-4) var(--space-5) var(--space-3);
    display: flex;
    flex-direction: column;
    gap: 4px;
    border-bottom: 1px solid var(--sep);
  }
  h1 {
    margin: 0;
    font-size: var(--text-title);
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
    border-radius: var(--radius-pill);
    font-size: var(--text-small);
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
    font-size: var(--text-caption);
    color: var(--text2);
  }

  /* The files beside their diff; in a narrow panel, over it. */
  .body {
    position: relative;
    flex-grow: 1;
    min-height: 0;
    display: flex;
  }
  .body.stacked {
    flex-direction: column;
  }
  .files {
    flex-shrink: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--sep);
  }
  /* Over the diff there is no room for lists that scroll apart: the files and the commit form
     scroll as one. */
  .stacked .files {
    width: auto;
    border-right: 0;
    overflow-y: auto;
  }
  .stacked .lists,
  .stacked .group.staged {
    flex: none;
    min-height: 0;
    overflow: visible;
  }
  /* The border to drag between the files and the diff, like the graph's. */
  .edge {
    position: absolute;
    top: 50%;
    width: 7px;
    height: 36px;
    margin-top: -18px;
    border-radius: var(--radius-xs);
    background: var(--win);
    border: 1px solid var(--glass-border);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.08);
    z-index: 2;
    cursor: col-resize;
  }
  .diff {
    flex-grow: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .stacked .diff {
    border-top: 1px solid var(--sep);
  }
  .lists {
    flex-grow: 1;
    min-height: 84px;
    overflow-y: auto;
    padding: 0 var(--space-2) var(--space-1);
  }
  .group {
    display: flex;
    flex-direction: column;
  }
  .group + .group {
    margin-top: var(--space-2);
  }
  /* In a short window the staged files give way first; the commit form always shows whole. */
  .group.staged {
    flex-shrink: 1;
    min-height: 30px;
    padding: 0 var(--space-2);
  }
  .staged-files {
    min-height: 26px;
    overflow-y: auto;
  }
  /* Where dragged files go: the other list, ringed in Steel, with a line saying what a drop does. */
  .drop {
    outline: 1.5px dashed var(--accent);
    outline-offset: -1px;
    border-radius: var(--radius-m);
  }
  .drop.over {
    background: var(--accent-quiet);
  }
  .drop-row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
    height: 28px;
    padding: 0 var(--space-3);
    font-size: var(--text-small);
    font-weight: 600;
    color: var(--accent-ink);
  }
  .drop-row .icon {
    width: 14px;
    height: 14px;
  }
  .files.dragging {
    cursor: grabbing;
    user-select: none;
  }
  .files.dragging .name {
    cursor: grabbing;
  }
  .file.moving {
    opacity: 0.4;
  }
  .ghost {
    position: fixed;
    z-index: 50;
    display: flex;
    align-items: center;
    gap: 8px;
    max-width: 280px;
    height: 28px;
    padding: 0 14px 0 10px;
    border-radius: var(--radius-m);
    background: var(--sheet);
    box-shadow: var(--shadow-pop);
    font-size: var(--text-body);
    color: var(--text);
    pointer-events: none;
  }
  /* Amend: the files the commit has already, under the staged ones. */
  .from-last {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    margin-top: 2px;
    padding: 0 var(--space-2) 0 22px;
    font-size: var(--text-caption);
    color: var(--text2);
  }
  /* Stage with its menu on the bar of the open file: one capsule cut in two. */
  .split {
    display: inline-flex;
    gap: 1px;
    flex-shrink: 0;
  }
  .split .button:first-child {
    border-radius: var(--radius-pill) 0 0 var(--radius-pill);
    padding-right: 8px;
  }
  .split .more {
    width: 22px;
    padding: 0;
    border-radius: 0 var(--radius-pill) var(--radius-pill) 0;
  }
  .split .more .icon {
    width: 11px;
    height: 11px;
    stroke-width: 1.8;
  }
  /* A border to drag: over the staged files, and between the files and the diff in one column. */
  .grip {
    flex-shrink: 0;
    height: 9px;
    margin: 0 var(--space-3);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: row-resize;
  }
  .grip::after {
    content: "";
    width: 28px;
    height: 3px;
    border-radius: var(--radius-pill);
    background: var(--sep);
  }
  .grip:hover::after {
    background: var(--text2);
  }
  .grip.across {
    margin: 0;
  }
  /* Each list's heading reads as a section title, as in Finder's sidebar, not as one more row:
     small capitals in the branch color, with the files under it. */
  .ghead {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 30px;
    padding: 0 var(--space-1) 0 2px;
    border-radius: var(--radius-s);
    font-size: var(--text-caption);
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text2);
    flex-shrink: 0;
  }
  .gname {
    flex-grow: 1;
    min-width: 0;
    height: 100%;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 4px;
    font: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    color: inherit;
    text-align: left;
  }
  .gname:disabled {
    opacity: 1;
    cursor: default;
  }
  .chev {
    width: 18px;
    height: 22px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text2);
    flex-shrink: 0;
  }
  .chev:disabled {
    opacity: 1;
    cursor: default;
  }
  .gtitle {
    font-weight: 700;
  }
  .gtitle.conflicted {
    color: var(--red);
    padding-left: 22px;
  }
  .ghead .n,
  .ghead .button {
    letter-spacing: normal;
    text-transform: none;
  }
  .ghead .button {
    flex-shrink: 0;
    white-space: nowrap;
  }
  .n {
    font-weight: 400;
  }
  .grow {
    flex-grow: 1;
    min-width: 0;
  }
  .empty {
    margin: 0;
    padding: var(--space-1) var(--space-3) var(--space-1) 26px;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .more-rows {
    display: block;
    width: 100%;
    height: 26px;
    border-radius: var(--radius-s);
    color: var(--text2);
    font-size: var(--text-small);
    text-align: center;
  }
  .more-rows:hover {
    background: var(--field);
  }
  .file {
    position: relative;
    display: flex;
    align-items: center;
    height: 28px;
    padding: 0 var(--space-2) 0 22px;
    border-radius: var(--radius-s);
  }
  .file:hover {
    background: var(--field);
  }
  .file.nested {
    padding-left: 44px;
  }
  /* Filter and List / Folders over the lists. */
  .tools {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: var(--space-3) var(--space-3) var(--space-1);
    flex-shrink: 0;
  }
  .filter {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-grow: 1;
    min-width: 0;
    height: 26px;
    padding: 0 6px 0 10px;
    border-radius: var(--radius-pill);
    background: var(--field);
    color: var(--text2);
  }
  .filter:focus-within {
    box-shadow: var(--focus-ring);
  }
  .filter .icon {
    width: 13px;
    height: 13px;
    flex-shrink: 0;
  }
  .filter input {
    flex-grow: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    font: inherit;
    font-size: var(--text-small);
    color: var(--text);
    user-select: text;
    -webkit-user-select: text;
    box-shadow: none;
  }
  .clear {
    width: 18px;
    height: 18px;
    border-radius: var(--radius-pill);
    color: var(--text2);
    line-height: 1;
    flex-shrink: 0;
  }
  .clear:hover {
    background: var(--side-sel);
    color: var(--text);
  }
  .segmented {
    flex-shrink: 0;
    display: flex;
    padding: 2px;
    border-radius: var(--radius-m);
    background: var(--field);
  }
  .segmented button {
    height: 22px;
    min-width: 58px;
    padding: 0 10px;
    border-radius: var(--radius-s);
    font-size: var(--text-small);
    font-weight: 500;
    color: var(--text2);
    white-space: nowrap;
  }
  .segmented button.on {
    background: var(--sheet);
    color: var(--text);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
  }
  .segmented button:disabled:not(.on) {
    opacity: 0.45;
    cursor: default;
  }
  .segmented.icons button {
    min-width: 0;
    width: 28px;
    padding: 0;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .segmented.icons .icon {
    width: 14px;
    height: 14px;
  }
  /* A folder over its files: fold them away. */
  .folder {
    display: flex;
    align-items: center;
    height: 26px;
    padding: 0 var(--space-1) 0 22px;
    border-radius: var(--radius-s);
  }
  .folder:hover {
    background: var(--field);
  }
  .fold {
    flex-grow: 1;
    min-width: 0;
    height: 100%;
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .chevron {
    width: 10px;
    height: 10px;
    flex-shrink: 0;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
    transition: transform 0.15s;
  }
  .chevron.open {
    transform: rotate(90deg);
  }
  .folder-icon {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
  }
  .dirname {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
    color: var(--text);
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
    border-radius: var(--radius-xs);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: var(--text-caption);
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
    text-align: left;
  }
  .dir {
    color: var(--text2);
  }
  .note {
    font-size: var(--text-caption);
    color: var(--text2);
    flex-shrink: 0;
  }
  .note.warn {
    color: var(--orange);
  }
  .partial {
    display: flex;
    align-items: center;
    gap: 3px;
  }
  .partial .icon {
    width: 11px;
    height: 11px;
  }
  .partial .fill {
    fill: currentColor;
    stroke: none;
  }
  .counts {
    font-size: var(--text-caption);
    flex-shrink: 0;
    white-space: nowrap;
  }
  .add {
    color: var(--green);
  }
  .del {
    color: var(--red);
  }
  /* The bin shows over the counts while the row is hovered. */
  .discard {
    position: absolute;
    right: 2px;
    top: 2px;
    width: 24px;
    height: 24px;
    border-radius: var(--radius-pill);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text2);
    opacity: 0;
  }
  .discard .icon {
    width: 14px;
    height: 14px;
  }
  .file:hover .discard,
  .discard:focus-visible {
    opacity: 1;
    background: var(--win);
  }
  .file.bin:hover .counts {
    visibility: hidden;
  }
  .discard:hover {
    color: var(--red);
  }

  /* Over the diff: which side of the file it is, or which side's files, and how it is drawn. */
  .dhead {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 46px;
    padding: var(--space-2) var(--space-3) var(--space-2) var(--space-4);
    flex-shrink: 0;
  }
  .caption {
    font-size: var(--text-small);
    color: var(--text2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dtitle {
    display: flex;
    align-items: baseline;
    gap: 8px;
    white-space: nowrap;
  }
  .dtitle b {
    font-size: var(--text-body);
    font-weight: 600;
  }
  .dtitle .n {
    font-size: var(--text-small);
    color: var(--text2);
  }
  .dtitle .mono {
    font-size: var(--text-small);
  }
  .tip {
    margin: 0 var(--space-3) var(--space-2);
    display: flex;
    flex-direction: column;
  }
  .scroll {
    flex-grow: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .error {
    margin: 12px 20px 0;
    font-size: var(--text-small);
    color: var(--red);
  }
  .nothing {
    margin: var(--space-5);
    color: var(--text2);
    font-size: var(--text-small);
  }
  /* A side with nothing left: one line with the way on. */
  .done-strip {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 40px;
    margin: 0 var(--space-3);
    padding: var(--space-1) var(--space-2) var(--space-1) var(--space-3);
    border-radius: var(--radius-m);
    background: var(--field);
    font-size: var(--text-small);
    color: var(--text2);
  }
  .done-strip .icon {
    width: 15px;
    height: 15px;
    flex-shrink: 0;
  }
  .done-strip .button {
    flex-shrink: 0;
    white-space: nowrap;
  }
  /* Hunk bar, as in the design: the @@ line, its state and the actions over it. */
  .hunk {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    margin: var(--space-2) 0 var(--space-1);
    padding: 0 var(--space-2) 0 var(--space-3);
    border-radius: var(--radius-m);
    background: var(--hunk-bg);
    color: var(--hunk-text);
    font-family: var(--mono);
    font-size: var(--text-small);
    line-height: normal;
  }
  .hunk .header {
    flex-grow: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .state {
    font-family: var(--font);
    font-size: var(--text-caption);
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
  /* A long fix, such as Stage All 1,000 Files, leaves Commit room on a line of its own. */
  .actions {
    flex-wrap: wrap;
    row-gap: 8px;
  }
  .go {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 var(--space-4);
    flex-shrink: 0;
  }

  .keys {
    font-size: var(--text-caption);
    font-weight: 500;
    opacity: 0.85;
  }
</style>
