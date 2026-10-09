<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { listen } from "@tauri-apps/api/event";
  import { api, settings } from "./lib/api";
  import type { History, HistoryRow, RepoSummary } from "./lib/types";
  import Sidebar from "./lib/Sidebar.svelte";
  import HistoryList from "./lib/HistoryList.svelte";
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
  import type { Operation } from "./lib/types";
  import { menuIcons, type MenuEntry } from "./lib/Menu.svelte";
  import { canPull, canPush, fetchRequest, pullRequest, pushRequest, type RemoteContext } from "./lib/remote";
  import Welcome from "./lib/Welcome.svelte";
  import { prefs } from "./lib/prefs.svelte";
  import { deleteTagRequest, isLocalTag, newTagRequest, pushTagsRequest } from "./lib/tags";
  import SearchField from "./lib/SearchField.svelte";
  import SearchBar from "./lib/SearchBar.svelte";
  import { search, type Found } from "./lib/search.svelte";
  import { untrack } from "svelte";
  import CompareView from "./lib/CompareView.svelte";
  import FileHistoryView from "./lib/FileHistoryView.svelte";
  import FindField from "./lib/FindField.svelte";
  import QuickOpen from "./lib/QuickOpen.svelte";
  import PairPanel from "./lib/PairPanel.svelte";
  import { nav } from "./lib/nav.svelte";
  import type { CompareMode } from "./lib/types";
  import CloneSheet from "./lib/CloneSheet.svelte";
  import NewRepoSheet from "./lib/NewRepoSheet.svelte";
  import { startPublish } from "./lib/publish";
  import { start } from "./lib/start.svelte";
  import OperationLog from "./lib/OperationLog.svelte";
  import { withKeys } from "./lib/keys";
  import { oplog, undoRequest } from "./lib/oplog.svelte";
  import EditStackView from "./lib/EditStackView.svelte";
  import ActivityPopover from "./lib/ActivityPopover.svelte";
  import { activity, failureShort, type ActivityItem } from "./lib/activity.svelte";
  import { addToCommitRequest, dropCommitRequest, rewordRequest, squashRequest } from "./lib/stack";
  import PullRequestView from "./lib/PullRequestView.svelte";
  import { github } from "./lib/github.svelte";
  import { lfs } from "./lib/lfs.svelte";
  import LfsBanner from "./lib/LfsBanner.svelte";

  let repo = $state<RepoSummary | null>(null);
  let history = $state<History | null>(null);
  let selected = $state<string | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let list = $state<HistoryList>();
  let searchField = $state<SearchField>();
  let findField = $state<FindField>();
  let panelWidth = $state(640);
  /** Goes up on every reload of the history, so the changes panel reloads too. */
  let version = $state(0);
  /** The Conflicts screen is open in place of History. */
  let resolving = $state(false);
  const operation = $derived(history?.operation ?? null);
  const showConflicts = $derived(resolving && !!operation);
  /** The screen next to the sidebar. */
  let view = $state<"history" | "stashes" | "compare" | "file" | "stack" | "pull">("history");
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
  let fileBack = $state<"history" | "stashes" | "compare">("history");
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

  const branchCount = $derived(history ? history.refs.filter((r) => r.kind === "local").length : 0);
  const rowsById = $derived(new Map(history?.rows.map((r) => [r.id, r]) ?? []));
  const selectedRow = $derived(selected ? (rowsById.get(selected) ?? null) : null);
  /** A second commit, ⌘-clicked, compared with the selected one in the details panel. */
  let second = $state<string | null>(null);
  const pair = $derived.by(() => {
    const other = second ? rowsById.get(second) : undefined;
    if (!history || !selectedRow || selectedRow.worktree || !other || other.worktree || other.id === selectedRow.id) return null;
    // Rows are newest first, so the later one in the list is the older commit.
    const rows = history.rows;
    return rows.indexOf(other) > rows.indexOf(selectedRow) ? { older: other, newer: selectedRow } : { older: selectedRow, newer: other };
  });

  /** ⌘-click: compare with the selected commit; on the selected one itself, or with nothing to compare against, just select. */
  function pickSecond(id: string) {
    if (!selectedRow || selectedRow.worktree) return select(id);
    second = id === selected ? null : id;
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
  /** Matching commits among the loaded ones, in graph order. */
  const matches = $derived(searching && search.result ? (history?.rows.filter((r) => hitMap.has(r.id)).map((r) => r.id) ?? []) : []);
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
    searchTimer = setTimeout(() => search.run(untrack(() => history?.rows ?? [])), 220);
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
    search.clear();
    oplog.forget();
    lfs.forget();
    activity.reset();
    if (view === "compare" || view === "file" || view === "stack" || view === "pull") view = "history";
    try {
      const summary = await api.openRepo(path);
      const next = await api.history();
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

  /** ⌘Z: back to before the newest step of the Operation Log. */
  async function undoLatest() {
    oplog.open = false;
    await oplog.load();
    const latest = oplog.entries[0];
    if (latest) run(undoRequest(latest));
    else confirm.say("Nothing to undo yet.");
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
      if (!isHead && history.head.commit) {
        editing.push({
          kind: "sub",
          label: `Reset ${here} to Here`,
          icon: menuIcons.reset,
          entries: resetModes.map((m) => ({ label: m.label, hint: m.keeps, danger: m.mode === "hard", run: () => run(resetRequest(ctx, row, m.mode)) })),
        });
      }
      group(editing);
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

  function copyText(text: string, done: string) {
    navigator.clipboard.writeText(text).then(
      () => confirm.say(done),
      () => confirm.say("Couldn’t copy to the clipboard."),
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
    },
  );

  /** A repository with no remote: make one on GitHub and push there. */
  async function publishToGitHub() {
    if (!repo || !history?.head.branch) return;
    if (!(await api.githubAccount().catch(() => null))) {
      confirm.say("Sign in to GitHub in Settings › Accounts first.");
      api.openSettings();
      return;
    }
    await run(startPublish(repo.path, repo.name, history.head.branch, headColor));
  }

  async function sync(kind: "fetch" | "pull" | "push") {
    // While a fetch runs, Fetch shows its progress, and Pull or Push waits for it in line.
    if (kind === "fetch" && (activity.running || activity.problem)) {
      activity.open = !activity.open;
      return;
    }
    if (kind !== "fetch" && activity.running) {
      const branch = history?.head.branch ?? "HEAD";
      activity.queue(kind, kind === "push" ? `Push ${branch}` : `Pull into ${branch}`);
      return;
    }
    // Commits made outside the app since the last reload belong in the sheet.
    await refresh();
    if (!remoteCtx) return;
    if (kind === "fetch") {
      // The sheet closes on Fetch: the fetch runs in the background, with its progress on the button.
      await confirm.run({ ...fetchRequest(remoteCtx), background: () => void backgroundFetch(false) });
      return;
    }
    const setup =
      kind === "pull"
        ? { ...(await api.pullSetup().catch(() => ({ mode: "rebase" as const, autostash: true }))), fetchFirst: prefs.get("oxbow.pull.fetchFirst") }
        : undefined;
    const request = kind === "pull" ? pullRequest(remoteCtx, setup) : pushRequest(remoteCtx);
    const opBefore = history?.operation ?? null;
    const ok = await confirm.run(request);
    // Even a failed pull or push may have fetched, so the counts are worth reloading either way.
    await refresh();
    checkRemoteTags();
    openConflicts(opBefore);
    if (ok) activity.note(kind, request.title.replace(/^Push /, "Pushed ").replace(/^Publish /, "Published ").replace(/^Pull /, "Pulled ").replace(/\?$/, ""), request.done ?? "");
  }
  activity.startQueued = (kind) => void sync(kind);

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
    if (!auto) confirm.say(activity.recent[0]?.sub === "Nothing new" ? `Fetched ${target.label}. Nothing new.` : `Fetched ${target.label}.`);
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

  /** Reload after the repository changed, here or outside the app, keeping the selection when it still exists. */
  async function refresh() {
    if (!repo || loading) return;
    const mine = generation;
    try {
      const next = await api.history();
      // Another repository was opened meanwhile: this answer may come from the old one.
      if (mine !== generation || loading) return;
      history = next;
      version++;
      oplog.load();
      lfs.load();
      if (!selected || !next.rows.some((r) => r.id === selected)) selected = next.head.commit ?? next.rows[0]?.id ?? null;
    } catch (err) {
      if (mine === generation) error = String(err);
    }
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

  function select(id: string) {
    selected = id;
    second = null;
    list?.reveal(id);
  }

  /** Picking a commit from the sidebar goes back to History, which mounts the graph again. */
  function pick(id: string) {
    selected = id;
    second = null;
    if (view === "history" && !showConflicts) return list?.reveal(id);
    view = "history";
    resolving = false;
    requestAnimationFrame(() => list?.reveal(id));
  }

  function startResize(event: PointerEvent) {
    const startX = event.clientX;
    const startWidth = panelWidth;
    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);
    const move = (e: PointerEvent) => {
      panelWidth = Math.min(Math.max(startWidth - (e.clientX - startX), 360), window.innerWidth - 560);
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
      if (typeof width === "number") panelWidth = Math.min(Math.max(width, 360), window.innerWidth - 560);
    });
    api.initialRepo().then((path) => {
      if (path) load(path);
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
  onfocus={refresh}
  onkeydown={(event) => {
    if ((event.metaKey || event.ctrlKey) && event.key === ",") {
      event.preventDefault();
      api.openSettings();
    } else if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "p" && history && !confirm.request) {
      // Quick Open: any file's history.
      event.preventDefault();
      nav.quickOpen = !nav.quickOpen;
    } else if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "f" && history && !confirm.request && view === "file" && !showConflicts) {
      // In File History, ⌘F finds in the file.
      event.preventDefault();
      findField?.focus();
    } else if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "f" && history && !confirm.request) {
      event.preventDefault();
      view = "history";
      resolving = false;
      requestAnimationFrame(() => searchField?.focus());
    } else if ((event.metaKey || event.ctrlKey) && !event.shiftKey && !event.altKey && event.key.toLowerCase() === "z" && history && !confirm.request && !start.sheet && !typing(event)) {
      // ⌘Z undoes the newest step of the Operation Log. In a text field it stays the field's own.
      event.preventDefault();
      undoLatest();
    } else if (event.key === "Escape" && second && view === "history" && !confirm.request && !event.defaultPrevented) {
      second = null;
    } else if ((event.metaKey || event.ctrlKey) && !event.altKey && !confirm.request && !start.sheet && !loading) {
      // Clone ⇧⌘C, Open ⌘O, New ⌘N, here and on the Welcome window.
      const key = event.key.toLowerCase();
      if (key === "c" && event.shiftKey) {
        event.preventDefault();
        start.clone();
      } else if (key === "o" && !event.shiftKey) {
        event.preventDefault();
        chooseRepo();
      } else if (key === "n" && !event.shiftKey) {
        event.preventDefault();
        start.newRepo();
      }
    }
  }}
/>

{#if !repo || !history}
  <Welcome {loading} {error} onOpen={chooseRepo} onOpenPath={load} />
{:else}
  <div class="window">
    <Sidebar {repo} {history} {selectedRow} ctx={branchCtx!} onOpen={chooseRepo} onOpenPath={load} onWelcome={closeRepo}
      onPick={pick}
      {run}
      {view}
      onView={(v) => {
        view = v;
        resolving = false;
        if (v === "history" && selected) requestAnimationFrame(() => list?.reveal(selected!));
      }}
      onPickStash={(id) => (stashSel = id)}
      onCompare={openCompare}
      onEditStack={openStack}
      onPullRequest={openPull}
    />
    <div class="main">
      <header data-tauri-drag-region>
        <div class="lead" data-tauri-drag-region>
          {#if view === "file" && !showConflicts}
            {@const back = fileBack === "compare" ? "Compare" : fileBack === "stashes" ? "Stashes" : "History"}
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
            {:else}
              <span class="name">History</span>
              {#if activity.running}
                <span class="sub">{activity.running.title}{activity.percent !== null ? ` · ${activity.percent}%` : "…"}</span>
              {:else if activity.problem?.failure}
                <span class="sub amber">Fetch failed · {failureShort(activity.problem.failure)}</span>
              {:else}
                <span class="sub">{branchCount} {branchCount === 1 ? "branch" : "branches"} · {history.head.branch ?? operation?.branch ?? "detached HEAD"}</span>
              {/if}
            {/if}
          </div>
          <!-- File History is about one file: a plain back arrow and no branch picker, as in the design, leave room for Find in file. -->
          {#if view !== "file" || showConflicts}
            <BranchPicker {history} {colorOf} onPick={(name) => branchCtx && run(switchRequest(branchCtx, name))} />
            <button class="capsule" onclick={newBranch} aria-label="New branch" title={history.head.branch ? "New Branch…" : "Create a branch at HEAD…"}>
              <svg class="icon" viewBox="0 0 16 16"><circle cx="4.5" cy="3.5" r="1.5" /><circle cx="4.5" cy="12.5" r="1.5" /><path d="M4.5 5v6M11.5 2.5v6M8.5 5.5h6" /></svg>
            </button>
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
            <SearchField bind:this={searchField} rows={history.rows} {matches} {selected} onGo={select} />
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
              <button
                onclick={() => sync("pull")}
                disabled={!canPull(remoteCtx)}
                aria-label={t?.behind ? `Pull ${t.behind} commits` : "Pull"}
                title={canPull(remoteCtx) ? `Pull from ${t?.remote}/${t?.branch}` : "This branch has no upstream to pull from"}
              >
                <svg class="icon" viewBox="0 0 16 16"><path d="M8 2v9M4.5 7.5 8 11l3.5-3.5M3 14h10" /></svg>
                {#if t?.behind}<span class="count">{t.behind}</span>{/if}
              </button>
              <button
                onclick={() => sync("push")}
                disabled={!canPush(remoteCtx)}
                aria-label={t ? (t.ahead ? `Push ${t.ahead} commits` : "Push") : "Publish branch"}
                title={!canPush(remoteCtx) ? "Check out a branch to push" : t && !t.gone ? `Push to ${t.remote}/${t.branch}` : `Publish ${history.head.branch} to ${remoteCtx.remote}`}
              >
                <svg class="icon" viewBox="0 0 16 16"><path d="M8 12V3M4.5 6.5 8 3l3.5 3.5M3 14h10" /></svg>
                {#if t?.ahead}<span class="count">{t.ahead}</span>{:else if !t && history.head.branch}<span class="count">Publish</span>{/if}
              </button>
            </div>
            <ActivityPopover fix={fixFetch} retry={() => backgroundFetch(false)} settings={() => ((activity.open = false), api.openSettings())} />
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
          <button class="capsule" onclick={refresh} aria-label="Reload history" title="Reload">
            <svg class="icon" viewBox="0 0 16 16"><path d="M13 8a5 5 0 1 1-1.5-3.5M13 2.5V5h-2.5" /></svg>
          </button>
          <button class="capsule" onclick={() => api.openSettings()} aria-label="Settings" title={withKeys("Settings", "Mod+,")}>
            <svg class="icon" viewBox="0 0 16 16"><path d="M12.78 6.52L14.44 6.56L14.44 9.44L12.78 9.48L12.42 10.33L13.57 11.54L11.54 13.57L10.33 12.42L9.48 12.78L9.44 14.44L6.56 14.44L6.52 12.78L5.67 12.42L4.46 13.57L2.43 11.54L3.58 10.33L3.22 9.48L1.56 9.44L1.56 6.56L3.22 6.52L3.58 5.67L2.43 4.46L4.46 2.43L5.67 3.58L6.52 3.22L6.56 1.56L9.44 1.56L9.48 3.22L10.33 3.58L11.54 2.43L13.57 4.46L12.42 5.67zM8 5.8a2.2 2.2 0 1 0 0 4.4a2.2 2.2 0 1 0 0-4.4" /></svg>
          </button>
          <OperationLog repo={repo.name} run={(request) => void run(request)} />
        </div>
      </header>
      {#if searching}
        <SearchBar {history} count={matches.length} copy={(text) => copyText(text, "Copied the command.")} />
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
      {:else if !history.head.branch && history.head.commit && branchCtx}
        <DetachedBanner {history} back={backTo} onBack={() => backTo && run(switchRequest(branchCtx!, backTo))} onKeep={newBranch} />
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
          <FileHistoryView {history} path={nav.file.path} start={nav.file.commit} {version} lookup={(id) => rowsById.get(id)} copy={(text) => copyText(text, `Copied ${text.length > 40 ? "the command" : text}.`)} />
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
            {panelWidth}
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
      {:else if view === "stashes" && branchCtx}
        {#key repo.path}
          <StashesView {history} ctx={branchCtx} {colorOf} bind:selected={stashSel} {version} {run} copy={(text) => copyText(text, `Copied ${text.length > 40 ? "it" : text}.`)} />
        {/key}
      {:else}
      <div class="content">
        <section class="history" aria-label="Commit history">
          <div class="columns"><span>{#if found?.only}Matching commits · graph hidden{:else}Description{/if}</span>{#if history.truncated}<span class="note">Showing the latest {history.rows.length.toLocaleString()} commits</span>{/if}</div>
          {#key repo.path}
            <HistoryList bind:this={list} {history} {selected} onSelect={select} menuFor={commitMenu} {found} second={pair ? second : null} onSecond={pickSecond} />
          {/key}
        </section>
        <aside class="panel" style:width="{panelWidth}px" aria-label="Commit details">
          <button class="grip" onpointerdown={startResize} aria-label="Resize commit details" title="Drag to resize"></button>
          <!-- A new repository starts the panels from scratch. -->
          {#key repo.path}
            {#if pair}
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
                branch={history.head.branch}
                color={selectedRow.graph.color}
                head={headRow}
                {version}
                onChanged={refresh}
                onResolve={operation ? () => (resolving = true) : undefined}
              />
            {:else if selectedRow}
              <CommitPanel row={selectedRow} childIds={childrenOf.get(selectedRow.id) ?? []} lookup={(id) => rowsById.get(id)} onSelect={select} localTags={history.localTags}
                find={found?.mode === "code" ? found.text : null}
                foundFiles={found?.hits.get(selectedRow.id) ?? []}
                foundText={found?.text ?? null}
              />
            {/if}
          {/key}
        </aside>
      </div>
      {/if}
    </div>
  </div>
  {#if nav.quickOpen}
    <QuickOpen commit={selectedRow && !selectedRow.worktree ? { id: selectedRow.id, summary: selectedRow.summary } : null} onClose={() => (nav.quickOpen = false)} />
  {/if}
  <ConfirmSheet repo={repo.name} branch={history.head.branch ?? operation?.branch ?? null} color={headColor} />
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
    left: 264px;
    top: 0;
    right: 0;
    bottom: 0;
    display: flex;
    flex-direction: column;
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
  }
  .title span {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .name {
    font-size: 15px;
    font-weight: 700;
  }
  .sub {
    font-size: 11px;
    color: var(--text2);
  }
  .capsule {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    height: 34px;
    padding: 0 12px;
    border-radius: 17px;
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
    border-radius: 14px;
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
    border-radius: 17px;
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
    border-radius: 17px;
  }
  .group button:disabled {
    opacity: 0.4;
  }
  .sync {
    position: relative;
    display: flex;
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
    font-size: 11px;
    font-weight: 600;
    color: var(--accent-text);
  }
  .segmented {
    display: flex;
    padding: 3px;
    border-radius: 17px;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
  }
  .segmented button {
    height: 28px;
    padding: 0 16px;
    border-radius: 14px;
    color: var(--text);
    font-weight: 500;
  }
  .segmented button.on {
    background: var(--side-sel);
    font-weight: 600;
  }
  .error {
    color: var(--red);
    font-size: 12px;
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
  /* The graph is a card of its own, like the file list in Summary. */
  .history {
    flex-grow: 1;
    min-width: 0;
    margin: 4px 12px 12px;
    border: 1px solid var(--sep);
    border-radius: 12px;
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
    padding: 0 18px 0 40px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
  }
  .columns span:first-child {
    flex-grow: 1;
  }
  .note {
    font-weight: 400;
  }
  .panel {
    position: relative;
    flex-shrink: 0;
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
    border-radius: 4px;
    background: var(--win);
    border: 1px solid var(--glass-border);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.08);
    z-index: 2;
    cursor: col-resize;
  }
</style>
