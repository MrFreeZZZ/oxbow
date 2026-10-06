<script lang="ts">
  // The Stashes screen: every stash on the left, the picked one on the right with whether it
  // applies to the checked-out branch, its actions, its files and their diffs.
  import { api } from "./api";
  import type { CommitDetail, FileChange, FileDiff, History, StashCheck, StashInfo } from "./types";
  import type { Request } from "./confirm.svelte";
  import type { BranchContext } from "./branches";
  import { applyRequest, dropRequest, stashBranchRequest, stashMenu, stashName, stashRequest } from "./stash";
  import { plate, relativeTime, shortId, splitPath, tint } from "./format";
  import DiffView from "./DiffView.svelte";
  import Menu, { type MenuEntry } from "./Menu.svelte";

  let {
    history,
    ctx,
    colorOf,
    selected = $bindable(),
    version,
    run,
    copy,
  }: {
    history: History;
    ctx: BranchContext;
    colorOf: (name: string) => number;
    /** The id of the picked stash. */
    selected: string | null;
    /** Goes up on every reload, so the check and the files load again. */
    version: number;
    run: (request: Request | Promise<Request>) => void;
    copy: (text: string) => void;
  } = $props();

  /** Grey, the color of stashes in the graph; also for a branch that is gone. */
  const STASH_COLOR = 10;

  /** A stash's files: the tracked ones against the commit it was made on, then the untracked ones. */
  interface Files {
    tracked: FileChange[];
    untracked: FileChange[];
  }

  let files = $state<Record<string, Files>>({});
  let check = $state<StashCheck | null>(null);
  let checking = $state(false);
  let keepIndex = $state(false);
  let path = $state<string | null>(null);
  let whole = $state(false);
  let diffs = $state<FileDiff[]>([]);
  let error = $state<string | null>(null);
  let menu = $state<{ x: number; y: number; entries: MenuEntry[] } | null>(null);

  const stashes = $derived(history.stashes);
  const stash = $derived(stashes.find((s) => s.id === selected) ?? stashes[0] ?? null);
  const locals = $derived(new Set(history.refs.filter((r) => r.kind === "local").map((r) => r.name)));
  const here = $derived(history.head.branch ?? "HEAD");
  const busy = $derived(!!history.operation);
  const baseRow = $derived(stash ? history.rows.find((r) => r.id === stash.base) : undefined);

  /** The branch a stash was made on, colored like in the graph, or grey once it is deleted. */
  const branchColor = (s: StashInfo) => (s.branch && locals.has(s.branch) ? colorOf(s.branch) : STASH_COLOR);
  const gone = (s: StashInfo) => !!s.branch && !locals.has(s.branch);

  const totals = (f: Files | undefined) => {
    if (!f) return null;
    const all = [...f.tracked, ...f.untracked];
    return { count: all.length, add: all.reduce((n, x) => n + x.additions, 0), del: all.reduce((n, x) => n + x.deletions, 0) };
  };

  // Keep the picked stash when the list reloads; pick the newest when it is gone.
  $effect(() => {
    if (!stashes.some((s) => s.id === selected)) selected = stashes[0]?.id ?? null;
  });

  // Files of every stash, for the counts on the cards. A stash's commits never change, so each
  // is loaded once.
  $effect(() => {
    for (const s of stashes) {
      if (files[s.id]) continue;
      const tracked = api.commitDetail(s.id);
      const untracked: Promise<CommitDetail | null> = s.untracked ? api.commitDetail(s.untracked) : Promise.resolve(null);
      Promise.all([tracked, untracked]).then(
        ([t, u]) => (files[s.id] = { tracked: t.files, untracked: u?.files ?? [] }),
        (err) => (error = String(err)),
      );
    }
  });

  // Whether the picked stash applies to HEAD: again after every reload, since HEAD and the
  // working copy may have changed.
  $effect(() => {
    version;
    const current = stash;
    check = null;
    if (!current) return;
    checking = true;
    api.stashCheck(current.index, current.id).then(
      (c) => {
        if (stash?.id === current.id) check = c;
      },
      () => {},
    ).finally(() => {
      if (stash?.id === current.id) checking = false;
    });
  });

  // A new stash starts on its first file.
  $effect(() => {
    const current = stash?.id;
    const f = current ? files[current] : undefined;
    if (!f) return;
    const all = [...f.tracked, ...f.untracked];
    if (!path || !all.some((x) => x.path === path)) path = all[0]?.path ?? null;
  });

  $effect(() => {
    const current = stash;
    const f = current ? files[current.id] : undefined;
    const file = path;
    const wholeFile = whole;
    diffs = [];
    if (!current || !f || !file) return;
    // An untracked file's diff is in the stash's third commit.
    const source = f.tracked.some((x) => x.path === file) ? current.id : current.untracked;
    if (!source) return;
    api.commitDiff(source, file, wholeFile).then(
      (d) => {
        if (stash?.id === current.id && path === file && whole === wholeFile) diffs = d;
      },
      (err) => (error = String(err)),
    );
  });

  function pick(s: StashInfo) {
    selected = s.id;
    path = null;
    error = null;
  }

  function openMenu(event: MouseEvent, s: StashInfo) {
    event.preventDefault();
    menu = { x: event.clientX, y: event.clientY, entries: stashMenu(ctx, s, run, copy) };
  }

  function onKey(event: KeyboardEvent) {
    const i = stashes.findIndex((s) => s.id === stash?.id);
    const next = event.key === "ArrowDown" ? i + 1 : event.key === "ArrowUp" ? i - 1 : -1;
    if (next < 0 || next >= stashes.length || (event.key !== "ArrowDown" && event.key !== "ArrowUp")) return;
    event.preventDefault();
    pick(stashes[next]);
    (event.currentTarget as HTMLElement).querySelector<HTMLElement>(`[data-id="${stashes[next].id}"]`)?.focus();
  }

  const statusIcon: Record<string, string> = { added: "A", deleted: "D", modified: "M", renamed: "R", copied: "C" };
  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
</script>

{#snippet branchChip(s: StashInfo)}
  {#if s.branch}
    {@const color = branchColor(s)}
    <span class="chip" class:gone={gone(s)} style:color={plate(color)} style:background={tint(color, "label")} title={gone(s) ? `${s.branch} was deleted` : `Stashed on ${s.branch}`}>
      {s.branch}
    </span>
  {:else}
    <span class="chip" style:color={plate(STASH_COLOR)} style:background={tint(STASH_COLOR, "label")} title="Stashed on a detached HEAD">{shortId(s.base)}</span>
  {/if}
{/snippet}

<div class="screen">
  <section class="list" aria-label="Stashes">
    <div class="list-head">
      <span class="grow">{stashes.length} {stashes.length === 1 ? "stash" : "stashes"}{stashes.length > 1 ? " · newest first" : ""}</span>
      <button class="stash-now" onclick={() => run(stashRequest(ctx))} disabled={!ctx.uncommitted || busy} title={ctx.uncommitted ? "Stash your uncommitted changes" : "No uncommitted changes to stash"}>
        <svg class="icon small" viewBox="0 0 16 16"><path d="M2.5 3.5h11v3h-11zM3.5 6.5v6h9v-6M6.5 9h3" /></svg>
        {ctx.uncommitted ? `Stash ${plural(ctx.uncommitted, "Change")}` : "Stash Changes"}
      </button>
    </div>
    {#if stashes.length}
      <div class="cards" role="listbox" tabindex="0" aria-label="Stashes, newest first" onkeydown={onKey}>
        {#each stashes as s (s.id)}
          {@const t = totals(files[s.id])}
          {@const on = s.id === stash?.id}
          <button
            class="card"
            class:on
            data-id={s.id}
            role="option"
            aria-selected={on}
            style:background={on ? tint(branchColor(s)) : undefined}
            onclick={() => pick(s)}
            oncontextmenu={(e) => openMenu(e, s)}
          >
            <span class="line">
              <span class="title ellipsis">{s.title}</span>
              <span class="mono name">{stashName(s)}</span>
            </span>
            <span class="line meta">
              {@render branchChip(s)}
              <span>{relativeTime(s.time)}</span>
              {#if files[s.id]?.untracked.length}<span class="badge-untracked">+ untracked</span>{/if}
              <span class="grow"></span>
              {#if t}
                <span>{plural(t.count, "file")}</span>
                <span class="mono add">+{t.add}</span>
                <span class="mono del">−{t.del}</span>
              {/if}
            </span>
          </button>
        {/each}
      </div>
    {:else}
      <div class="empty">
        <svg class="icon big" viewBox="0 0 16 16"><path d="M2.5 3.5h11v3h-11zM3.5 6.5v6h9v-6M6.5 9h3" /></svg>
        <strong>No stashes</strong>
        <span>Stash uncommitted changes to switch branches without committing them.</span>
      </div>
    {/if}
  </section>

  <section class="detail" aria-label="Stash details">
    {#if stash}
      {@const f = files[stash.id]}
      {@const conflicts = check?.conflicts ?? []}
      {@const inTheWay = check?.inTheWay ?? []}
      <div class="head">
        <h1 class="selectable">{stash.title}</h1>
        <div class="meta-line">
          {@render branchChip(stash)}
          <span>on</span>
          <span class="mono sha">{shortId(stash.base)}</span>
          {#if baseRow}<span class="ellipsis base">{baseRow.summary}</span>{/if}
          <span class="nowrap">· {relativeTime(stash.time)}</span>
        </div>
        <div
          class="check"
          class:ok={check && !conflicts.length && !inTheWay.length && check.conflicts}
          class:warn={inTheWay.length || conflicts.length}
        >
          <span class="grow">
            {#if checking && !check}
              Checking whether it applies to {here}…
            {:else if inTheWay.length}
              Your changes to {inTheWay.slice(0, 2).join(", ")}{inTheWay.length > 2 ? ` and ${inTheWay.length - 2} more` : ""} are in the way. Commit or stash them first.
            {:else if conflicts.length}
              {plural(conflicts.length, "conflict")} if applied to {here}: {conflicts.slice(0, 3).join(", ")}{conflicts.length > 3 ? ` and ${conflicts.length - 3} more` : ""}.
            {:else if check?.conflicts}
              Applies cleanly to {here}.
            {:else if check}
              Can’t tell whether it conflicts with {here}: that needs git 2.40 or newer.
            {/if}
            {#if gone(stash)}<span class="note"> Its branch was deleted.</span>{/if}
          </span>
          <button class="switch" role="switch" aria-checked={keepIndex} onclick={() => (keepIndex = !keepIndex)} title="Files that were staged when you stashed come back staged">
            <span class="track" class:on={keepIndex}><span class="knob"></span></span>
            Keep staged
          </button>
        </div>
        <div class="actions">
          {#if busy}
            <span class="busy">Finish or abort the operation in progress first.</span>
          {:else}
            <button class="action primary" onclick={() => run(applyRequest(ctx, stash, false, keepIndex, check ?? undefined))}>Apply</button>
            <button class="action" onclick={() => run(applyRequest(ctx, stash, true, keepIndex, check ?? undefined))}>Pop</button>
            <button class="action" onclick={() => run(stashBranchRequest(ctx, stash))}>New Branch…</button>
          {/if}
          <span class="grow"></span>
          <button class="action danger" disabled={busy} onclick={() => run(dropRequest(ctx, stash))}>Drop</button>
        </div>
      </div>

      <div class="scroll">
        {#if error}<p class="error" role="alert">{error}</p>{/if}
        {#if f}
          {@const t = totals(f)}
          <div class="files">
            <div class="count">
              <span class="grow">{plural(t?.count ?? 0, "file")}</span>
              <span class="add">+{t?.add}</span>
              <span class="del">−{t?.del}</span>
            </div>
            {#each [...f.tracked.map((x) => ({ file: x, untracked: false })), ...f.untracked.map((x) => ({ file: x, untracked: true }))] as { file, untracked } (file.path + untracked)}
              {@const p = splitPath(file.path)}
              <button class="file" class:on={path === file.path} onclick={() => (path = file.path)}>
                <span class="badge {untracked ? 'added' : file.status}">{untracked ? "A" : statusIcon[file.status]}</span>
                <span class="grow ellipsis"><span class="dir">{p.dir}</span>{p.name}</span>
                {#if untracked}<span class="meta">untracked</span>{/if}
                {#if file.binary}
                  <span class="meta">binary</span>
                {:else}
                  <span class="mono add">+{file.additions}</span>
                  <span class="mono del num">−{file.deletions}</span>
                {/if}
              </button>
            {/each}
          </div>
        {/if}
        <DiffView {diffs} color={branchColor(stash)} {whole} onToggleWhole={() => (whole = !whole)} />
      </div>
    {/if}
  </section>
</div>

{#if menu}
  <Menu x={menu.x} y={menu.y} label="Stash menu" entries={menu.entries} onClose={() => (menu = null)} />
{/if}

<style>
  .screen {
    flex-grow: 1;
    display: flex;
    min-height: 0;
    margin: 4px 12px 12px;
    border: 1px solid var(--sep);
    border-radius: 12px;
    overflow: hidden;
  }
  .list {
    width: 400px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--sep);
    background: var(--graph-bg);
    min-height: 0;
  }
  .list-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px 8px 16px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
  }
  .stash-now {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 26px;
    padding: 0 10px;
    border-radius: 13px;
    background: var(--field);
    color: var(--text);
    font-size: 12px;
    font-weight: 500;
  }
  .stash-now:disabled {
    opacity: 0.45;
  }
  .cards {
    flex-grow: 1;
    overflow-y: auto;
    padding: 0 8px 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    outline: none;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 5px;
    width: 100%;
    padding: 10px 12px;
    border-radius: 12px;
    text-align: left;
  }
  .card:hover:not(.on) {
    background: var(--field);
  }
  .line {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .title {
    font-weight: 600;
    flex-grow: 1;
    min-width: 0;
  }
  .name {
    font-size: 11px;
    color: var(--text2);
    flex-shrink: 0;
  }
  .meta {
    font-size: 11px;
    color: var(--text2);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    height: 18px;
    padding: 0 7px;
    border-radius: 9px;
    font-size: 11px;
    font-weight: 500;
    white-space: nowrap;
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    flex-shrink: 0;
  }
  .chip.gone {
    text-decoration: line-through;
  }
  .badge-untracked {
    height: 16px;
    padding: 0 6px;
    border-radius: 8px;
    border: 1px solid var(--sep);
    font-size: 10px;
    line-height: 14px;
    white-space: nowrap;
  }
  .empty {
    flex-grow: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 24px;
    text-align: center;
    color: var(--text2);
  }
  .empty strong {
    color: var(--text);
    font-size: 14px;
  }
  .big {
    width: 32px;
    height: 32px;
    margin-bottom: 4px;
  }
  .small {
    width: 14px;
    height: 14px;
  }
  .detail {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .head {
    padding: 16px 16px 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    border-bottom: 1px solid var(--sep);
  }
  h1 {
    margin: 0;
    font-size: 17px;
    font-weight: 700;
    line-height: 1.3;
  }
  .meta-line {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    font-size: 12px;
    color: var(--text2);
  }
  .sha {
    color: var(--accent-text);
    flex-shrink: 0;
  }
  .base {
    min-width: 0;
  }
  .nowrap {
    white-space: nowrap;
    flex-shrink: 0;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 10px 8px 12px;
    border-radius: 10px;
    background: var(--field);
    color: var(--text2);
    font-size: 12px;
    min-height: 36px;
  }
  .check.ok {
    background: var(--green-soft);
    color: var(--green);
  }
  .check.warn {
    background: var(--orange-soft);
    color: var(--orange);
  }
  .note {
    color: var(--text2);
  }
  .switch {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
    color: var(--text);
    font-size: 12px;
  }
  .track {
    position: relative;
    width: 26px;
    height: 16px;
    border-radius: 8px;
    background: var(--switch-off);
    transition: background 0.15s;
  }
  .track.on {
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
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.2);
    transition: left 0.15s;
  }
  .track.on .knob {
    left: 12px;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .action {
    min-width: 96px;
    height: 30px;
    padding: 0 14px;
    border-radius: 15px;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    color: var(--text);
    font-weight: 500;
  }
  .action.primary {
    background: var(--accent);
    border-color: transparent;
    color: #fff;
    font-weight: 600;
  }
  .action.danger {
    background: var(--danger-soft);
    border-color: transparent;
    box-shadow: none;
    color: var(--danger);
  }
  .action:disabled {
    opacity: 0.45;
  }
  .busy {
    font-size: 12px;
    color: var(--text2);
  }
  .scroll {
    flex-grow: 1;
    overflow-y: auto;
    min-height: 0;
  }
  .files {
    margin: 12px 12px 6px;
    padding: 8px 4px 6px;
    border-radius: 12px;
    border: 1px solid var(--sep);
  }
  .count {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px 6px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
  }
  .file {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 26px;
    padding: 0 8px;
    border-radius: 7px;
  }
  .file:hover {
    background: var(--field);
  }
  .file.on {
    background: var(--side-sel);
  }
  .badge {
    width: 16px;
    height: 16px;
    border-radius: 5px;
    font-size: 10px;
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--field);
    color: var(--text2);
    flex-shrink: 0;
  }
  .badge.added {
    color: var(--green);
  }
  .badge.deleted {
    color: var(--red);
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
  .dir {
    color: var(--text2);
  }
  .add {
    color: var(--green);
    font-size: 11px;
  }
  .del {
    color: var(--red);
    font-size: 11px;
  }
  .num {
    min-width: 22px;
    text-align: right;
  }
  .error {
    color: var(--red);
    margin: 12px 20px;
  }
</style>
