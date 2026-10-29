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
  import type { ChangelistEdit, Changelists, FileChange, FileDiff, Hunk, HistoryRow, Moving, Side, WorkingTree } from "./types";
  import { plate, splitPath, tint } from "./format";
  import { keyFocus as focusByKey, tabStepper } from "./tabs";
  import ChangesDiff from "./ChangesDiff.svelte";
  import type { LinePicker } from "./DiffView.svelte";
  import PathName from "./PathName.svelte";
  import CommitForm from "./CommitForm.svelte";
  import Menu, { menuIcons, type MenuEntry, type SubEntry } from "./Menu.svelte";
  import { fileMenu, filesMenu, type FileMenuActions } from "./changesMenu";
  import { BIG_FILE, bytes } from "./unusual";
  import { discardRequest, stageRequest, stashFilesRequest, unstageRequest } from "./changes";
  import { discardPartRequest, groupFiles, hunkLists, idOf, keyOf, listName, nameRequest, runList, runsOfLines, shareOf, stagePartRequest, type ListKey } from "./changelists";
  import type { Changelist } from "./types";
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
  /** The changelists, read with the files; null until read, or when they can't be. */
  let lists = $state<Changelists | null>(null);
  /** The unstaged files sorted into Unstaged, Untracked and the changelists. */
  const grouped = $derived(groupFiles(tree?.unstaged ?? [], lists));
  /** What the diff column shows: a file on one side, or every file of a side. */
  let shown = $state<Shown | null>(null);
  /** The diffs in the diff column, and what they were read for: until a new read is in, the
   *  column keeps the old diffs, with their own side's actions. */
  let diffs = $state<FileDiff[]>([]);
  let diffsOf = $state<Shown | null>(null);
  /** The lists of files: untracked files are unstaged ones listed apart, and each changelist
   *  holds some of the unstaged ones. */
  type List = ListKey;
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

  /** Which reading of the files `tree` is, for diffs of the same one; null for one of its own. */
  let reading: number | null = null;

  async function load() {
    try {
      const given = changes;
      const next = given ?? (await api.workingTree());
      const read = given ? snapshot : null;
      let listsError: string | null = null;
      const found = await api.changelists(read).catch((err) => ((listsError = String(err)), null));
      reading = read;
      tree = next;
      lists = found;
      error = listsError;
      // A file staged or unstaged whole is shown where it went; a selection keeps the files still there.
      shown = inItsList(keepShown(shown, next));
      // Ticks go with the files that left their list.
      pruneTicks();
      loads++;
    } catch (err) {
      error = String(err);
    }
  }

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
      lists = await api.editChangelists(edit);
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
    untrack(load);
  });

  $effect(() => {
    const target = shown;
    const wholeFile = whole;
    diffSettings();
    loads;
    const t = tree;
    const g = grouped;
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
          : target.list
            ? (g.lists.find((l) => l.list.id === target.list)?.files ?? [])
            : target.untracked
              ? g.untracked
              : [...t.conflicted, ...g.tracked];
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
      if (!needle) view.untracked = !view.untracked;
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

  /** A checkbox gives the keyboard to its row or heading, so ↑ ↓ and Space go on from there. */
  const keysTo = (event: MouseEvent, selector: string) => (event.currentTarget as HTMLElement).parentElement?.querySelector<HTMLElement>(selector)?.focus({ preventScroll: true });

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
        lists = await api.editChangelists({ kind: "create", name, moving });
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
        lists = await api.editChangelists({ kind: "rename", id: list.id, name });
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
      return (shown.side === "staged" ? staged : shown.list ? filesOf(keyOf(shown.list)) : shown.untracked ? untracked : tracked).filter((f) => chosen.has(f.path));
    }
    return shown.side === "staged" ? shownStaged : shown.list ? listOf(keyOf(shown.list)) : shown.untracked ? shownUntracked : [...shownConflicted, ...shownTracked];
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
  {@const on = group !== "conflicted" && ticksOf(group).has(file.path)}
  {@const open = isOpen(file, group)}
  <!-- The part of a file staged in part that the other list holds. -->
  {@const other = group === "staged" ? unstagedByPath.get(file.path) : unstagedList(group) ? stagedByPath.get(file.path) : undefined}
  <!-- What the list holds of a file it shares with another. -->
  {@const share = unstagedList(group) ? shareOf(lists, file.path, idOf(group)) : null}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="file"
    class:open
    class:nested
    class:bin={unstagedList(group)}
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
      {#if share}
        <span class="note partial" title="{share.runs} of its {share.total} changes are here; the other{share.total - share.runs === 1 ? ' is' : 's are'} in {share.others.map((id) => listName(lists, id)).join(' and ')}">
          <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="5.5" /><path class="fill" d="M8 2.5a5.5 5.5 0 0 0 0 11z" /></svg>{share.runs} of {share.total}
        </span>
      {/if}
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
    {#if unstagedList(group)}
      <button class="discard" onclick={() => discardIn(group, [file])} aria-label="{file.status === 'untracked' ? 'Delete' : 'Discard changes in'} {file.path}" title={file.status === "untracked" ? "Delete this new file" : "Discard changes"}>
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
  {@const n = files.filter((f) => ticksOf(group).has(f.path)).length}
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
{#snippet binPicked(files: FileChange[], group: List)}
  {@const fresh = files.every((f) => f.status === "untracked")}
  {@const what = files.length === 1 ? splitPath(files[0].path).name : `${files.length} files`}
  <button class="discard held" onclick={() => discardIn(group, files)} aria-label={fresh ? `Delete ${what}` : `Discard changes in ${what}`} title={fresh ? `Delete ${what}…` : `Discard changes in ${what}…`}>
    <svg class="icon" viewBox="0 0 16 16"><path d="M2.5 4.5h11M6 4.5V3a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1v1.5M4 4.5l.7 9a1 1 0 0 0 1 .9h4.6a1 1 0 0 0 1-.9l.7-9" /></svg>
  </button>
{/snippet}

{#snippet fileList(files: FileChange[], group: List | "conflicted")}
  {@const limit = listed[group] ?? LIST_STEP}
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
  <button class="more-rows" onclick={() => (listed = { ...listed, [group]: (listed[group] ?? LIST_STEP) + LIST_STEP })}
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
      {fileCount === 1 ? "file" : "files"}{#if conflicted.length} · {conflicted.length} conflicted{/if} · {staged.length} staged · {tracked.length} unstaged{#if listedCount}{` · ${listedCount} in ${grouped.lists.length === 1 ? "a changelist" : `${grouped.lists.length} changelists`}`}{/if}{#if untracked.length}{` · ${untracked.length} untracked`}{/if}
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
        <button class="button icon-only small" onclick={() => newList()} aria-label="New Changelist…" title="New Changelist: a named group of unstaged changes, committed on its own">
          <svg class="icon" viewBox="0 0 16 16"><path d="M2.5 4h7M2.5 8h7M2.5 12h4.5M12 9.5v5M9.5 12h5" /></svg>
        </button>
      </div>

      <div class="lists" class:drop={dragging?.group === "staged"} class:over={dragging?.group === "staged" && dropOn === "unstaged"} data-drop={dragging?.group === "staged" ? "unstaged" : undefined} bind:this={listsBox}>
        {#if dragging?.group === "staged"}{@render dropRow("Drop to Unstage")}{/if}
        {#if shownConflicted.length}
          <div class="group">
            <div class="ghead"><span class="gtitle conflicted">Conflicted</span><span class="n">{shownConflicted.length}</span></div>
            {@render fileList(shownConflicted, "conflicted")}
          </div>
        {/if}
        <div class="group" class:drop={dragging?.group !== "staged" && canDrop("unstaged")} class:over={dragging?.group !== "staged" && dropOn === "unstaged"} data-drop="unstaged">
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="ghead" class:on={isAll("unstaged")} style:background={isAll("unstaged") ? tint(color) : undefined} data-group="unstaged" oncontextmenu={openUnstagedMenu}>
            {@render tickAllBox("unstaged", shownTracked, "Select every unstaged file", ".gname")}
            {@render chevron(!shut.unstaged, shut.unstaged ? "Show the unstaged files" : "Hide the unstaged files", () => (shut.unstaged = !shut.unstaged))}
            <button class="gname" onclick={(e) => openGroup(e, "unstaged")} aria-current={isAll("unstaged") ? "true" : undefined} aria-expanded={!shut.unstaged} title={!isAll("unstaged") ? "Show every unstaged change" : shut.unstaged ? "Show the unstaged files" : "Hide the unstaged files"}>
              <span class="gtitle" style:color={plate(color)}>Unstaged</span><span class="n">{needle ? `${shownTracked.length} of ${tracked.length}` : tracked.length}</span>
            </button>
            {#if pickedUnstaged.length}
              {@render binPicked(pickedUnstaged, "unstaged")}
              <button class="button mini primary" onclick={() => actOnPicked("unstaged")}>{@render pickedLabel("Stage", pickedUnstaged.length)}</button>
            {:else if shownTracked.length}
              <button class="button mini" onclick={() => stageIn("unstaged", shownTracked)}>{needle && shownTracked.length < tracked.length ? "Stage Shown" : "Stage All"}</button>
            {/if}
          </div>
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
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="ghead" class:on={all} style:background={all ? tint(color) : undefined} data-group={key} oncontextmenu={(e) => openListMenu(e, g)}>
              {@render tickAllBox(key, g.shown, `Select every file of ${g.list.name}`, ".gname")}
              {@render chevron(open, open ? `Hide the files of ${g.list.name}` : `Show the files of ${g.list.name}`, () => (shut = { ...shut, [key]: open }))}
              <button class="gname" onclick={(e) => openGroup(e, key)} aria-current={all ? "true" : undefined} aria-expanded={open} title={!all ? `Show every change of ${g.list.name}` : open ? "Hide its files" : "Show its files"}>
                <svg class="icon clmark" style:color={plate(color)} viewBox="0 0 16 16"><path d="M3 3.5h10v9.5l-5-2.6L3 13z" /></svg>
                <span class="gtitle cltitle" style:color={plate(color)}>{g.list.name}</span><span class="n">{needle ? `${g.shown.length} of ${g.files.length}` : g.files.length}</span>
                {#if lists?.active === g.list.id}<span class="active" title="New changes go here: files that start changing join this list">Active</span>{/if}
              </button>
              {#if picked.length}
                {@render binPicked(picked, key)}
                <button class="button mini primary" onclick={() => actOnPicked(key)}>{@render pickedLabel("Stage", picked.length)}</button>
              {:else if g.files.length}
                <button class="button mini" onclick={() => commitList(g.list.id)} title="Commit the changes of {g.list.name}, and only those">Commit…</button>
              {/if}
              <button class="button icon-only mini" onclick={(e) => openListMenu(e, g)} aria-label="{g.list.name} menu" title="Stage, stash, rename or delete {g.list.name}">
                <svg class="icon" viewBox="0 0 16 16"><path d="M3.5 8h.01M8 8h.01M12.5 8h.01" /></svg>
              </button>
            </div>
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
            <div class="ghead" class:on={isAll("unstaged", true)} style:background={isAll("unstaged", true) ? tint(color) : undefined} data-group="untracked">
              {@render tickAllBox("untracked", shownUntracked, "Select every untracked file", ".gname")}
              {@render chevron(untrackedOpen, untrackedOpen ? "Hide the untracked files" : "Show the new files git does not track yet", () => (view.untracked = !view.untracked), !!needle)}
              <button class="gname" onclick={(e) => openGroup(e, "untracked")} aria-current={isAll("unstaged", true) ? "true" : undefined} aria-expanded={untrackedOpen} title={!isAll("unstaged", true) ? "Show every new file git does not track yet" : untrackedOpen ? "Hide the untracked files" : "Show the untracked files"}>
                <span class="gtitle" style:color={plate(color)}>Untracked</span><span class="n">{needle ? `${shownUntracked.length} of ${untracked.length}` : untracked.length}</span>
              </button>
              {#if pickedUntracked.length}
                {@render binPicked(pickedUntracked, "untracked")}
                <button class="button mini primary" onclick={() => actOnPicked("untracked")}>{@render pickedLabel("Stage", pickedUntracked.length)}</button>
              {:else if shownUntracked.length}
                <button class="button mini" onclick={() => stageIn("untracked", shownUntracked)}>{needle && shownUntracked.length < untracked.length ? "Stage Shown" : "Stage All"}</button>
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
        class:drop={canDrop("staged")}
        class:over={dropOn === "staged"}
        data-drop="staged"
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
  /* A changelist's name: the words it was given, as typed, after the list's mark. */
  .cltitle {
    min-width: 2em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-small);
    letter-spacing: normal;
    text-transform: none;
  }
  .clmark {
    width: 13px;
    height: 13px;
    flex-shrink: 0;
  }
  .active {
    display: inline-flex;
    align-items: center;
    height: 16px;
    padding: 0 6px;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: normal;
    text-transform: none;
    color: var(--accent-ink);
    background: var(--accent-quiet);
    flex-shrink: 0;
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
    /* No list mark, and Active as a dot: the list's name keeps the room. */
    .clmark {
      display: none;
    }
    .active {
      width: 7px;
      height: 7px;
      padding: 0;
      font-size: 0;
      background: var(--accent);
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
