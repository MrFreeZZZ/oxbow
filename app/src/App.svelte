<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { listen } from "@tauri-apps/api/event";
  import { api, settings } from "./lib/api";
  import { WORKTREE_ID, type History, type HistoryRow, type RecentRepo, type RefInfo, type ReflogEntry, type RepoSummary } from "./lib/types";
  import Sidebar from "./lib/Sidebar.svelte";
  import HistoryList from "./lib/HistoryList.svelte";
  import GraphLegend from "./lib/GraphLegend.svelte";
  import { focusedView } from "./lib/graph";
  import CommitPanel from "./lib/CommitPanel.svelte";
  import ChangesPanel from "./lib/ChangesPanel.svelte";
  import ConfirmSheet from "./lib/ConfirmSheet.svelte";
  import { confirm, type Request } from "./lib/confirm.svelte";
  import BranchPicker from "./lib/BranchPicker.svelte";
  import { detachRequest, keepRequest, newBranchRequest, switchRequest, type BranchContext } from "./lib/branches";
  import DetachedBanner from "./lib/DetachedBanner.svelte";
  import OperationBanner from "./lib/OperationBanner.svelte";
  import ConflictsView from "./lib/ConflictsView.svelte";
  import { abortRequest, continueRequest, describe, mergeRequest, skipRequest } from "./lib/merge";
  import { stashMenu, stashRequest } from "./lib/stash";
  import StashesView from "./lib/StashesView.svelte";
  import { ancestors, cherryPickRequest, editMessageRequest, resetModes, resetRequest, revertRequest, undoCommitRequest } from "./lib/commits";
  import type { FileChange, Operation } from "./lib/types";
  import Menu, { menuIcons, type MenuEntry } from "./lib/Menu.svelte";
  import { fetchRequest, pullRequest, pushRequest, syncBlock, unshallowRequest, type RemoteContext, type SyncFix } from "./lib/remote";
  import WhyPopover from "./lib/WhyPopover.svelte";
  import Welcome from "./lib/Welcome.svelte";
  import { prefs } from "./lib/prefs.svelte";
  import { deleteTagRequest, isLocalTag, newTagRequest, pushTagsRequest } from "./lib/tags";
  import SearchField from "./lib/SearchField.svelte";
  import SearchBar from "./lib/SearchBar.svelte";
  import { search, type Found } from "./lib/search.svelte";
  import { tick, untrack } from "svelte";
  import CompareView from "./lib/CompareView.svelte";
  import FileHistoryView from "./lib/FileHistoryView.svelte";
  import FindField from "./lib/FindField.svelte";
  import QuickOpen from "./lib/QuickOpen.svelte";
  import CommandPalette from "./lib/CommandPalette.svelte";
  import type { PaletteItem } from "./lib/palette";
  import { Keymap, combo, type ShortcutId } from "./lib/shortcuts";
  import { stepChange } from "./lib/diffRows";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import PairPanel from "./lib/PairPanel.svelte";
  import SnapshotPanel from "./lib/SnapshotPanel.svelte";
  import LostCommitPanel from "./lib/LostCommitPanel.svelte";
  import SubmodulePanel from "./lib/SubmodulePanel.svelte";
  import { nav } from "./lib/nav.svelte";
  import type { CompareMode } from "./lib/types";
  import CloneSheet from "./lib/CloneSheet.svelte";
  import NewRepoSheet from "./lib/NewRepoSheet.svelte";
  import { startPublish } from "./lib/publish";
  import { start } from "./lib/start.svelte";
  import OperationLog from "./lib/OperationLog.svelte";
  import { withKeys } from "./lib/keys";
  import { isRedo, lostRow, oplog, undoRequest } from "./lib/oplog.svelte";
  import EditStackView from "./lib/EditStackView.svelte";
  import ActivityPopover from "./lib/ActivityPopover.svelte";
  import { activity, failureShort, type ActivityItem } from "./lib/activity.svelte";
  import { addToCommitRequest, dropCommitRequest, rewordRequest, squashRequest } from "./lib/stack";
  import PullRequestView from "./lib/PullRequestView.svelte";
  import { github } from "./lib/github.svelte";
  import { lfs } from "./lib/lfs.svelte";
  import LfsBanner from "./lib/LfsBanner.svelte";
  import BisectBanner from "./lib/BisectBanner.svelte";
  import StatusLine from "./lib/StatusLine.svelte";
  import { lens } from "./lib/lens.svelte";
  import TipCard from "./lib/TipCard.svelte";
  import { tips } from "./lib/tips.svelte";
  import FixSheet from "./lib/FixSheet.svelte";
  import { fixChoices, fixRequest, fixStateLine, loadFixState, type FixContext, type FixId } from "./lib/fixes";
  import { bisectMarkRequest, bisectResetRequest, bisectStartRequest } from "./lib/bisect";
  import { keys, mac } from "./lib/keys";
  import type { BisectMark, OpEntry } from "./lib/types";
  import { fitWindow } from "./lib/windowSize";
  import { stepperOf } from "./lib/tabs";

  let repo = $state<RepoSummary | null>(null);
  let history = $state<History | null>(null);

  /** Past the unpushed commits a load walks, the rest are told in the background, and a reload
   *  marks them. */
  let settling = false;
  $effect(() => {
    const h = history;
    if (!h?.unpushedPending) return;
    untrack(() => {
      if (settling) return;
      const mine = generation;
      settling = true;
      const ids = h.rows.filter((r) => !r.unpushed && !r.worktree).map((r) => r.id);
      api.settleUnpushed(ids, "history-unpushed").then(
        (found) => {
          settling = false;
          if (found && mine === generation) refresh();
        },
        () => (settling = false),
      );
    });
  });
  let selected = $state<string | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  /** Commits of the first load. Each load as the list scrolls on doubles what is loaded, up to
   *  `MAX_STEP` more at a time, and brings only the rows that are new. */
  const PAGE = 2_000;
  const MAX_STEP = 50_000;
  /** Commits to load: a page, and more once the list has scrolled on or a commit further down was asked for. */
  let limit = PAGE;
  /** Older commits on their way: "more" for the next page, or the commit loaded down to. */
  let paging = $state<string | null>(null);
  /** Commits loaded down to that no branch reaches, so History never gets them. */
  let unreachable = new Set<string>();
  let list = $state<HistoryList>();
  /** Current Branch Only: History shows the checked-out branch, its upstream and the trunk alone. */
  let focusOnly = $state(false);
  let searchField = $state<SearchField>();
  let findField = $state<FindField>();
  let panelWidth = $state(640);
  /** Goes up on every reload of the history, so the changes panel reloads too. */
  let version = $state(0);
  /** The Conflicts screen is open in place of History. */
  let resolving = $state(false);
  const operation = $derived(history?.operation ?? null);
  const showConflicts = $derived(resolving && !!operation);
  /** Below this width the sidebar steps aside, so the graph keeps its room: a toolbar button
   *  slides it in over the content, and Settings and New Branch go into the «…» menu. */
  const NARROW = 1180;
  let windowWidth = $state(1360);
  /** The sidebar slid in over a narrow window. */
  let sideShown = $state(false);
  const narrow = $derived(windowWidth < NARROW);
  $effect(() => {
    if (!narrow) sideShown = false;
  });
  /** The sidebar of a wide window put away with its button, kept in settings.json between launches. */
  let sideHidden = $state(false);
  /** Whether the sidebar is out now: docked in a wide window, slid in over a narrow one. */
  const sideOut = $derived(narrow ? sideShown : !sideHidden);
  const SIDEBAR_KEYS = combo("sidebar");
  /** The sidebar is sliding in or out of a wide window. */
  let sliding = $state(false);
  let slideTimer: ReturnType<typeof setTimeout> | undefined;
  /** The sidebar button, always next to the traffic lights as in Mail: a narrow window slides the
   *  sidebar in over the content, a wide one docks or hides it and remembers that. */
  function toggleSidebar() {
    if (narrow) {
      sideShown = !sideShown;
      return;
    }
    sideHidden = !sideHidden;
    api.setSetting(settings.sidebarHidden, sideHidden).catch(() => {});
    // The details panel grows or shrinks in step with the sidebar sliding, so the graph doesn't jump.
    sliding = true;
    clearTimeout(slideTimer);
    slideTimer = setTimeout(() => (sliding = false), 300);
  }
  /** A sidebar action that also slides the sidebar of a narrow window out of the way. */
  function aside<A extends unknown[]>(act: (...args: A) => void): (...args: A) => void {
    return (...args) => {
      sideShown = false;
      act(...args);
    };
  }
  /** The «…» menu of a narrow window, open under its button. */
  let moreMenu = $state<{ x: number; y: number } | null>(null);
  /** The details panel can take any width that leaves the graph 360 px. */
  const GRAPH_MIN = 360;
  const PANEL_MIN = 320;
  const SIDEBAR = 248;
  const SETTINGS_ICON =
    "M12.78 6.52L14.44 6.56L14.44 9.44L12.78 9.48L12.42 10.33L13.57 11.54L11.54 13.57L10.33 12.42L9.48 12.78L9.44 14.44L6.56 14.44L6.52 12.78L5.67 12.42L4.46 13.57L2.43 11.54L3.58 10.33L3.22 9.48L1.56 9.44L1.56 6.56L3.22 6.52L3.58 5.67L2.43 4.46L4.46 2.43L5.67 3.58L6.52 3.22L6.56 1.56L9.44 1.56L9.48 3.22L10.33 3.58L11.54 2.43L13.57 4.46L12.42 5.67zM8 5.8a2.2 2.2 0 1 0 0 4.4a2.2 2.2 0 1 0 0-4.4";
  const CONSOLE_ICON = "M1.8 4a1.5 1.5 0 0 1 1.5-1.5h9.4A1.5 1.5 0 0 1 14.2 4v8a1.5 1.5 0 0 1-1.5 1.5H3.3A1.5 1.5 0 0 1 1.8 12zM4.8 6.2 6.8 8l-2 1.8M8.3 10H11";
  const NEW_BRANCH_ICON = "M3 3.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0-3 0M3 12.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0-3 0M4.5 5v6M11.5 2.5v6M8.5 5.5h6";
  /** New Branch, Console and Settings, which a narrow window's toolbar has no room for. */
  function moreEntries(): MenuEntry[] {
    const entries: MenuEntry[] = [];
    if (view !== "file" || showConflicts) entries.push({ kind: "item", label: history?.head.branch ? "New Branch…" : "Create a Branch at HEAD…", icon: NEW_BRANCH_ICON, run: newBranch });
    entries.push({ kind: "item", label: "Console", icon: CONSOLE_ICON, keys: keys(combo("console")), run: () => api.openConsole() });
    entries.push({ kind: "item", label: "Settings…", icon: SETTINGS_ICON, keys: keys("Mod+,"), run: () => api.openSettings() });
    return entries;
  }
  /** The details panel put away, so the graph takes the whole width; kept in settings.json. */
  let detailsHidden = $state(false);
  /** The details panel over the whole content, for a long diff; the graph comes back with the same button or Esc. */
  let expanded = $state(false);
  const DETAILS_KEYS = combo("details");
  const EXPAND_KEYS = combo("expand");
  function toggleDetails() {
    expanded = false;
    detailsHidden = !detailsHidden;
    api.setSetting(settings.detailsHidden, detailsHidden).catch(() => {});
    // Back from the whole width, the list keeps the selected commit in view.
    if (selected) requestAnimationFrame(() => list?.reveal(selected!));
  }
  function showDetails() {
    if (!detailsHidden) return;
    detailsHidden = false;
    api.setSetting(settings.detailsHidden, false).catch(() => {});
  }
  function toggleExpanded() {
    if (!expanded) showDetails();
    expanded = !expanded;
    if (!expanded && selected) requestAnimationFrame(() => list?.reveal(selected!));
  }
  /** Changes from the sidebar or the status line: the panel of the row above HEAD, shown even if put away. */
  function showChanges() {
    showDetails();
    pick(WORKTREE_ID);
  }
  const contentWidth = $derived(windowWidth - (narrow || sideHidden ? 0 : SIDEBAR));
  const panelMax = $derived(Math.max(PANEL_MIN, contentWidth - GRAPH_MIN));
  /** A hidden sidebar's room goes to the details panel, as in Mail: the graph keeps its width and only moves to the edge. */
  const panelBonus = $derived(!narrow && sideHidden ? SIDEBAR : 0);
  const shownPanelWidth = $derived(Math.min(Math.max(panelWidth + panelBonus, PANEL_MIN), panelMax));
  /** The screen next to the sidebar. */
  let view = $state<"history" | "stashes" | "oplog" | "compare" | "file" | "stack" | "pull">("history");
  /** The branch whose stack Edit Stack shows; the checked-out one when null. */
  let stackBranch = $state<string | null>(null);

  function openStack(branch: string | null) {
    stackBranch = branch;
    view = "stack";
    resolving = false;
  }
  /** The branch the Pull Request screen shows. */
  let pullBranch = $state<string | null>(null);

  function openPull(branch: string) {
    pullBranch = branch;
    view = "pull";
    resolving = false;
    if (repo) github.load(repo.path, true);
  }
  const shownPull = $derived(view === "pull" && pullBranch ? github.pullOf(pullBranch) : null);

  /** The screen File History goes back to. */
  let fileBack = $state<"history" | "stashes" | "oplog" | "compare">("history");
  // A diff's File History button opens the screen.
  $effect(() => {
    if (!nav.file) return;
    untrack(() => {
      // Coming from another screen starts without a find.
      if (view !== "file") {
        fileBack = view === "stack" || view === "pull" ? "history" : view;
        nav.find = { ...nav.find, text: "" };
      }
      view = "file";
      resolving = false;
    });
  });
  // After Quick Open, typing goes straight into Find in file.
  $effect(() => {
    if (!nav.focusFind || view !== "file" || !findField) return;
    nav.focusFind = false;
    requestAnimationFrame(() => findField?.focus());
  });
  /** What Compare shows: `target` against `base`. */
  let comparing = $state<{ base: string; target: string; mode: CompareMode }>({ base: "", target: "", mode: "split" });

  function openCompare(base: string, target: string, mode: CompareMode = comparing.mode) {
    comparing = { base, target, mode };
    view = "compare";
    resolving = false;
  }
  /** The stash picked on the Stashes screen. */
  let stashSel = $state<string | null>(null);
  /** Fix a Mistake's list, open, with what it planned from. */
  let fixing = $state<FixContext | null>(null);

  const branchCount = $derived(history ? history.refs.filter((r) => r.kind === "local").length : 0);
  const rowsById = $derived(new Map(history?.rows.map((r) => [r.id, r]) ?? []));
  /** What the graph shows: the whole history, or Current Branch Only once its rows are in. */
  const graphHistory = $derived(history && focusOnly ? focusedView(history) : history);
  /** The commits Current Branch Only shows; null while it is off. */
  const focusIds = $derived(focusOnly && history?.focus ? new Set(history.focus.rows.map((i) => history!.rows[i].id)) : null);
  const selectedRow = $derived(selected ? (rowsById.get(selected) ?? null) : null);
  // Another screen gives the graph back.
  $effect(() => {
    if (view !== "history" || showConflicts) expanded = false;
  });
  /** A second commit, ⌘-clicked, compared with the selected one in the details panel. */
  let second = $state<string | null>(null);
  /** The path of the submodule picked in the sidebar, shown in place of the commit's details. */
  let submoduleSel = $state<string | null>(null);
  const pickedSubmodule = $derived(submoduleSel ? (history?.submodules.find((s) => s.path === submoduleSel) ?? null) : null);
  const pair = $derived.by(() => {
    const other = second ? rowsById.get(second) : undefined;
    if (!history || !selectedRow || selectedRow.worktree || !other || other.worktree || other.id === selectedRow.id) return null;
    // Rows are newest first, so the later one in the list is the older commit.
    const rows = history.rows;
    return rows.indexOf(other) > rows.indexOf(selectedRow) ? { older: other, newer: selectedRow } : { older: selectedRow, newer: other };
  });
  /** What the details panel shows, named in its bar. */
  const panelLabel = $derived(
    pickedSubmodule
      ? "Submodule"
      : oplog.compare
        ? "Compared with a step"
        : oplog.lost
          ? "Lost commit"
          : pair
            ? "Two commits"
            : selectedRow?.worktree
              ? (selectedRow.summary === "Uncommitted changes" ? "Changes" : selectedRow.summary)
              : selectedRow
                ? "Commit"
                : "Details",
  );

  /** ⌘-click: compare with the selected commit; on the selected one itself, or with nothing to compare against, just select. */
  function pickSecond(id: string) {
    if (!selectedRow || selectedRow.worktree) return select(id);
    second = id === selected ? null : id;
    // The comparison is only in the details panel.
    if (second) showDetails();
  }
  const headRow = $derived(history?.head.commit ? (rowsById.get(history.head.commit) ?? null) : null);
  // The line the next commit goes on: the uncommitted row's, or HEAD's.
  const headColor = $derived(history?.rows.find((r) => r.worktree)?.graph.color ?? headRow?.graph.color ?? 0);
  // Children of every commit, newest first (rows are already newest first).
  const childrenOf = $derived.by(() => {
    const map = new Map<string, string[]>();
    for (const r of history?.rows ?? []) {
      if (r.worktree) continue;
      for (const parent of r.parents) {
        const list = map.get(parent);
        if (list) list.push(r.id);
        else map.set(parent, [r.id]);
      }
    }
    return map;
  });

  // Searching the history: the search runs again when the query, its filters or the commits change.
  const searching = $derived(!!history && view === "history" && !showConflicts && search.active);
  const hitMap = $derived(new Map(search.result?.hits.map((h) => [h.id, h.files]) ?? []));
  /** Matching commits further down than the loaded history, newest first. */
  const olderMatches = $derived(
    searching && search.result && history?.truncated ? search.result.hits.filter((h) => !rowsById.has(h.id) && !unreachable.has(h.id)).map((h) => h.id) : [],
  );
  /** Matching commits: the loaded ones in graph order, then the older ones. */
  const matches = $derived(
    searching && search.result ? [...(history?.rows.filter((r) => hitMap.has(r.id)).map((r) => r.id) ?? []), ...olderMatches] : [],
  );
  const found = $derived<Found | null>(
    searching && search.result ? { hits: hitMap, mode: search.parsed.mode, text: search.parsed.text, only: search.only } : null,
  );
  // Cheap to compare, so a reload that changed nothing doesn't search again.
  const historyKey = $derived(history ? `${history.rows.length}:${history.rows[0]?.id}:${history.head.commit}:${history.refs.map((r) => r.target).join()}` : "");
  const searchKey = $derived(search.key);
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    historyKey;
    searchKey;
    clearTimeout(searchTimer);
    if (!search.active) {
      untrack(() => search.run([]));
      return;
    }
    searchTimer = setTimeout(() => search.run(untrack(() => history?.rows ?? []), untrack(() => !!history?.truncated)), 220);
  });
  // A new search selects its first match; a reload with the same search keeps the selection.
  let selectedFor = "";
  $effect(() => {
    const first = matches[0];
    const key = search.resultKey;
    const only = search.only;
    if (!first) return;
    untrack(() => {
      const stale = !!selected && !matches.includes(selected);
      if (key === selectedFor && !(only && stale)) return;
      selectedFor = key;
      if (stale || !selected) select(first);
      else requestAnimationFrame(() => selected && list?.reveal(selected));
    });
  });

  // Opening a repository switches the one the backend answers for. Each open gets a number, and an
  // answer that belongs to an older one (a reload started before the switch, an earlier open) is
  // dropped, so the window never shows one repository's commits while the backend has another.
  let generation = 0;
  let opening: Promise<void> = Promise.resolve();

  function load(path: string): Promise<void> {
    const mine = ++generation;
    // One open at a time: the backend keeps whichever repository was opened last.
    opening = opening.then(() => loadNow(path, mine));
    return opening;
  }

  async function loadNow(path: string, mine: number) {
    if (mine !== generation) return;
    loading = true;
    error = null;
    // The commit panel must not ask the new repository about the old one's commits.
    selected = null;
    second = null;
    submoduleSel = null;
    search.clear();
    oplog.forget();
    lfs.forget();
    activity.reset();
    stopPaging();
    if (view === "compare" || view === "file" || view === "stack" || view === "pull") view = "history";
    try {
      limit = PAGE;
      unreachable = new Set();
      focusOnly = false;
      const summary = await api.openRepo(path);
      const next = await api.history(limit);
      if (mine !== generation) return;
      repo = summary;
      nav.repo = summary.path;
      github.load(summary.path);
      history = next;
      version++;
      oplog.load();
      lfs.load();
      if (repo) github.load(repo.path);
      // Start on the checked-out commit, like the design: HEAD's latest commit is selected.
      selected = next.head.commit ?? next.rows[0]?.id ?? null;
      requestAnimationFrame(() => selected && list?.reveal(selected));
    } catch (err) {
      if (mine === generation) error = String(err);
    } finally {
      if (mine === generation) loading = false;
    }
  }

  const targetOf = $derived(new Map(history?.refs.map((r) => [r.kind === "tag" ? `tag:${r.name}` : r.name, r.target]) ?? []));
  /** Lane color of a branch's latest commit; a name that doesn't exist yet gets HEAD's. */
  function colorOf(name: string): number {
    const target = targetOf.get(name);
    const row = target ? rowsById.get(target) : undefined;
    return row ? row.graph.color : headColor;
  }

  // Branch names and SHAs in the terminal blocks open their commit and light it up in History.
  $effect(() => {
    const h = history;
    lens.know(h && { refs: h.refs, rows: h.rows, row: (id) => rowsById.get(id), colorOf });
  });
  $effect(() => {
    const id = lens.pointed;
    if (id && view === "history") untrack(() => list?.reveal(id));
  });

  const branchCtx = $derived<BranchContext | null>(
    history && { history, colorOf, uncommitted: history.rows.find((r) => r.worktree)?.worktree?.files ?? 0 },
  );

  /** Confirm and run a change to the repository, then reload. After a checkout the new HEAD's
   *  latest commit is selected, as in the design. */
  /** Confirm and run an action, then reload. True when it ran and succeeded. */
  async function run(pending: Request | Promise<Request>): Promise<boolean> {
    let request: Request;
    try {
      request = await pending;
    } catch (err) {
      error = String(err);
      return false;
    }
    const before = history?.head;
    const opBefore = history?.operation ?? null;
    const ok = await confirm.run(request);
    await refresh();
    if (ok && request.action?.kind === "fetchTags") checkRemoteTags();
    // A stash made or put back is the newest, and the Stashes screen shows it.
    if (ok && (request.action?.kind === "stashPush" || request.action?.kind === "stashStore")) stashSel = history?.stashes[0]?.id ?? null;
    const after = history?.head;
    if (after?.commit && (after.branch !== before?.branch || after.commit !== before?.commit)) select(after.commit);
    openConflicts(opBefore);
    return ok;
  }
  confirm.runner = (request) => void run(request);
  confirm.onRan = (ran) => activity.ran(ran);

  /** ⌘Z: back to before the newest step of the Operation Log. */
  async function undoLatest() {
    oplog.open = false;
    await oplog.load();
    const latest = oplog.entries[0];
    if (latest) run(undoRequest(latest));
    else confirm.say("Nothing to undo yet.");
  }

  /** ⇧⌘Z: redo, when the newest step of the Operation Log is an undo. */
  async function redoLatest() {
    oplog.open = false;
    await oplog.load();
    const latest = oplog.entries[0];
    if (latest && isRedo(latest)) run(undoRequest(latest));
    else confirm.say("Nothing to redo: the newest step isn’t an undo.");
  }

  /** The Command Palette, or the list of recent repositories (⌃R), over the window. */
  let picker = $state<{ kind: "commands"; initial: string } | { kind: "recent"; repos: RecentRepo[] } | null>(null);
  /** What Quick Open starts with when the palette's ">" was taken away. */
  let quickInitial = $state("");
  let branchPicker = $state<BranchPicker>();
  const keymap = new Keymap();
  /** The toast saying a chord waits for its second key. */
  let chordNote: string | null = null;

  function openPalette() {
    nav.quickOpen = false;
    picker = picker?.kind === "commands" ? null : { kind: "commands", initial: "" };
  }

  async function openRecent() {
    nav.quickOpen = false;
    const repos = await api.recentRepos().catch(() => []);
    picker = { kind: "recent", repos: repos.filter((r) => !r.missing && r.path !== repo?.path) };
  }

  /** Changes, with the cursor in the commit's summary, as VS Code's Source Control view has it. */
  function commitChanges() {
    view = "history";
    resolving = false;
    if (!history?.rows.some((r) => r.worktree)) {
      confirm.say("Nothing to commit: there are no uncommitted changes.");
      return;
    }
    showChanges();
    // The panel comes up a frame or two later.
    let tries = 0;
    const focus = () => {
      const box = content?.querySelector<HTMLInputElement>('input[aria-label="Commit summary"]');
      if (box) box.focus();
      else if (++tries < 20) requestAnimationFrame(focus);
    };
    requestAnimationFrame(focus);
  }

  /** The next or the previous tab of the details panel, as ⌥⌘→ steps through VS Code's editors. */
  function stepTab(by: 1 | -1): boolean {
    const tabs = [...(content?.querySelectorAll<HTMLElement>('aside.panel [role="tablist"] [role="tab"]') ?? [])];
    if (!tabs.length) return false;
    const focused = document.activeElement && tabs.includes(document.activeElement as HTMLElement);
    // File tabs go by their whole list: of thousands of files only some tabs are drawn.
    const list = tabs[0].closest('[role="tablist"]');
    const step = stepperOf(list);
    if (step) {
      step(by, true);
      if (focused) focusOpenTab(list!);
      return true;
    }
    const at = tabs.findIndex((tab) => tab.getAttribute("aria-selected") === "true");
    const next = tabs[(at + by + tabs.length) % tabs.length];
    next.click();
    if (focused) keyFocus(next);
    return true;
  }

  /** Once a stepped tab list has drawn its new tab, the focus goes to it. */
  function focusOpenTab(list: Element) {
    tick().then(() => {
      const open = list.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]');
      if (open) keyFocus(open);
    });
  }

  /** The window's zoom as VS Code counts it: each step is 20% more or less. Kept in settings.json. */
  let zoomLevel = 0;
  function zoom(level: number, save = true) {
    zoomLevel = Math.max(-3, Math.min(5, Math.round(level)));
    getCurrentWebview()
      .setZoom(1.2 ** zoomLevel)
      .catch(() => {});
    if (save) api.setSetting(settings.zoomLevel, zoomLevel).catch(() => {});
  }

  /** History's search on, in the given mode: ⌘F for messages and the rest, ⇧⌘F for changed code. */
  function findInHistory(mode?: "code") {
    view = "history";
    resolving = false;
    if (mode) search.pick(mode);
    requestAnimationFrame(() => searchField?.focus());
  }

  /** Every command there is here and now, for the Command Palette. */
  function paletteCommands(): PaletteItem[] {
    const out: PaletteItem[] = [];
    const add = (id: string, category: string, title: string, run: () => void, keys?: ShortcutId) => out.push({ id, category, title, run, keys });
    add("settings", "Preferences", "Open Settings", () => api.openSettings(), "settings");
    add("console", "View", "Show Console", () => api.openConsole(), "console");
    add("open", "File", "Open Repository…", chooseRepo, "open");
    add("openRecent", "File", "Open Recent…", () => void openRecent(), "openRecent");
    add("clone", "Git", "Clone…", () => start.clone(), "clone");
    add("newRepo", "Git", "Initialize Repository…", () => start.newRepo(), "newRepo");
    add("zoomIn", "View", "Zoom In", () => zoom(zoomLevel + 1), "zoomIn");
    add("zoomOut", "View", "Zoom Out", () => zoom(zoomLevel - 1), "zoomOut");
    add("zoomReset", "View", "Reset Zoom", () => zoom(0), "zoomReset");
    if (!repo || !history || !branchCtx) return out;
    const h = history;
    const t = h.tracking;
    const busy = !!h.operation || !!h.bisect;
    add("closeRepo", "File", "Close Repository", closeRepo, "closeRepo");
    add("reload", "File", "Reload Repository", () => void reload(), "reload");
    add("quickOpen", "Go", "Go to File History…", () => (nav.quickOpen = true), "quickOpen");
    add("find", "Search", "Find in History", () => findInHistory(), "find");
    add("findCode", "Search", "Find in Changed Code", () => findInHistory("code"), "findCode");
    add("sidebar", "View", sideOut ? "Hide Sidebar" : "Show Sidebar", toggleSidebar, "sidebar");
    add("diffLayout", "View", prefs.get("oxbow.diff.layout") === "split" ? "Show Diffs Unified" : "Show Diffs Side by Side", () => void prefs.set("oxbow.diff.layout", prefs.get("oxbow.diff.layout") === "split" ? "unified" : "split"));
    add("nextChange", "Go", "Go to Next Change", () => void stepChange(1), "nextChange");
    add("previousChange", "Go", "Go to Previous Change", () => void stepChange(-1), "previousChange");
    add("history", "View", "Show History", () => ((view = "history"), (resolving = false)));
    add("stashes", "View", "Show Stashes", () => ((view = "stashes"), (resolving = false)));
    add("oplog", "View", "Show Operation Log", () => ((view = "oplog"), (resolving = false)));
    if (view === "history" && !showConflicts) {
      add("details", "View", detailsHidden ? "Show Details" : "Hide Details", toggleDetails, "details");
      add("expand", "View", expanded ? "Show Graph" : "Expand Details", toggleExpanded, "expand");
      add("focusOnly", "View", focusOnly ? "Show All Branches" : "Show Current Branch Only", () => void setFocusOnly(!focusOnly));
    }
    if (h.rows.some((r) => r.worktree)) {
      add("changes", "View", "Show Changes", () => ((view = "history"), (resolving = false), showChanges()), "changes");
      add("commit", "Git", "Commit…", commitChanges);
      if (!busy) add("stash", "Git", "Stash", () => void run(stashRequest(branchCtx!)));
    }
    if (h.remotes.length && remoteCtx) {
      add("fetch", "Git", "Fetch", () => void sync("fetch"));
      if (t) {
        add("pull", "Git", "Pull", () => void sync("pull"));
        if (!pullBlock) {
          add("pullRebase", "Git", "Pull (Rebase)", () => void sync("pull", "rebase"));
          add("pullMerge", "Git", "Pull (Merge)", () => void sync("pull", "merge"));
        }
        add("push", "Git", "Push", () => void sync("push"));
      } else if (h.head.branch) {
        add("publish", "Git", "Publish Branch…", () => void sync("push"));
      }
    } else if (h.head.branch && h.head.commit) {
      add("publishGitHub", "Git", "Publish to GitHub…", () => void publishToGitHub());
    }
    if (!busy) {
      add("checkout", "Git", "Checkout to…", () => {
        view = view === "file" ? "history" : view;
        resolving = false;
        requestAnimationFrame(() => branchPicker?.show());
      });
    }
    add("branch", "Git", h.head.branch ? "Create Branch…" : "Create Branch at HEAD…", newBranch);
    if (h.head.branch && !busy) add("editStack", "Git", "Edit Stack…", () => openStack(null));
    if (operation?.conflicted) add("resolve", "Git", "Resolve Conflicts", () => (resolving = true));
    add("undo", "Git", "Undo Last Step", () => void undoLatest(), "undo");
    add("redo", "Git", "Redo", () => void redoLatest(), "redo");
    add("fixMistake", "Git", "Fix a Mistake…", () => void openFixes(), "fixMistake");
    return out;
  }

  /** Run a window-wide shortcut. False when it can't run here and now, so the key does what it
   *  would do anyway, e.g. ⌘Z in a text field. */
  function shortcutKey(id: ShortcutId, event: KeyboardEvent): boolean {
    // A sheet keeps the keyboard: the window's commands wait until it closes, but for those
    // that open another window or zoom.
    const free = !confirm.request && !start.sheet && !document.querySelector('[aria-modal="true"]');
    const inRepo = !!history && free;
    switch (id) {
      case "settings":
        api.openSettings();
        return true;
      case "console":
        api.openConsole();
        return true;
      case "palette":
        if (!free) return false;
        openPalette();
        return true;
      case "openRecent":
        if (!free || loading) return false;
        if (picker?.kind === "recent") picker = null;
        else void openRecent();
        return true;
      case "open":
        if (!free || loading) return false;
        chooseRepo();
        return true;
      case "clone":
        if (!free || loading) return false;
        start.clone();
        return true;
      case "newRepo":
        if (!free || loading) return false;
        start.newRepo();
        return true;
      case "zoomIn":
      case "zoomOut":
      case "zoomReset":
        zoom(id === "zoomReset" ? 0 : zoomLevel + (id === "zoomIn" ? 1 : -1));
        return true;
    }
    if (!history || !free) return false;
    switch (id) {
      case "quickOpen":
        picker = null;
        nav.quickOpen = !nav.quickOpen;
        return true;
      case "find":
        // In File History, ⌘F finds in the file.
        if (view === "file" && !showConflicts) findField?.focus();
        else findInHistory();
        return true;
      case "findCode":
        findInHistory("code");
        return true;
      case "findNext":
      case "findPrevious": {
        const by = id === "findNext" ? 1 : -1;
        if (view === "file" && !showConflicts) nav.findStep(by);
        else if (view === "history" && !showConflicts) searchField?.step(by);
        else return false;
        return true;
      }
      case "nextChange":
      case "previousChange":
        return stepChange(id === "nextChange" ? 1 : -1);
      case "reload":
        reload();
        return true;
    }
    if (!inRepo) return false;
    switch (id) {
      case "fixMistake":
        if (fixing) return false;
        openFixes();
        return true;
      case "undo":
      case "redo":
        // In a text field ⌘Z stays the field's own.
        if (typing(event)) return false;
        if (id === "undo") undoLatest();
        else redoLatest();
        return true;
      case "sidebar":
        toggleSidebar();
        return true;
      case "details":
      case "expand":
        if (view !== "history" || showConflicts) return false;
        if (id === "details") toggleDetails();
        else toggleExpanded();
        return true;
      case "changes":
        view = "history";
        resolving = false;
        if (history.rows.some((r) => r.worktree)) showChanges();
        else confirm.say("No uncommitted changes.");
        return true;
      case "closeRepo":
        closeRepo();
        return true;
      case "nextTab":
      case "previousTab":
        return stepTab(id === "nextTab" ? 1 : -1);
    }
    return false;
  }

  /** The key went to a text field, where ⌘Z undoes typing. */
  function typing(event: KeyboardEvent): boolean {
    const target = event.target as HTMLElement | null;
    return !!target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable);
  }

  /** After an action stopped on conflicts, or a rebase stopped again on its next commit, the
   *  Conflicts screen opens on its own. */
  function openConflicts(before: Operation | null) {
    const op = history?.operation;
    if (!op) resolving = false;
    else if (op.conflicted && (!before || before.kind !== op.kind || before.commit?.id !== op.commit?.id)) resolving = true;
  }

  /** Where "Back to" goes from a detached HEAD: the branch checked out before, else the trunk. */
  const backTo = $derived.by(() => {
    if (!history || history.head.branch) return null;
    const local = (name: string | null) => (name && history!.refs.some((r) => r.kind === "local" && r.name === name) ? name : null);
    return local(history.previousBranch) ?? local(history.trunk);
  });

  /** The new branch button: while HEAD is detached, it keeps HEAD's commits on a branch. */
  function newBranch() {
    if (!branchCtx) return;
    run(history?.head.branch || !history?.head.commit ? newBranchRequest(branchCtx) : keepRequest(branchCtx));
  }

  /** Commits of the checked-out branch that are not on the trunk, in a straight line: the part of
   *  its stack the commit menu can rewrite. Empty with a merge among them. */
  const headStack = $derived.by(() => {
    const out = new Set<string>();
    if (!history?.head.branch || !history.head.commit || history.head.branch === history.trunk) return out;
    const trunkTip = history.trunk ? targetOf.get(history.trunk) : undefined;
    const onTrunk = ancestors(history, trunkTip ?? null);
    for (const id of ancestors(history, history.head.commit)) {
      if (onTrunk.has(id)) continue;
      if ((rowsById.get(id)?.parents.length ?? 0) > 1) return new Set<string>();
      out.add(id);
    }
    return out;
  });

  /** Open Fix a Mistake, from the repository as it is now; `commit` is a commit it was opened on. */
  async function openFixes(commit: string | null = null) {
    if (!branchCtx || confirm.request) return;
    try {
      fixing = await fixContext(commit);
    } catch (err) {
      confirm.fail(err);
    }
  }

  async function fixContext(commit: string | null): Promise<FixContext> {
    // The sheet's words come from History, its steps from the core: both from the repository as
    // it is now, e.g. after a push that turned a move into a revert.
    await refresh();
    const state = await loadFixState();
    const fc: FixContext = {
      ctx: branchCtx!,
      state,
      commit,
      back: () => void openFixes(commit),
      reopen: async (id) => fixRequest(await fixContext(commit), id),
    };
    return fc;
  }

  /** Open the sheet of the fix picked in Fix a Mistake. `fc` is taken before the picker closes. */
  function pickFix(fc: FixContext, id: FixId) {
    fixing = null;
    void run(fixRequest(fc, id));
  }

  /** Why no fix can run now: something git is in the middle of comes first. */
  const fixBusy = $derived(operation ? `Finish or abort the ${describe(operation).noun.toLowerCase()} in progress first.` : history?.bisect ? "Finish the bisect first." : null);

  /** The right-click menu of a commit in the graph. */
  function commitMenu(row: HistoryRow): MenuEntry[] {
    const ctx = branchCtx;
    if (!ctx || !history) return [];
    if (row.worktree) {
      return [
        { kind: "header", label: row.summary },
        { kind: "item", label: "Stash Changes…", icon: menuIcons.stash, run: () => run(stashRequest(ctx)) },
      ];
    }
    const stash = history.stashes.find((s) => s.id === row.id);
    if (stash) return stashMenu(ctx, stash, run, (text) => copyText(text, `Copied ${text.length > 40 ? "it" : text}.`));
    const item = (label: string, icon: string, act: () => void, danger = false): MenuEntry => ({ kind: "item", label, icon, run: act, danger });
    if (history.bisect) return bisectMenu(row, ctx, item);
    const isHead = row.id === history.head.commit;
    const here = history.head.branch ?? "HEAD";
    const entries: MenuEntry[] = [{ kind: "header", label: `${row.id.slice(0, 7)} · ${row.summary}` }];
    const group = (items: MenuEntry[]) => {
      if (items.length) entries.push(...(entries.length > 1 ? [{ kind: "sep" } as MenuEntry] : []), ...items);
    };
    // A commit many branches point at lists the first few; the rest are in the branch list.
    const others = row.labels.filter((l) => l.kind === "local" && l.name !== history!.head.branch);
    const switching: MenuEntry[] = others.slice(0, 3).map((label) => item(`Check Out ${label.name}`, menuIcons.checkout, () => run(switchRequest(ctx, label.name))));
    if (others.length > 3) switching.push({ kind: "note", label: `${others.length - 3} more branches here, in the branch list.` });
    if (!isHead) switching.push(item("Check Out This Commit…", menuIcons.checkout, () => run(detachRequest(ctx, row))));
    group(switching);

    // Branches at this commit can be merged or rebased onto right here, as from the branch list;
    // the commit itself can be copied, undone or made the branch's tip. None of it while a merge
    // or rebase waits to be finished.
    if (!history.operation) {
      const locals = new Set(row.labels.filter((l) => l.kind === "local").map((l) => l.name));
      const mergeable = row.labels
        .filter((l) => (l.kind === "local" || (l.kind === "remote" && !locals.has(l.name.slice(l.name.indexOf("/") + 1)))) && l.name !== here)
        .slice(0, 2);
      const merging: MenuEntry[] = [];
      for (const label of mergeable) {
        merging.push(item(`Merge ${label.name} into ${here}…`, menuIcons.merge, () => run(mergeRequest(ctx, label.name))));
        if (history.head.branch) merging.push(item(`Rebase ${here} onto ${label.name}…`, menuIcons.rebase, () => run(mergeRequest(ctx, label.name, "rebase"))));
      }
      group(merging);

      const inHead = ancestors(history, history.head.commit).has(row.id);
      const editing: MenuEntry[] = [];
      if (isHead && row.parents.length === 1) editing.push(item("Undo Commit", menuIcons.undo, () => run(undoCommitRequest(ctx, row))));
      if (isHead) editing.push(item("Edit Message…", menuIcons.edit, () => run(editMessageRequest(ctx, row))));
      // The checked-out branch's own commits, above the trunk, can be rewritten in place.
      const own = headStack.has(row.id);
      if (own && !isHead) {
        editing.push(item("Edit Message…", menuIcons.edit, () => run(rewordRequest(ctx, row))));
        const staged = history.rows[0]?.worktree?.staged ?? 0;
        if (staged) editing.push(item("Add Staged Changes to This Commit…", menuIcons.stage, () => run(addToCommitRequest(ctx, row))));
      }
      if (own && headStack.has(row.parents[0] ?? "")) editing.push(item("Squash into Previous…", menuIcons.merge, () => run(squashRequest(ctx, row))));
      if (own) editing.push(item("Drop Commit…", menuIcons.drop, () => run(dropCommitRequest(ctx, row)), true));
      if (own) editing.push(item("Edit Stack…", menuIcons.rebase, () => openStack(null)));
      if (!inHead && history.head.commit) editing.push(item(`Cherry-Pick onto ${here}`, menuIcons.cherry, () => run(cherryPickRequest(ctx, row))));
      if (inHead) editing.push(item("Revert Commit…", menuIcons.revert, () => run(revertRequest(ctx, row))));
      if (inHead && history.head.branch) editing.push(item("Fix a Mistake…", menuIcons.fix, () => openFixes(row.id)));
      if (!isHead && history.head.commit) {
        editing.push({
          kind: "sub",
          label: `Reset ${here} to Here`,
          icon: menuIcons.reset,
          entries: resetModes.map((m) => ({ label: m.label, hint: m.keeps, danger: m.mode === "hard", run: () => run(resetRequest(ctx, row, m.mode)) })),
        });
      }
      group(editing);

      // Bisect: from a branch tip (or HEAD) back to a tag, or from an older commit up to HEAD.
      const finding: MenuEntry[] = [];
      if (isHead || row.labels.some((l) => l.kind === "local")) finding.push(item("Find What Broke This…", menuIcons.bisect, () => run(bisectStartRequest(ctx, { bad: row }))));
      else if (inHead) finding.push(item("Find First Bad Commit After This…", menuIcons.bisect, () => run(bisectStartRequest(ctx, { good: row }))));
      group(finding);
    }

    group([isHead && !history.head.branch ? item("Create Branch…", menuIcons.branch, () => run(keepRequest(ctx))) : item("New Branch from Here…", menuIcons.branch, () => run(newBranchRequest(ctx, undefined, row)))]);
    // Tags: a new one here, and pushing or deleting the ones on this commit.
    // Push Tag and Delete Tag list the commit's tags in submenus, like Reset and Copy.
    const tagging: MenuEntry[] = [item("New Tag Here…", menuIcons.tag, () => run(newTagRequest(ctx, row)))];
    const tagsHere = row.labels.filter((l) => l.kind === "tag").map((l) => l.name);
    const unpushed = history.defaultRemote ? tagsHere.filter((name) => isLocalTag(ctx, name)) : [];
    if (unpushed.length) {
      tagging.push({
        kind: "sub",
        label: `Push Tag to ${history.defaultRemote}`,
        icon: menuIcons.push,
        entries: [
          ...unpushed.map((name) => ({ label: name, run: () => run(pushTagsRequest(ctx, [name])) })),
          ...(unpushed.length > 1 ? [{ label: `All ${unpushed.length}`, run: () => run(pushTagsRequest(ctx, unpushed)) }] : []),
        ],
      });
    }
    if (tagsHere.length) {
      tagging.push({
        kind: "sub",
        label: "Delete Tag",
        icon: menuIcons.drop,
        entries: tagsHere.map((name) => ({ label: `${name}…`, danger: true, run: () => run(deleteTagRequest(ctx, name)) })),
      });
    }
    group(tagging);
    // Compare this commit, named by its branch when one ends here, with HEAD or the default branch.
    const name = row.labels.find((l) => l.kind === "local")?.name ?? row.id.slice(0, 7);
    const against = [...new Set([isHead ? null : here, history.trunk])].filter((b): b is string => !!b && b !== name);
    if (against.length) {
      group([{ kind: "sub", label: "Compare", icon: menuIcons.compare, entries: against.map((b) => ({ label: `With ${b}`, run: () => openCompare(b, name) })) }]);
    }
    group([
      {
        kind: "sub",
        label: "Copy",
        icon: menuIcons.copy,
        entries: [
          { label: "SHA", hint: row.id.slice(0, 7) + "…", run: () => copyText(row.id, `Copied ${row.id.slice(0, 7)}.`) },
          { label: "Short SHA", hint: row.id.slice(0, 7), run: () => copyText(row.id.slice(0, 7), `Copied ${row.id.slice(0, 7)}.`) },
          { label: "Message", run: () => copyMessage(row.id) },
        ],
      },
    ]);
    return entries;
  }

  /** The commit menu while bisecting: answers about the commit, Stop or Finish, and the
   *  things that don't move HEAD. */
  function bisectMenu(row: HistoryRow, ctx: BranchContext, item: (label: string, icon: string, act: () => void, danger?: boolean) => MenuEntry): MenuEntry[] {
    const bisect = history!.bisect!;
    const entries: MenuEntry[] = [{ kind: "header", label: `${row.id.slice(0, 7)} · ${row.summary}` }];
    if (!bisect.found) {
      const testing = row.id === history!.head.commit;
      const mark = (label: string, icon: string, kind: BisectMark, key: string): MenuEntry => ({ ...item(label, icon, () => run(bisectMarkRequest(ctx, kind, row))), ...(testing ? { keys: keys(key) } : {}) });
      entries.push(mark("Mark as Good", menuIcons.good, "good", "G"), mark("Mark as Bad", menuIcons.bad, "bad", "B"), mark("Skip This Commit", menuIcons.skip, "skip", "S"), { kind: "sep" });
    }
    entries.push(item(bisect.found ? "Finish Bisect…" : "Stop Bisect…", menuIcons.bisect, () => run(bisectResetRequest(ctx, bisect))), { kind: "sep" });
    entries.push(item("New Branch from Here…", menuIcons.branch, () => run(newBranchRequest(ctx, undefined, row))), item("New Tag Here…", menuIcons.tag, () => run(newTagRequest(ctx, row))));
    const name = row.labels.find((l) => l.kind === "local")?.name ?? row.id.slice(0, 7);
    const against = [bisect.startIsBranch ? bisect.start : null, history!.trunk].filter((b, i, all): b is string => !!b && b !== name && all.indexOf(b) === i);
    entries.push({ kind: "sep" });
    if (against.length) entries.push({ kind: "sub", label: "Compare", icon: menuIcons.compare, entries: against.map((b) => ({ label: `With ${b}`, run: () => openCompare(b, name) })) });
    entries.push({
      kind: "sub",
      label: "Copy",
      icon: menuIcons.copy,
      entries: [
        { label: "SHA", hint: row.id.slice(0, 7) + "…", run: () => copyText(row.id, `Copied ${row.id.slice(0, 7)}.`) },
        { label: "Short SHA", hint: row.id.slice(0, 7), run: () => copyText(row.id.slice(0, 7), `Copied ${row.id.slice(0, 7)}.`) },
        { label: "Message", run: () => copyMessage(row.id) },
      ],
    });
    return entries;
  }

  /** G, B or S while bisecting: the answer for the commit under test. */
  function bisectKey(mark: BisectMark) {
    if (!branchCtx || !history?.bisect || history.bisect.found || !history.bisect.testing) return;
    if (mark === "skip" && !(history.bisect.good.length && history.bisect.bad.length)) return;
    run(bisectMarkRequest(branchCtx, mark));
  }

  function copyText(text: string, done: string) {
    navigator.clipboard.writeText(text).then(
      () => confirm.say(done, "done"),
      () => confirm.say("Couldn’t copy to the clipboard.", "error"),
    );
  }

  async function copyMessage(id: string) {
    try {
      const detail = await api.commitDetail(id);
      copyText([detail.summary, detail.body.trim()].filter(Boolean).join("\n\n"), "Copied the message.");
    } catch (err) {
      error = String(err);
    }
  }

  const remoteCtx = $derived<RemoteContext | null>(
    history && {
      branch: history.head.branch,
      color: headColor,
      tracking: history.tracking,
      remote: history.tracking?.remote ?? history.defaultRemote,
      remotes: history.remotes,
      unpushed: history.rows.filter((r) => r.unpushed && !r.worktree && r.graph.branch === history!.head.branch),
      operation: history.operation,
      bisect: history.bisect,
    },
  );
  const pullBlock = $derived(remoteCtx && syncBlock(remoteCtx, "pull"));
  const pushBlock = $derived(remoteCtx && syncBlock(remoteCtx, "push"));
  /** The grey Pull or Push button clicked last: its reason shows under it. */
  let why = $state<"pull" | "push" | null>(null);
  const whyBlock = $derived(why === "pull" ? pullBlock : why === "push" ? pushBlock : null);
  // Once the button can run again, its reason is gone for good.
  $effect(() => {
    if (why && !whyBlock) why = null;
  });

  /** The way out a reason offers, e.g. Publish… when the branch has no upstream to pull from. */
  function fixSync(fix: SyncFix) {
    why = null;
    const op = history?.operation;
    if (fix === "publish") void sync("push");
    else if (fix === "newBranch") newBranch();
    else if (fix === "finishBisect" && history?.bisect && branchCtx) run(bisectResetRequest(branchCtx, history.bisect));
    else if (fix === "resolve" && op) resolving = true;
    else if (fix === "continue" && op && branchCtx) run(continueRequest(branchCtx, op));
    else if (fix === "abort" && op && branchCtx) run(abortRequest(branchCtx, op));
  }

  /** A repository with no remote: make one on GitHub and push there. */
  async function publishToGitHub() {
    if (!repo || !history?.head.branch) return;
    if (!(await api.githubAccount().catch(() => null))) {
      confirm.say("Sign in to GitHub in Settings › Accounts first.", "warn");
      api.openSettings();
      return;
    }
    await run(startPublish(repo.path, repo.name, history.head.branch, headColor));
  }

  /** `mode` picks merge or rebase for this one pull, over Settings › Git. */
  async function sync(kind: "fetch" | "pull" | "push", mode?: "merge" | "rebase") {
    // While a fetch runs, Fetch shows its progress, and Pull or Push waits for it in line.
    if (kind === "fetch" && (activity.running || activity.problem)) {
      activity.open = !activity.open;
      return;
    }
    if (kind !== "fetch" && activity.running) {
      const branch = history?.head.branch ?? "HEAD";
      activity.queue(kind, kind === "push" ? `Push ${branch}` : `Pull into ${branch}`, mode);
      return;
    }
    // Commits made outside the app since the last reload belong in the sheet.
    await refresh();
    if (!remoteCtx) return;
    // Pull or Push that can't run says why under its button, as the grey button does.
    if (kind !== "fetch" && syncBlock(remoteCtx, kind)) {
      why = kind;
      return;
    }
    if (kind === "fetch") {
      // The sheet closes on Fetch: the fetch runs in the background, with its progress on the button.
      await confirm.run({ ...fetchRequest(remoteCtx), background: () => void backgroundFetch(false) });
      return;
    }
    const setup =
      kind === "pull"
        ? {
            ...(await api.pullSetup().catch(() => ({ mode: "rebase" as const, autostash: true }))),
            ...(mode ? { mode } : {}),
            fetchFirst: prefs.get("oxbow.pull.fetchFirst"),
            // Not only when there are submodules now: the pull may bring the first one.
            submodules: prefs.get("oxbow.pull.submodules"),
            hasSubmodules: !!history?.submodules.length,
          }
        : undefined;
    // Submodule commits nobody else has would leave the pushed commits pointing at nothing.
    const unpushedSubs = kind === "push" && history?.submodules.length ? await api.unpushedSubmodules().catch(() => []) : [];
    const request = kind === "pull" ? pullRequest(remoteCtx, setup) : pushRequest(remoteCtx, unpushedSubs);
    const opBefore = history?.operation ?? null;
    await confirm.run(request);
    // Even a failed pull or push may have fetched, so the counts are worth reloading either way.
    await refresh();
    checkRemoteTags();
    openConflicts(opBefore);
  }
  activity.startQueued = (item) => {
    if (item.kind !== "action" && item.kind !== "reload") void sync(item.kind, item.mode);
  };

  /** What Fetch fetches: the one remote, or all of them, as its sheet says. */
  function fetchTarget(): { remote: string | null; label: string } | null {
    if (!history?.remotes.length) return null;
    const remote = remoteCtx?.remote ?? history.remotes[0];
    return history.remotes.length > 1 ? { remote: null, label: "all remotes" } : { remote, label: remote };
  }

  /** Fetch in the background; one someone asked for says how it went. */
  async function backgroundFetch(auto: boolean) {
    const target = fetchTarget();
    if (!target) return;
    const ok = await activity.fetch(target.remote, target.label, auto);
    if (!ok) return;
    await api.refreshRemoteTags().catch(() => false);
    await refresh();
    if (!auto) confirm.say(activity.recent[0]?.sub === "Nothing new" ? `Fetched ${target.label}. Nothing new.` : `Fetched ${target.label}.`, "done");
  }

  /** Fix… of a failed fetch in Activity: the sheet with the way out, e.g. Add Key to Agent. */
  async function fixFetch(item: ActivityItem) {
    activity.open = false;
    const target = fetchTarget();
    if (!remoteCtx || !item.failure || !target) return;
    const ok = await confirm.show(fetchRequest(remoteCtx), item.failure, item.lines);
    const ran = confirm.ran;
    await refresh();
    if (!ok) return;
    if (ran?.kind === "fetch") {
      activity.fixed(target.label);
      checkRemoteTags();
    } else if (ran?.kind === "setRemoteUrl") {
      // A new address: see whether it works.
      backgroundFetch(false);
    }
  }

  /** Ask the remote which tags it has, so the ones it lacks show as local; quiet when offline. */
  async function checkRemoteTags() {
    if (!history?.remotes.length) return;
    if (await api.refreshRemoteTags().catch(() => false)) await refresh();
  }

  /** Reload after the repository changed, here or outside the app, keeping the selection when it still exists.
   *  One reload runs at a time: asked for meanwhile (focus, a fetch, an action finishing), one more
   *  follows it, for everyone who asked, so that it sees what changed. */
  function refresh(): Promise<void> {
    if (!repo || loading) return Promise.resolve();
    // Older commits are on their way: reload once they are in, with the new limit.
    if (paging) {
      refreshAfterPaging = true;
      return Promise.resolve();
    }
    if (refreshQueued) return refreshQueued;
    if (refreshing) {
      const queued = refreshing.then(() => {
        if (refreshQueued === queued) refreshQueued = null;
        return refresh();
      });
      refreshQueued = queued;
      return queued;
    }
    const done = refreshNow().finally(() => {
      if (refreshing === done) refreshing = null;
    });
    refreshing = done;
    return done;
  }
  /** Reload, asked for: Activity gets what changed and the git commands it ran, and unless
   *  Activity is open to show it, so does a toast. */
  async function reload() {
    const before = history;
    await refresh();
    const after = history;
    if (!before || !after || after === before) return;
    const sub = reloadNews(before, after);
    activity.reloaded(sub, after.reads);
    if (!activity.open) confirm.say(`Reloaded. ${sub}.`, "done");
  }

  /** What a reload found: "Nothing new", or "3 new commits · origin/main moved · 2 uncommitted files". */
  function reloadNews(before: History, after: History): string {
    const parts: string[] = [];
    const known = new Set(before.rows.map((r) => r.id));
    const fresh = after.rows.filter((r) => !r.worktree && !known.has(r.id)).length;
    if (fresh) parts.push(`${fresh} new ${fresh === 1 ? "commit" : "commits"}`);
    const key = (r: RefInfo) => `${r.kind} ${r.name}`;
    const was = new Map(before.refs.map((r) => [key(r), r.target]));
    const now = new Set(after.refs.map(key));
    const refs = [
      ...after.refs.filter((r) => was.get(key(r)) !== r.target).map((r) => `${r.name} ${was.has(key(r)) ? "moved" : "is new"}`),
      ...before.refs.filter((r) => !now.has(key(r))).map((r) => `${r.name} is gone`),
    ];
    if (refs.length === 1) parts.push(refs[0]);
    else if (refs.length) parts.push(`${refs.length} branches and tags changed`);
    const files = (h: History) => h.rows[0]?.worktree?.files ?? 0;
    if (files(after) !== files(before)) parts.push(files(after) ? `${files(after)} uncommitted ${files(after) === 1 ? "file" : "files"}` : "No uncommitted changes");
    // The same number of files can still be other files, or other edits in them.
    else if (changesKey(after) !== changesKey(before) || after.changesStamp !== before.changesStamp) parts.push("Uncommitted changes updated");
    if (after.stashes.length !== before.stashes.length) parts.push(`${after.stashes.length} ${after.stashes.length === 1 ? "stash" : "stashes"}`);
    else if (after.stashes.map((s) => s.id).join() !== before.stashes.map((s) => s.id).join()) parts.push("Stashes changed");
    return parts.length ? parts.join(" · ") : "Nothing new";
  }

  /** Which files are changed, staged or not, and by how many lines: staging part of a file
   *  changes it, which the files on disk don't show. */
  function changesKey(h: History): string {
    const c = h.changes;
    if (!c) return "";
    const side = (name: string, files: FileChange[]) => files.map((f) => `${name} ${f.status} ${f.path} ${f.additions} ${f.deletions}`);
    return [...side("s", c.staged), ...side("u", c.unstaged), ...side("c", c.conflicted)].join("\n");
  }

  let refreshing: Promise<void> | null = null;
  /** The reload that follows the one under way. */
  let refreshQueued: Promise<void> | null = null;
  let refreshAfterPaging = false;
  /** Number of the page load under way: one that finishes after another started, or after the
   *  repository changed, leaves `paging` to the newer one. */
  let pagingToken = 0;

  /** Another repository, or none: older commits on their way belong to the old one. */
  function stopPaging() {
    pagingToken++;
    paging = null;
    refreshAfterPaging = false;
    settling = false;
    api.stopReads(["history-unpushed"]).catch(() => {});
  }

  async function refreshNow() {
    const mine = generation;
    const shown = history;
    try {
      const next = withRows(await api.history(limit, null, shown?.key ?? null, focusOnly), shown);
      // Another repository was opened meanwhile: this answer may come from the old one.
      if (mine !== generation || loading) return;
      history = next;
      version++;
      oplog.load();
      lfs.load();
      if (!selected || !next.rows.some((r) => r.id === selected)) selected = next.head.commit ?? next.rows[0]?.id ?? null;
      // Current Branch Only follows a checkout: a commit it no longer shows gives way to HEAD.
      else if (next.focus && !next.focus.rows.some((i) => next.rows[i].id === selected)) selected = next.head.commit ?? selected;
    } catch (err) {
      if (mine === generation) error = String(err);
    }
  }

  /** `next` with the commit rows it left out because `shown`, the history it was asked with, has
   *  them. The same key, the same rows: only the row of uncommitted changes comes anew.
   *  Otherwise `pieces` say which of `shown`'s rows stand where, between the new ones. */
  function withRows(next: History, shown: History | null): History {
    if (!shown) return next;
    if (next.same) {
      const commits = shown.rows[0]?.worktree ? shown.rows.slice(1) : shown.rows;
      return { ...next, rows: [...next.rows, ...commits], runs: shown.runs, leadIns: shown.leadIns };
    }
    if (!next.pieces.length) return next;
    const rows: HistoryRow[] = [];
    let fresh = 0;
    for (const piece of next.pieces) {
      for (let k = 0; k < piece.len; k++) rows.push(piece.known === null ? next.rows[fresh++] : shown.rows[piece.known + k]);
    }
    return { ...next, rows, pieces: [] };
  }

  /** Load the next page of older commits, or with `until` the history down to that commit.
   *  Whether `until` is in History now. */
  async function loadOlder(until: string | null = null): Promise<boolean> {
    if (!repo || loading || paging || !history?.truncated) return false;
    const mine = generation;
    const token = ++pagingToken;
    paging = until ?? "more";
    try {
      // A reload already on its way would land after this one, with fewer commits.
      while (refreshing) await refreshing;
      const shown = history;
      const next = withRows(await api.history(until ? limit : limit + Math.min(limit, MAX_STEP), until, shown?.key ?? null, focusOnly), shown);
      if (mine !== generation || loading) return false;
      limit = Math.max(limit, next.loaded);
      history = next;
      const found = !!until && next.rows.some((r) => r.id === until);
      if (until && !found) unreachable.add(until);
      return found;
    } catch (err) {
      if (mine === generation) error = String(err);
      return false;
    } finally {
      if (token === pagingToken) {
        paging = null;
        if (refreshAfterPaging) {
          refreshAfterPaging = false;
          refresh();
        }
      }
    }
  }

  /** Make sure History has the commit, loading older commits down to it when it is further down
   *  than what is loaded. Whether History has it. */
  async function ensureLoaded(id: string): Promise<boolean> {
    if (rowsById.has(id)) return true;
    if (!history?.truncated || unreachable.has(id)) return false;
    return loadOlder(id);
  }

  // Background fetch, from Settings › General: keeps ahead/behind counts current. It skips a
  // turn while a sheet is open or the window is hidden; a failure only puts the amber dot on
  // Fetch, and Activity says what is wrong.
  const hasRemotes = $derived(!!history?.remotes.length);
  $effect(() => {
    if (!repo || !hasRemotes || !prefs.get("oxbow.fetch.auto")) return;
    const fetchNow = () => {
      if (confirm.request || document.hidden || loading || activity.running) return;
      backgroundFetch(true);
    };
    const first = setTimeout(fetchNow, 5000);
    const timer = setInterval(fetchNow, prefs.get("oxbow.fetch.interval") * 60_000);
    return () => {
      clearTimeout(first);
      clearInterval(timer);
    };
  });

  async function chooseRepo() {
    const path = await open({ directory: true, multiple: false, title: "Open Repository" });
    if (typeof path === "string") await load(path);
  }

  /** A clone or a new repository is ready: open it. */
  function started(path: string) {
    start.sheet = null;
    load(path);
  }

  /** Back to the Welcome window; the repository stays in Recent Repositories. */
  function closeRepo() {
    ++generation;
    stopPaging();
    activity.reset();
    repo = null;
    history = null;
    selected = null;
    second = null;
    search.clear();
    oplog.forget();
    view = "history";
    nav.quickOpen = false;
    error = null;
    loading = false;
  }

  /** Current Branch Only on or off. Off shows the whole history at once; on loads its rows. */
  async function setFocusOnly(on: boolean) {
    focusOnly = on;
    if (on && !history?.focus) await refresh();
    // The selected commit stays in view, or gives way to HEAD when the filter hides it.
    if (focusIds && selected && !focusIds.has(selected)) selected = history?.head.commit ?? selected;
    const keep = selected;
    requestAnimationFrame(() => keep && keep === selected && list?.reveal(keep));
  }

  /** A commit Current Branch Only hides turns it off, so the graph shows what is selected. */
  function unhide(id: string) {
    if (focusIds && !focusIds.has(id)) focusOnly = false;
  }

  function select(id: string) {
    unhide(id);
    selected = id;
    second = null;
    submoduleSel = null;
    oplog.compare = null;
    oplog.lost = null;
    if (rowsById.has(id)) return list?.reveal(id);
    ensureLoaded(id).then((ok) => ok && selected === id && requestAnimationFrame(() => list?.reveal(id)));
  }

  /** A commit from git's reflog: selected in History when it is there, otherwise shown on its
   *  own in the details panel. */
  async function showMove(entry: ReflogEntry) {
    second = null;
    oplog.compare = null;
    view = "history";
    resolving = false;
    showDetails();
    if (await ensureLoaded(entry.id)) {
      oplog.lost = null;
      selected = entry.id;
      requestAnimationFrame(() => list?.reveal(entry.id));
    } else {
      oplog.lost = entry;
    }
  }

  /** The repository now beside how it was at a step of the Operation Log, in History's details. */
  function compareStep(entry: OpEntry, after: boolean) {
    second = null;
    oplog.lost = null;
    oplog.compare = { entry, after };
    view = "history";
    resolving = false;
    showDetails();
  }

  /** From the lost commit's panel: a parent History has, or another lost commit. */
  function selectFromLost(id: string) {
    const other = oplog.reflog.find((e) => e.id === id);
    if (rowsById.has(id) || !other) select(id);
    else oplog.lost = other;
  }

  /** A commit from the reflog takes the color of the branch whose reflog has it, else HEAD's. */
  function moveColor(entry: ReflogEntry): number {
    const branch = history?.refs.some((r) => r.kind === "local" && r.name === entry.source);
    return branch ? colorOf(entry.source) : headColor;
  }

  /** New Branch at a commit from the reflog, staying where you are. */
  function branchAtMove(entry: ReflogEntry) {
    if (!branchCtx) return;
    run(newBranchRequest(branchCtx, undefined, rowsById.get(entry.id) ?? lostRow(entry, moveColor(entry)), false));
  }

  // Once a branch keeps the lost commit shown, History has it: select it there.
  $effect(() => {
    const lost = oplog.lost;
    if (lost && rowsById.has(lost.id)) {
      oplog.lost = null;
      selected = lost.id;
      requestAnimationFrame(() => list?.reveal(lost.id));
    }
  });

  /** Picking a commit from the sidebar goes back to History, which mounts the graph again. */
  function pick(id: string) {
    unhide(id);
    selected = id;
    second = null;
    submoduleSel = null;
    oplog.compare = null;
    oplog.lost = null;
    if (!rowsById.has(id)) ensureLoaded(id).then((ok) => ok && selected === id && requestAnimationFrame(() => list?.reveal(id)));
    if (view === "history" && !showConflicts) return list?.reveal(id);
    view = "history";
    resolving = false;
    requestAnimationFrame(() => list?.reveal(id));
  }

  /** A submodule from the sidebar: its card in the details panel, next to History. */
  function showSubmodule(path: string) {
    showDetails();
    submoduleSel = path;
    second = null;
    oplog.compare = null;
    oplog.lost = null;
    if (view !== "history" || showConflicts) {
      view = "history";
      resolving = false;
      if (selected) requestAnimationFrame(() => list?.reveal(selected!));
    }
  }

  /** Once the repository to open at launch is known, so Welcome doesn't shrink the window first. */
  let launched = $state(false);
  // Welcome is a compact window; a repository gets the repository window's size back.
  $effect(() => {
    const welcome = !repo || !history;
    if (launched && !(welcome && loading)) fitWindow(welcome);
  });

  let content = $state<HTMLDivElement>();
  // The commit list has the keyboard when History opens and nothing else asked for it, so ↑ ↓
  // work at once and the selection shows in its branch color.
  $effect(() => {
    if (view !== "history" || !list || showConflicts) return;
    requestAnimationFrame(() => {
      if (document.activeElement === document.body || document.activeElement === null) list?.focus();
    });
  });

  /** Tab moves between the commit list and the details panel, as a whole: from the list to the
   *  open tab of the details, and Shift+Tab from that tab back to the list. Inside the details Tab
   *  keeps its usual way through buttons and fields, and ← → step through the tabs. */
  function paneKeys(event: KeyboardEvent) {
    const target = event.target as HTMLElement;
    const panel = content?.querySelector<HTMLElement>("aside.panel");
    if (!panel || event.metaKey || event.ctrlKey || event.altKey) return;
    const tab = target.closest<HTMLElement>('[role="tab"]');
    // Shift+Tab comes as ISO_Left_Tab on Linux, which names no key; its code is still Tab.
    const isTab = event.key === "Tab" || event.code === "Tab";
    if (isTab && !event.shiftKey && target.closest(".history")) {
      const open = panel.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]') ?? panel.querySelector<HTMLElement>("button, input, textarea, [tabindex='0']");
      if (!open) return;
      event.preventDefault();
      keyFocus(open);
    } else if (isTab && event.shiftKey && tab && panel.contains(tab)) {
      event.preventDefault();
      list?.focus();
    } else if ((event.key === "ArrowLeft" || event.key === "ArrowRight") && tab && panel.contains(tab)) {
      const list = tab.closest('[role="tablist"]');
      const step = stepperOf(list);
      if (step) {
        event.preventDefault();
        if (step(event.key === "ArrowRight" ? 1 : -1, false)) focusOpenTab(list!);
        return;
      }
      const tabs = [...(list?.querySelectorAll<HTMLElement>('[role="tab"]') ?? [])];
      const next = tabs[tabs.indexOf(tab) + (event.key === "ArrowRight" ? 1 : -1)];
      if (!next) return;
      event.preventDefault();
      next.click();
      keyFocus(next);
    }
  }

  /** Focus moved by a key shows its ring, also after a click put the focus where it came from
   *  (the browser then counts the move as a mouse one). */
  function keyFocus(el: HTMLElement) {
    el.focus({ focusVisible: true } as FocusOptions);
    el.classList.add("key-focus");
    el.addEventListener("blur", () => el.classList.remove("key-focus"), { once: true });
  }

  function startResize(event: PointerEvent) {
    const startX = event.clientX;
    // From the width it shows, which a narrow window may have cut down from the one saved.
    const startWidth = shownPanelWidth;
    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);
    const move = (e: PointerEvent) => {
      // Saved without the hidden sidebar's room, so the graph keeps its width when the sidebar comes back.
      panelWidth = Math.max(Math.min(Math.max(startWidth - (e.clientX - startX), PANEL_MIN), panelMax) - panelBonus, PANEL_MIN);
    };
    const up = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
      api.setSetting(settings.detailsWidth, Math.round(panelWidth)).catch(() => {});
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  }

  // Palettes differ in how many colors branch names hash into: lay the graph out again.
  let palette: string | null = null;
  $effect(() => {
    const next = prefs.get("oxbow.branchPalette");
    if (palette !== null && palette !== next && repo) untrack(() => refresh());
    palette = next;
  });

  $effect(() => {
    api.getSetting<number>(settings.detailsWidth).then((width) => {
      if (typeof width === "number") panelWidth = Math.max(width, PANEL_MIN);
    });
    api.getSetting<boolean>(settings.sidebarHidden).then((hidden) => {
      if (typeof hidden === "boolean") sideHidden = hidden;
    });
    api.getSetting<boolean>(settings.detailsHidden).then((hidden) => {
      if (typeof hidden === "boolean") detailsHidden = hidden;
    });
    api.getSetting<number>(settings.zoomLevel).then((level) => {
      if (typeof level === "number" && level !== 0) zoom(level, false);
    });
    api.initialRepo().then((path) => {
      if (path) load(path);
      launched = true;
    });
    // Settings changed remotes or packed the repository.
    const unlisten = listen("repo-touched", () => refresh());
    // Signing in or out changes what GitHub tells about the pull requests.
    const account = listen("account-changed", () => repo && github.accountChanged());
    return () => {
      unlisten.then((stop) => stop());
      account.then((stop) => stop());
    };
  });
</script>

<svelte:window
  bind:innerWidth={windowWidth}
  onfocus={refresh}
  onkeydown={(event) => {
    const hit = keymap.match(event);
    // The chord's note goes once its second key came.
    if (hit && chordNote && confirm.toast?.text === chordNote) confirm.dismiss();
    if (hit) chordNote = null;
    if (hit && "chord" in hit) {
      // As VS Code says it in its status bar.
      event.preventDefault();
      chordNote = `(${keys(hit.chord)}) was pressed. Waiting for the second key of the chord…`;
      confirm.say(chordNote);
    } else if (hit && "missed" in hit) {
      event.preventDefault();
      confirm.say(`The key combination (${keys(hit.missed).replace(" ", ", ")}) is not a command.`);
    } else if (hit && "id" in hit && shortcutKey(hit.id, event)) {
      event.preventDefault();
    } else if (history?.bisect && !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey && ["g", "b", "s"].includes(event.key.toLowerCase()) && view === "history" && !showConflicts && !confirm.request && !start.sheet && !nav.quickOpen && !picker && !typing(event)) {
      // Bisecting: G, B and S answer for the commit under test, as the banner's tooltips say.
      event.preventDefault();
      bisectKey(({ g: "good", b: "bad", s: "skip" } as const)[event.key.toLowerCase() as "g" | "b" | "s"]);
    } else if (event.key === "Escape" && sideShown && !confirm.request && !event.defaultPrevented) {
      sideShown = false;
    } else if (event.key === "Escape" && submoduleSel && view === "history" && !confirm.request && !event.defaultPrevented) {
      submoduleSel = null;
    } else if (event.key === "Escape" && (oplog.compare || oplog.lost) && view === "history" && !confirm.request && !event.defaultPrevented) {
      oplog.compare = null;
      oplog.lost = null;
    } else if (event.key === "Escape" && second && view === "history" && !confirm.request && !event.defaultPrevented) {
      second = null;
    } else if (event.key === "Escape" && expanded && view === "history" && !confirm.request && !event.defaultPrevented) {
      toggleExpanded();
    }
  }}
/>

{#if !repo || !history}
  <Welcome {loading} {error} onOpen={chooseRepo} onOpenPath={load} />
{:else}
  <div class="window" class:narrow class:mac class:side-shown={sideShown} class:side-hidden={sideHidden && !narrow} class:sliding>
    <button class="capsule side-toggle" onclick={toggleSidebar} aria-label={sideOut ? "Hide sidebar" : "Show sidebar"} aria-expanded={sideOut} title={withKeys(sideOut ? "Hide Sidebar" : "Show Sidebar", SIDEBAR_KEYS)}>
      <svg class="icon" viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="10" rx="2" /><path d="M6 3v10" /></svg>
    </button>
    {#if narrow && sideShown}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="side-catcher" onclick={() => (sideShown = false)}></div>
    {/if}
    <!-- Over a narrow window the sidebar steps aside again once something in it is picked. -->
    <Sidebar {repo} {history} {selectedRow} ctx={branchCtx!} onOpen={chooseRepo} onOpenPath={load} onWelcome={closeRepo}
      onPick={aside((id: string) => ((expanded = false), pick(id), requestAnimationFrame(() => list?.focus())))}
      {run}
      {view}
      onView={(v) => {
        sideShown = false;
        view = v;
        resolving = false;
        if (v === "history" && selected) requestAnimationFrame(() => list?.reveal(selected!));
      }}
      onChanges={aside(showChanges)}
      onPickStash={aside((id: string) => (stashSel = id))}
      onCompare={aside(openCompare)}
      onEditStack={aside(openStack)}
      onPullRequest={aside(openPull)}
      submodule={pickedSubmodule?.path ?? null}
      onSubmodule={aside(showSubmodule)}
      onReload={reload}
    />
    <div class="main">
      <header data-tauri-drag-region>
        <div class="lead" data-tauri-drag-region>
          {#if view === "file" && !showConflicts}
            {@const back = fileBack === "compare" ? "Compare" : fileBack === "stashes" ? "Stashes" : fileBack === "oplog" ? "Operation Log" : "History"}
            <button class="back" onclick={() => ((view = fileBack), (nav.file = null))} aria-label="Back to {back}" title="Back to {back}">
              <svg class="icon" viewBox="0 0 16 16"><path d="M10 3.5 5.5 8l4.5 4.5" /></svg>
            </button>
          {/if}
          <div class="title" data-tauri-drag-region>
            {#if showConflicts && operation}
              <span class="name">Resolve Conflicts</span>
              <span class="sub">{describe(operation).noun} in progress · {operation.conflicted} {operation.conflicted === 1 ? "file" : "files"} left</span>
            {:else if view === "file" && nav.file}
              <span class="name">{nav.file.path.slice(nav.file.path.lastIndexOf("/") + 1)}</span>
              <span class="sub">{repo.name} · {nav.file.path}</span>
            {:else if view === "pull" && pullBranch}
              <span class="name">{pullBranch}</span>
              <span class="sub">{shownPull ? `Pull request #${shownPull.number}` : "No pull request yet"}{github.repo ? ` · ${github.repo.owner}/${github.repo.name}` : ""}</span>
            {:else if view === "stack"}
              <span class="name">Edit Stack</span>
              <span class="sub">Interactive rebase · {repo.name}</span>
            {:else if view === "compare"}
              <span class="name">Compare</span>
              <span class="sub">{comparing.target} with {comparing.base}</span>
            {:else if view === "stashes"}
              <span class="name">Stashes</span>
              <span class="sub">{repo.name} · on {history.head.branch ?? "detached HEAD"}</span>
            {:else if view === "oplog"}
              <span class="name">Operation Log</span>
              <span class="sub">{repo.name} · every step, to undo or restore</span>
            {:else}
              <span class="name">History</span>
              {#if activity.running}
                <span class="sub">{activity.running.title}{activity.percent !== null ? ` · ${activity.percent}%` : "…"}</span>
              {:else if activity.problem?.failure}
                <span class="sub amber">Fetch failed · {failureShort(activity.problem.failure)}</span>
              {:else}
                <span class="sub">{branchCount} {branchCount === 1 ? "branch" : "branches"}{history.bisect ? (history.bisect.found ? " · Bisect done" : " · Bisecting") : !history.head.branch && !operation?.branch ? " · detached HEAD" : ""}</span>
              {/if}
            {/if}
          </div>
          <!-- File History is about one file: a plain back arrow and no branch picker, as in the design, leave room for Find in file. -->
          {#if view !== "file" || showConflicts}
            <!-- The branch and a new one beside it: one control, as Fetch, Pull and Push are. -->
            <div class="joined branch" role="group" aria-label="Branch">
              <BranchPicker bind:this={branchPicker} {history} {colorOf} onPick={(name) => branchCtx && run(switchRequest(branchCtx, name))} />
              {#if !narrow}
                <span class="divider"></span>
                <button onclick={newBranch} aria-label="New branch" title={history.head.branch ? "New Branch…" : "Create a branch at HEAD…"}>
                  <svg class="icon" viewBox="0 0 16 16"><path d={NEW_BRANCH_ICON} /></svg>
                </button>
              {/if}
            </div>
          {/if}
          {#if view === "file" && !showConflicts}
            <div class="segmented" role="radiogroup" aria-label="File history mode">
              <button role="radio" aria-checked={nav.fileMode === "changes"} class:on={nav.fileMode === "changes"} onclick={() => (nav.fileMode = "changes")}>Changes</button>
              <button role="radio" aria-checked={nav.fileMode === "blame"} class:on={nav.fileMode === "blame"} onclick={() => (nav.fileMode = "blame")}>Blame</button>
            </div>
          {/if}
          {#if (view === "compare" || view === "stack" || view === "pull") && !showConflicts}
            <button class="capsule" onclick={() => (view = "history")} title="Back to the commit graph">
              <svg class="icon" viewBox="0 0 16 16"><path d="M10 3.5 5.5 8l4.5 4.5" /></svg>History
            </button>
          {/if}
          {#if showConflicts}
            <button class="capsule" onclick={() => (resolving = false)} title={view === "stashes" ? "Back to Stashes" : "Back to the commit graph"}>
              <svg class="icon" viewBox="0 0 16 16"><path d="M10 3.5 5.5 8l4.5 4.5" /></svg>{view === "stashes" ? "Stashes" : "History"}
            </button>
          {/if}
        </div>
        <div class="center" data-tauri-drag-region>
          {#if view === "file" && !showConflicts}
            <FindField bind:this={findField} />
          {/if}
          {#if view === "history" && !showConflicts}
            <SearchField bind:this={searchField} onLeave={() => list?.focus()} rows={history.rows} {matches} {selected} onGo={select} />
          {/if}
        </div>
        <div class="trail" data-tauri-drag-region>
          {#if error}<span class="error" role="alert">{error}</span>{/if}
          {#if history.remotes.length && remoteCtx}
            {@const t = history.tracking}
            <div class="sync">
            <div class="group" role="group" aria-label="Sync with remote">
              <button
                class="fetch"
                onclick={() => sync("fetch")}
                oncontextmenu={(event) => {
                  event.preventDefault();
                  activity.open = !activity.open;
                }}
                aria-label={activity.running ? "Fetching, show Activity" : activity.problem ? "Fetch failed, show Activity" : "Fetch"}
                aria-haspopup="dialog"
                aria-expanded={activity.open}
                title={activity.running
                  ? `${activity.running.title} · click for Activity`
                  : activity.problem
                    ? `${activity.problem.sub} · click for Activity`
                    : `Fetch from ${history.remotes.length > 1 ? "all remotes" : remoteCtx.remote} · right-click for Activity`}
              >
                {#if activity.running}
                  <svg class="icon ring" class:spin={activity.percent === null} viewBox="0 0 16 16">
                    <circle cx="8" cy="8" r="5.5" class="ring-track" />
                    <circle cx="8" cy="8" r="5.5" class="ring-fill" pathLength="100" stroke-dasharray="{activity.percent ?? 25} 100" />
                  </svg>
                {:else}
                  <svg class="icon" viewBox="0 0 16 16"><path d="M13 8a5 5 0 0 1-8.6 3.5M3 8a5 5 0 0 1 8.6-3.5" /><path d="M11.8 1.8v2.9H8.9M4.2 14.2v-2.9h2.9" /></svg>
                  {#if activity.problem}<span class="dot"></span>{/if}
                {/if}
              </button>
              <!-- Grey when it can't run, but it still answers a click: with why, under it. -->
              <button
                class:off={!!pullBlock}
                class:asking={why === "pull"}
                onclick={() => (pullBlock ? (why = why === "pull" ? null : "pull") : sync("pull"))}
                aria-disabled={!!pullBlock}
                aria-haspopup={pullBlock ? "dialog" : undefined}
                aria-label={t?.behind && !t.rewritten ? `Pull ${t.behind} commits` : "Pull"}
                title={why === "pull" ? undefined : pullBlock ? `${pullBlock.title} · click for more` : `Pull from ${t?.remote}/${t?.branch}`}
              >
                <svg class="icon" viewBox="0 0 16 16"><path d="M8 2v9M4.5 7.5 8 11l3.5-3.5M3 14h10" /></svg>
                <!-- Old versions of commits rewritten here are nothing to pull. -->
                {#if t?.behind && !t.rewritten}<span class="count">{t.behind}</span>{/if}
              </button>
              <button
                class:off={!!pushBlock}
                class:asking={why === "push"}
                onclick={() => (pushBlock ? (why = why === "push" ? null : "push") : sync("push"))}
                aria-disabled={!!pushBlock}
                aria-haspopup={pushBlock ? "dialog" : undefined}
                aria-label={t ? (t.ahead ? `Push ${t.ahead} commits` : "Push") : "Publish branch"}
                title={why === "push" ? undefined : pushBlock ? `${pushBlock.title} · click for more` : t?.rewritten ? `Force push to ${t.remote}/${t.branch}: it has the old versions of commits you rewrote` : t && !t.gone ? `Push to ${t.remote}/${t.branch}` : `Publish ${history.head.branch} to ${remoteCtx.remote}`}
              >
                <svg class="icon" viewBox="0 0 16 16"><path d="M8 12V3M4.5 6.5 8 3l3.5 3.5M3 14h10" /></svg>
                {#if t?.ahead}<span class="count">{t.ahead}</span>{:else if !t && history.head.branch}<span class="count">Publish</span>{/if}
              </button>
            </div>
            <ActivityPopover fix={fixFetch} retry={() => backgroundFetch(false)} settings={() => ((activity.open = false), api.openSettings())} reload={reload} />
            {#if whyBlock}
              {@const block = whyBlock}
              <WhyPopover title={block.title} body={block.body} fixes={block.fixes} onFix={(i) => fixSync(block.fixes[i].fix)} onClose={() => (why = null)} />
            {/if}
            </div>
          {:else if history.head.branch && history.head.commit}
            <button class="capsule publish" onclick={publishToGitHub} title="Make a repository on GitHub and push {history.head.branch} there">
              <svg class="icon" viewBox="0 0 16 16"><path d="M8 12V3M4.5 6.5 8 3l3.5 3.5M3 14h10" /></svg>
              Publish to GitHub
            </button>
          {/if}
          {#if view === "pull" && shownPull && !showConflicts}
            <button class="capsule" onclick={() => api.openGitHub(shownPull!.htmlUrl)} title="Open the pull request on GitHub">
              <svg class="icon" viewBox="0 0 16 16"><path d="M9.5 2.5h4v4M13.5 2.5 8 8M12 9.5v3a1 1 0 0 1-1 1H3.5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h3" /></svg>
              Open on GitHub
            </button>
          {/if}
          <!-- Undo and Fix a Mistake: the two ways back, side by side. -->
          <div class="joined" role="group" aria-label="Undo">
            <OperationLog
              repo={repo.name}
              run={(request) => void run(request)}
              onShow={showMove}
              onBranch={branchAtMove}
              onCompare={compareStep}
            />
            <span class="divider"></span>
            <button onclick={() => openFixes()} aria-label="Fix a mistake" title={withKeys("Fix a Mistake…", combo("fixMistake"))}>
              <svg class="icon" viewBox="0 0 16 16"><path d="M8 1.8a6.2 6.2 0 1 0 0 12.4a6.2 6.2 0 1 0 0-12.4M8 5.4a2.6 2.6 0 1 0 0 5.2a2.6 2.6 0 1 0 0-5.2M3.6 3.6l2.6 2.6M12.4 3.6 9.8 6.2M3.6 12.4l2.6-2.6M12.4 12.4 9.8 9.8" /></svg>
            </button>
          </div>
          {#if narrow}
            <button
              class="capsule"
              onclick={(event) => {
                const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
                moreMenu = moreMenu ? null : { x: box.left, y: box.bottom + 6 };
              }}
              aria-label="More"
              aria-haspopup="menu"
              aria-expanded={!!moreMenu}
              title={view !== "file" || showConflicts ? "New Branch, Console, Settings" : "Console, Settings"}
            >
              <svg class="icon" viewBox="0 0 16 16"><circle cx="3.5" cy="8" r=".6" /><circle cx="8" cy="8" r=".6" /><circle cx="12.5" cy="8" r=".6" /></svg>
            </button>
          {:else}
            <button class="capsule" onclick={() => api.openConsole()} aria-label="Console" title={withKeys("Console", combo("console"))}>
              <svg class="icon" viewBox="0 0 16 16"><path d={CONSOLE_ICON} /></svg>
            </button>
            <button class="capsule" onclick={() => api.openSettings()} aria-label="Settings" title={withKeys("Settings", "Mod+,")}>
              <svg class="icon" viewBox="0 0 16 16"><path d={SETTINGS_ICON} /></svg>
            </button>
          {/if}
        </div>
      </header>
      {#if searching}
        <SearchBar
          {history}
          count={matches.length}
          older={olderMatches.length}
          loading={!!paging}
          onLoadOlder={() => olderMatches.length && loadOlder(olderMatches[olderMatches.length - 1])}
          copy={(text) => copyText(text, "Copied the command.")}
        />
      {/if}
      {#if view === "history" && !showConflicts && !searching && !operation && !history.bisect && history.head.branch}
        <StatusLine
          {history}
          color={headColor}
          onChanges={showChanges}
          onPush={() => sync("push")}
          onPull={(mode) => sync("pull", mode)}
          onFetch={() => sync("fetch")}
          onTheirs={(upstream) => openCompare(history!.head.branch!, upstream, "split")}
          onPublish={publishToGitHub}
        />
        {@const t = history.tracking}
        {@const tip = ([t && t.behind > 0 && "fetchedOnly", t && t.ahead > 0 && !t.rewritten && "localCommits"] as const).find((id) => id && tips.shows(id))}
        {#if tip}<TipCard id={tip} wide />{/if}
      {/if}
      {#if operation && branchCtx}
        {@const op = operation}
        <OperationBanner
          {op}
          color={op.branch ? colorOf(op.branch) : headColor}
          resolving={showConflicts}
          onResolve={() => (resolving = true)}
          onAbort={() => run(abortRequest(branchCtx!, op))}
          onSkip={() => run(skipRequest(branchCtx!, op))}
          onContinue={() => run(continueRequest(branchCtx!, op))}
        />
      {:else if !history.bisect && !history.head.branch && history.head.commit && branchCtx}
        <DetachedBanner {history} back={backTo} onBack={() => backTo && run(switchRequest(branchCtx!, backTo))} onKeep={newBranch} />
        <TipCard id="detached" wide />
      {/if}
      {#if history.bisect && branchCtx}
        {@const bisect = history.bisect}
        <BisectBanner
          {bisect}
          lookup={(id) => rowsById.get(id)}
          onStop={() => run(bisectResetRequest(branchCtx!, bisect))}
          onFinish={() => run(bisectResetRequest(branchCtx!, bisect))}
          onSkip={() => bisectKey("skip")}
          onGood={() => bisectKey("good")}
          onBad={() => bisectKey("bad")}
          onShow={pick}
          copy={(text) => copyText(text, "Copied the bisect log.")}
        />
      {/if}
      {#if lfs.missing && !lfs.dismissed}
        <LfsBanner repoName={repo.name} run={(request) => void run(request)} />
      {/if}
      {#if showConflicts && operation}
        {#key repo.path}
          <ConflictsView {history} op={operation} {colorOf} {version} {run} />
        {/key}
      {:else if view === "file" && nav.file}
        {#key repo.path + nav.file.path}
          <FileHistoryView {history} path={nav.file.path} start={nav.file.commit} lookup={(id) => rowsById.get(id)} copy={(text) => copyText(text, `Copied ${text.length > 40 ? "the command" : text}.`)} />
        {/key}
      {:else if view === "compare"}
        {#key repo.path}
          <CompareView
            {history}
            base={comparing.base}
            target={comparing.target}
            mode={comparing.mode}
            {version}
            {colorOf}
            lookup={(id) => rowsById.get(id)}
            childrenOf={(id) => childrenOf.get(id) ?? []}
            onChange={(next) => (comparing = next)}
            copy={(text) => copyText(text, "Copied the command.")}
            panelWidth={shownPanelWidth}
            onGrip={startResize}
          />
        {/key}
      {:else if view === "pull" && pullBranch && branchCtx}
        {#key repo.path}
          <PullRequestView
            branch={pullBranch}
            {history}
            ctx={branchCtx}
            {colorOf}
            {version}
            {run}
            onBranch={(name) => (pullBranch = name)}
            copy={(text) => copyText(text, `Copied ${text.length > 40 ? "it" : text}.`)}
          />
        {/key}
      {:else if view === "stack" && branchCtx}
        {#key repo.path}
          <EditStackView branch={stackBranch} ctx={branchCtx} {colorOf} {version} {run} onClose={() => (view = "history")} />
        {/key}
      {:else if view === "oplog"}
        {#key repo.path}
          <OperationLog screen repo={repo.name} run={(request) => void run(request)} onShow={showMove} onBranch={branchAtMove} onCompare={compareStep} />
        {/key}
      {:else if view === "stashes" && branchCtx}
        {#key repo.path}
          <StashesView {history} ctx={branchCtx} {colorOf} bind:selected={stashSel} {version} {run} copy={(text) => copyText(text, `Copied ${text.length > 40 ? "it" : text}.`)} />
        {/key}
      {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="content" class:expanded bind:this={content} onkeydown={paneKeys}>
        <section class="history" aria-label="Commit history" aria-hidden={expanded}>
          <div class="columns">
            <span class="what">
              {#if found?.only}Matching commits · graph hidden{:else if focusOnly && history.focus}
                {@const hidden = history.rows.length - history.focus.rows.length}
                {history.head.branch ?? "HEAD"}{history.trunk && history.head.branch !== history.trunk ? ` with ${history.trunk}` : ""} · {hidden.toLocaleString()} {hidden === 1 ? "commit" : "commits"} hidden
              {:else}Description{/if}
            </span>
            {#if history.truncated && !found?.only}<span class="note">Latest {history.loaded.toLocaleString()} commits · older ones load as you scroll</span>{/if}
            {#if !found?.only}
              <button
                class="focus-toggle"
                class:on={focusOnly}
                aria-pressed={focusOnly}
                onclick={() => setFocusOnly(!focusOnly)}
                title={focusOnly ? "Show All Branches" : `Show only ${history.head.branch ?? "HEAD"}, its upstream and ${history.trunk ?? "its base"}`}
              >
                <svg class="icon" viewBox="0 0 16 16"><circle cx="5" cy="3.8" r="1.6" /><circle cx="5" cy="12.2" r="1.6" /><circle cx="11" cy="6.4" r="1.6" /><path d="M5 5.4v5.2M11 8c0 2-3 2.2-5.4 3.2" /></svg>
                Current Branch Only
              </button>
              <GraphLegend {history} shown={graphHistory!} onShow={select} />
            {/if}
            {#if detailsHidden}
              <button class="bar-button" onclick={toggleDetails} aria-label="Show details" title={withKeys("Show Details", DETAILS_KEYS)}>
                <svg class="icon" viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="10" rx="2" /><path d="M10 3v10" /></svg>
              </button>
            {/if}
          </div>
          {#key repo.path}
            <HistoryList
              bind:this={list}
              history={graphHistory!}
              {selected}
              onSelect={select}
              menuFor={commitMenu}
              {found}
              second={pair ? second : null}
              onSecond={pickSecond}
              {paging}
              onMore={() => loadOlder()}
              onUnshallow={history.defaultRemote ? () => run(unshallowRequest(history!.defaultRemote!)) : null}
              pointed={lens.pointed}
            />
          {/key}
        </section>
        {#if !detailsHidden}
        <aside class="panel" style:width={expanded ? null : `${shownPanelWidth}px`} aria-label="Commit details">
          {#if !expanded}
            <button class="grip" tabindex="-1" onpointerdown={startResize} aria-label="Resize commit details" title="Drag to resize"></button>
          {/if}
          <!-- In line with the graph's column bar: what the panel shows, and the room it takes. -->
          <div class="columns panel-bar">
            <span class="what">{panelLabel}</span>
            <button class="bar-button" class:on={expanded} onclick={toggleExpanded} aria-pressed={expanded} aria-label={expanded ? "Show the graph again" : "Expand details"} title={withKeys(expanded ? "Show Graph" : "Expand Details", EXPAND_KEYS)}>
              <svg class="icon" viewBox="0 0 16 16"><path d={expanded ? "M13.5 2.5 9.5 6.5M9.5 3.2v3.3h3.3M2.5 13.5l4-4M6.5 12.8V9.5H3.2" : "M9.5 2.5h4v4M13.5 2.5 9.5 6.5M6.5 13.5h-4v-4M2.5 13.5l4-4"} /></svg>
            </button>
            {#if !expanded}
              <button class="bar-button" onclick={toggleDetails} aria-label="Hide details" title={withKeys("Hide Details", DETAILS_KEYS)}>
                <svg class="icon" viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="10" rx="2" /><path d="M10 3v10" /></svg>
              </button>
            {/if}
          </div>
          <div class="panel-body">
          <!-- A new repository starts the panels from scratch. -->
          {#key repo.path}
            {#if pickedSubmodule}
              <SubmodulePanel
                sub={pickedSubmodule}
                subs={history.submodules}
                repoPath={repo.path}
                {version}
                run={(request) => void run(request)}
                onClose={() => (submoduleSel = null)}
              />
            {:else if oplog.compare}
              <SnapshotPanel
                entry={oplog.compare.entry}
                after={oplog.compare.after}
                color={headColor}
                {version}
                copy={(text) => copyText(text, "Copied the command.")}
                run={(request) => void run(request)}
                onClose={() => (oplog.compare = null)}
              />
            {:else if oplog.lost}
              {@const lost = oplog.lost}
              <LostCommitPanel
                entry={lost}
                color={moveColor(lost)}
                lookup={(id) => rowsById.get(id)}
                onSelect={selectFromLost}
                onBranch={branchAtMove}
                onClose={() => (oplog.lost = null)}
              />
            {:else if pair}
              <PairPanel
                older={pair.older}
                newer={pair.newer}
                {version}
                copy={(text) => copyText(text, "Copied the command.")}
                onSelect={select}
                onClear={() => (second = null)}
                onOpenCompare={(from, to) => openCompare(from.slice(0, 7), to.slice(0, 7), "tips")}
              />
            {:else if selectedRow?.worktree}
              <ChangesPanel
                repo={repo.path}
                branch={history.head.branch}
                color={selectedRow.graph.color}
                head={headRow}
                {version}
                changes={history.changes}
                snapshot={history.snapshot}
                onChanged={refresh}
                onResolve={operation ? () => (resolving = true) : undefined}
              />
            {:else if selectedRow}
              <CommitPanel row={selectedRow} childIds={childrenOf.get(selectedRow.id) ?? []} lookup={(id) => rowsById.get(id)} onSelect={select} localTags={history.localTags} {version}
                onUnshallow={history.shallow && history.defaultRemote ? () => run(unshallowRequest(history!.defaultRemote!)) : null}
                find={found?.mode === "code" ? found.text : null}
                foundFiles={found?.hits.get(selectedRow.id) ?? []}
                foundText={found?.text ?? null}
              />
            {/if}
          {/key}
          </div>
        </aside>
        {/if}
      </div>
      {/if}
    </div>
  </div>
  {#if moreMenu && narrow}
    <Menu x={moreMenu.x} y={moreMenu.y} label="More" entries={moreEntries()} onClose={() => (moreMenu = null)} />
  {/if}
  {#if nav.quickOpen}
    <QuickOpen
      commit={selectedRow && !selectedRow.worktree ? { id: selectedRow.id, summary: selectedRow.summary } : null}
      initial={quickInitial}
      onClose={() => ((nav.quickOpen = false), (quickInitial = ""))}
      onCommands={(query) => {
        nav.quickOpen = false;
        quickInitial = "";
        picker = { kind: "commands", initial: query };
      }}
    />
  {/if}
  {#if fixing}
    {@const fc = fixing}
    <FixSheet
      choices={fixChoices(fc, fixBusy)}
      summary={fixStateLine(fc)}
      onPick={(id) => pickFix(fc, id)}
      onClose={() => (fixing = null)}
    />
  {/if}
  <ConfirmSheet repo={repo.name} branch={history.head.branch ?? operation?.branch ?? null} color={headColor} />
{/if}
{#if picker?.kind === "commands"}
  <CommandPalette
    items={paletteCommands()}
    initial={picker.initial}
    placeholder="Type the name of a command to run"
    empty={() => "No matching commands"}
    onClose={() => (picker = null)}
    onFiles={repo && history
      ? (query) => {
          picker = null;
          quickInitial = query;
          nav.quickOpen = true;
        }
      : undefined}
  />
{:else if picker?.kind === "recent"}
  <CommandPalette
    commands={false}
    items={picker.repos.map((r) => ({ id: r.path, title: r.name, detail: r.path, run: () => void load(r.path) }))}
    placeholder="Open a recent repository"
    empty={(query) => (query.trim() ? `No recent repository matches “${query.trim()}”.` : "No other repositories opened lately.")}
    onClose={() => (picker = null)}
  />
{/if}
{#if start.sheet?.kind === "clone"}
  <CloneSheet url={start.sheet.url} onDone={started} onClose={() => (start.sheet = null)} />
{:else if start.sheet?.kind === "new"}
  <NewRepoSheet folder={start.sheet.folder} onDone={started} onClose={() => (start.sheet = null)} />
{/if}

<style>
  .window {
    position: relative;
    height: 100%;
    color: var(--text);
  }
  .main {
    position: absolute;
    left: 248px;
    top: 0;
    right: 0;
    bottom: 0;
    display: flex;
    flex-direction: column;
  }
  /* The sidebar button stays where Mail has it, just right of the traffic lights and on the
     toolbar's middle line, whether the sidebar is out or not. Elsewhere it opens the toolbar. */
  .window .side-toggle {
    position: absolute;
    z-index: 21;
    top: 11px;
    left: 14px;
    width: 34px;
    padding: 0;
    justify-content: center;
  }
  .window.mac .side-toggle {
    left: 96px;
  }
  .window > :global(nav),
  .main,
  header {
    transition:
      transform 0.22s ease,
      left 0.22s ease,
      padding-left 0.22s ease;
  }
  .window.sliding :global(aside.panel) {
    transition: width 0.22s ease;
  }
  /* A hidden sidebar, or a narrow window, gives the content the whole width. */
  .window.narrow .main,
  .window.side-hidden .main {
    left: 0;
  }
  .window.narrow > :global(nav),
  .window.side-hidden > :global(nav) {
    transform: translateX(-100%);
    visibility: hidden;
    transition:
      transform 0.22s ease,
      visibility 0s linear 0.22s;
  }
  /* Over a narrow window the sidebar slides in on an opaque background, and slides out once
     something in it is picked or the content is clicked. */
  .window.narrow > :global(nav) {
    z-index: 20;
    background: linear-gradient(var(--side), var(--side)), var(--win);
  }
  .window.narrow.side-shown > :global(nav) {
    transform: none;
    visibility: visible;
    box-shadow: var(--overlay-shadow);
    transition: transform 0.22s ease;
  }
  @media (prefers-reduced-motion: reduce) {
    .window > :global(nav),
    .window.narrow > :global(nav),
    .window.side-hidden > :global(nav),
    .window.narrow.side-shown > :global(nav),
    .window.sliding :global(aside.panel),
    .main,
    header {
      transition: none;
    }
  }
  .side-catcher {
    position: absolute;
    inset: 0;
    z-index: 19;
  }
  /* A narrow window gives up centering the search: the search gives way first, down to 200 px,
     then the title and the branch picker, and the buttons on the right keep their size. */
  .window.narrow .lead {
    flex: 0 1 auto;
  }
  .window.narrow .center {
    flex: 1 1 200px;
    min-width: 200px;
    justify-content: center;
  }
  .window.narrow .trail {
    flex: 0 0 auto;
  }
  /* The traffic lights and the sidebar button sit on the sidebar; without it, the toolbar makes room for them. */
  .window.narrow header,
  .window.side-hidden header {
    padding-left: 60px;
  }
  .window.narrow.mac header,
  .window.side-hidden.mac header {
    padding-left: 142px;
  }
  /* Three zones: the two sides share the room equally, so the search sits in the middle of the
     toolbar. In a narrow window the buttons on the right keep their size, and the title and then
     the search give way, rather than anything covering a button. */
  header {
    height: 56px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 12px 0 14px;
    flex-shrink: 0;
  }
  .lead,
  .trail {
    flex: 1 1 0;
    display: flex;
    align-items: center;
    gap: 10px;
    align-self: stretch;
  }
  .lead {
    min-width: 0;
  }
  .trail {
    min-width: max-content;
    justify-content: flex-end;
  }
  .center {
    flex: 0 1 auto;
    min-width: 0;
    display: flex;
    align-items: center;
    align-self: stretch;
  }
  .title {
    display: flex;
    flex-direction: column;
    line-height: 1.2;
    margin-right: 4px;
    min-width: 0;
    flex-shrink: 100;
  }
  .title span {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .name {
    font-size: var(--text-heading);
    font-weight: 700;
  }
  .sub {
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .capsule {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    height: 34px;
    padding: 0 12px;
    border-radius: var(--radius-pill);
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    color: var(--icon);
    font-weight: 500;
  }
  .back {
    width: 28px;
    height: 28px;
    margin-right: -4px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-pill);
    color: var(--icon);
    flex-shrink: 0;
  }
  .back:hover {
    background: var(--field);
  }
  .group {
    display: flex;
    align-items: center;
    height: 34px;
    padding: 0 4px;
    border-radius: var(--radius-pill);
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    color: var(--icon);
  }
  .group button {
    height: 34px;
    min-width: 36px;
    padding: 0 9px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 3px;
    border-radius: var(--radius-pill);
  }
  .group button:disabled,
  .group button.off {
    opacity: 0.4;
  }
  .group button.off.asking {
    opacity: 0.55;
    background: var(--hover-wash);
  }
  .sync {
    position: relative;
    display: flex;
  }
  /* Controls that belong together share one glass capsule, split by a hairline. The parts drop
     their own glass; a hover washes just the part under the pointer. */
  .joined {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    height: 34px;
    border-radius: var(--radius-pill);
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    color: var(--icon);
  }
  .joined.branch {
    flex-shrink: 1;
    min-width: 0;
  }
  .joined > button,
  .joined :global(.capsule) {
    height: 32px;
    min-width: 36px;
    padding: 0 10px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    border-radius: var(--radius-pill);
    background: none;
    border: 0;
    box-shadow: none;
  }
  .joined.branch :global(.picker .capsule) {
    justify-content: flex-start;
    gap: 6px;
    padding: 0 10px 0 12px;
  }
  .joined > button:hover {
    background: var(--hover-wash);
  }
  .joined > :first-child,
  .joined > :first-child :global(.capsule) {
    margin-left: 1px;
  }
  .joined > :last-child,
  .joined > :last-child :global(.capsule) {
    margin-right: 1px;
  }
  .divider {
    flex-shrink: 0;
    width: 1px;
    height: 18px;
    background: var(--sep);
  }
  .fetch {
    position: relative;
  }
  .ring {
    transform: rotate(-90deg);
  }
  .ring circle {
    fill: none;
    stroke-width: 1.8;
  }
  .ring-track {
    stroke: var(--sep);
  }
  .ring-fill {
    stroke: var(--accent);
    stroke-linecap: round;
    transition: stroke-dasharray 0.2s;
  }
  .ring.spin {
    animation: ring-spin 1.2s linear infinite;
  }
  @keyframes ring-spin {
    from {
      transform: rotate(-90deg);
    }
    to {
      transform: rotate(270deg);
    }
  }
  .dot {
    position: absolute;
    top: 7px;
    right: 7px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--orange);
  }
  .sub.amber {
    color: var(--orange);
  }
  .count {
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text);
  }
  .segmented {
    display: flex;
    padding: 3px;
    border-radius: var(--radius-pill);
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
  }
  .segmented button {
    height: 28px;
    padding: 0 16px;
    border-radius: var(--radius-pill);
    color: var(--text);
    font-weight: 500;
  }
  .segmented button.on {
    background: var(--side-sel);
    font-weight: 600;
  }
  .error {
    color: var(--red);
    font-size: var(--text-small);
    max-width: 360px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .content {
    flex-grow: 1;
    display: flex;
    min-height: 0;
  }
  /* The graph, the details and the status line have no frames of their own: hairlines part them. */
  .history {
    flex-grow: 1;
    min-width: 0;
    border-top: 1px solid var(--sep);
    overflow: hidden;
    background: var(--graph-bg);
    display: flex;
    flex-direction: column;
  }
  .columns {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 28px;
    flex-shrink: 0;
    padding: 0 10px 0 40px;
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
  }
  .columns .what {
    flex-grow: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .note {
    font-weight: 400;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* Current Branch Only: a soft chip, Steel while on. */
  .focus-toggle {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    flex-shrink: 0;
    height: 22px;
    padding: 0 9px 0 7px;
    border-radius: var(--radius-pill);
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
    white-space: nowrap;
  }
  .focus-toggle .icon {
    width: 13px;
    height: 13px;
  }
  .focus-toggle:hover {
    background: var(--field);
    color: var(--text);
  }
  .focus-toggle.on {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    color: var(--accent);
  }
  /* The details over the whole content: the graph steps aside, kept as it was. */
  .content.expanded .history {
    display: none;
  }
  .content.expanded .panel {
    flex-grow: 1;
    min-width: 0;
    border-left: none;
  }
  .panel-body {
    position: relative;
    flex-grow: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  /* The details panel's bar, level with the graph's. */
  .panel-bar {
    padding: 0 8px 0 16px;
    gap: 2px;
  }
  .bar-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 22px;
    height: 22px;
    border-radius: var(--radius-pill);
    color: var(--text2);
  }
  .bar-button:hover,
  .bar-button.on {
    background: var(--field);
    color: var(--text);
  }
  .bar-button .icon {
    width: 15px;
    height: 15px;
  }
  /* Its width comes from shownPanelWidth: never leaving the graph under 360 px. */
  .panel {
    position: relative;
    flex-shrink: 0;
    border-top: 1px solid var(--sep);
    border-left: 1px solid var(--sep);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .grip {
    position: absolute;
    left: -4px;
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
</style>
