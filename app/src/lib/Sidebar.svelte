<script lang="ts">
  import { lens } from "./lens.svelte";
  import { oplog } from "./oplog.svelte";
  import { shortcut } from "./keys";
  import type { History, HistoryRow, RefInfo, RepoSummary } from "./types";
  import type { ForgeKind } from "./forge";
  import { lane, NO_BRANCH_COLOR, shortTime, tint } from "./format";
  import { sharedLeads } from "./refNames";
  import type { Request } from "./confirm.svelte";
  import { confirm } from "./confirm.svelte";
  import { api } from "./api";
  import Menu, { menuIcons, type MenuEntry } from "./Menu.svelte";
  import {
    deleteRemoteRequest,
    deleteRequest,
    detachRequest,
    keepRequest,
    newBranchRequest,
    renameRequest,
    switchRequest,
    trackRequest,
    upstreamOf,
    type BranchContext,
  } from "./branches";
  import { pushRequest } from "./remote";
  import { mergeRequest } from "./merge";
  import { dropsPushed, isProtected, lockedNote } from "./protect";
  import { branchMergeRequest, gitlabOf, signedInTo } from "./mergeRequest";
  import { savePatchesRequest } from "./patches";
  import { stashMenu } from "./stash";
  import { deleteTagRequest, fetchTagsRequest, isLocalTag, newTagRequest, pushTagsRequest } from "./tags";
  import { start } from "./start.svelte";
  import { github } from "./github.svelte";
  import { downloadRequest, installRequest, lfs, patternNote, pruneRequest, lfsTrackRequest, untrackRequest } from "./lfs.svelte";
  import { bytes } from "./unusual";
  import { nav } from "./nav.svelte";
  import type { LfsPattern, ReadPart, RecentRepo, Submodule, Tracking, Worktree } from "./types";
  import { PART_NAMES, age, errorLine, type ReadStates } from "./reads";
  import { checking, stale } from "./folders";
  import { addSubmoduleRequest, openSubmoduleWindow, submoduleMenu as submoduleEntries, submoduleNote, submoduleWhere, syncRequest, updateRequest } from "./submodules";
  import { actionIcons } from "./icons";
  import { notInHistory } from "./objects";
  import { locateRequest, lockRequest, newWorktreeRequest, pruneWorktreesRequest, removeWorktreeRequest, tilde, unlockRequest, worktreeOf } from "./worktrees";

  const PULL_ICON = menuIcons.pull;
  /** A speech bubble with a tick: pull requests to review. */
  const REVIEW_ICON = "M2.5 4.2c0-.7.5-1.2 1.2-1.2h8.6c.7 0 1.2.5 1.2 1.2v5.6c0 .7-.5 1.2-1.2 1.2H7l-3 2.5V11h-.3c-.7 0-1.2-.5-1.2-1.2zM5.5 7l1.6 1.5 3.4-3";
  const FOLDER = "M2.5 4.5a1 1 0 0 1 1-1h3l1.5 1.5h4.5a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1z";
  const HOME = "M2.5 7.5 8 3l5.5 4.5M4 6.5v6.5h8V6.5";
  /** A list with a ×: Clear Recent. */
  const CLEAR = "M2.5 4h11M2.5 8h6M2.5 12h4M10 9.5l3.5 3.5M13.5 9.5 10 13";
  const RELOAD = "M13 8a5 5 0 1 1-1.5-3.5M13 2.5V5h-2.5";
  const ACTIVITY = "M1.5 8.5h3l2-5 3 9 2-4h3";
  const LOCK = "M4.5 7.5h7v6h-7zM6 7.5V5.5a2 2 0 0 1 4 0v2";

  let {
    repo,
    history,
    reads,
    onRetry,
    selectedRow,
    ctx,
    onOpen,
    onOpenPath,
    onWelcome,
    onPick,
    run,
    view,
    onView,
    onChanges,
    onPickStash,
    onCompare,
    onEditStack,
    onPullRequest,
    submodule = null,
    onSubmodule,
    onReload,
    onActivity,
  }: {
    repo: RepoSummary;
    history: History;
    /** How well the worktrees and submodules were read; null before the first read. */
    reads: ReadStates | null;
    /** Read a part again after it failed. */
    onRetry: (part: ReadPart) => void;
    selectedRow: HistoryRow | null;
    ctx: BranchContext;
    onOpen: () => void;
    onOpenPath: (path: string) => void;
    /** Close the repository and show the Welcome window. */
    onWelcome: () => void;
    /** Select a commit in History, loading older commits down to it; `missing` says why it
     *  can't be shown when no branch or tag reaches it. */
    onPick: (commit: string, missing?: string) => void;
    /** Confirm and run a change to the repository. */
    run: (request: Request | Promise<Request>) => void;
    /** The screen shown next to the sidebar. */
    view: "history" | "stashes" | "oplog" | "compare" | "file" | "stack" | "pull" | "reviews";
    onView: (view: "history" | "stashes" | "oplog" | "reviews") => void;
    /** Open the uncommitted changes in the details panel, next to History. */
    onChanges: () => void;
    /** On the Stashes screen a stash in the list is picked there instead of in History. */
    onPickStash: (id: string) => void;
    /** Open Compare: what `target` has against `base`. */
    onCompare: (base: string, target: string) => void;
    /** Open Edit Stack for the stack `branch` is in. */
    onEditStack: (branch: string) => void;
    /** Open the Pull Request screen of `branch`. */
    /** The Pull Request screen of `branch`; `forge` "gitlab": its merge request. */
    onPullRequest: (branch: string, number?: number | null, forge?: ForgeKind) => void;
    /** The path of the submodule shown next to the sidebar, if one is. */
    submodule?: string | null;
    /** Show a submodule's card in place of the commit's. */
    onSubmodule: (path: string) => void;
    /** Read the repository again, for changes the app hasn't noticed yet. */
    onReload: () => void;
    /** Open Activity, under Fetch; null with no remote to fetch from. */
    onActivity: (() => void) | null;
  } = $props();

  /** The uncommitted changes, counted on their row above HEAD; null when there are none. */
  const changes = $derived(history.rows.find((r) => r.worktree)?.worktree ?? null);
  const showingChanges = $derived(view === "history" && !!selectedRow?.worktree);

  /** The checked-out branch's pull request, for the Workspace item. */
  const headPull = $derived(history.head.branch ? github.pullOf(history.head.branch) : null);
  const headMr = $derived(history.head.branch && github.mrProject ? github.mergeRequestOf(history.head.branch) : null);

  const headName = $derived(history.head.branch ?? "HEAD");
  const compareItem = (name: string): MenuEntry => ({ kind: "item", label: `Compare with ${headName}`, icon: menuIcons.compare, run: () => onCompare(headName, name) });

  /** The open right-click menu and the ref it belongs to. */
  let menu = $state<{ x: number; y: number; label: string; ref: string; entries: MenuEntry[] } | null>(null);
  let collapsedRemotes = $state<Record<string, boolean>>({});

  /** Lists longer than this show only their first items until expanded. */
  const COLLAPSED = 10;

  // The ref clicked last, while its commit is still the selected one.
  let clicked = $state<RefInfo | null>(null);
  let expanded = $state<Record<string, boolean>>({});
  let list = $state<HTMLDivElement>();

  // Keep the highlighted item in view when the selection moves to another branch.
  $effect(() => {
    const name = focused;
    if (!name || !list) return;
    const item = [...list.querySelectorAll<HTMLElement>("[data-ref]")].find((el) => el.dataset.ref === name);
    item?.scrollIntoView({ block: "nearest" });
  });

  /** The branch or tag highlighted in the sidebar follows the selected commit. */
  const focused = $derived(
    clicked && clicked.target === selectedRow?.id
      ? clicked.name
      : (selectedRow?.graph.branch ??
          (selectedRow?.noBranch ? "HEAD" : null) ??
          selectedRow?.labels.find((l) => l.kind === "stash")?.name ??
          null),
  );

  const rowOf = $derived(new Map(history.rows.map((row) => [row.id, row])));
  /** While HEAD is detached, its commit. */
  const detached = $derived(!history.head.branch && history.head.commit && history.operation?.kind !== "rebase" ? (rowOf.get(history.head.commit) ?? null) : null);
  /** The branch a merge, rebase, cherry-pick or revert is changing, and the word for it. */
  const busy = $derived.by(() => {
    const op = history.operation;
    if (!op?.branch) return null;
    const word = { merge: "MERGING", squash: "SQUASHING", rebase: "REBASING", cherryPick: "PICKING", revert: "REVERTING", stashApply: "APPLYING", am: "PATCHING" }[op.kind];
    return { branch: op.branch, word };
  });

  /** Local branches: the checked-out one first, the rest by latest commit, newest first. */
  const branches = $derived(
    history.refs
      .filter((r) => r.kind === "local")
      .map((r) => ({ ref: r, time: rowOf.get(r.target)?.time ?? 0, color: rowOf.get(r.target)?.graph.color ?? 0 }))
      .sort((a, b) => {
        const ha = a.ref.name === history.head.branch ? 1 : 0;
        const hb = b.ref.name === history.head.branch ? 1 : 0;
        return hb - ha || b.time - a.time || a.ref.name.localeCompare(b.ref.name);
      }),
  );
  /** How much of each local branch's name is a start it shares with another, to give way first. */
  const branchLeads = $derived(sharedLeads(branches.map((b) => b.ref.name)));
  const tags = $derived(
    history.refs
      .filter((r) => r.kind === "tag")
      .map((r) => ({ ref: r, time: rowOf.get(r.target)?.time ?? 0 }))
      .sort((a, b) => b.time - a.time || b.ref.name.localeCompare(a.ref.name)),
  );

  /** Stashes, newest first. */
  const stashes = $derived(
    history.stashes.map((s) => ({
      ref: { name: `stash@{${s.index}}`, kind: "stash" as const, target: s.id, remote: null, tracking: null },
      title: s.title,
      branch: s.branch,
      info: s,
    })),
  );

  /** Per remote, its branches that no local branch tracks, newest first, and how many are tracked. */
  const remoteBranches = $derived(
    new Map(
      history.remotes.map((remote) => {
        // A remote branch some local branch tracks, or one with the same name, is "tracked locally".
        const tracked = new Set(
          history.refs
            .filter((r) => r.kind === "local")
            .flatMap((r) => [`${remote}/${r.name}`, ...(r.tracking?.remote === remote ? [`${remote}/${r.tracking.branch}`] : [])]),
        );
        const all = history.refs.filter((r) => r.kind === "remote" && r.remote === remote);
        const own = all
          .filter((r) => !tracked.has(r.name))
          .map((r) => {
            const row = rowOf.get(r.target);
            return { ref: r, time: row?.time ?? 0, color: row?.graph.color ?? 0, author: row?.authorName ?? "" };
          })
          .sort((a, b) => b.time - a.time || a.ref.name.localeCompare(b.ref.name));
        return [remote, { own, tracked: all.filter((r) => tracked.has(r.name)).map((r) => r.name) }];
      }),
    ),
  );

  const firstName = (name: string) => name.split(/\s+/)[0] ?? name;

  function copy(text: string) {
    navigator.clipboard.writeText(text).then(
      () => confirm.say(`Copied ${text}.`, "done"),
      () => confirm.say("Couldn’t copy to the clipboard.", "error"),
    );
  }

  function open(event: MouseEvent, label: string, ref: string, entries: MenuEntry[]) {
    event.preventDefault();
    menu = { x: event.clientX, y: event.clientY, label, ref, entries };
  }

  /** The repository button's menu: recent repositories, then clone, open, new. */
  async function repoMenu(event: MouseEvent) {
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const recent = (await api.recentRepos().catch(() => [])).filter((r) => r.path !== repo.path && !r.missing);
    menu = { x: box.left, y: box.bottom + 4, label: `Repository ${repo.name}`, ref: repo.path, entries: repoEntries(recent) };
  }

  /** How many recent repositories the repository menu lists. */
  const RECENT_IN_MENU = 8;

  function repoEntries(recent: RecentRepo[]): MenuEntry[] {
    const entries: MenuEntry[] = [];
    if (recent.length) {
      entries.push({ kind: "header", label: "Recent" });
      for (const r of recent.slice(0, RECENT_IN_MENU)) {
        entries.push({
          kind: "item",
          label: r.name,
          detail: tilde(r.path),
          icon: FOLDER,
          run: () => onOpenPath(r.path),
          remove: { label: "Remove from Recent", run: () => forgetRecent(recent, r.path) },
        });
      }
      entries.push({ kind: "item", label: "Clear Recent", icon: CLEAR, run: clearRecent }, { kind: "sep" });
    }
    entries.push(
      { kind: "item", label: "Clone Repository…", icon: "M8 2.5v8M5 7.5l3 3 3-3M3 13.5h10", keys: shortcut("clone"), run: () => start.clone() },
      { kind: "item", label: "Open Local Repository…", icon: FOLDER, keys: shortcut("open"), run: onOpen },
      { kind: "item", label: "New Repository…", icon: "M8 3v10M3 8h10", keys: shortcut("newRepo"), run: () => start.newRepo() },
      { kind: "sep" },
    );
    if (history.head.commit) entries.push({ kind: "item", label: "Add Submodule…", icon: actionIcons.box, run: () => run(addSubmoduleRequest(submodules, repo.path)) });
    entries.push(
      { kind: "item", label: "Reload", icon: RELOAD, keys: shortcut("reload"), run: onReload },
      ...(onActivity ? [{ kind: "item" as const, label: "Activity", icon: ACTIVITY, run: onActivity }] : []),
      { kind: "item", label: "Open in Terminal", icon: menuIcons.terminal, run: () => api.openInTerminal().catch((err) => confirm.fail(err)) },
      { kind: "item", label: "Welcome Window", icon: HOME, run: onWelcome },
    );
    return entries;
  }

  /** × on a recent repository: off the list, the menu stays open and the next one moves up. */
  function forgetRecent(recent: RecentRepo[], path: string) {
    api.forgetRepo(path).catch((err) => confirm.fail(err));
    if (menu) menu = { ...menu, entries: repoEntries(recent.filter((r) => r.path !== path)) };
  }

  /** Clear Recent: every repository but the one open now. */
  async function clearRecent() {
    try {
      await api.clearRecent([repo.path]);
      confirm.say("Cleared Recent Repositories.", "done");
    } catch (err) {
      confirm.fail(err);
    }
  }

  /** The right-click menu of a local branch. */
  function branchMenu(event: MouseEvent, ref: RefInfo) {
    const isHead = ref.name === history.head.branch;
    const isTrunk = ref.name === history.trunk;
    const t = ref.tracking;
    const status = isHead
      ? "checked out"
      : t?.gone
        ? "deleted on the remote"
        : !t
          ? "not on a remote yet"
          : t.ahead
            ? `${t.ahead} to push`
            : `on ${t.remote}`;
    const item = (label: string, icon: string, act: () => void, danger = false): MenuEntry => ({ kind: "item", label, icon, run: act, danger });
    const elsewhere = worktreeOf(history, ref.name);
    const entries: MenuEntry[] = [{ kind: "header", label: `${ref.name} · ${elsewhere ? `in ${elsewhere.name}` : status}` }];
    const remove = () => run(api.deletionCheck(ref.name).then((check) => deleteRequest(ctx, ref, check)));
    if (t?.gone && !isHead && !elsewhere) {
      entries.push(item(`Delete ${ref.name}…`, menuIcons.drop, remove, true));
      entries.push({ kind: "note", label: "Its branch on the remote is gone, usually after its pull request was merged." });
      entries.push({ kind: "sep" });
    }
    if (elsewhere && !elsewhere.missing) entries.push(item(`Open ${elsewhere.name}`, menuIcons.open, () => openWorktree(elsewhere)));
    else if (!isHead) entries.push(item("Check Out", menuIcons.checkout, () => run(switchRequest(ctx, ref.name))));
    if (!isHead && !elsewhere) entries.push(item("Open in New Worktree…", menuIcons.folder, () => run(newWorktreeRequest(ctx, repo.path, ref.name))));
    if (!isHead) entries.push(...mergeItems(ref.name));
    if (!isHead) entries.push(compareItem(ref.name));
    if (!upstreamOf(ref) && history.defaultRemote) {
      entries.push({ kind: "sep" });
      const unpushed = history.rows.filter((r) => r.unpushed && !r.worktree && r.graph.branch === ref.name);
      entries.push(
        item(`Publish to ${history.defaultRemote}`, menuIcons.push, () =>
          run(
            pushRequest({
              branch: ref.name,
              color: ctx.colorOf(ref.name),
              tracking: null,
              remote: history.defaultRemote,
              remotes: history.remotes,
              unpushed,
            }),
          ),
        ),
      );
    }
    // A protected branch on a remote has pushed commits Edit Stack would rewrite.
    const locked = isProtected(history.protected, ref.name) && !!t && !t.gone;
    if (!isTrunk && !history.operation) {
      entries.push({ kind: "sep" });
      if (locked) entries.push({ kind: "note", label: lockedNote(ref.name) });
      else entries.push(item("Edit Stack…", menuIcons.rebase, () => onEditStack(ref.name)));
    }
    if (!isTrunk && github.repo) {
      const pull = github.pullOf(ref.name);
      if (history.operation) entries.push({ kind: "sep" });
      entries.push(item(pull ? `Pull Request #${pull.number}` : "Create Pull Request…", PULL_ICON, () => onPullRequest(ref.name)));
    }
    const project = !isTrunk ? gitlabOf(history, github.gitlab, ref) : null;
    const mr = project ? github.mergeRequestOf(ref.name) : null;
    if (mr) entries.push(item(`Merge Request !${mr.number}`, PULL_ICON, () => onPullRequest(ref.name, null, "gitlab")));
    else if (project) entries.push(item("Create Merge Request…", PULL_ICON, () => (ref.kind === "local" && signedInTo(project) ? onPullRequest(ref.name, null, "gitlab") : run(branchMergeRequest(ctx, ref, project)))));
    entries.push({ kind: "sep" });
    entries.push(item(`New Branch from ${ref.name}…`, menuIcons.branch, () => run(newBranchRequest(ctx, ref.name))));
    if (!isTrunk) entries.push(item("Rename…", menuIcons.edit, () => run(renameRequest(ctx, ref))));
    // The branch's own commits, the ones the default branch doesn't have.
    const trunk = history.trunk;
    if (!isTrunk && trunk) {
      const what = ["The commits of ", { branch: ref.name, color: ctx.colorOf(ref.name) }, " that ", { branch: trunk, color: ctx.colorOf(trunk) }, " doesn’t have"];
      entries.push(item("Save as Patches…", menuIcons.patch, () => run(savePatchesRequest({ to: ref.name, since: trunk, count: 0, name: ref.name, what }))));
    }
    entries.push(item("Copy Name", menuIcons.copy, () => copy(ref.name)));
    if (!isHead && !isTrunk && !t?.gone && !elsewhere) {
      entries.push({ kind: "sep" });
      entries.push(item("Delete…", menuIcons.drop, remove, true));
    }
    if (elsewhere) entries.push({ kind: "note", label: elsewhere.missing ? `Checked out in ${elsewhere.name}, whose folder is gone. Prune it under Worktrees to free the branch.` : `Checked out in ${elsewhere.name}. Git keeps a branch in one folder at a time.` });
    else if (isTrunk) entries.push({ kind: "note", label: "Default branch: rename and delete are turned off." });
    else if (isHead) entries.push({ kind: "note", label: "Checked out. Switch to another branch to delete it." });
    open(event, "Branch menu", ref.name, entries);
  }

  /** Merge and Rebase items for a branch other than the checked-out one. */
  function mergeItems(name: string): MenuEntry[] {
    const here = history.head.branch ?? "HEAD";
    // A merge in progress has to be finished first; the banner says so.
    if (history.operation) return [];
    // Not a rebase that would rewrite pushed commits of a protected branch.
    const tip = history.refs.find((r) => (r.kind === "local" || r.kind === "remote") && r.name === name)?.target ?? null;
    return [
      { kind: "item", label: `Merge into ${here}…`, icon: menuIcons.merge, run: () => run(mergeRequest(ctx, name)) },
      ...(history.head.branch && !dropsPushed(history, tip) ? [{ kind: "item", label: `Rebase ${here} onto ${name}…`, icon: menuIcons.rebase, run: () => run(mergeRequest(ctx, name, "rebase")) } as MenuEntry] : []),
    ];
  }

  /** The right-click menu of a remote branch nobody tracks locally. */
  function remoteMenu(event: MouseEvent, ref: RefInfo, author: string) {
    const short = ref.name.slice((ref.remote ?? "").length + 1);
    const taken = history.refs.some((r) => r.kind === "local" && r.name === short);
    const entries: MenuEntry[] = [{ kind: "header", label: `${ref.name}${author ? ` · ${author}` : ""}` }];
    if (!taken) entries.push({ kind: "item", label: "Check Out as Local Branch", icon: menuIcons.checkout, run: () => run(trackRequest(ctx, ref, author)) });
    else entries.push({ kind: "note", label: `A local branch ${short} already exists, so it can’t be checked out under that name.` });
    entries.push(...mergeItems(ref.name));
    entries.push(compareItem(ref.name));
    entries.push({ kind: "item", label: "Copy Name", icon: menuIcons.copy, run: () => copy(ref.name) });
    entries.push({ kind: "sep" });
    entries.push({ kind: "item", label: `Delete on ${ref.remote}…`, icon: menuIcons.drop, danger: true, run: () => run(api.remoteDeletionCheck(ref.name).then((lost) => deleteRemoteRequest(ctx, ref, author, lost))) });
    open(event, "Remote branch menu", ref.name, entries);
  }

  /** The right-click menu of the detached HEAD row. */
  function detachedMenu(event: MouseEvent, row: HistoryRow) {
    const sha = row.id.slice(0, 7);
    open(event, "Detached HEAD menu", "HEAD", [
      { kind: "header", label: `HEAD · ${sha} · not on a branch` },
      { kind: "item", label: "Create Branch…", icon: menuIcons.branch, run: () => run(keepRequest(ctx)) },
      { kind: "item", label: "Copy SHA", icon: menuIcons.copy, run: () => copy(row.id) },
    ]);
  }

  /** The right-click menu of a tag. */
  function tagMenu(event: MouseEvent, ref: RefInfo) {
    const row = rowOf.get(ref.target);
    const entries: MenuEntry[] = [{ kind: "header", label: ref.name }];
    if (row && row.id !== history.head.commit) {
      entries.push({ kind: "item", label: "Check Out", icon: menuIcons.checkout, run: () => run(detachRequest(ctx, row, ref.name)) });
      entries.push({ kind: "note", label: "HEAD goes to the tagged commit, on no branch." });
    }
    if (row) entries.push({ kind: "item", label: `New Branch from ${ref.name}…`, icon: menuIcons.branch, run: () => run(newBranchRequest(ctx, undefined, row)) });
    entries.push(compareItem(ref.name));
    const remote = history.defaultRemote;
    if (remote && isLocalTag(ctx, ref.name)) {
      entries.push({ kind: "sep" });
      entries.push({ kind: "item", label: `Push to ${remote}`, icon: menuIcons.push, run: () => run(pushTagsRequest(ctx, [ref.name])) });
    }
    entries.push({ kind: "item", label: "Copy Name", icon: menuIcons.copy, run: () => copy(ref.name) });
    entries.push({ kind: "sep" });
    entries.push({ kind: "item", label: "Delete Tag…", icon: menuIcons.drop, danger: true, run: () => run(deleteTagRequest(ctx, ref.name)) });
    open(event, "Tag menu", ref.name, entries);
  }

  /** The menu of the Tags heading: tags as a whole. */
  function tagsMenu(event: MouseEvent) {
    const remote = history.defaultRemote;
    const local = history.localTags;
    const entries: MenuEntry[] = [{ kind: "item", label: "New Tag…", icon: menuIcons.tag, run: () => run(newTagRequest(ctx)) }];
    if (remote) {
      entries.push({ kind: "sep" });
      if (local.length) entries.push({ kind: "item", label: `Push ${local.length} Local ${local.length === 1 ? "Tag" : "Tags"} to ${remote}`, icon: menuIcons.push, run: () => run(pushTagsRequest(ctx, local)) });
      entries.push({ kind: "item", label: `Fetch Tags from ${remote}`, icon: menuIcons.fetch, run: () => run(fetchTagsRequest(ctx)) });
    }
    open(event, "Tags menu", "tags-heading", entries);
  }

  /** What to do when an LFS action needs git-lfs and it isn't ready: install it or turn it on. */
  function needsLfs(): MenuEntry[] {
    const request = installRequest(repo.name);
    if (request) return [{ kind: "item", label: `${request.button} Git LFS…`, icon: menuIcons.lfs, run: () => run(request) }];
    return [
      { kind: "item", label: "Download Git LFS…", icon: menuIcons.open, run: () => api.openLfsDownload().catch((err) => confirm.fail(err)) },
      { kind: "note", label: "git-lfs isn’t installed, and Oxbow didn’t find Homebrew to install it with." },
    ];
  }

  /** LFS files that are only pointers here, and how big they are once downloaded. */
  const lfsMissing = $derived(lfs.patterns.reduce((n, p) => n + p.missing, 0));
  const lfsMissingSize = $derived((lfs.status?.missingFiles ?? []).reduce((n, f) => n + f.size, 0));

  /** The menu of the Git LFS heading: LFS as a whole. */
  function lfsMenu(event: MouseEvent) {
    const missing = lfs.patterns.reduce((n, p) => n + p.missing, 0);
    const entries: MenuEntry[] = [];
    if (!lfs.ready) entries.push(...needsLfs(), { kind: "sep" });
    else {
      entries.push({ kind: "item", label: "Track Files…", icon: menuIcons.lfs, run: () => run(lfsTrackRequest(null, null)) });
      entries.push({ kind: "sep" });
      entries.push({ kind: "item", label: missing ? `Download ${missing} ${missing === 1 ? "File" : "Files"}` : "Download All Files", icon: menuIcons.fetch, run: () => run(downloadRequest(null)) });
      entries.push({ kind: "item", label: "Free Up Space…", icon: menuIcons.drop, run: () => run(api.lfsPrunePreview().catch(() => null).then(pruneRequest)) });
    }
    open(event, "Git LFS menu", "lfs-heading", entries);
  }

  /** The menu of one LFS pattern. */
  function patternMenu(event: MouseEvent, p: LfsPattern) {
    const entries: MenuEntry[] = [{ kind: "header", label: `${p.pattern} · ${p.files} ${p.files === 1 ? "file" : "files"}` }];
    if (!lfs.ready) entries.push(...needsLfs());
    else if (p.missing) entries.push({ kind: "item", label: `Download ${p.missing} ${p.missing === 1 ? "File" : "Files"}`, icon: menuIcons.fetch, run: () => run(downloadRequest(p)) });
    entries.push({
      kind: "item",
      label: "Show Matching Files",
      icon: menuIcons.folder,
      run: () => {
        nav.quickPattern = p.pattern;
        nav.quickOpen = true;
      },
    });
    entries.push({ kind: "item", label: "Copy Pattern", icon: menuIcons.copy, run: () => copy(p.pattern) });
    if (lfs.ready) {
      entries.push({ kind: "sep" });
      entries.push({ kind: "item", label: "Stop Tracking…", icon: menuIcons.lfs, run: () => run(untrackRequest(p.pattern)) });
    }
    open(event, "Git LFS pattern menu", `lfs:${p.pattern}`, entries);
  }

  /** Working folders of the repository, the main one first, as git lists them. */
  const worktrees = $derived(history.worktrees);

  /** A worktree in its own window, or the window that already has it. */
  function openWorktree(wt: Worktree) {
    api.openWindow(wt.path).catch((err) => confirm.fail(err));
  }

  /** The commit a worktree has checked out, selected in History, older commits loaded down to it. */
  function showWorktree(wt: Worktree) {
    if (!wt.head) return;
    clicked = wt.branch ? (history.refs.find((r) => r.kind === "local" && r.name === wt.branch) ?? null) : null;
    onPick(wt.head, notInHistory(wt.head, wt));
  }

  /** What the note on a worktree's row says. */
  function worktreeNote(wt: Worktree): { text: string; tone?: "accent" | "warn" } | null {
    if (wt.current) return { text: "this window", tone: "accent" };
    if (wt.missing) return { text: "missing", tone: "warn" };
    if (checking(wt)) return { text: "checking…" };
    if (wt.changed === null) return { text: "status unknown", tone: "warn" };
    if (stale(wt)) return { text: "may be stale", tone: "warn" };
    if (wt.changed) return { text: `${wt.changed} changed` };
    return null;
  }

  /** How far a worktree's branch is ahead of and behind its upstream, when it has one. */
  function worktreeTracking(wt: Worktree): Tracking | null {
    if (!wt.branch) return null;
    const tracking = history.refs.find((r) => r.kind === "local" && r.name === wt.branch)?.tracking;
    return tracking && !tracking.gone ? tracking : null;
  }

  /** What locking does, for the row's tooltip and the menu. */
  const LOCK_WHY = "Git won’t prune or remove a locked worktree, so it stays known while its folder is away, e.g. on a drive that isn’t connected.";

  /** The tooltip of a worktree's row. */
  function worktreeTitle(wt: Worktree): string {
    const lines = [wt.missing ? `Folder not found at ${tilde(wt.path)}` : wt.current ? tilde(wt.path) : `${tilde(wt.path)} · double-click to open its window`];
    if (checking(wt)) lines.push("Counting its uncommitted changes…");
    else if (wt.changed === null) lines.push(`Couldn’t count its uncommitted changes: ${errorLine(wt.statusError)}`);
    else if (stale(wt))
      lines.push(`The last count found ${wt.changed ? `${wt.changed} ${wt.changed === 1 ? "file" : "files"} with uncommitted changes` : "no uncommitted changes"}; counting again failed: ${errorLine(wt.statusError)}`);
    else if (wt.changed) lines.push(`${wt.changed} ${wt.changed === 1 ? "file" : "files"} with uncommitted changes there`);
    const tracking = worktreeTracking(wt);
    if (tracking && (tracking.ahead || tracking.behind))
      lines.push(`${wt.branch}: ${[tracking.ahead && `${tracking.ahead} to push`, tracking.behind && `${tracking.behind} to pull`].filter(Boolean).join(", ")} (${tracking.remote}/${tracking.branch})`);
    const pull = wt.branch && github.repo ? github.pullOf(wt.branch) : null;
    if (pull) lines.push(`Pull request #${pull.number} (${pull.draft && pull.state === "open" ? "draft" : pull.state}): ${pull.title}`);
    if (wt.locked !== null) lines.push(`Locked${wt.locked ? `: ${wt.locked}` : ""}. ${LOCK_WHY}`);
    return lines.join("\n");
  }

  /** The menu of a worktree. */
  function worktreeMenu(event: MouseEvent, wt: Worktree) {
    const where = wt.branch ?? (wt.head ? `detached at ${wt.head.slice(0, 7)}` : "no commits");
    const entries: MenuEntry[] = [{ kind: "header", label: `${wt.name} · ${where}` }];
    const lock: MenuEntry[] = wt.main
      ? []
      : wt.locked !== null
        ? [{ kind: "item", label: "Unlock…", icon: LOCK, run: () => run(unlockRequest(wt)) }]
        : [{ kind: "item", label: "Lock…", icon: LOCK, run: () => run(lockRequest(wt)) }];
    if (wt.missing) {
      entries.push({ kind: "note", label: `Folder not found at ${tilde(wt.path)}.` });
      entries.push({ kind: "item", label: "Locate Folder…", icon: menuIcons.folder, run: () => locateRequest(ctx, wt).then((request) => request && run(request)) });
      if (wt.locked === null) entries.push({ kind: "item", label: "Prune…", icon: menuIcons.drop, run: () => run(pruneWorktreesRequest(ctx)) });
      entries.push(...lock);
      entries.push({ kind: "item", label: "Copy Path", icon: menuIcons.copy, run: () => copy(wt.path) });
      if (wt.locked !== null) entries.push({ kind: "note", label: "Locked, so Prune leaves it. Unlock it to let Prune forget it." });
      open(event, "Worktree menu", `worktree:${wt.path}`, entries);
      return;
    }
    if (!wt.current) entries.push({ kind: "item", label: "Open in New Window", icon: menuIcons.open, run: () => openWorktree(wt) });
    if (wt.head) entries.push({ kind: "item", label: wt.branch ? `Show ${wt.branch} in History` : "Show in History", icon: menuIcons.history, run: () => showWorktree(wt) });
    entries.push(
      { kind: "item", label: "Show in Finder", icon: menuIcons.reveal, run: () => api.revealWorktree(wt.path).catch((err) => confirm.fail(err)) },
      { kind: "item", label: "Open in Terminal", icon: menuIcons.terminal, run: () => api.openInTerminal(wt.path).catch((err) => confirm.fail(err)) },
      { kind: "item", label: "Copy Path", icon: menuIcons.copy, run: () => copy(wt.path) },
    );
    const removable = !wt.main && !wt.current;
    if (lock.length || removable) entries.push({ kind: "sep" });
    entries.push(...lock);
    if (wt.locked !== null) entries.push({ kind: "note", label: `Locked${wt.locked ? `: ${wt.locked}` : ""}. Prune leaves it, and Remove unlocks it first.` });
    else if (!wt.main) entries.push({ kind: "note", label: "Lock keeps Prune from forgetting it while its folder is away, e.g. on a drive that isn’t connected." });
    if (removable) entries.push({ kind: "item", label: "Remove Worktree…", icon: menuIcons.drop, danger: true, run: () => run(removeWorktreeRequest(ctx, wt)) });
    if (wt.main) entries.push({ kind: "note", label: "The main folder: it holds the repository, so it can’t be removed." });
    else if (wt.current) entries.push({ kind: "note", label: "Open in this window. To remove it, use the main folder’s window." });
    open(event, "Worktree menu", `worktree:${wt.path}`, entries);
  }

  /** Submodules: other repositories in folders here, at commits this one records. */
  const submodules = $derived(history.submodules);

  /** The menu of a submodule, the same as on its card. */
  function submoduleMenu(event: MouseEvent, sub: Submodule) {
    open(event, "Submodule menu", `submodule:${sub.path}`, submoduleEntries(sub, submodules, repo.path, run));
  }

  /** The menu of the Submodules heading: all of them at once. */
  function submodulesMenu(event: MouseEvent) {
    const entries: MenuEntry[] = [];
    if (submodules.length) {
      entries.push(
        { kind: "item", label: "Update All", icon: actionIcons.box, run: () => run(updateRequest(submodules, null, repo.path)) },
        { kind: "item", label: "Sync All URLs", icon: menuIcons.compare, run: () => run(syncRequest(null)) },
        { kind: "sep" },
      );
    }
    entries.push({ kind: "item", label: "Add Submodule…", icon: menuIcons.folder, run: () => run(addSubmoduleRequest(submodules, repo.path)) });
    open(event, "Submodules menu", "submodules-heading", entries);
  }

  /** Select the branch's or tag's latest commit, as if it were clicked in the graph. */
  function focus(r: RefInfo) {
    clicked = r;
    onPick(r.target);
  }

  /** The first items of a list, plus the highlighted one if it would be hidden. */
  function shown<T extends { ref: RefInfo }>(key: string, items: T[]): T[] {
    if (expanded[key] || items.length <= COLLAPSED) return items;
    const head = items.slice(0, COLLAPSED);
    const current = items.slice(COLLAPSED).find((item) => item.ref.name === focused);
    return current ? [...head, current] : head;
  }
</script>

<!-- A branch's name: a start it shares with others in the list gives way first, so what tells
     it apart stays in sight; the whole name is in the row's tooltip. -->
{#snippet refName(name: string, lead: number, bold = false)}
  <span class="grow ref-name" class:bold>{#if lead}<span class="lead">{name.slice(0, lead)}</span>{/if}<span class="rest">{name.slice(lead)}</span></span>
{/snippet}

{#snippet more(key: string, count: number)}
  {#if count > COLLAPSED}
    <button class="more" onclick={() => (expanded[key] = !expanded[key])} aria-expanded={!!expanded[key]}>
      <svg class="icon small" viewBox="0 0 16 16"><path d={expanded[key] ? "M4.5 10 8 6.5l3.5 3.5" : "M4.5 6 8 9.5 11.5 6"} /></svg>
      {expanded[key] ? "Show less" : `Show all ${count}`}
    </button>
  {/if}
{/snippet}

<nav aria-label="Sidebar">
  <div class="lights" data-tauri-drag-region></div>

  <button
    class="repo"
    onclick={repoMenu}
    oncontextmenu={(event) => {
      event.preventDefault();
      repoMenu(event);
    }}
    title="Switch, clone or start a repository"
  >
    <svg class="icon" viewBox="0 0 16 16"><path d="M2.5 4.5a1 1 0 0 1 1-1h3l1.5 1.5h4.5a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1z" /></svg>
    <span class="repo-text">
      <span class="repo-name">{repo.name}</span>
      <span class="repo-path">{tilde(repo.path)}</span>
    </span>
    <svg class="icon small" viewBox="0 0 16 16"><path d="M5 6l3-3 3 3M5 10l3 3 3-3" /></svg>
  </button>

  <div class="scroll" bind:this={list}>
    <div class="heading">Workspace</div>
    <button class="item" class:current={view === "history" && !showingChanges} onclick={() => onView("history")}>
      <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" /><path d="M8 4.5V8l2.5 1.5" /></svg>
      <span class="grow">History</span>
    </button>
    <!-- Always here, as in the design: the same panel as the row of uncommitted changes above HEAD. -->
    <button
      class="item"
      class:current={showingChanges}
      disabled={!changes}
      onclick={onChanges}
      title={!changes ? "No uncommitted changes" : changes.unknown ? `Couldn’t read the uncommitted changes: ${errorLine(reads?.changes.error ?? null)}` : changes.conflicted ? `${changes.conflicted} conflicted` : `${changes.staged} staged · ${changes.unstaged} not staged`}
    >
      <svg class="icon" viewBox="0 0 16 16"><path d="M4 1.5h5.5l3 3v10h-8.5zM6.5 7h3M8 5.5v3M6.5 11h3" /></svg>
      <span class="grow">Changes</span>
      {#if changes?.unknown}<span class="meta warn">?</span>{:else if changes}<span class="meta" class:warn={changes.conflicted > 0}>{changes.files}</span>{/if}
    </button>
    <button class="item" class:current={view === "stashes"} onclick={() => onView("stashes")}>
      <svg class="icon" viewBox="0 0 16 16"><path d={menuIcons.stash} /></svg>
      <span class="grow">Stashes</span>
      {#if history.stashes.length}<span class="meta">{history.stashes.length}</span>{/if}
    </button>
    {#if github.repo && history.head.branch && history.head.branch !== history.trunk}
      {@const branch = history.head.branch}
      <button class="item" class:current={view === "pull"} onclick={() => onPullRequest(branch)} title={headPull ? `Pull request #${headPull.number} of ${branch}` : `Open a pull request for ${branch}`}>
        <svg class="icon" viewBox="0 0 16 16"><path d={PULL_ICON} /></svg>
        <span class="grow">Pull Request</span>
        {#if headPull}<span class="meta">#{headPull.number}</span>{/if}
      </button>
    {/if}
    {#if github.mrProject && history.head.branch && history.head.branch !== history.trunk}
      {@const branch = history.head.branch}
      <button class="item" class:current={view === "pull"} onclick={() => onPullRequest(branch, null, "gitlab")} title={headMr ? `Merge request !${headMr.number} of ${branch}` : `Open a merge request for ${branch}`}>
        <svg class="icon" viewBox="0 0 16 16"><path d={PULL_ICON} /></svg>
        <span class="grow">Merge Request</span>
        {#if headMr}<span class="meta">!{headMr.number}</span>{/if}
      </button>
    {/if}
    {#if github.repo || github.mrProject}
      {@const waiting = github.waiting.length}
      {@const noun = github.repo ? "pull request" : "merge request"}
      <button class="item" class:current={view === "reviews"} onclick={() => onView("reviews")} title={waiting ? `${waiting} ${noun}${waiting === 1 ? " asks" : "s ask"} for your review` : `Open ${noun}s, yours and others’`}>
        <svg class="icon" viewBox="0 0 16 16"><path d={REVIEW_ICON} /></svg>
        <span class="grow">Reviews</span>
        {#if waiting}<span class="meta count">{waiting}</span>{/if}
      </button>
    {/if}
    <button class="item" class:current={view === "oplog"} onclick={() => onView("oplog")} title="Every step Oxbow ran here, to undo or restore">
      <svg class="icon" viewBox="0 0 16 16"><path d="M5.5 4 2.5 7l3 3M3 7h6.5a3.5 3.5 0 0 1 0 7H7" /></svg>
      <span class="grow">Operation Log</span>
      {#if oplog.entries.length}<span class="meta">{oplog.entries.length}</span>{/if}
    </button>

    <div class="heading with-button">
      <span>Branches</span>
      <button class="add" onclick={() => run(newBranchRequest(ctx))} aria-label="New branch" title="New Branch…">
        <svg class="icon" viewBox="0 0 16 16"><path d="M8 3.5v9M3.5 8h9" /></svg>
      </button>
    </div>
    {#if detached}
      <!-- Gray, the color of what belongs to no branch. -->
      {@const color = NO_BRANCH_COLOR}
      <button
        class="item"
        data-ref="HEAD"
        class:menu-open={menu?.ref === "HEAD"}
        style:--ring={lane(color)}
        style:background={focused === "HEAD" ? tint(color) : undefined}
        onclick={() => onPick(detached.id)}
        oncontextmenu={(e) => detachedMenu(e, detached)}
      >
        <svg class="icon" viewBox="0 0 16 16" style:color={lane(color)}><path d="M2 8h3M11 8h3M5 8a3 3 0 1 0 6 0a3 3 0 1 0-6 0" /></svg>
        <span class="grow ellipsis bold">Detached HEAD</span>
        <span class="head">HEAD</span>
      </button>
    {/if}
    {#each shown("branches", branches) as b (b.ref.name)}
      {@const head = b.ref.name === history.head.branch || busy?.branch === b.ref.name}
      <button
        class="item"
        data-ref={b.ref.name}
        class:menu-open={menu?.ref === b.ref.name || lens.pointedRef === b.ref.name}
        style:--ring={lane(b.color)}
        style:background={focused === b.ref.name || lens.pointedRef === b.ref.name ? tint(b.color) : undefined}
        title={b.ref.name}
        onclick={() => focus(b.ref)}
        oncontextmenu={(e) => branchMenu(e, b.ref)}
      >
        <span class="dot" style:background={lane(b.color)}></span>
        {@render refName(b.ref.name, branchLeads.get(b.ref.name) ?? 0, head)}
        {#if b.ref.tracking?.gone}<span class="badge" title="Its branch on {b.ref.tracking.remote} was deleted">gone</span>{/if}
        {#if worktreeOf(history, b.ref.name)}
          {@const wt = worktreeOf(history, b.ref.name)!}
          <svg class="icon tiny" class:off={wt.missing} viewBox="0 0 16 16" role="img" aria-label="Checked out in {wt.name}"><title>Checked out in {wt.name}</title><path d={menuIcons.folder} /></svg>
        {/if}
        {#if github.openPullOf(b.ref.name) && !head}<span class="meta" title="Pull request #{github.openPullOf(b.ref.name)?.number}">#{github.openPullOf(b.ref.name)?.number}</span>{/if}
        {#if github.openMergeRequestOf(b.ref.name)}<span class="meta" title="Merge request !{github.openMergeRequestOf(b.ref.name)?.number}">!{github.openMergeRequestOf(b.ref.name)?.number}</span>{/if}
        {#if busy?.branch === b.ref.name}<span class="head busy">{busy.word}</span>{:else if head}<span class="head">HEAD</span>{/if}
      </button>
    {/each}
    {@render more("branches", branches.length)}

    {#if stashes.length}
      <div class="heading">Stashes</div>
      {#each shown("stashes", stashes) as s (s.ref.name)}
        <button
          class="item"
          data-ref={s.ref.name}
          style:background={view === "history" && focused === s.ref.name ? "var(--side-sel)" : undefined}
          onclick={() => (view === "stashes" ? onPickStash(s.ref.target) : focus(s.ref))}
          oncontextmenu={(e) => open(e, "Stash menu", s.ref.name, stashMenu(ctx, s.info, run, copy))}
          title="{s.ref.name}{s.branch ? ` on ${s.branch}` : ''}: {s.title}"
        >
          <svg class="icon" viewBox="0 0 16 16"><rect x="3" y="6.5" width="10" height="7" rx="1.5" /><path d="M4.5 4.5h7M6 2.5h4" /></svg>
          <span class="grow ellipsis">{s.title}</span>
          <span class="meta">{s.ref.name}</span>
        </button>
      {/each}
      {@render more("stashes", stashes.length)}
    {/if}

    {#if lfs.patterns.length}
      <div class="heading with-button" oncontextmenu={lfsMenu} role="group" aria-label="Git LFS">
        <span>Git LFS{#if !lfs.ready}<span class="missing">{lfs.status?.version ? "turned off" : "not installed"}</span>{/if}</span>
        <button class="add" onclick={lfsMenu} aria-label="Git LFS actions">
          <svg class="icon" viewBox="0 0 16 16"><path d="M3.2 8a.8.8 0 1 0 1.6 0a.8.8 0 1 0-1.6 0M7.2 8a.8.8 0 1 0 1.6 0a.8.8 0 1 0-1.6 0M11.2 8a.8.8 0 1 0 1.6 0a.8.8 0 1 0-1.6 0" /></svg>
        </button>
        {#if lfs.ready}
          <button class="add" onclick={() => run(lfsTrackRequest(null, null))} aria-label="Track files with Git LFS" title="Track Files…">
            <svg class="icon" viewBox="0 0 16 16"><path d="M8 3.5v9M3.5 8h9" /></svg>
          </button>
        {/if}
      </div>
      {#if lfs.ready && lfsMissing}
        <button class="item lfs" style:--ring="var(--sep)" onclick={() => run(downloadRequest(null, undefined, true))} title="List the files that are only pointers here, and download them">
          <svg class="icon" viewBox="0 0 16 16"><path d={menuIcons.fetch} /></svg>
          <span class="grow lfs-text">
            <span class="ellipsis pattern">Not downloaded</span>
            <span class="ellipsis sub">{lfsMissing} {lfsMissing === 1 ? "file" : "files"} · {bytes(lfsMissingSize)}</span>
          </span>
          <span class="meta warn">Download…</span>
        </button>
      {/if}
      {#each lfs.patterns as p (p.pattern)}
        <button
          class="item lfs"
          class:menu-open={menu?.ref === `lfs:${p.pattern}`}
          style:--ring="var(--sep)"
          onclick={(e) => patternMenu(e, p)}
          oncontextmenu={(e) => patternMenu(e, p)}
        >
          <svg class="icon" class:off={!lfs.ready} viewBox="0 0 16 16"><path d={menuIcons.lfs} /></svg>
          <span class="grow lfs-text">
            <span class="ellipsis mono pattern">{p.pattern}</span>
            <span class="ellipsis sub">{patternNote(p)}</span>
          </span>
          {#if lfs.ready && p.missing}<span class="meta warn">{p.missing} to download</span>{:else if p.new}<span class="meta">new</span>{/if}
        </button>
      {/each}
    {/if}

    {#snippet readNote(part: ReadPart)}
      {@const state = reads?.[part]}
      {#if state && (state.phase === "failed" || state.phase === "stale" || (state.phase === "loading" && state.error))}
        <div class="read-note" title={state.at === null ? errorLine(state.error) : `Couldn’t read the ${PART_NAMES[part]} again, so this is the list from ${age(state, Date.now())}: ${errorLine(state.error)}`}>
          <svg class="icon" viewBox="0 0 16 16"><path d="M8 2.5 14 13H2zM8 6.5v3M8 11.2v.1" /></svg>
          <span class="grow ellipsis">{state.at === null ? "Couldn’t read the list" : `Stale · from ${age(state, Date.now())}`}</span>
          <button class="retry" disabled={state.phase === "loading"} onclick={() => onRetry(part)}>{state.phase === "loading" ? "Reading…" : "Retry"}</button>
        </div>
      {/if}
    {/snippet}

    {#if history.head.commit && (worktrees.length || reads?.worktrees.error)}
      <div class="heading with-button">
        <span>Worktrees</span>
        <button class="add" onclick={() => run(newWorktreeRequest(ctx, repo.path))} aria-label="New worktree" title="New Worktree…">
          <svg class="icon" viewBox="0 0 16 16"><path d="M8 3.5v9M3.5 8h9" /></svg>
        </button>
      </div>
      {@render readNote("worktrees")}
      {#each worktrees as wt (wt.path)}
        {@const color = wt.branch ? ctx.colorOf(wt.branch) : NO_BRANCH_COLOR}
        {@const note = worktreeNote(wt)}
        {@const tracking = worktreeTracking(wt)}
        {@const pull = wt.branch && github.repo ? github.pullOf(wt.branch) : null}
        <button
          class="item two"
          class:menu-open={menu?.ref === `worktree:${wt.path}`}
          style:--ring={lane(color)}
          onclick={() => (wt.missing ? undefined : showWorktree(wt))}
          ondblclick={() => !wt.current && !wt.missing && openWorktree(wt)}
          oncontextmenu={(e) => worktreeMenu(e, wt)}
          title={worktreeTitle(wt)}
        >
          {#if wt.missing}
            <svg class="icon off" viewBox="0 0 16 16"><path d="M8 2 1.5 13.5h13zM8 6.5v3.5M8 12v.2" /></svg>
          {:else}
            <svg class="icon" class:here={wt.current} viewBox="0 0 16 16"><path d={menuIcons.folder} /></svg>
          {/if}
          <span class="grow two-text">
            <span class="wt-line">
              <span class="ellipsis" class:bold={wt.current}>{wt.name}</span>
              {#if wt.locked !== null}<svg class="icon tiny" viewBox="0 0 16 16" role="img" aria-label="Locked"><path d={LOCK} /></svg>{/if}
            </span>
            <span class="wt-line sub">
              <span class="ellipsis"><span class="mini-dot" style:background={lane(color)}></span>{wt.branch ?? (wt.head ? `detached at ${wt.head.slice(0, 7)}` : "no commits")}</span>
              {#if tracking?.ahead}<span class="mono nowrap" aria-label="{tracking.ahead} to push">↑{tracking.ahead}</span>{/if}
              {#if tracking?.behind}<span class="mono nowrap" aria-label="{tracking.behind} to pull">↓{tracking.behind}</span>{/if}
              {#if pull}<span class="nowrap pr" class:closed={pull.state !== "open"}>#{pull.number}</span>{/if}
            </span>
          </span>
          {#if note}<span class="meta nowrap" class:accent={note.tone === "accent"} class:warn={note.tone === "warn"}>{note.text}</span>{/if}
        </button>
      {/each}
    {/if}

    {#if history.head.commit && (submodules.length || reads?.submodules.error)}
      <div class="heading with-button" oncontextmenu={submodulesMenu} role="group" aria-label="Submodules">
        <span>Submodules</span>
        <button class="add" onclick={submodulesMenu} aria-label="Submodule actions">
          <svg class="icon" viewBox="0 0 16 16"><path d="M3.2 8a.8.8 0 1 0 1.6 0a.8.8 0 1 0-1.6 0M7.2 8a.8.8 0 1 0 1.6 0a.8.8 0 1 0-1.6 0M11.2 8a.8.8 0 1 0 1.6 0a.8.8 0 1 0-1.6 0" /></svg>
        </button>
        <button class="add" onclick={() => run(addSubmoduleRequest(submodules, repo.path))} aria-label="Add submodule" title="Add Submodule…">
          <svg class="icon" viewBox="0 0 16 16"><path d="M8 3.5v9M3.5 8h9" /></svg>
        </button>
      </div>
      {@render readNote("submodules")}
      {#each submodules as sub (sub.path)}
        {@const note = submoduleNote(sub)}
        <button
          class="item two"
          class:menu-open={menu?.ref === `submodule:${sub.path}`}
          style:--ring="var(--accent)"
          style:background={submodule === sub.path ? "var(--side-sel)" : undefined}
          onclick={() => onSubmodule(sub.path)}
          ondblclick={() => sub.cloned && openSubmoduleWindow(sub)}
          oncontextmenu={(e) => submoduleMenu(e, sub)}
          title={sub.cloned ? `${sub.path} · double-click to open its window` : `${sub.path} · not cloned yet`}
        >
          <svg class="icon" class:dim={!sub.cloned} viewBox="0 0 16 16"><path d={actionIcons.box} /></svg>
          <span class="grow two-text">
            <span class="ellipsis">{sub.title}</span>
            <span class="ellipsis sub">{submoduleWhere(sub)}</span>
          </span>
          {#if note}<span class="meta nowrap" class:warn={note.warn}>{note.text}</span>{/if}
        </button>
      {/each}
    {/if}

    {#if history.remotes.length}
      <div class="heading">Remotes</div>
      {#each history.remotes as remote (remote)}
        {@const lists = remoteBranches.get(remote)!}
        {@const leads = sharedLeads(lists.own.map((rb) => rb.ref.name.slice(remote.length + 1)))}
        {@const count = lists.own.length + lists.tracked.length}
        {@const expandedRemote = !collapsedRemotes[remote]}
        <button class="item" onclick={() => (collapsedRemotes[remote] = expandedRemote)} aria-expanded={expandedRemote} title="{expandedRemote ? 'Hide' : 'Show'} the branches on {remote}">
          <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" /><path d="M2 8h12M8 2c2 2 2 10 0 12M8 2c-2 2-2 10 0 12" /></svg>
          <span class="grow">{remote}</span>
          <span class="meta">{count} {count === 1 ? "branch" : "branches"}</span>
          <svg class="icon tiny" viewBox="0 0 16 16"><path d={expandedRemote ? "M4 6l4 4 4-4" : "M6 4l4 4-4 4"} /></svg>
        </button>
        {#if expandedRemote}
          {#each shown(`remote:${remote}`, lists.own) as rb (rb.ref.name)}
            {@const short = rb.ref.name.slice(remote.length + 1)}
            <button
              class="item nested"
              data-ref={rb.ref.name}
              class:menu-open={menu?.ref === rb.ref.name || lens.pointedRef === rb.ref.name}
              style:--ring={lane(rb.color)}
              style:background={focused === rb.ref.name || lens.pointedRef === rb.ref.name ? tint(rb.color) : undefined}
              title="{rb.ref.name}{rb.author ? ` · ${rb.author}` : ''}{rb.time ? ` · ${shortTime(rb.time)}` : ''}"
              onclick={() => focus(rb.ref)}
              oncontextmenu={(e) => remoteMenu(e, rb.ref, rb.author)}
            >
              <svg class="icon small" viewBox="0 0 16 16"
                ><circle cx="4.5" cy="3.5" r="1.5" /><circle cx="4.5" cy="12.5" r="1.5" /><circle cx="11.5" cy="5.5" r="1.5" /><path d="M4.5 5v6M11.5 7c0 3-7 2-7 4" /></svg
              >
              {@render refName(short, leads.get(short) ?? 0)}
              <!-- The name matters more than who moved it: only the time stays, and the row's
                   tooltip has the author. -->
              <span class="meta when">{rb.time ? shortTime(rb.time) : firstName(rb.author)}</span>
            </button>
          {/each}
          {@render more(`remote:${remote}`, lists.own.length)}
          {#if lists.tracked.length}
            <div class="tracked" title={lists.tracked.join(", ")}>{lists.tracked.length} more tracked locally</div>
          {/if}
        {/if}
      {/each}
    {/if}

    {#if history.head.commit}
      <div class="heading with-button" oncontextmenu={tagsMenu} role="group" aria-label="Tags">
        <span>Tags</span>
        <button class="add" onclick={tagsMenu} aria-label="Tag actions">
          <svg class="icon" viewBox="0 0 16 16"><path d="M3.2 8a.8.8 0 1 0 1.6 0a.8.8 0 1 0-1.6 0M7.2 8a.8.8 0 1 0 1.6 0a.8.8 0 1 0-1.6 0M11.2 8a.8.8 0 1 0 1.6 0a.8.8 0 1 0-1.6 0" /></svg>
        </button>
        <button class="add" onclick={() => run(newTagRequest(ctx))} aria-label="New tag" title="New Tag…">
          <svg class="icon" viewBox="0 0 16 16"><path d="M8 3.5v9M3.5 8h9" /></svg>
        </button>
      </div>
      {#each shown("tags", tags) as t (t.ref.name)}
        <button
          class="item"
          data-ref={t.ref.name}
          class:menu-open={menu?.ref === t.ref.name}
          style:--ring="var(--tag-border)"
          style:background={focused === t.ref.name ? "var(--side-sel)" : undefined}
          onclick={() => focus(t.ref)}
          oncontextmenu={(e) => tagMenu(e, t.ref)}
        >
          <svg class="icon" viewBox="0 0 16 16"><path d="M2.5 2.5h5l6 6-5 5-6-6z" /><circle cx="5.5" cy="5.5" r="0.8" /></svg>
          <span class="grow ellipsis">{t.ref.name}</span>
          {#if history.localTags.includes(t.ref.name)}<span class="meta" title="Not on {history.defaultRemote} yet">local</span>{/if}
        </button>
      {/each}
      {@render more("tags", tags.length)}
    {/if}
  </div>
</nav>

{#if menu}
  <Menu x={menu.x} y={menu.y} label={menu.label} entries={menu.entries} onClose={() => (menu = null)} />
{/if}

<style>
  nav {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 248px;
    background: var(--side);
    border-right: 1px solid var(--sep);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .lights {
    height: 56px;
    flex-shrink: 0;
  }
  .repo {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    margin: 0 8px 6px;
    padding: 0 10px;
    border-radius: var(--radius-m);
    background: var(--field);
    color: var(--icon);
    flex-shrink: 0;
  }
  .repo-text {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.2;
  }
  .repo-name {
    font-weight: 600;
    color: var(--text);
  }
  .repo-path {
    font-size: var(--text-caption);
    color: var(--text2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .small {
    width: 14px;
    height: 14px;
  }
  .scroll {
    flex-grow: 1;
    overflow-y: auto;
    padding-bottom: 10px;
  }
  .heading {
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
    padding: 14px 18px 4px;
  }
  .heading:first-child {
    padding-top: 8px;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: calc(100% - 16px);
    height: 28px;
    margin: 0 8px;
    padding: 0 10px;
    border-radius: var(--radius-m);
    color: var(--icon);
  }
  .item .grow {
    color: var(--text);
  }
  .item.current {
    background: var(--side-sel);
    font-weight: 600;
  }
  /* A clean working tree: Changes stays in its place, greyed. */
  .item:disabled {
    opacity: 0.5;
  }
  .grow {
    flex-grow: 1;
    min-width: 0;
  }
  /* Small enough to keep whole: the name takes the room that is left. */
  .when {
    flex-shrink: 0;
    white-space: nowrap;
  }
  .ref-name {
    display: flex;
    overflow: hidden;
    white-space: nowrap;
  }
  /* The shared start gives way first, down to "…", so the part that tells the branch apart is
     the last thing to go. */
  .lead {
    flex: 0 1000 auto;
    min-width: 2ch;
    /* A hair of air, so the cut start doesn't run into the part that was kept. */
    margin-right: 1px;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text2);
  }
  .rest {
    flex: 0 1 auto;
    min-width: 8ch;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bold {
    font-weight: 600;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: var(--radius-pill);
    margin: 0 4px;
    flex-shrink: 0;
  }
  .head {
    font-size: var(--text-caption);
    font-weight: 700;
    color: var(--text2);
  }
  .head.busy {
    color: var(--orange);
  }
  .meta {
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .with-button {
    display: flex;
    align-items: center;
    padding-right: 12px;
  }
  .with-button span {
    flex-grow: 1;
  }
  .add {
    width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-s);
    color: var(--text2);
  }
  .add:hover {
    background: var(--side-sel);
    color: var(--text);
  }
  .add .icon {
    width: 14px;
    height: 14px;
  }
  .item.menu-open {
    box-shadow: inset 0 0 0 1.5px var(--ring);
  }
  .item.lfs,
  .item.two {
    height: 38px;
  }
  .two-text {
    display: flex;
    flex-direction: column;
    line-height: 1.25;
    text-align: left;
  }
  .two-text .sub {
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .wt-line {
    display: flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
  }
  .wt-line .ellipsis {
    min-width: 0;
  }
  .wt-line .tiny {
    flex-shrink: 0;
  }
  .wt-line .mono {
    font-size: var(--text-caption);
  }
  .pr.closed {
    opacity: 0.6;
  }
  .mini-dot {
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: var(--radius-pill);
    margin-right: 5px;
    vertical-align: 1px;
  }
  .icon.dim {
    color: var(--text2);
  }
  .icon.here {
    color: var(--icon);
  }
  .meta.accent {
    color: var(--text2);
    font-weight: 600;
  }
  .lfs-text {
    display: flex;
    flex-direction: column;
    line-height: 1.25;
    text-align: left;
  }
  .lfs-text .pattern {
    font-size: var(--text-small);
  }
  .lfs-text .sub {
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .icon.off,
  .missing {
    color: var(--orange);
  }
  .missing {
    font-weight: 500;
    margin-left: 6px;
  }
  /* Pull requests waiting for your review: a count to act on. */
  .meta.count {
    min-width: 18px;
    padding: 0 6px;
    border-radius: var(--radius-pill);
    background: var(--accent);
    color: #fff;
    font-weight: 600;
    line-height: 16px;
    text-align: center;
  }
  .meta.warn {
    color: var(--orange);
    white-space: nowrap;
  }
  /* A list git couldn't read this time: shown from an earlier read, or not at all. */
  .read-note {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 8px 2px;
    padding: 4px 6px 4px 10px;
    border-radius: var(--radius-m);
    background: color-mix(in srgb, var(--orange) 9%, transparent);
    color: var(--orange);
    font-size: var(--text-caption);
  }
  .read-note .icon {
    width: 13px;
    height: 13px;
    flex-shrink: 0;
  }
  .retry {
    flex-shrink: 0;
    padding: 1px 6px;
    border-radius: var(--radius-s);
    color: var(--accent-text);
    font-size: var(--text-caption);
    font-weight: 600;
  }
  .retry:hover:not(:disabled) {
    background: var(--field);
  }
  .retry:disabled {
    color: var(--text2);
  }
  .item.nested {
    padding-left: 30px;
  }
  .tiny {
    width: 12px;
    height: 12px;
    color: var(--text2);
  }
  .nowrap {
    white-space: nowrap;
  }
  .badge {
    font-size: var(--text-caption);
    font-weight: 600;
    padding: 1px 6px;
    border-radius: var(--radius-s);
    background: var(--field);
    color: var(--text2);
  }
  .tracked {
    padding: 4px 18px 0 52px;
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .more {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    margin: 2px 8px 0;
    padding: 0 10px;
    border-radius: var(--radius-m);
    font-size: var(--text-small);
    color: var(--text2);
  }
  .more:hover {
    color: var(--text);
  }
</style>
