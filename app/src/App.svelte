<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
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

  let repo = $state<RepoSummary | null>(null);
  let history = $state<History | null>(null);
  let selected = $state<string | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let list = $state<HistoryList>();
  let panelWidth = $state(640);
  /** Goes up on every reload of the history, so the changes panel reloads too. */
  let version = $state(0);
  /** The Conflicts screen is open in place of History. */
  let resolving = $state(false);
  const operation = $derived(history?.operation ?? null);
  const showConflicts = $derived(resolving && !!operation);
  /** The screen next to the sidebar. */
  let view = $state<"history" | "stashes">("history");
  /** The stash picked on the Stashes screen. */
  let stashSel = $state<string | null>(null);

  const branchCount = $derived(history ? history.refs.filter((r) => r.kind === "local").length : 0);
  const rowsById = $derived(new Map(history?.rows.map((r) => [r.id, r]) ?? []));
  const selectedRow = $derived(selected ? (rowsById.get(selected) ?? null) : null);
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
    try {
      const summary = await api.openRepo(path);
      const next = await api.history();
      if (mine !== generation) return;
      repo = summary;
      history = next;
      version++;
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
    // A stash made or put back is the newest, and the Stashes screen shows it.
    if (ok && (request.action.kind === "stashPush" || request.action.kind === "stashStore")) stashSel = history?.stashes[0]?.id ?? null;
    const after = history?.head;
    if (after?.commit && (after.branch !== before?.branch || after.commit !== before?.commit)) select(after.commit);
    openConflicts(opBefore);
    return ok;
  }
  confirm.runner = (request) => void run(request);

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

  async function sync(kind: "fetch" | "pull" | "push") {
    // Commits made outside the app since the last reload belong in the sheet.
    await refresh();
    if (!remoteCtx) return;
    const request = kind === "fetch" ? fetchRequest(remoteCtx) : kind === "pull" ? pullRequest(remoteCtx) : pushRequest(remoteCtx);
    const opBefore = history?.operation ?? null;
    await confirm.run(request);
    // Even a failed pull or push may have fetched, so the counts are worth reloading either way.
    await refresh();
    openConflicts(opBefore);
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
      if (!selected || !next.rows.some((r) => r.id === selected)) selected = next.head.commit ?? next.rows[0]?.id ?? null;
    } catch (err) {
      if (mine === generation) error = String(err);
    }
  }

  async function chooseRepo() {
    const path = await open({ directory: true, multiple: false, title: "Open Repository" });
    if (typeof path === "string") await load(path);
  }

  function select(id: string) {
    selected = id;
    list?.reveal(id);
  }

  /** Picking a commit from the sidebar goes back to History, which mounts the graph again. */
  function pick(id: string) {
    selected = id;
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

  $effect(() => {
    api.getSetting<number>(settings.detailsWidth).then((width) => {
      if (typeof width === "number") panelWidth = Math.min(Math.max(width, 360), window.innerWidth - 560);
    });
    api.initialRepo().then((path) => {
      if (path) load(path);
    });
  });
</script>

<svelte:window
  onfocus={refresh}
  onkeydown={(event) => {
    if ((event.metaKey || event.ctrlKey) && event.key === ",") {
      event.preventDefault();
      api.openSettings();
    }
  }}
/>

{#if !repo || !history}
  <Welcome {loading} {error} onOpen={chooseRepo} />
{:else}
  <div class="window">
    <Sidebar {repo} {history} {selectedRow} ctx={branchCtx!} onOpen={chooseRepo}
      onPick={pick}
      {run}
      {view}
      onView={(v) => {
        view = v;
        resolving = false;
        if (v === "history" && selected) requestAnimationFrame(() => list?.reveal(selected!));
      }}
      onPickStash={(id) => (stashSel = id)}
    />
    <div class="main">
      <header data-tauri-drag-region>
        <div class="title" data-tauri-drag-region>
          {#if showConflicts && operation}
            <span class="name">Resolve Conflicts</span>
            <span class="sub">{describe(operation).noun} in progress · {operation.conflicted} {operation.conflicted === 1 ? "file" : "files"} left</span>
          {:else if view === "stashes"}
            <span class="name">Stashes</span>
            <span class="sub">{repo.name} · on {history.head.branch ?? "detached HEAD"}</span>
          {:else}
            <span class="name">History</span>
            <span class="sub">{branchCount} {branchCount === 1 ? "branch" : "branches"} · {history.head.branch ?? operation?.branch ?? "detached HEAD"}</span>
          {/if}
        </div>
        <BranchPicker {history} {colorOf} onPick={(name) => branchCtx && run(switchRequest(branchCtx, name))} />
        {#if showConflicts}
          <button class="capsule" onclick={() => (resolving = false)} title={view === "stashes" ? "Back to Stashes" : "Back to the commit graph"}>
            <svg class="icon" viewBox="0 0 16 16"><path d="M10 3.5 5.5 8l4.5 4.5" /></svg>{view === "stashes" ? "Stashes" : "History"}
          </button>
        {/if}
        <span class="spacer" data-tauri-drag-region></span>
        {#if error}<span class="error" role="alert">{error}</span>{/if}
        {#if history.remotes.length && remoteCtx}
          {@const t = history.tracking}
          <div class="group" role="group" aria-label="Sync with remote">
            <button onclick={() => sync("fetch")} aria-label="Fetch" title="Fetch from {history.remotes.length > 1 ? 'all remotes' : remoteCtx.remote}">
              <svg class="icon" viewBox="0 0 16 16"><path d="M13 8a5 5 0 0 1-8.6 3.5M3 8a5 5 0 0 1 8.6-3.5" /><path d="M11.8 1.8v2.9H8.9M4.2 14.2v-2.9h2.9" /></svg>
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
        {/if}
        <button class="capsule" onclick={newBranch} aria-label="New branch" title={history.head.branch ? "New Branch…" : "Create a branch at HEAD…"}>
          <svg class="icon" viewBox="0 0 16 16"><circle cx="4.5" cy="3.5" r="1.5" /><circle cx="4.5" cy="12.5" r="1.5" /><circle cx="11.5" cy="5.5" r="1.5" /><path d="M4.5 5v6M11.5 7c0 3-7 2-7 4" /></svg>
        </button>
        <button class="capsule" onclick={refresh} aria-label="Reload history" title="Reload">
          <svg class="icon" viewBox="0 0 16 16"><path d="M13 8a5 5 0 1 1-1.5-3.5M13 2.5V5h-2.5" /></svg>
        </button>
        <button class="capsule" onclick={() => api.openSettings()} aria-label="Settings" title={navigator.platform.startsWith("Mac") ? "Settings (⌘,)" : "Settings (Ctrl+,)"}>
          <svg class="icon" viewBox="0 0 16 16"><path d="M8 5.6a2.4 2.4 0 1 0 0 4.8a2.4 2.4 0 1 0 0-4.8M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4" /></svg>
        </button>
      </header>
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
      {#if showConflicts && operation}
        {#key repo.path}
          <ConflictsView {history} op={operation} {colorOf} {version} {run} />
        {/key}
      {:else if view === "stashes" && branchCtx}
        {#key repo.path}
          <StashesView {history} ctx={branchCtx} {colorOf} bind:selected={stashSel} {version} {run} copy={(text) => copyText(text, `Copied ${text.length > 40 ? "it" : text}.`)} />
        {/key}
      {:else}
      <div class="content">
        <section class="history" aria-label="Commit history">
          <div class="columns"><span>Description</span>{#if history.truncated}<span class="note">Showing the latest {history.rows.length.toLocaleString()} commits</span>{/if}</div>
          {#key repo.path}
            <HistoryList bind:this={list} {history} {selected} onSelect={select} menuFor={commitMenu} />
          {/key}
        </section>
        <aside class="panel" style:width="{panelWidth}px" aria-label="Commit details">
          <button class="grip" onpointerdown={startResize} aria-label="Resize commit details" title="Drag to resize"></button>
          <!-- A new repository starts the panels from scratch. -->
          {#key repo.path}
            {#if selectedRow?.worktree}
              <ChangesPanel
                branch={history.head.branch}
                color={selectedRow.graph.color}
                head={headRow}
                {version}
                onChanged={refresh}
                onResolve={operation ? () => (resolving = true) : undefined}
              />
            {:else if selectedRow}
              <CommitPanel row={selectedRow} childIds={childrenOf.get(selectedRow.id) ?? []} lookup={(id) => rowsById.get(id)} onSelect={select} />
            {/if}
          {/key}
        </aside>
      </div>
      {/if}
    </div>
  </div>
  <ConfirmSheet repo={repo.name} branch={history.head.branch ?? operation?.branch ?? null} color={headColor} />
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
  header {
    height: 56px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 12px 0 14px;
    flex-shrink: 0;
  }
  .title {
    display: flex;
    flex-direction: column;
    line-height: 1.2;
    margin-right: 4px;
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
  .count {
    font-size: 11px;
    font-weight: 600;
    color: var(--accent-text);
  }
  .spacer {
    flex-grow: 1;
    align-self: stretch;
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
