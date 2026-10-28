<script lang="ts" module>
  /** Untracked files start folded away, apart from the edits, and stay open once opened until
   *  the app quits; every window and repository shares that. */
  const view = $state({ untracked: false });
</script>

<script lang="ts">
  import { tick, untrack } from "svelte";
  import { pressed } from "./shortcuts";
  import { api, stopped } from "./api";
  import { diffSettings, wholeByDefault } from "./prefs.svelte";
  import { confirm, type Part, type Request } from "./confirm.svelte";
  import type { FileChange, FileDiff, Hunk, HistoryRow, Side, WorkingTree } from "./types";
  import { plate, splitPath, tint } from "./format";
  import { keyFocus as focusByKey, tabStepper } from "./tabs";
  import ChangesDiff from "./ChangesDiff.svelte";
  import type { LinePicker } from "./DiffView.svelte";
  import PathName from "./PathName.svelte";
  import CommitForm from "./CommitForm.svelte";
  import Menu, { type MenuEntry } from "./Menu.svelte";
  import { fileMenu, filesMenu, type FileMenuActions } from "./changesMenu";
  import { BIG_FILE, bytes } from "./unusual";
  import { discardRequest, stageRequest, unstageRequest } from "./changes";
  import { keepShown, nextAfter, pickRange, sameShown, stepShown, type Shown } from "./changesView";
  import { draftFor, saveDraft } from "./commitDrafts.svelte";
  import { byFolder, filesWidth, listView, resizeFiles, setFolders } from "./listView.svelte";

  let {
    repo,
    branch,
    color,
    head,
    locked = null,
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
    /** The protected branch whose last commit is already pushed: Amend stays off. */
    locked?: string | null;
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
    const { summary, description, amend, amendOn, prefilled, signOff } = draft;
    saveDraft(repo, { summary, description, amend, amendOn, prefilled, signOff });
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
      // Ticks go with the files that left their list.
      const left = (list: FileChange[]) => new Set(list.map((f) => f.path));
      const now = { unstaged: left(next.unstaged.filter((f) => f.status !== "untracked")), untracked: left(next.unstaged.filter((f) => f.status === "untracked")), staged: left(next.staged) };
      ticked = { unstaged: ticked.unstaged.filter((p) => now.unstaged.has(p)), staged: ticked.staged.filter((p) => now.staged.has(p)), untracked: ticked.untracked.filter((p) => now.untracked.has(p)) };
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

  /** The files ticked in each list, for Stage, Unstage and Discard of several at once: apart
   *  from the file open, as in any list with checkboxes. */
  let ticked = $state<Record<List, string[]>>({ unstaged: [], staged: [], untracked: [] });
  /** The same ticks as sets, so a list of thousands of files checks each one at once. */
  const tickedSet = $derived({ unstaged: new Set(ticked.unstaged), staged: new Set(ticked.staged), untracked: new Set(ticked.untracked) });

  /** A clicked row or heading takes the keyboard, so ↑ ↓ and Space go on from it: WebKit on
   *  macOS doesn't focus a button on a click. */
  const keyFocus = (event: MouseEvent) => (event.currentTarget as HTMLElement).focus({ preventScroll: true });
  /** A list's name shows every file of it; once shown, it folds or unfolds the list, as its
   *  chevron does. */
  function openGroup(event: MouseEvent, group: List) {
    keyFocus(event);
    const fresh = group === "untracked";
    const side = fresh ? "unstaged" : group;
    if (!isAll(side, fresh)) return show(fresh ? { kind: "all", side, untracked: true } : { kind: "all", side });
    if (group === "untracked") {
      if (!needle) view.untracked = !view.untracked;
    } else shut[group] = !shut[group];
  }
  const isOpen = (file: FileChange, group: List | "conflicted") => !lastOpen && shown?.kind === "file" && shown.path === file.path && shown.side === sideOf(group);
  const isAll = (side: Side, fresh = false) => !lastOpen && shown?.kind === "all" && shown.side === side && !!shown.untracked === fresh;

  /** The side and file each hunk shown belongs to. */
  const hunkFile = $derived.by(() => {
    const map = new Map<Hunk, FileChange>();
    for (const d of diffs) for (const h of d.hunks) map.set(h, d.file);
    return map;
  });

  /** The file opened, ticked or unticked last, where Shift-click starts a range. */
  let anchor: { group: List; path: string } | null = null;

  /** The files a list shows, the filter applied. */
  const listOf = (group: List) => (group === "staged" ? shownStaged : group === "untracked" ? shownUntracked : shownTracked);

  /** The files of a list in the order they are shown, without those in closed folders. */
  function order(group: List | "conflicted"): string[] {
    const list = group === "conflicted" ? shownConflicted : listOf(group);
    if (!listView.folders) return list.map((f) => f.path);
    return byFolder(list).flatMap((g) => (folded.has(`${group}:${g.dir}`) ? [] : g.files.map((f) => f.path)));
  }

  /** A file's name opens its diff; ⌘-click (Ctrl) ticks or unticks it instead, Shift-click
   *  ticks every file from the one clicked last. */
  function rowClick(event: MouseEvent, file: FileChange, group: List | "conflicted") {
    keyFocus(event);
    if (dragged) return;
    if (group !== "conflicted" && (event.metaKey || event.ctrlKey || event.shiftKey)) {
      event.preventDefault();
      tickFile(group, file.path, event.shiftKey);
      return;
    }
    show({ kind: "file", path: file.path, side: sideOf(group) });
  }

  /** A file's checkbox, ⌘-click or Space ticks or unticks it; with `range`, ticks every file
   *  from the one clicked last to it. */
  function tickFile(group: List, path: string, range = false) {
    const list = ticked[group];
    const on = tickedSet[group];
    ticked = {
      ...ticked,
      [group]: range ? pickRange(order(group), list, anchor?.group === group ? anchor.path : null, path) : on.has(path) ? list.filter((p) => p !== path) : [...list, path],
    };
    anchor = { group, path };
  }

  /** The checkbox of a list or a folder: ticks all of its files, or unticks them when they all are. */
  function tickAll(group: List, paths: string[]) {
    const list = ticked[group];
    const on = tickedSet[group];
    const these = new Set(paths);
    const all = paths.length > 0 && paths.every((p) => on.has(p));
    ticked = { ...ticked, [group]: all ? list.filter((p) => !these.has(p)) : [...list, ...paths.filter((p) => !on.has(p))] };
  }

  /** A checkbox gives the keyboard to its row or heading, so ↑ ↓ and Space go on from there. */
  const keysTo = (event: MouseEvent, selector: string) => (event.currentTarget as HTMLElement).parentElement?.querySelector<HTMLElement>(selector)?.focus({ preventScroll: true });

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

  /** Enter: the ticked files of a list, else the open file, else every file of the open side,
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
   *  the buttons keep theirs. Esc unticks every file, then shows the open file's list whole. */
  function onFilesKey(event: KeyboardEvent) {
    const target = event.target as HTMLElement;
    if (!target.closest(".name, .gname")) return;
    keysInList = true;
    const by = pressed("nextFile", event) ? 1 : pressed("previousFile", event) ? -1 : 0;
    if (by) {
      event.preventDefault();
      if (stepper(by, false))
        requestAnimationFrame(() => {
          const open = filesBox?.querySelector<HTMLElement>('[aria-current="true"]');
          if (open) focusByKey(open);
        });
    } else if (pressed("stageAndNext", event)) {
      event.preventDefault();
      if (shown?.kind === "file" && shown.side === "unstaged" && openUnstaged && openUnstaged.status !== "conflicted") stageAndNext(openUnstaged);
    } else if (pressed("stageFile", event)) {
      event.preventDefault();
      stageOrUnstage();
    } else if (pressed("tickFile", event) || pressed("selectAllFiles", event)) {
      // On a file, Space ticks it; on a list's heading, all of the list, as its checkbox does.
      event.preventDefault();
      const group = target.closest<HTMLElement>("[data-group]")?.dataset.group as List | "conflicted" | undefined;
      if (!group || group === "conflicted") return;
      const path = target.closest<HTMLElement>("[data-path]")?.dataset.path;
      if (pressed("selectAllFiles", event)) ticked = { ...ticked, [group]: listOf(group).map((f) => f.path) };
      else if (path) tickFile(group, path);
      else tickAll(group, listOf(group).map((f) => f.path));
    } else if (event.key === "Escape" && (ticked.unstaged.length || ticked.untracked.length || ticked.staged.length)) {
      event.preventDefault();
      ticked = { unstaged: [], staged: [], untracked: [] };
    } else if (event.key === "Escape" && shown && shown.kind !== "all" && !lastOpen) {
      // As in a Finder list, the selection goes; its list is shown whole, with Stage All.
      event.preventDefault();
      const fresh = shown.kind === "files" ? !!shown.untracked : groupOf(shown.path, shown.side) === "untracked";
      show(fresh ? { kind: "all", side: shown.side, untracked: true } : { kind: "all", side: shown.side });
    }
  }

  /** Space ticks: the row or heading it is pressed on must not take it as a click too. */
  function onFilesKeyUp(event: KeyboardEvent) {
    if (event.key === " " && (event.target as HTMLElement).closest(".name, .gname")) event.preventDefault();
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
  let stagedList = $state<HTMLDivElement>();
  /** How tall the staged files are, all of them: they keep no more than that. */
  let stagedRows = $state(0);

  function dragFile(event: PointerEvent, file: FileChange, group: List | "conflicted") {
    if (group === "conflicted" || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey) return;
    const from = { x: event.clientX, y: event.clientY };
    const files = () => (tickedSet[group].has(file.path) ? pickedFiles(group) : [file]);
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
  const pickedFiles = (group: List) => listOf(group).filter((f) => tickedSet[group].has(f.path));
  const pickedUnstaged = $derived(pickedFiles("unstaged"));
  const pickedUntracked = $derived(pickedFiles("untracked"));
  const pickedStaged = $derived(pickedFiles("staged"));
  /** The heading's button for the selected files of its list. */

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

  const stage = (files: FileChange[]) => act(stageRequest(files, branchPart));
  const unstage = (files: FileChange[]) => act(unstageRequest(files));
  const discard = (files: FileChange[]) => act(discardRequest(files));

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

  /** Every file with changes once, for a stash of a few of them. */
  function stashCandidates() {
    const seen = new Map<string, { path: string; untracked: boolean; staged: boolean; unstaged: boolean }>();
    for (const f of unstaged) seen.set(f.path, { path: f.path, untracked: f.status === "untracked", staged: stagedByPath.has(f.path), unstaged: true });
    for (const f of staged) if (!seen.has(f.path)) seen.set(f.path, { path: f.path, untracked: false, staged: true, unstaged: false });
    return [...seen.values()];
  }

  const modClick = navigator.platform.startsWith("Mac") ? "⌘-click" : "Ctrl-click";

  function openFileMenu(event: MouseEvent, file: FileChange, group: List | "conflicted") {
    event.preventDefault();
    const actions: FileMenuActions = { stage, unstage, discard, act, actOnPicked: (how) => group !== "conflicted" && actOnPicked(group, how), stashCandidates, stagedCount: () => staged.length };
    // A right-click on one of several picked files acts on all of them.
    const several = group !== "conflicted" && tickedSet[group].has(file.path) ? pickedFiles(group) : [];
    const opened = several.length > 1 && group !== "conflicted" ? filesMenu(repo, several, group, actions) : fileMenu(repo, file, group, actions);
    menu = { x: event.clientX, y: event.clientY, ...opened };
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

  /** Two columns, the files beside their diff, from this width of the panel; under it the files
   *  sit over the diff, with a border to drag between them. */
  const TWO_COLUMNS = 600;
  let bodyWidth = $state(0);
  let bodyHeight = $state(0);
  const stacked = $derived(bodyWidth > 0 && bodyWidth < TWO_COLUMNS);
  /** The height the staged files keep when the lists over them want the room too: four rows at
   *  first, then as the border over them is dragged. Kept for every window and repository. Room
   *  the lists over them leave empty, the staged files take, up to all of them. */
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
    // From the height they have now, which can be more than kept when there was room for them.
    const from = stagedList ? Math.min(Math.max(stagedList.clientHeight - lastRoom, ROW), 20 * ROW) : stagedHeight;
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
  {@const on = group !== "conflicted" && tickedSet[group].has(file.path)}
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
    class:ticked={on && !open}
    style:background={open ? tint(color) : undefined}
    data-group={group}
    data-path={file.path}
    oncontextmenu={(e) => openFileMenu(e, file, group)}
  >
    {#if group !== "conflicted"}
      <button
        class="tick"
        class:on
        role="checkbox"
        aria-checked={on}
        tabindex="-1"
        aria-label="{on ? 'Unselect' : 'Select'} {file.path}"
        title="Select for {group === 'staged' ? 'Unstage' : 'Stage'} and the file menu · Shift-click selects a range"
        onclick={(e) => {
          keysTo(e, ".name");
          if (!dragged) tickFile(group, file.path, e.shiftKey);
        }}
      >{@render tickMark(on, false)}</button>
    {/if}
    <button
      class="name"
      onclick={(e) => rowClick(e, file, group)}
      onpointerdown={(e) => dragFile(e, file, group)}
      aria-current={open ? "true" : undefined}
      title={dragging ? undefined : `${file.oldPath ? `${file.oldPath} → ${file.path}` : file.path}${group === "conflicted" ? "" : ` · ${modClick} to select, Shift-click to select a range, drag to ${group === "staged" ? "unstage" : "stage"}`}`}
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

{#snippet tickMark(on: boolean, some: boolean)}
  <svg viewBox="0 0 16 16"><path d={some ? "M4 8h8" : "M3.5 8.5l3 3 6-7"} /></svg>
{/snippet}

<!-- Ticks every listed file of a list or a folder, or none once they all are. -->
{#snippet tickAllBox(group: List, files: FileChange[], label: string, focus: string)}
  {@const n = files.filter((f) => tickedSet[group].has(f.path)).length}
  {@const all = n > 0 && n === files.length}
  <button
    class="tick"
    class:on={all}
    class:some={n > 0 && !all}
    role="checkbox"
    aria-checked={all ? true : n ? "mixed" : false}
    tabindex="-1"
    aria-label={label}
    title={all ? "Select none" : "Select all"}
    disabled={!files.length}
    onclick={(e) => {
      keysTo(e, focus);
      tickAll(group, files.map((f) => f.path));
    }}
  >{@render tickMark(all || n > 0, n > 0 && !all)}</button>
{/snippet}

<!-- Stage 4 Files; Stage 4 in a narrow list, where the bin beside it would push the list's name out. -->
{#snippet pickedLabel(verb: string, n: number)}
  {#if n === 1}{verb} File{:else}{verb} {n}<span class="word"> Files</span>{/if}
{/snippet}

<!-- Discard or Delete for the selected files, beside Stage: the bin of a row, held in view. -->
{#snippet binPicked(files: FileChange[])}
  {@const fresh = files.every((f) => f.status === "untracked")}
  {@const what = files.length === 1 ? splitPath(files[0].path).name : `${files.length} files`}
  <button class="discard held" onclick={() => discard(files)} aria-label={fresh ? `Delete ${what}` : `Discard changes in ${what}`} title={fresh ? `Delete ${what}…` : `Discard changes in ${what}…`}>
    <svg class="icon" viewBox="0 0 16 16"><path d="M2.5 4.5h11M6 4.5V3a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1v1.5M4 4.5l.7 9a1 1 0 0 0 1 .9h4.6a1 1 0 0 0 1-.9l.7-9" /></svg>
  </button>
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
        {#if group !== "conflicted"}{@render tickAllBox(group, folder.files, `Select the files in ${folder.dir || "the top folder"}`, ".fold")}{/if}
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

<!-- A list's heading: the chevron folds it; the name folds it too and shows every file of the side. -->
{#snippet chevron(open: boolean, label: string, toggle: () => void, disabled = false)}
  <button class="chev" aria-expanded={open} aria-label={label} title={disabled ? "Shown while the filter is on" : label} {disabled} onclick={toggle}>
    <svg class="chevron" class:open viewBox="0 0 16 16"><path d="M6 4l4 4-4 4" /></svg>
  </button>
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
    <div class="files" bind:this={filesBox} aria-label="Working copy" data-steps use:tabStepper={stepper} onkeydown={onFilesKey} onkeyup={onFilesKeyUp} class:dragging={!!dragging} style:height={stacked ? `${Math.round(filesShare * 100)}%` : null} style:width={stacked ? null : `${filesWidth(bodyWidth)}px`}>
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
          <div class="ghead" class:on={isAll("unstaged")} style:background={isAll("unstaged") ? tint(color) : undefined} data-group="unstaged">
            {@render tickAllBox("unstaged", shownTracked, "Select every unstaged file", ".gname")}
            {@render chevron(!shut.unstaged, shut.unstaged ? "Show the unstaged files" : "Hide the unstaged files", () => (shut.unstaged = !shut.unstaged))}
            <button class="gname" onclick={(e) => openGroup(e, "unstaged")} aria-current={isAll("unstaged") ? "true" : undefined} aria-expanded={!shut.unstaged} title={!isAll("unstaged") ? "Show every unstaged change" : shut.unstaged ? "Show the unstaged files" : "Hide the unstaged files"}>
              <span class="gtitle" style:color={plate(color)}>Unstaged</span><span class="n">{needle ? `${shownTracked.length} of ${tracked.length}` : tracked.length}</span>
            </button>
            {#if pickedUnstaged.length}
              {@render binPicked(pickedUnstaged)}
              <button class="button mini primary" onclick={() => actOnPicked("unstaged")}>{@render pickedLabel("Stage", pickedUnstaged.length)}</button>
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
            <div class="ghead" class:on={isAll("unstaged", true)} style:background={isAll("unstaged", true) ? tint(color) : undefined} data-group="untracked">
              {@render tickAllBox("untracked", shownUntracked, "Select every untracked file", ".gname")}
              {@render chevron(untrackedOpen, untrackedOpen ? "Hide the untracked files" : "Show the new files git does not track yet", () => (view.untracked = !view.untracked), !!needle)}
              <button class="gname" onclick={(e) => openGroup(e, "untracked")} aria-current={isAll("unstaged", true) ? "true" : undefined} aria-expanded={untrackedOpen} title={!isAll("unstaged", true) ? "Show every new file git does not track yet" : untrackedOpen ? "Hide the untracked files" : "Show the untracked files"}>
                <span class="gtitle" style:color={plate(color)}>Untracked</span><span class="n">{needle ? `${shownUntracked.length} of ${untracked.length}` : untracked.length}</span>
              </button>
              {#if pickedUntracked.length}
                {@render binPicked(pickedUntracked)}
                <button class="button mini primary" onclick={() => actOnPicked("untracked")}>{@render pickedLabel("Stage", pickedUntracked.length)}</button>
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
      <div
        class="group staged"
        class:drop={!!dragging && dragging.group !== "staged"}
        class:over={!!dragging && dragging.group !== "staged" && dropping}
        bind:this={stagedBox}
        style:min-height={stacked || shut.staged ? null : `min(${30 + Math.min(stagedHeight + lastRoom, stagedRows)}px, 45%)`}
      >
        <div class="ghead" class:on={isAll("staged")} style:background={isAll("staged") ? tint(color) : undefined} data-group="staged">
          {@render tickAllBox("staged", shownStaged, "Select every staged file", ".gname")}
          {@render chevron(!shut.staged, shut.staged ? "Show the staged files" : "Hide the staged files", () => (shut.staged = !shut.staged))}
          <button class="gname" onclick={(e) => openGroup(e, "staged")} aria-current={isAll("staged") ? "true" : undefined} aria-expanded={!shut.staged} title={!isAll("staged") ? "Show the next commit: every staged change" : shut.staged ? "Show the staged files" : "Hide the staged files"}>
            <span class="gtitle" style:color={plate(color)}>Staged</span><span class="n">{needle ? `${shownStaged.length} of ${staged.length}` : staged.length}</span>
          </button>
          {#if pickedStaged.length}
            <button class="button mini primary" onclick={() => actOnPicked("staged")}>{@render pickedLabel("Unstage", pickedStaged.length)}</button>
          {:else if shownStaged.length}
            <button class="button mini" onclick={() => unstage(shownStaged)}>{needle && shownStaged.length < staged.length ? "Unstage Shown" : "Unstage All"}</button>
          {/if}
        </div>
        {#if dragging && dragging.group !== "staged"}{@render dropRow("Drop to Stage")}{/if}
        {#if !shut.staged}
          <div class="staged-files" bind:this={stagedList}>
            <div bind:clientHeight={stagedRows}>
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
          </div>
        {/if}
      </div>

      <CommitForm {draft} {branch} {color} {head} {locked} {staged} {unstaged} {conflicted} {amendedCount} {stage} {onChanged} {onResolve} />
    </div>

    {#if stacked}
      <button class="grip across" tabindex="-1" onpointerdown={dragShare} aria-label="Resize the files and the diff" title="Drag to resize"></button>
    {/if}

    {#if !stacked}<button class="edge" tabindex="-1" style:left={`${filesWidth(bodyWidth) - 4}px`} onpointerdown={(e) => resizeFiles(e, bodyWidth)} aria-label="Resize the files" title="Drag to resize"></button>{/if}
    <div class="diff" aria-label="Diff">
      <ChangesDiff
        {color}
        {shown}
        {diffs}
        {diffsOf}
        {pending}
        {error}
        {lastOpen}
        {lastDiffs}
        {needle}
        bind:whole
        bind:sel
        bind:scroller={diffScroller}
        {openUnstaged}
        {openStaged}
        {sideFiles}
        {staged}
        {tracked}
        {untracked}
        {conflicted}
        {stagedByPath}
        {picker}
        onShow={show}
        onMore={loadMore}
        {onResolve}
        {act}
        {stage}
        {unstage}
        {discard}
        {stageAndNext}
      />
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
  /* One line: what this is, the branch, and the counts, so the files and the diff get the height. */
  .head {
    min-height: 40px;
    padding: var(--space-2) var(--space-4);
    display: flex;
    align-items: center;
    gap: var(--space-3);
    border-bottom: 1px solid var(--sep);
  }
  h1 {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    margin: 0;
    font-size: var(--text-heading);
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
  /* Cut before the title and the branch are. */
  .sub {
    flex-shrink: 100;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
  /* The staged files take the room the lists over them leave, and when both want more, give
     way first, down to the height kept for them; the commit form always shows whole. */
  .group.staged {
    flex-shrink: 1000;
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
    padding-left: 44px;
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
  .ghead {
    container-type: inline-size;
  }
  @container (max-width: 300px) {
    .ghead .word {
      display: none;
    }
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
  .file.ticked {
    background: var(--side-sel);
  }
  /* A row's own button has the keys, but its ring goes round the whole row, on the row's rounded
     plate, checkbox and bin included. */
  .ghead {
    position: relative;
  }
  .name:focus-visible,
  .name:global(.key-focus),
  .fold:focus-visible,
  .fold:global(.key-focus),
  .gname:focus-visible,
  .gname:global(.key-focus) {
    outline: none;
  }
  .name:focus-visible::after,
  .name:global(.key-focus)::after,
  .fold:focus-visible::after,
  .fold:global(.key-focus)::after,
  .gname:focus-visible::after,
  .gname:global(.key-focus)::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: var(--radius-s);
    outline: 2px solid var(--accent);
    outline-offset: -2px;
    pointer-events: none;
  }
  :global(:root[data-contrast="more"]) .files :is(.name, .fold, .gname):focus-visible::after {
    outline-width: 3px;
  }
  /* Always shown, one column of them left of the badges, the folders and the list names; drawn
     like the line checkboxes of a diff. */
  .tick {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    padding: 0;
    border-radius: var(--radius-xs);
    border: 1.2px solid var(--text2);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .tick:disabled {
    opacity: 0.4;
  }
  .file > .tick,
  .folder > .tick {
    position: absolute;
    left: 4px;
    top: calc(50% - 7px);
  }
  .ghead > .tick {
    margin: 0 2px;
  }
  .tick svg {
    width: 11px;
    height: 11px;
    fill: none;
    stroke: transparent;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .tick.on,
  .tick.some {
    background: var(--accent);
    border-color: var(--accent);
  }
  .tick.on svg,
  .tick.some svg {
    stroke: #fff;
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
    position: relative;
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
  .discard.held {
    position: static;
    flex-shrink: 0;
    margin: 0 2px 0 4px;
    opacity: 1;
  }
  .discard.held:hover,
  .discard.held:focus-visible {
    background: var(--win);
  }

  input::placeholder {
    color: var(--text2);
    font-weight: 400;
  }
</style>
