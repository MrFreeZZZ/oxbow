<script lang="ts">
  import type { History, HistoryRow, RefInfo, RepoSummary } from "./types";
  import { lane, NO_BRANCH_COLOR, shortTime, tint } from "./format";
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

  let {
    repo,
    history,
    selectedRow,
    ctx,
    onOpen,
    onPick,
    run,
  }: {
    repo: RepoSummary;
    history: History;
    selectedRow: HistoryRow | null;
    ctx: BranchContext;
    onOpen: () => void;
    onPick: (commit: string) => void;
    /** Confirm and run a change to the repository. */
    run: (request: Request | Promise<Request>) => void;
  } = $props();

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
  const detached = $derived(!history.head.branch && history.head.commit ? (rowOf.get(history.head.commit) ?? null) : null);

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
  const tags = $derived(
    history.refs
      .filter((r) => r.kind === "tag")
      .map((r) => ({ ref: r, time: rowOf.get(r.target)?.time ?? 0 }))
      .sort((a, b) => b.time - a.time || b.ref.name.localeCompare(a.ref.name)),
  );

  /** Stashes, newest first, with the "On main: " prefix git adds moved out of the title. */
  const stashes = $derived(
    history.stashes.map((s) => {
      const name = `stash@{${s.index}}`;
      const match = /^(?:WIP on|On) ([^:]+): (.*)$/.exec(s.message);
      return {
        ref: { name, kind: "stash" as const, target: s.id, remote: null, tracking: null },
        title: match ? match[2] : s.message,
        branch: match ? match[1] : null,
      };
    }),
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
      () => confirm.say(`Copied ${text}.`),
      () => confirm.say("Couldn’t copy to the clipboard."),
    );
  }

  function open(event: MouseEvent, label: string, ref: string, entries: MenuEntry[]) {
    event.preventDefault();
    menu = { x: event.clientX, y: event.clientY, label, ref, entries };
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
    const entries: MenuEntry[] = [{ kind: "header", label: `${ref.name} · ${status}` }];
    const remove = () => run(api.deletionCheck(ref.name).then((check) => deleteRequest(ctx, ref, check)));
    if (t?.gone && !isHead) {
      entries.push(item(`Delete ${ref.name}…`, menuIcons.drop, remove, true));
      entries.push({ kind: "note", label: "Its branch on the remote is gone, usually after its pull request was merged." });
      entries.push({ kind: "sep" });
    }
    if (!isHead) entries.push(item("Check Out", menuIcons.checkout, () => run(switchRequest(ctx, ref.name))));
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
    entries.push({ kind: "sep" });
    entries.push(item(`New Branch from ${ref.name}…`, menuIcons.branch, () => run(newBranchRequest(ctx, ref.name))));
    if (!isTrunk) entries.push(item("Rename…", menuIcons.edit, () => run(renameRequest(ctx, ref))));
    entries.push(item("Copy Name", menuIcons.copy, () => copy(ref.name)));
    if (!isHead && !isTrunk && !t?.gone) {
      entries.push({ kind: "sep" });
      entries.push(item("Delete…", menuIcons.drop, remove, true));
    }
    if (isTrunk) entries.push({ kind: "note", label: "Default branch: rename and delete are turned off." });
    else if (isHead) entries.push({ kind: "note", label: "Checked out. Switch to another branch to delete it." });
    open(event, "Branch menu", ref.name, entries);
  }

  /** The right-click menu of a remote branch nobody tracks locally. */
  function remoteMenu(event: MouseEvent, ref: RefInfo, author: string) {
    const short = ref.name.slice((ref.remote ?? "").length + 1);
    const taken = history.refs.some((r) => r.kind === "local" && r.name === short);
    const entries: MenuEntry[] = [{ kind: "header", label: `${ref.name}${author ? ` · ${author}` : ""}` }];
    if (!taken) entries.push({ kind: "item", label: "Check Out as Local Branch", icon: menuIcons.checkout, run: () => run(trackRequest(ctx, ref, author)) });
    else entries.push({ kind: "note", label: `A local branch ${short} already exists, so it can’t be checked out under that name.` });
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
    entries.push({ kind: "item", label: "Copy Name", icon: menuIcons.copy, run: () => copy(ref.name) });
    open(event, "Tag menu", ref.name, entries);
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

  <button class="repo" onclick={onOpen} title="Open another repository">
    <svg class="icon" viewBox="0 0 16 16"><path d="M2.5 4.5a1 1 0 0 1 1-1h3l1.5 1.5h4.5a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1z" /></svg>
    <span class="repo-text">
      <span class="repo-name">{repo.name}</span>
      <span class="repo-path">{repo.path}</span>
    </span>
    <svg class="icon small" viewBox="0 0 16 16"><path d="M5 6l3-3 3 3M5 10l3 3 3-3" /></svg>
  </button>

  <div class="scroll" bind:this={list}>
    <div class="heading">Workspace</div>
    <div class="item current">
      <svg class="icon" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" /><path d="M8 4.5V8l2.5 1.5" /></svg>
      <span class="grow">History</span>
    </div>

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
      {@const head = b.ref.name === history.head.branch}
      <button
        class="item"
        data-ref={b.ref.name}
        class:menu-open={menu?.ref === b.ref.name}
        style:--ring={lane(b.color)}
        style:background={focused === b.ref.name ? tint(b.color) : undefined}
        onclick={() => focus(b.ref)}
        oncontextmenu={(e) => branchMenu(e, b.ref)}
      >
        <span class="dot" style:background={lane(b.color)}></span>
        <span class="grow ellipsis" class:bold={head}>{b.ref.name}</span>
        {#if b.ref.tracking?.gone}<span class="badge" title="Its branch on {b.ref.tracking.remote} was deleted">gone</span>{/if}
        {#if head}<span class="head">HEAD</span>{/if}
      </button>
    {/each}
    {@render more("branches", branches.length)}

    {#if stashes.length}
      <div class="heading">Stashes</div>
      {#each shown("stashes", stashes) as s (s.ref.name)}
        <button
          class="item"
          data-ref={s.ref.name}
          style:background={focused === s.ref.name ? "var(--side-sel)" : undefined}
          onclick={() => focus(s.ref)}
          title="{s.ref.name}{s.branch ? ` on ${s.branch}` : ''}: {s.title}"
        >
          <svg class="icon" viewBox="0 0 16 16"><rect x="3" y="6.5" width="10" height="7" rx="1.5" /><path d="M4.5 4.5h7M6 2.5h4" /></svg>
          <span class="grow ellipsis">{s.title}</span>
          <span class="meta">{s.ref.name}</span>
        </button>
      {/each}
      {@render more("stashes", stashes.length)}
    {/if}

    {#if history.remotes.length}
      <div class="heading">Remotes</div>
      {#each history.remotes as remote (remote)}
        {@const lists = remoteBranches.get(remote)!}
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
              class:menu-open={menu?.ref === rb.ref.name}
              style:--ring={lane(rb.color)}
              style:background={focused === rb.ref.name ? tint(rb.color) : undefined}
              onclick={() => focus(rb.ref)}
              oncontextmenu={(e) => remoteMenu(e, rb.ref, rb.author)}
            >
              <svg class="icon small" viewBox="0 0 16 16"
                ><circle cx="4.5" cy="3.5" r="1.5" /><circle cx="4.5" cy="12.5" r="1.5" /><circle cx="11.5" cy="5.5" r="1.5" /><path d="M4.5 5v6M11.5 7c0 3-7 2-7 4" /></svg
              >
              <span class="grow ellipsis">{short}</span>
              <span class="meta nowrap">{[firstName(rb.author), rb.time ? shortTime(rb.time) : ""].filter(Boolean).join(" · ")}</span>
            </button>
          {/each}
          {@render more(`remote:${remote}`, lists.own.length)}
          {#if lists.tracked.length}
            <div class="tracked" title={lists.tracked.join(", ")}>{lists.tracked.length} more tracked locally</div>
          {/if}
        {/if}
      {/each}
    {/if}

    {#if tags.length}
      <div class="heading">Tags</div>
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
    left: 8px;
    top: 8px;
    bottom: 8px;
    width: 248px;
    border-radius: 18px;
    background: var(--side);
    border: 0.5px solid var(--side-border);
    box-shadow: var(--panel-shadow);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .lights {
    height: 44px;
    flex-shrink: 0;
  }
  .repo {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    margin: 0 8px 6px;
    padding: 0 10px;
    border-radius: 10px;
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
    font-size: 11px;
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
    font-size: 11px;
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
    border-radius: 8px;
    color: var(--icon);
  }
  .item .grow {
    color: var(--text);
  }
  .item.current {
    background: var(--side-sel);
    font-weight: 600;
  }
  .grow {
    flex-grow: 1;
    min-width: 0;
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
    border-radius: 4px;
    margin: 0 4px;
    flex-shrink: 0;
  }
  .head {
    font-size: 10px;
    font-weight: 700;
    color: var(--accent-text);
  }
  .meta {
    font-size: 11px;
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
    border-radius: 6px;
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
    font-size: 10px;
    font-weight: 600;
    padding: 1px 6px;
    border-radius: 7px;
    background: var(--field);
    color: var(--text2);
  }
  .tracked {
    padding: 4px 18px 0 52px;
    font-size: 11px;
    color: var(--text2);
  }
  .more {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    margin: 2px 8px 0;
    padding: 0 10px;
    border-radius: 8px;
    font-size: 12px;
    color: var(--text2);
  }
  .more:hover {
    color: var(--text);
  }
</style>
