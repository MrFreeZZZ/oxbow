<script lang="ts" module>
  /** Untracked files start folded away, apart from the edits, and stay open once opened until
   *  the app quits; every window and repository shares that. */
  const view = $state({ untracked: false, untrackedTouched: false });
</script>

<script lang="ts">
  import FileFilter from "./FileFilter.svelte";
  import { tick, untrack } from "svelte";
  import { pressed } from "./shortcuts";
  import { api, stopped } from "./api";
  import { diffSettings, wholeByDefault } from "./prefs.svelte";
  import { confirm, type Part, type Request } from "./confirm.svelte";
  import type { ChangelistEdit, FileChange, FileDiff, Hunk, HistoryRow, Moving, Side, WorkingTree } from "./types";
  import { lane, plate, splitPath, tint } from "./format";
  import { keyFocus as focusByKey, tabStepper } from "./tabs";
  import ChangesDiff from "./ChangesDiff.svelte";
  import type { LinePicker } from "./DiffView.svelte";
  import ChangesAmendFiles from "./ChangesAmendFiles.svelte";
  import ChangesGroupHeading from "./ChangesGroupHeading.svelte";
  import CommitForm from "./CommitForm.svelte";
  import Menu, { menuIcons, type MenuEntry, type SubEntry } from "./Menu.svelte";
  import { fileMenu, filesMenu, type FileMenuActions } from "./changesMenu";
  import ChangesFileRow from "./ChangesFileRow.svelte";
  import ChangesFileList from "./ChangesFileList.svelte";
  import { changeBadge as badge } from "./changesList";
  import { discardRequest, stageRequest, stashFilesRequest, unstageRequest } from "./changes";
  import { discardPartRequest, groupFiles, hunkLists, idOf, keyOf, listName, nameRequest, runList, runsOfLines, shareOf, stagePartRequest, type ListKey } from "./changelists";
  import type { Changelist } from "./types";
  import { countedFile, keepShown, lineCounts, nextAfter, pickRange, sameShown, stepShown, type Shown } from "./changesView";
  import { draftFor, saveDraft } from "./commitDrafts.svelte";
  import { ChangesReads } from "./changesReads.svelte";
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

  // What the panel reads from git: changesReads.svelte.ts.
  const reads = new ChangesReads(
    (next) => {
      // A file staged or unstaged whole is shown where it went; a selection keeps the files still there.
      const first = shown === null;
      shown = inItsList(keepShown(shown, next));
      const opened = shown;
      // The initial diff must have a visible row, unless the user folded this group.
      if (first && opened?.kind === "file" && opened.side === "unstaged" && !opened.list &&
          next.unstaged.some((f) => f.path === opened.path && f.status === "untracked") && !view.untrackedTouched) {
        view.untracked = true;
      }
      // Ticks go with the files that left their list.
      pruneTicks();
    },
    // Picked lines belong to the hunks they were picked in.
    () => (sel = null),
  );
  const tree = $derived(reads.tree);
  const lists = $derived(reads.lists);
  /** The unstaged files sorted into Unstaged, Untracked and the changelists. */
  const grouped = $derived(groupFiles(tree?.unstaged ?? [], lists));
  /** What the diff column shows: a file on one side, or every file of a side. */
  let shown = $state<Shown | null>(null);
  const diffs = $derived(reads.diffs);
  const diffsOf = $derived(reads.diffsOf);
  /** The lists of files: untracked files are unstaged ones listed apart, and each changelist
   *  holds some of the unstaged ones. */
  type List = ListKey;
  let whole = $state(wholeByDefault());
  // Diff & Text's default view applies again when it changes.
  $effect(() => {
    whole = wholeByDefault();
  });
  const error = $derived(reads.error);
  /** Lines picked in one hunk, with the one clicked last for Shift-click. */
  let sel = $state<{ hunk: Hunk; lines: number[]; anchor: number } | null>(null);
  let menu = $state<{ x: number; y: number; entries: MenuEntry[]; label: string } | null>(null);
  let diffScroller = $state<HTMLDivElement>();
  let diffHeaderHeight = $state(46);

  const staged = $derived(tree?.staged ?? []);
  const unstaged = $derived(tree?.unstaged ?? []);
  const conflicted = $derived(tree?.conflicted ?? []);
  /** Unstaged files split as Sublime Merge does: edits to tracked files, and new files git does
   *  not track yet; those in a changelist are listed in it. */
  const tracked = $derived(grouped.tracked);
  const untracked = $derived(grouped.untracked);
  /** Only files whose path has this text are listed, and their diffs shown with every file of a side. */
  let filter = $state("");
  const needle = $derived(filter.trim().toLowerCase());
  const matches = (file: FileChange) => !needle || file.path.toLowerCase().includes(needle) || !!file.oldPath?.toLowerCase().includes(needle);
  /** Untracked files show while a filter is typed, so it finds them too. */
  const untrackedOpen = $derived(view.untracked || !!needle);
  /** Unstaged, Staged and the changelists fold to their heading, until the app quits. */
  let shut = $state<Record<string, boolean>>({ unstaged: false, staged: false });
  /** Each list's files by path, for the other side of a file staged in part. */
  const stagedByPath = $derived(new Map(staged.map((f) => [f.path, f])));
  const unstagedByPath = $derived(new Map(unstaged.map((f) => [f.path, f])));
  /** Unstaged files in a changelist. */
  const listedCount = $derived(new Set(grouped.lists.flatMap((g) => g.files.map((f) => f.path))).size);
  const fileCount = $derived(new Set([...conflicted, ...unstaged, ...staged].map((f) => f.path)).size);
  const branchPart = $derived<Part>(branch ? { branch, color } : { code: "HEAD" });

  const pending = $derived(reads.pending);

  const draft = $derived(draftFor(repo));

  $effect(() => {
    const { summary, description, amend, amendOn, prefilled, signOff, changelist } = draft;
    saveDraft(repo, { summary, description, amend, amendOn, prefilled, signOff, changelist });
  });

  const shownConflicted = $derived(conflicted.filter(matches));
  const shownTracked = $derived(tracked.filter(matches));
  const shownUntracked = $derived(untracked.filter(matches));
  const shownStaged = $derived(staged.filter(matches));
  /** Each changelist with its files, and those of them the filter lets through. */
  const shownLists = $derived(grouped.lists.map((g) => ({ ...g, shown: g.files.filter(matches) })));
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
  let listed = $state<Record<string, number>>({});

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

  /** The files of the diffs on screen by "side:path": their rows take the counts from them
   *  while the list is not counted yet, so the file open shows its counts first. */
  const diffFiles = $derived(new Map(diffsOf ? diffs.map((d) => [`${diffsOf!.side}:${d.file.path}`, d.file]) : []));
  /** `file` of a list with its changed lines, or null while they are being counted. */
  const lined = (file: FileChange, side: Side) => countedFile(file, tree?.counted !== false, diffFiles, side);

  /** All the files of a list, the filter aside. */
  function filesOf(group: List | "conflicted"): FileChange[] {
    if (group === "conflicted") return conflicted;
    if (group === "staged") return staged;
    if (group === "untracked") return untracked;
    if (group === "unstaged") return tracked;
    return grouped.lists.find((g) => keyOf(g.list.id) === group)?.files ?? [];
  }

  /** Ticks of files that left their list go. */
  function pruneTicks() {
    const kept: Record<string, string[]> = {};
    for (const [group, paths] of Object.entries(ticked)) {
      const there = new Set(filesOf(group as List).map((f) => f.path));
      kept[group] = paths.filter((p) => there.has(p));
    }
    ticked = kept;
  }

  /** What is shown, with the changelist it is in now: a file moved to another list is shown in
   *  that one, and every file of a deleted list gives way to Unstaged. */
  function inItsList(next: Shown | null): Shown | null {
    if (!next || next.side !== "unstaged") return next;
    if (next.kind === "all") return next.list && !lists?.lists.some((l) => l.id === next.list) ? { kind: "all", side: "unstaged" } : next;
    if (next.kind === "files") return next;
    const group = groupOf(next.path, "unstaged", next.list ?? null);
    const list = group ? idOf(group) : null;
    return (next.list ?? null) === list ? next : { ...next, list };
  }

  /** Change the changelists; `said` goes to the toast. */
  async function editLists(edit: ChangelistEdit, said?: string): Promise<boolean> {
    try {
      reads.lists = await api.editChangelists(edit);
      shown = inItsList(shown);
      pruneTicks();
      if (said) confirm.say(said, "done");
      return true;
    } catch (err) {
      confirm.fail(err);
      return false;
    }
  }

  $effect(() => {
    version;
    // The files often come with the reload, so `load` runs at once: what it reads must not tie
    // this effect to it.
    untrack(() => reads.load(changes, snapshot));
  });

  $effect(() => reads.show(shown, whole, diffSettings(), grouped, () => diffNeedle));

  // The panel's reads stop when it closes: their answers would go nowhere.
  $effect(() => () => reads.stop());

  let filesBox = $state<HTMLDivElement>();

  function show(next: Shown) {
    if ((lastOpen || !sameShown(next, shown)) && diffScroller) diffScroller.scrollTop = 0;
    lastOpen = null;
    shown = next;
    if (next.kind === "file") {
      const group = groupOf(next.path, next.side, next.list ?? null);
      anchor = group ? { group, path: next.path } : null;
    }
    // Stepped to with the keys, the file may be scrolled out of its list.
    requestAnimationFrame(() => filesBox?.querySelector('[aria-current="true"]')?.scrollIntoView({ block: "nearest" }));
  }

  const sideOf = (group: List | "conflicted"): Side => (group === "staged" ? "staged" : "unstaged");
  /** Unstaged, Untracked or a changelist: the lists whose files can be staged, moved and discarded. */
  const unstagedList = (group: List | "conflicted"): group is List => group !== "staged" && group !== "conflicted";
  /** The list a file on a side is in, `prefer` (a changelist's id; null: Unstaged) when it holds
   *  part of a shared file; null for a conflicted file, which is never selected with others. */
  function groupOf(path: string, side: Side, prefer: string | null = null): List | null {
    if (side === "staged") return "staged";
    const file = unstagedByPath.get(path);
    if (!file || file.status === "conflicted") return null;
    const holding = grouped.lists.filter((g) => g.files.some((f) => f.path === path)).map((g) => keyOf(g.list.id));
    if (prefer !== null && holding.includes(keyOf(prefer))) return keyOf(prefer);
    const free = (file.status === "untracked" ? untracked : tracked).some((f) => f.path === path);
    if (free) return file.status === "untracked" ? "untracked" : "unstaged";
    return holding[0] ?? null;
  }
  /** One file of a list, as the diff shows it. */
  const fileShown = (path: string, group: List | "conflicted"): Shown => ({ kind: "file", path, side: sideOf(group), list: group === "staged" ? undefined : idOf(group) });
  /** Every file of a list. */
  function allOf(group: List): Shown {
    if (group === "staged") return { kind: "all", side: "staged" };
    if (group === "untracked") return { kind: "all", side: "unstaged", untracked: true };
    const list = idOf(group);
    return list ? { kind: "all", side: "unstaged", list } : { kind: "all", side: "unstaged" };
  }
  /** The list the diff column shows, or the open file's. */
  const shownGroup = $derived.by((): List | null => {
    if (!shown) return null;
    if (shown.kind === "file") return groupOf(shown.path, shown.side, shown.list ?? null);
    if (shown.side === "staged") return "staged";
    if (shown.list) return keyOf(shown.list);
    return shown.untracked ? "untracked" : "unstaged";
  });

  /** The files ticked in each list, for Stage, Unstage and Discard of several at once: apart
   *  from the file open, as in any list with checkboxes. */
  let ticked = $state<Record<string, string[]>>({ unstaged: [], staged: [], untracked: [] });
  /** The same ticks as sets, so a list of thousands of files checks each one at once. */
  const tickedSet = $derived(Object.fromEntries(Object.entries(ticked).map(([group, paths]) => [group, new Set(paths)])) as Record<string, Set<string>>);
  const NONE = new Set<string>();
  const ticksOf = (group: List | "conflicted") => tickedSet[group] ?? NONE;

  /** A clicked row or heading takes the keyboard, so ↑ ↓ and Space go on from it: WebKit on
   *  macOS doesn't focus a button on a click. */
  const keyFocus = (event: MouseEvent) => (event.currentTarget as HTMLElement).focus({ preventScroll: true });
  /** A list's name shows every file of it; once shown, it folds or unfolds the list, as its
   *  chevron does. */
  function openGroup(event: MouseEvent, group: List) {
    keyFocus(event);
    const fresh = group === "untracked";
    const list = idOf(group);
    if (!isAll(sideOf(group), fresh, list)) return show(allOf(group));
    if (group === "untracked") {
      if (!needle) {
        view.untracked = !view.untracked;
        view.untrackedTouched = true;
      }
    } else shut[group] = !shut[group];
  }
  const isOpen = (file: FileChange, group: List | "conflicted") =>
    !lastOpen && shown?.kind === "file" && shown.path === file.path && shown.side === sideOf(group) && (group === "staged" || (shown.list ?? null) === idOf(group));
  const isAll = (side: Side, fresh = false, list: string | null = null) => !lastOpen && shown?.kind === "all" && shown.side === side && !!shown.untracked === fresh && (shown.list ?? null) === list;

  /** The side and file each hunk shown belongs to. */
  const hunkFile = $derived.by(() => {
    const map = new Map<Hunk, FileChange>();
    for (const d of diffs) for (const h of d.hunks) map.set(h, d.file);
    return map;
  });

  /** The file opened, ticked or unticked last, where Shift-click starts a range. */
  let anchor: { group: List; path: string } | null = null;

  /** The files a list shows, the filter applied. */
  const listOf = (group: List): FileChange[] =>
    group === "staged" ? shownStaged : group === "untracked" ? shownUntracked : group === "unstaged" ? shownTracked : (shownLists.find((g) => keyOf(g.list.id) === group)?.shown ?? []);

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
    show(fileShown(file.path, group));
  }

  /** A file's checkbox, ⌘-click or Space ticks or unticks it; with `range`, ticks every file
   *  from the one clicked last to it. */
  function tickFile(group: List, path: string, range = false) {
    const list = ticked[group] ?? [];
    const on = ticksOf(group);
    ticked = {
      ...ticked,
      [group]: range ? pickRange(order(group), list, anchor?.group === group ? anchor.path : null, path) : on.has(path) ? list.filter((p) => p !== path) : [...list, path],
    };
    anchor = { group, path };
  }

  /** The checkbox of a list or a folder: ticks all of its files, or unticks them when they all are. */
  function tickAll(group: List, paths: string[]) {
    const list = ticked[group] ?? [];
    const on = ticksOf(group);
    const these = new Set(paths);
    const all = paths.length > 0 && paths.every((p) => on.has(p));
    ticked = { ...ticked, [group]: all ? list.filter((p) => !these.has(p)) : [...list, ...paths.filter((p) => !on.has(p))] };
  }

  /** Every file shown once, in the order of the lists, each side led by all of its files: the
   *  steps of Next Tab and Previous Tab. */
  const steps = $derived.by((): Shown[] => {
    const out: Shown[] = [];
    const add = (group: List | "conflicted") => out.push(...order(group).map((path) => fileShown(path, group)));
    if (conflicted.length || tracked.length) out.push({ kind: "all", side: "unstaged" });
    add("conflicted");
    if (!shut.unstaged) add("unstaged");
    for (const g of grouped.lists) {
      const key = keyOf(g.list.id);
      if (g.files.length) out.push(allOf(key));
      if (!shut[key]) add(key);
    }
    if (untracked.length) out.push({ kind: "all", side: "unstaged", untracked: true });
    if (untrackedOpen) add("untracked");
    if (staged.length) out.push({ kind: "all", side: "staged" });
    if (!shut.staged) add("staged");
    return out;
  });
  const stepper = $derived((by: 1 | -1, wrap: boolean) => {
    // From several files, the keys go on from the one clicked last.
    const from: Shown | null = shown?.kind === "files" && anchor ? fileShown(anchor.path, anchor.group) : shown;
    const next = stepShown(steps, from, by, wrap);
    if (!next) return false;
    show(next);
    return true;
  });

  /** The files Stage & Next File goes through: the unstaged edits, each changelist's, then the
   *  new files; each as its list and its path. */
  const unstagedOrder = () => {
    const groups: List[] = ["unstaged", ...grouped.lists.map((g) => keyOf(g.list.id)), ...(untrackedOpen ? (["untracked"] as const) : [])];
    return groups.flatMap((group) => order(group).map((path) => `${group}\n${path}`));
  };

  /** Stages the open unstaged file and opens the one after it; the next commit when none is left. */
  async function stageAndNext(file: FileChange) {
    const group = shownGroup ?? "unstaged";
    const here = `${group}\n${file.path}`;
    const next = nextAfter(unstagedOrder(), here);
    if (!(await stageIn(group, [file]))) return;
    if (!next || next === here) return show({ kind: "all", side: "staged" });
    const at = next.indexOf("\n");
    show(fileShown(next.slice(at + 1), next.slice(0, at) as List));
  }

  /** Enter: the ticked files of a list, else the open file, else every file of the open side,
   *  go to the other side. */
  function stageOrUnstage() {
    for (const group of Object.keys(ticked) as List[]) if (pickedFiles(group).length) return actOnPicked(group);
    const group = shownGroup;
    if (shown?.kind === "all" && group) {
      const files = listOf(group);
      return files.length && (group === "staged" ? unstage(files) : stageIn(group, files));
    }
    if (shown?.kind !== "file") return;
    if (shown.side === "staged") return openStaged && unstage([openStaged]);
    if (openUnstaged && openUnstaged.status !== "conflicted") return stageIn(group ?? "unstaged", [openUnstaged]);
  }

  /** The list's own keys, on a file's row or a list's heading (each of them has these two
   *  handlers): the filter field and the other buttons keep theirs. Esc unticks every file, then
   *  shows the open file's list whole. */
  function onFilesKey(event: KeyboardEvent) {
    const target = event.currentTarget as HTMLElement;
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
    } else if (event.key === "Escape" && Object.values(ticked).some((paths) => paths.length)) {
      event.preventDefault();
      ticked = { unstaged: [], staged: [], untracked: [] };
    } else if (event.key === "Escape" && shown && shown.kind !== "all" && !lastOpen) {
      // As in a Finder list, the selection goes; its list is shown whole, with Stage All.
      event.preventDefault();
      const group = shown.kind === "files" ? (shown.list ? keyOf(shown.list) : shown.untracked ? "untracked" : shown.side) : shownGroup;
      if (group) show(allOf(group));
    }
  }

  /** Space ticks: the row or heading it is pressed on must not take it as a click too. */
  function onFilesKeyUp(event: KeyboardEvent) {
    if (event.key === " ") event.preventDefault();
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
  /** Where the files would be dropped: a list they can go to, under the pointer. */
  let dropOn = $state<List | null>(null);
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
    const files = () => (ticksOf(group).has(file.path) ? pickedFiles(group) : [file]);
    const move = (e: PointerEvent) => {
      if (!dragging && Math.hypot(e.clientX - from.x, e.clientY - from.y) < 5) return;
      dragging = { group, files: dragging?.files ?? files(), x: e.clientX, y: e.clientY };
      dropOn = dropTarget(e.clientX, e.clientY, group);
    };
    const end = (drop: boolean) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("keydown", key, true);
      const done = dragging;
      dragging = null;
      const to = dropOn;
      dropOn = null;
      if (!done) return;
      dragged = true;
      setTimeout(() => (dragged = false));
      if (!drop || !to) return;
      if (done.group === "staged") unstageInto(done.files, idOf(to));
      else if (to === "staged") stageIn(done.group, done.files);
      else moveFiles(done.files, done.group, idOf(to));
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

  /** The list files dragged from `from` would go to with the pointer at `x`, `y`: Staged, a
   *  changelist, or Unstaged; null where they can't be dropped. */
  function dropTarget(x: number, y: number, from: List): List | null {
    const to = (document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-drop]")?.dataset.drop as List | undefined) ?? null;
    return to && canGo(from, to) ? to : null;
  }

  /** Whether files can go from list `from` to list `to`: Unstaged and Untracked are both in no
   *  changelist, so neither takes the other's files. */
  const canGo = (from: List, to: List) => to !== from && (from === "staged" || to === "staged" || idOf(from) !== idOf(to));
  /** Whether files being dragged can be dropped on list `group`. */
  const canDrop = (group: List) => !!dragging && canGo(dragging.group, group);

  /** Move what list `from` holds of `files` to the changelist `to` (null: Unstaged). */
  function moveFiles(files: FileChange[], from: List, to: string | null) {
    const what = files.length === 1 ? splitPath(files[0].path).name : `${files.length} files`;
    return editLists({ kind: "move", moving: { kind: "part", paths: files.map((f) => f.path), from: idOf(from) }, to }, `Moved ${what} to ${listName(lists, to)}.`);
  }

  /** Unstage `files` into the changelist `to` (null: Unstaged). */
  async function unstageInto(files: FileChange[], to: string | null) {
    if (!to) return unstage(files);
    // Into the list first: the unstaged changes land there.
    if (await editLists({ kind: "move", moving: { kind: "files", paths: files.map((f) => f.path) }, to })) await unstage(files);
  }

  /** Only picked files that are listed: the filter can hide some. */
  const pickedFiles = (group: List) => listOf(group).filter((f) => ticksOf(group).has(f.path));
  const pickedUnstaged = $derived(pickedFiles("unstaged"));
  const pickedUntracked = $derived(pickedFiles("untracked"));
  const pickedStaged = $derived(pickedFiles("staged"));
  /** The heading's button for the selected files of its list. */

  /** Changed lines of `file` on `side`, "+12 −3", or what the change is when it has none. */
  const counts = (file: FileChange, side: Side): string => lineCounts(lined(file, side));

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
    await (how === "discard" ? discardIn(group, files) : group === "staged" ? unstage(files) : stageIn(group, files));
  }

  const stage = (files: FileChange[]) => act(stageRequest(files, branchPart));
  const unstage = (files: FileChange[]) => act(unstageRequest(files));
  const discard = (files: FileChange[]) => act(discardRequest(files));
  /** Whether list `group` holds only part of one of `files`, which it shares with another. */
  const shares = (group: List | "conflicted", files: FileChange[]) => unstagedList(group) && files.some((f) => !!shareOf(lists, f.path, idOf(group)));
  /** Stage or discard files of a list: of a file it shares with another list, only its part. */
  const stageIn = (group: List | "conflicted", files: FileChange[]) => (shares(group, files) ? act(stagePartRequest(files, idOf(group), lists, branchPart)) : stage(files));
  const discardIn = (group: List | "conflicted", files: FileChange[]) => (shares(group, files) ? act(discardPartRequest(files, idOf(group), lists)) : discard(files));
  /** Stage and discard in the diff column: of the list it shows. */
  const stageShown = (files: FileChange[]) => stageIn(shownGroup ?? "unstaged", files);
  const discardShown = (files: FileChange[]) => discardIn(shownGroup ?? "unstaged", files);

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
    // A right-click on one of several picked files acts on all of them.
    const several = group !== "conflicted" && ticksOf(group).has(file.path) ? pickedFiles(group) : [];
    const list = idOf(group);
    const actions: FileMenuActions = {
      stage: (files) => stageIn(group, files),
      unstage,
      discard: (files) => discardIn(group, files),
      act,
      actOnPicked: (how) => group !== "conflicted" && actOnPicked(group, how),
      stashCandidates,
      stagedCount: () => staged.length,
      extra: group === "conflicted" ? [] : [moveEntry(several.length > 1 ? several : [file], group)],
      where: list ? `in ${listName(lists, list)}` : undefined,
    };
    // The menus of Unstaged and Untracked serve the changelists' files too.
    const kind = group === "staged" || group === "conflicted" ? group : file.status === "untracked" ? "untracked" : "unstaged";
    const opened = several.length > 1 && kind !== "conflicted" ? filesMenu(repo, several, kind, actions) : fileMenu(repo, file, kind, actions);
    menu = { x: event.clientX, y: event.clientY, ...opened };
  }

  /** Move to Changelist ▸: the other lists, Unstaged, and a new one. A staged file is unstaged into it. */
  function moveEntry(files: FileChange[], group: List): MenuEntry {
    const paths = files.map((f) => f.path);
    const what = files.length === 1 ? splitPath(files[0].path).name : `${files.length} files`;
    const to = (list: string | null) => (group === "staged" ? unstageInto(files, list) : moveFiles(files, group, list));
    const entries: SubEntry[] = [];
    if (group !== "unstaged" && group !== "untracked") entries.push({ label: "Unstaged", hint: group === "staged" ? "unstage" : undefined, run: () => to(null) });
    for (const l of lists?.lists ?? []) if (keyOf(l.id) !== group) entries.push({ label: l.name, hint: lists?.active === l.id ? "active" : undefined, run: () => to(l.id) });
    const moving: Moving = group === "staged" ? { kind: "files", paths } : { kind: "part", paths, from: idOf(group) };
    entries.push({ label: "New Changelist…", run: () => newList(moving, what, group === "staged" ? () => unstage(files) : null) });
    return { kind: "sub", label: "Move to Changelist", icon: menuIcons.changelist, entries };
  }

  /** Ask for a new list's name and make it, with `moving` in it; `then` runs once it is made. */
  async function newList(moving: Moving | null = null, what = "", then: (() => unknown) | null = null) {
    const made = await confirm.run(
      nameRequest(lists?.lists ?? [], null, "", async (name) => {
        reads.lists = await api.editChangelists({ kind: "create", name, moving });
        shown = inItsList(shown);
        pruneTicks();
        return moving ? `Made the changelist “${name}” with ${what}.` : `Made the changelist “${name}”.`;
      }, what),
    );
    if (made && then) await then();
  }

  /** A list's menu, from its ⋯ button or a right-click on its heading. */
  function openListMenu(event: MouseEvent, g: { list: Changelist; files: FileChange[] }) {
    event.preventDefault();
    const { list, files } = g;
    const key = keyOf(list.id);
    const n = files.length;
    const none = n ? {} : { off: true, detail: "It has no changes" };
    const active = lists?.active === list.id;
    const entries: MenuEntry[] = [
      { kind: "header", label: `${list.name} · ${n === 1 ? "1 file" : `${n} files`}` },
      { kind: "item", label: `Commit “${list.name}”…`, icon: menuIcons.commit, run: () => commitList(list.id), ...none },
      { kind: "item", label: n === 1 ? "Stage File" : `Stage All ${n} Files`, icon: menuIcons.stage, run: () => stageIn(key, files), ...none },
      { kind: "item", label: "Stash These Files…", icon: menuIcons.stash, run: () => act(stashFilesRequest(stashCandidates(), files.map((f) => f.path), list.name)), ...none },
      { kind: "sep" },
      {
        kind: "item",
        label: active ? "Stop Taking New Changes" : "New Changes Go Here",
        icon: menuIcons.good,
        detail: active ? "Files that start changing go to Unstaged again" : "Files that start changing go to this list",
        run: () => editLists({ kind: "activate", id: active ? null : list.id }, active ? "New changes go to Unstaged." : `New changes go to “${list.name}”.`),
      },
      { kind: "item", label: "Rename…", icon: menuIcons.edit, run: () => renameList(list) },
      { kind: "item", label: "New Changelist…", icon: menuIcons.changelist, run: () => newList() },
      { kind: "sep" },
      { kind: "item", label: "Delete Changelist", icon: menuIcons.bad, detail: "Its changes go back to Unstaged", run: () => editLists({ kind: "delete", id: list.id }, `Deleted “${list.name}”: its changes are in Unstaged.`) },
      { kind: "item", label: "Discard All Changes…", icon: menuIcons.drop, danger: true, run: () => discardIn(key, files), ...none },
    ];
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const at = event.type === "contextmenu" ? { x: event.clientX, y: event.clientY } : { x: box.right - 240, y: box.bottom + 4 };
    menu = { ...at, entries, label: `${list.name} menu` };
  }

  /** The menu of Unstaged's heading. */
  function openUnstagedMenu(event: MouseEvent) {
    event.preventDefault();
    const entries: MenuEntry[] = [{ kind: "header", label: "Unstaged" }];
    if (shownTracked.length) entries.push({ kind: "item", label: "Stage All", icon: menuIcons.stage, run: () => stageIn("unstaged", shownTracked) });
    entries.push({ kind: "item", label: "New Changelist…", icon: menuIcons.changelist, run: () => newList() });
    if (lists?.active) entries.push({ kind: "item", label: "New Changes Go Here", icon: menuIcons.good, detail: `Instead of to “${listName(lists, lists.active)}”`, run: () => editLists({ kind: "activate", id: null }, "New changes go to Unstaged.") });
    menu = { x: event.clientX, y: event.clientY, entries, label: "Unstaged menu" };
  }

  function renameList(list: Changelist) {
    confirm.run(
      nameRequest(lists?.lists ?? [], list, list.name, async (name) => {
        reads.lists = await api.editChangelists({ kind: "rename", id: list.id, name });
        return `Renamed “${list.name}” to “${name}”.`;
      }),
    );
  }

  let commitForm = $state<CommitForm>();
  /** Commit takes list `id`, and the cursor goes to the message. */
  function commitList(id: string) {
    commitForm?.pickList(id, true);
  }

  /** The chip on an unstaged hunk's bar: the list its changes are in. */
  function hunkChip(d: FileDiff, hunk: Hunk): { label: string; listed: boolean } {
    const ids = hunkLists(lists, d.file.path, hunk);
    return { label: ids.map((id) => listName(lists, id)).join(" + "), listed: ids.some((id) => id !== null) };
  }

  /** The chip's menu: move the hunk's changes to another list, or only those of the lines
   *  picked in it. */
  function openHunkMenu(event: MouseEvent, d: FileDiff, hunk: Hunk) {
    const path = d.file.path;
    const picked = sel?.hunk === hunk && sel.lines.length ? sel.lines : null;
    const keys = picked ? runsOfLines(hunk, picked) : (hunk.runs ?? []);
    const ids = picked ? [...new Set(keys.map((k) => runList(lists, path, k)))] : hunkLists(lists, path, hunk);
    const what = picked ? (keys.length === 1 ? "the change" : "the changes") : "the change";
    const move = async (to: string | null) => {
      if (await editLists({ kind: "move", moving: { kind: "runs", path, keys }, to }, `Moved ${what} to ${listName(lists, to)}.`)) sel = null;
    };
    const entries: MenuEntry[] = [{ kind: "header", label: picked ? `Move the picked lines’ ${keys.length === 1 ? "change" : "changes"} to` : "Move this change to" }];
    const off = keys.length ? {} : { off: true, detail: "Its lines aren't shown, so it goes with its file" };
    const here = (id: string | null) => ids.length === 1 && ids[0] === id;
    entries.push({ kind: "item", label: "Unstaged", icon: here(null) ? menuIcons.good : menuIcons.changelist, run: () => move(null), ...(here(null) ? { off: true, detail: "It is here" } : off) });
    for (const l of lists?.lists ?? []) entries.push({ kind: "item", label: l.name, icon: here(l.id) ? menuIcons.good : menuIcons.changelist, run: () => move(l.id), ...(here(l.id) ? { off: true, detail: "It is here" } : off) });
    entries.push({ kind: "sep" });
    entries.push({ kind: "item", label: "New Changelist…", icon: menuIcons.changelist, run: () => newList({ kind: "runs", path, keys }, `a change of ${splitPath(path).name}`), ...off });
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    menu = { x: box.left, y: box.bottom + 4, entries, label: "Move this change menu" };
  }

  /** Under the switch of a file a list shares with others: what of it is here. */
  const fileCaption = $derived.by(() => {
    if (shown?.kind !== "file" || shown.side !== "unstaged" || !lists?.lists.length) return null;
    const list = shown.list ?? null;
    const share = shareOf(lists, shown.path, list);
    if (!share) return list ? `In ${listName(lists, list)}: stays out of the commit until staged or committed with the list` : null;
    const rest = share.total - share.runs;
    const others = share.others.map((id) => listName(lists, id)).join(" and ");
    return `In ${listName(lists, list)}: ${share.runs} of its ${share.total} changes. The other${rest === 1 ? " is" : "s are"} in ${others}`;
  });

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
        if (live && !stopped(err)) reads.error = String(err);
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
  const ROW = 34;
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
      return (shown.side === "staged" ? staged : shown.list ? filesOf(keyOf(shown.list)) : shown.untracked ? untracked : tracked).filter((f) => chosen.has(f.path));
    }
    return shown.side === "staged" ? shownStaged : shown.list ? listOf(keyOf(shown.list)) : shown.untracked ? shownUntracked : [...shownConflicted, ...shownTracked];
  });
</script>

{#snippet fileRow(file: FileChange, group: List | "conflicted", nested: boolean)}
  {@const other = group === "staged" ? unstagedByPath.get(file.path) : unstagedList(group) ? stagedByPath.get(file.path) : undefined}
  {@const share = unstagedList(group) ? shareOf(lists, file.path, idOf(group)) : null}
  <ChangesFileRow {file} {group} {nested} on={group !== "conflicted" && ticksOf(group).has(file.path)}
    open={isOpen(file, group)} moving={dragging?.group === group && dragging.files.includes(file)}
    dragging={!!dragging} counted={lined(file, sideOf(group))} {modClick}
    partialTitle={other ? (group === "staged" ? `Staged in part: ${counts(file, "staged")} go into the commit, ${counts(other, "unstaged")} stay out` : `Staged in part: ${counts(other, "staged")} go into the commit, ${counts(file, "unstaged")} stay out`) : null}
    share={share ? { runs: share.runs, total: share.total, title: `${share.runs} of its ${share.total} changes are here; the other${share.total - share.runs === 1 ? " is" : "s are"} in ${share.others.map((id) => listName(lists, id)).join(" and ")}` } : null}
    events={{
      tick: (e) => { if (group !== "conflicted" && !dragged) tickFile(group, file.path, e.shiftKey); },
      open: (e) => rowClick(e, file, group), drag: (e) => dragFile(e, file, group),
      menu: (e) => openFileMenu(e, file, group), discard: () => discardIn(group, [file]),
      keyDown: onFilesKey, keyUp: onFilesKeyUp,
    }} />
{/snippet}

{#snippet fileList(files: FileChange[], group: List | "conflicted")}
  {@const limit = listed[group] ?? LIST_STEP}
  <ChangesFileList {files} {group} folders={listView.folders} {folded}
    cut={listView.folders ? foldersWithin(byFolder(files), group, limit) : null} {limit} step={LIST_STEP} ticks={ticksOf(group)}
    onFold={fold} onTickAll={(paths) => { if (group !== "conflicted") tickAll(group, paths); }}
    onMore={() => (listed = { ...listed, [group]: (listed[group] ?? LIST_STEP) + LIST_STEP })}>
    {#snippet row(file: FileChange, nested: boolean)}{@render fileRow(file, group, nested)}{/snippet}
  </ChangesFileList>
{/snippet}

{#snippet dropRow(label: string)}
  <div class="drop-row" aria-hidden="true">
    <svg class="icon" viewBox="0 0 16 16"><path d="M8 3v9M4.5 8.5 8 12l3.5-3.5" /></svg>{label}
  </div>
{/snippet}

<div class="panel">
  <div class="head">
    <h1>
      <span class="head-title">Uncommitted changes</span>
      {#if branch}
        <span class="branch" style:color={plate(color)} style:background={tint(color, "label")} style:box-shadow={`inset 0 0 0 1px color-mix(in srgb, ${lane(color)} 40%, transparent)`} title="Checked-out branch">
          <svg class="icon tiny" viewBox="0 0 16 16"><circle cx="4.5" cy="3.5" r="1.5" /><circle cx="4.5" cy="12.5" r="1.5" /><circle cx="11.5" cy="5.5" r="1.5" /><path d="M4.5 5v6M11.5 7c0 3-7 2-7 4" /></svg>
          {branch}
        </span>
      {/if}
    </h1>
    <span class="sub">
      {fileCount}
      {fileCount === 1 ? "file" : "files"}{#if conflicted.length} · {conflicted.length} conflicted{/if} · {staged.length} staged · {tracked.length} unstaged{#if listedCount}{` · ${listedCount} in ${grouped.lists.length === 1 ? "a changelist" : `${grouped.lists.length} changelists`}`}{/if}{#if untracked.length}{` · ${untracked.length} untracked`}{/if}
    </span>
  </div>

  <div class="body" class:stacked bind:clientWidth={bodyWidth} bind:clientHeight={bodyHeight}>
    <div class="files" bind:this={filesBox} aria-label="Working copy" data-steps use:tabStepper={stepper} class:dragging={!!dragging} style:--changes-row-cursor={dragging ? "grabbing" : null} style:--selection-fill={tint(color)} style:--selection-border={`color-mix(in srgb, ${lane(color)} 40%, transparent)`} style:height={stacked ? `${Math.round(filesShare * 100)}%` : null} style:width={stacked ? null : `${filesWidth(bodyWidth)}px`}>
      <div class="tools" style:height={!stacked && (shown || lastOpen) ? `${diffHeaderHeight}px` : null}>
        <div class="filter"><FileFilter bind:value={filter} {color} /></div>
        <div class="segmented icons" role="radiogroup" aria-label="Show files">
          <button role="radio" aria-checked={!listView.folders} class:on={!listView.folders} onclick={() => setFolders(false)} aria-label="List" title="One list of paths">
            <svg class="icon" viewBox="0 0 16 16"><path d="M5.5 4h8M5.5 8h8M5.5 12h8M2.5 4h.01M2.5 8h.01M2.5 12h.01" /></svg>
          </button>
          <button role="radio" aria-checked={listView.folders} class:on={listView.folders} onclick={() => setFolders(true)} aria-label="Folders" title="Files under their folders">
            <svg class="icon" viewBox="0 0 16 16"><path d="M1.5 4.5a1 1 0 0 1 1-1H6l1.5 1.5h6a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" /></svg>
          </button>
        </div>
        <button class="button icon-only small" onclick={() => newList()} aria-label="New Changelist…" title="New Changelist: a named group of unstaged changes, committed on its own">
          <svg class="icon" viewBox="0 0 16 16"><path d="M2.5 4h7M2.5 8h7M2.5 12h4.5M12 9.5v5M9.5 12h5" /></svg>
        </button>
      </div>

      <div class="lists" class:drop={dragging?.group === "staged"} class:over={dragging?.group === "staged" && dropOn === "unstaged"} data-drop={dragging?.group === "staged" ? "unstaged" : undefined} bind:this={listsBox}>
        {#if dragging?.group === "staged"}{@render dropRow("Drop to Unstage")}{/if}
        {#if shownConflicted.length}
          <div class="group">
            <div class="ghead diff-list-row diff-list-group"><span class="gtitle conflicted">Conflicted</span><span class="n">{shownConflicted.length}</span></div>
            {@render fileList(shownConflicted, "conflicted")}
          </div>
        {/if}
        <div class="group" class:drop={dragging?.group !== "staged" && canDrop("unstaged")} class:over={dragging?.group !== "staged" && dropOn === "unstaged"} data-drop="unstaged">
          <ChangesGroupHeading group={"unstaged"} files={shownTracked} total={tracked.length} {color} {needle}
            expanded={!shut.unstaged} selected={isAll("unstaged")} ticked={ticksOf("unstaged")} picked={pickedUnstaged}
            events={{ open: (e) => openGroup(e, "unstaged"), toggle: () => { shut.unstaged = !shut.unstaged; },
              selectAll: () => tickAll("unstaged", shownTracked.map((f) => f.path)), keyDown: onFilesKey, keyUp: onFilesKeyUp,
              picked: () => actOnPicked("unstaged"), all: () => stageIn("unstaged", shownTracked),
              discard: () => discardIn("unstaged", pickedUnstaged),
              menu: openUnstagedMenu,
            }} />
          {#if !shut.unstaged}
            {@render fileList(shownTracked, "unstaged")}
            {#if !shownTracked.length}
              <p class="empty">{needle && tracked.length ? "No unstaged files match the filter." : grouped.lists.some((g) => g.files.length) ? "Every edit is in a changelist." : untracked.length ? "No edits to tracked files." : "Nothing left to stage."}</p>
            {/if}
          {/if}
        </div>
        {#each shownLists as g (g.list.id)}
          <!-- A changelist: some of the unstaged changes, under the name it was given. -->
          {@const key = keyOf(g.list.id)}
          {@const all = isAll("unstaged", false, g.list.id)}
          {@const open = !shut[key]}
          {@const picked = pickedFiles(key)}
          <div class="group" class:drop={canDrop(key)} class:over={dropOn === key} data-drop={key}>
            <ChangesGroupHeading group={key} files={g.shown} total={g.files.length} {color} {needle}
            expanded={open} selected={all} ticked={ticksOf(key)} picked={picked} changelist={g.list} active={lists?.active === g.list.id}
            events={{ open: (e) => openGroup(e, key), toggle: () => { shut = { ...shut, [key]: open }; },
              selectAll: () => tickAll(key, g.shown.map((f) => f.path)), keyDown: onFilesKey, keyUp: onFilesKeyUp,
              picked: () => actOnPicked(key), all: () => commitList(g.list.id),
              discard: () => discardIn(key, picked),
              menu: (e) => openListMenu(e, g),
            }} />
            {#if open}
              {@render fileList(g.shown, key)}
              {#if !g.shown.length}
                <p class="empty">{needle && g.files.length ? "No files of this list match the filter." : "No changes yet: drag files here, or use Move to Changelist."}</p>
              {/if}
            {/if}
          </div>
        {/each}
        {#if untracked.length}
          <!-- New files git does not track yet, apart from the edits and folded at first, as in Sublime Merge. -->
          <div class="group">
            <ChangesGroupHeading group={"untracked"} files={shownUntracked} total={untracked.length} {color} {needle}
            expanded={untrackedOpen} selected={isAll("unstaged", true)} ticked={ticksOf("untracked")} picked={pickedUntracked} foldDisabled={!!needle}
            events={{ open: (e) => openGroup(e, "untracked"), toggle: () => { view.untracked = !view.untracked; view.untrackedTouched = true; },
              selectAll: () => tickAll("untracked", shownUntracked.map((f) => f.path)), keyDown: onFilesKey, keyUp: onFilesKeyUp,
              picked: () => actOnPicked("untracked"), all: () => stageIn("untracked", shownUntracked),
              discard: () => discardIn("untracked", pickedUntracked),

            }} />
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
        class:drop={canDrop("staged")}
        class:over={dropOn === "staged"}
        data-drop="staged"
        bind:this={stagedBox}
        style:min-height={stacked || shut.staged ? null : `min(calc(var(--diff-header-height) + ${Math.min(stagedHeight + lastRoom, stagedRows)}px), 45%)`}
      >
        <ChangesGroupHeading group={"staged"} files={shownStaged} total={staged.length} {color} {needle}
            expanded={!shut.staged} selected={isAll("staged")} ticked={ticksOf("staged")} picked={pickedStaged}
            events={{ open: (e) => openGroup(e, "staged"), toggle: () => { shut.staged = !shut.staged; },
              selectAll: () => tickAll("staged", shownStaged.map((f) => f.path)), keyDown: onFilesKey, keyUp: onFilesKeyUp,
              picked: () => actOnPicked("staged"), all: () => unstage(shownStaged),

            }} />
        {#if dragging && dragging.group !== "staged"}{@render dropRow("Drop to Stage")}{/if}
        {#if !shut.staged}
          <div class="staged-files" bind:this={stagedList}>
            <div bind:clientHeight={stagedRows}>
              {@render fileList(shownStaged, "staged")}
              {#if !shownStaged.length && !lastFiles.length}
                <p class="empty">{needle && staged.length ? "No staged files match the filter." : "Stage files or lines to add them to the next commit."}</p>
              {/if}
              {#if lastFiles.length}
                <ChangesAmendFiles files={lastFiles.filter(matches)} total={lastFiles.length} opened={lastOpen}
                  onOpen={openLast} onKeyDown={onFilesKey} onKeyUp={onFilesKeyUp} />
              {/if}
            </div>
          </div>
        {/if}
      </div>

      <CommitForm bind:this={commitForm} {draft} {branch} {color} {head} {locked} {staged} {unstaged} {conflicted} {amendedCount} {stage} {onChanged} {onResolve} lists={lists?.lists ?? null} listFiles={grouped.lists} />
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
        bind:headerHeight={diffHeaderHeight}
        {openUnstaged}
        {openStaged}
        {sideFiles}
        counted={tree?.counted !== false}
        {staged}
        {tracked}
        {untracked}
        {conflicted}
        {stagedByPath}
        {picker}
        onShow={show}
        onMore={() => reads.loadMore()}
        {onResolve}
        {act}
        stage={stageShown}
        {unstage}
        discard={discardShown}
        {stageAndNext}
        hunkChip={lists?.lists.length ? hunkChip : null}
        onHunkChip={openHunkMenu}
        {fileCaption}
        listTitle={shown?.kind === "all" && shown.list ? listName(lists, shown.list) : null}
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
  .head {
    min-height: 40px;
    padding: var(--space-2) var(--space-4);
    display: flex;
    align-items: center;
    gap: var(--space-3);
    border-bottom: 1px solid var(--sep);
  }
  h1 {
    display: flex;
    align-items: center;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    margin: 0;
    font-size: var(--text-heading);
    font-weight: 600;
    line-height: 1.3;
  }
  .head-title {
    overflow: hidden;
    text-overflow: ellipsis;
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
  }
  .tiny {
    width: 12px;
    height: 12px;
  }
  .sub {
    flex-shrink: 100;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .body {
    position: relative;
    flex-grow: 1;
    min-height: 0;
    display: flex;
  }
  .body.stacked {
    flex-direction: column;
  }
  .body:not(.stacked) {
    --changes-content-top: var(--space-2);
  }
  .files {
    flex-shrink: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--sep);
  }
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
    padding: var(--changes-content-top, 0px) var(--space-2) var(--space-1);
  }
  .group {
    display: flex;
    flex-direction: column;
  }
  .group + .group {
    margin-top: var(--space-2);
  }
  .group.staged {
    flex-shrink: 1000;
    min-height: var(--diff-header-height);
    padding: 0 var(--space-2);
  }
  .staged-files {
    min-height: 26px;
    overflow-y: auto;
  }
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
  .ghead {
    gap: 2px;
    padding: 0 var(--space-1) 0 2px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }
  .gtitle {
    font-weight: 700;
  }
  .gtitle.conflicted {
    color: var(--red);
    padding-left: 44px;
  }
  .ghead .n {
    letter-spacing: normal;
    text-transform: none;
  }
  .ghead {
    container-type: inline-size;
  }
  .n {
    font-weight: 400;
  }
  .empty {
    margin: 0;
    padding: var(--space-1) var(--space-3) var(--space-1) 26px;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .ghead {
    position: relative;
  }
  .tools {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: var(--space-3) var(--space-3) var(--space-1);
    flex-shrink: 0;
  }
  .filter {
    flex-grow: 1;
    min-width: 0;
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
</style>
