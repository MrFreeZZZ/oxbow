<script lang="ts">
  import { onMount, onDestroy, untrack, type Snippet } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import Menu, { type MenuEntry } from "./Menu.svelte";
  import { prefs } from "./prefs.svelte";
  import { looksLikeUrl, start } from "./start.svelte";
  import { shortcut } from "./keys";
  import { tilde } from "./term";
  import { api } from "./api";
  import { OverviewReader, overviewFacts, selectedOverview, trackingNote, overviewStatus } from "./overview";
  import { actionIcons } from "./icons";
  import { menuIcons } from "./Menu.svelte";
  import { plate, tint, relativeTime } from "./format";
  import SettingsRow from "./SettingsRow.svelte";
  import type { OverviewRow, RepoOverview } from "./types";

  let { onBack, onOpen, onOpenPath, onLocate, currentPath = null, loading, error: sessionError, gone }: {
    onBack?: () => void;
    loading: boolean;
    error: string | null;
    gone?: Snippet;
    onOpen: () => void;
    onOpenPath: (path: string) => void;
    onLocate: (path: string) => void;
    currentPath?: string | null;
  } = $props();
  let rows = $state<OverviewRow[] | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let filter = $state("");
  let selectedPath = $state<string | null>(null);
  let list = $state<HTMLDivElement>();
  const reader = new OverviewReader(api.repositoryOverview, api.cancelRepositoryOverview);
  const shown = $derived((rows ?? []).filter(({ repository: r }) =>
    `${r.name} ${r.path}`.toLowerCase().includes(filter.trim().toLowerCase())));
  const selected = $derived(selectedOverview(shown, selectedPath, currentPath));

  function refresh() {
    busy = true;
    error = null;
    void reader.refresh((next, problem) => {
      if (next) rows = next;
      error = problem;
      busy = false;
    });
  }
  function activate(row: OverviewRow) {
    if (loading) return;
    if (row.repository.missing) onLocate(row.repository.path);
    else onOpenPath(row.repository.path);
  }
  function listKey(event: KeyboardEvent) {
    if (event.key === "Enter" && selected) {
      event.preventDefault();
      activate(selected);
      return;
    }
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const at = shown.findIndex(r => r.repository.path === selected?.repository.path);
    const next = event.key === "Home" ? 0 : event.key === "End" ? shown.length - 1 :
      Math.max(0, Math.min(shown.length - 1, at + (event.key === "ArrowDown" ? 1 : -1)));
    const row = shown[next];
    if (row) {
      selectedPath = row.repository.path;
      list?.querySelector<HTMLElement>(`[data-path="${CSS.escape(row.repository.path)}"]`)?.focus();
    }
  }
  const status = (row: OverviewRow) => overviewStatus(row.repository, row.summary);
  const checked = (at: number) => new Date(at * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  onMount(() => { void start.loadInfo(); });
  $effect(() => {
    if (!gone) untrack(refresh);
    return () => reader.stop();
  });
  onDestroy(() => reader.stop());

  const info = $derived(start.info);
  const home = $derived(info?.home ?? "");
  const needsIdentity = $derived(!!info && (!info.name || !info.email));
  let name = $state("");
  let email = $state("");
  let savingIdentity = $state(false);
  let identityError = $state<string | null>(null);
  let menu = $state<{ x: number; y: number; label: string; entries: MenuEntry[] } | null>(null);
  const env = $derived.by(() => {
    if (!info) return [];
    const rows: { ok: boolean; text: string; action?: { label: string; run: () => void } }[] = [];
    rows.push(
      info.git.version
        ? { ok: true, text: `git ${info.git.version} · ${tilde(info.git.path.replace(/[/\\]git(\.exe)?$/, ""), home)}` }
        : { ok: false, text: "Git not found", action: { label: "Settings…", run: () => api.openSettings() } },
    );
    rows.push(
      info.name && info.email
        ? { ok: true, text: `${info.name} · ${info.email}` }
        : { ok: false, text: "No name and email for commits yet" },
    );
    rows.push({ ok: info.ssh.ok, text: info.ssh.label });
    return rows;
  });

  async function forget(path: string) {
    try {
      await api.forgetRepo(path);
      rows = (rows ?? []).filter(row => row.repository.path !== path);
      refresh();
    } catch (problem) { error = String(problem); }
  }
  function rowMenu(event: MouseEvent, row: OverviewRow) {
    event.preventDefault();
    selectedPath = row.repository.path;
    menu = { x: event.clientX, y: event.clientY, label: row.repository.name, entries: [
      { kind: "item", label: row.repository.missing ? "Locate…" : "Open", icon: menuIcons.folder, run: () => activate(row) },
      { kind: "item", label: "Copy Path", icon: menuIcons.copy, run: () => { void navigator.clipboard.writeText(row.repository.path).catch(() => {}); } },
      { kind: "sep" },
      { kind: "item", label: "Remove from Recent", icon: menuIcons.drop, run: () => { void forget(row.repository.path); } },
    ] };
  }
  async function saveIdentity() {
    savingIdentity = true;
    identityError = null;
    try {
      if (name.trim()) await api.setGitConfig("global", "user.name", name.trim());
      if (email.trim()) await api.setGitConfig("global", "user.email", email.trim());
      await start.loadInfo();
    } catch (err) {
      identityError = String(err);
    } finally {
      savingIdentity = false;
    }
  }

  /** A dropped folder opens, or starts a repository when it isn't one; a dropped URL clones. */
  async function dropFolder(path: string) {
    const plan = await api
      .planNewRepo({ path, branch: "main", readme: false, gitignore: null, license: null, commit: false })
      .catch(() => null);
    if (plan?.repository) onOpenPath(plan.repository);
    else if (plan?.exists) start.newRepo(path);
  }

  $effect(() => {
    let stop: (() => void) | undefined;
    let disposed = false;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "drop" && event.payload.paths[0] && !start.sheet) dropFolder(event.payload.paths[0]);
      })
      .then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; })
      .catch(() => {});
    return () => { disposed = true; stop?.(); };
  });

  function onDrop(event: DragEvent) {
    const text = event.dataTransfer?.getData("text/uri-list") || event.dataTransfer?.getData("text/plain") || "";
    if (looksLikeUrl(text)) {
      event.preventDefault();
      start.clone(text.trim().split(/\s+/)[0]);
    }
  }

  function onPaste(event: ClipboardEvent) {
    if (start.sheet || event.target instanceof HTMLInputElement) return;
    const text = event.clipboardData?.getData("text/plain") ?? "";
    if (looksLikeUrl(text)) {
      event.preventDefault();
      start.clone(text.trim());
    }
  }

  const actions = [
    { label: "Clone Repository…", keys: shortcut("clone"), icon: "M8 2.5v8M5 7.5l3 3 3-3M3 13.5h10", run: () => start.clone() },
    { label: "Open Local Repository…", keys: shortcut("open"), icon: "M1.5 4.5a1 1 0 0 1 1-1H6l1.5 1.5h6a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z", run: () => onOpen() },
    { label: "New Repository…", keys: shortcut("newRepo"), icon: "M8 3v10M3 8h10", run: () => start.newRepo() },
  ];
</script>
<svelte:window onpaste={onPaste} />
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="welcome" ondragover={(event) => event.preventDefault()} ondrop={onDrop}>
  <section class="start" aria-label="Start">
    <div class="drag" data-tauri-drag-region></div>
    {#if onBack}<button class="button small back" onclick={onBack}>Back to repository</button>{/if}
    <img class="app-icon" src="/app-icon.png" alt="Oxbow app icon" width="96" height="96" />
    <div class="app-name">Oxbow</div>
    <div class="version">{info ? `Version ${info.version}` : ""}</div>

    <div class="actions">
      {#each actions as action (action.label)}
        <button class="action" onclick={action.run} disabled={loading}>
          <span class="action-icon"><svg class="icon" viewBox="0 0 16 16"><path d={action.icon} /></svg></span>
          <span class="action-label">{action.label}</span>
          <span class="keys">{action.keys}</span>
        </button>
      {/each}
    </div>

    {#if loading}<p class="opening">Opening…</p>{/if}
    {#if error || sessionError}<p class="error" role="alert">{error ?? sessionError}</p>{/if}

    <span class="grow"></span>
    {#if env.length}
      <div class="env" aria-label="Environment">
        {#each env as row, i (i)}
          <div class="env-row" class:warn={!row.ok}>
            <span class="dot">
              <svg viewBox="0 0 16 16"><path d={row.ok ? "M3.5 8.5 6.5 11.5 12.5 4.5" : "M8 4v5M8 11.5v.1"} /></svg>
            </span>
            <span class="env-text" title={row.text}>{row.text}</span>
            {#if row.action}<button class="env-action" onclick={row.action.run}>{row.action.label}</button>{/if}
          </div>
        {/each}
      </div>
    {/if}
    <button
      class="launch"
      role="checkbox"
      aria-checked={!prefs.get("oxbow.startup.reopenRepository")}
      onclick={() => prefs.set("oxbow.startup.reopenRepository", !prefs.get("oxbow.startup.reopenRepository"))}
    >
      <span class="box" class:on={!prefs.get("oxbow.startup.reopenRepository")}><svg viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg></span>
      Show this window when Oxbow opens
    </button>
  </section>

  <section class="repos" aria-label="Repositories">
    <div class="drag top-drag" data-tauri-drag-region></div>
    {#if gone}{@render gone()}{:else}
      <div class="head">
        <span class="title">Recent Repositories</span>
        <label class="filter">
          <svg class="icon" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5" /><path d="m10.5 10.5 3 3" /></svg>
          <input bind:value={filter} placeholder="Filter" aria-label="Filter repositories" spellcheck="false" autocomplete="off" />
        </label>
        <button class="button icon-only small" onclick={refresh} disabled={busy || loading} aria-label="Refresh repositories" title={busy ? "Refreshing…" : "Refresh local snapshots"}>
          <svg class="icon" viewBox="0 0 16 16"><path d={actionIcons.fetch} /></svg>
        </button>
      </div>
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <div class="list" role="listbox" aria-label="Recent repositories" aria-busy={busy} bind:this={list} onkeydown={listKey}>
        {#each shown as row (row.repository.path)}
          {@const r = row.repository}
          {@const s = row.summary}
          {@const state = status(row)}
          <button class="row" class:on={selected?.repository.path === r.path} class:missing={r.missing}
            role="option" aria-selected={selected?.repository.path === r.path} data-path={r.path} disabled={loading}
            tabindex={selected?.repository.path === r.path ? 0 : -1} onclick={() => (selectedPath = r.path)}
            ondblclick={() => activate(row)} oncontextmenu={(event) => rowMenu(event, row)}>
            <span class="folder" style:background={r.missing || !s ? "var(--field)" : tint(s.color, "label")} style:color={r.missing || !s ? "var(--text2)" : plate(s.color)}>
              <svg class="icon" viewBox="0 0 16 16"><path d={menuIcons.folder} /></svg>
            </span>
            <span class="words"><span class="name">{r.name}</span><span class="where" title={r.path}>{tilde(r.path, home)} · {relativeTime(r.opened)}</span></span>
            <span class="side">
              {#if s}<span class="branch" style:background={tint(s.color, "label")} style:color={plate(s.color)} title={s.branch ?? s.commit ?? "No commits"}>{s.branch ?? (s.commit ? "Detached HEAD" : "No commits")}</span>{/if}
              <span class="status" class:warn={state.warn} title={state.text}>{#if state.warn}<svg viewBox="0 0 16 16"><path d={actionIcons.warn} /></svg>{/if}{state.text}
                {#if r.missing}
                  <!-- svelte-ignore a11y_click_events_have_key_events -->
                  <span class="locate" role="button" tabindex="-1" onclick={(event) => { event.stopPropagation(); selectedPath = r.path; onLocate(r.path); }}>Locate…</span>
                {/if}
              </span>
            </span>
          </button>
        {/each}
        {#if !rows && busy}<p class="none" role="status">Reading repositories…</p>
        {:else if rows && !shown.length}
          {#if rows.length}<p class="none">No repository matches your filter.</p>{:else}
      <div class="empty">
        <span class="empty-icon"><svg viewBox="0 0 16 16"><path d="M1.5 4.5a1 1 0 0 1 1-1H6l1.5 1.5h6a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" /></svg></span>
        <div class="empty-title">No repositories yet</div>
        <div class="empty-sub">Clone one, open a folder that already has Git, or drop it onto this window.</div>
      </div>
          {/if}
        {/if}
      </div>
      {#if needsIdentity}{@render identity()}{/if}
      <div class="foot"><svg class="icon" viewBox="0 0 16 16"><path d="M8 2.5v8M5 7.5l3 3 3-3M3 13.5h10" /></svg>Double-click to open · drop a folder or a Git URL anywhere in this window</div>
    {/if}
  </section>
{#if !gone}    <aside aria-label="Selected repository">
      {#if selected}
        {@const r = selected.repository}
        {@const s = selected.summary}
        <div class="inspector">
          <div class="repo-identity">
            <div><h2>{r.name}</h2><p class="full-path">{tilde(r.path, home)}</p></div>
          </div>
          {#if s}
            <div class="branch-line">{@render branch(s)}
              {#if !s.branch || !s.commit || !s.upstream || s.ahead === null || s.behind === null}
                <span class="note">{trackingNote(s)}</span>
              {/if}
            </div>
            {#if s.operation}<p class="warn">{s.operation}</p>{/if}
            {#if s.conflicts}<p class="warn conflicts"><svg class="icon" viewBox="0 0 16 16"><path d={actionIcons.warn} /></svg>{s.conflicts} conflict{s.conflicts === 1 ? "" : "s"} to resolve</p>{/if}
            <div class="facts" title={s.upstream ?? "No upstream"}>
              {#each overviewFacts(s) as row (row.id)}
                <SettingsRow {row} menuOn={false} onMenu={() => {}} editing={null} />
              {/each}
            </div>
            <details class="snapshot">
              <summary title={`Local read completed ${new Date(selected.checkedAt * 1000).toLocaleString()}`}>Local refs · Checked {checked(selected.checkedAt)}</summary>
              <p>Refresh reads local changes. Open the repository to fetch and check the server.</p>
              <p>Submodule contents and Git filters are excluded from this overview.</p>
              {#if s.fetchedAt}<p>FETCH_HEAD recorded {new Date(s.fetchedAt * 1000).toLocaleString()}; it may cover only some remotes.</p>{/if}
            </details>
            {#if s.filtersDisabled}<p class="note">Filters disabled for this snapshot.</p>{/if}
          {:else}
            <div class="problem"><p class="error">{r.missing ? "Folder not found" : "Unable to read repository"}</p>
              {#if selected.error}<p class="note">{selected.error}</p>{/if}
            </div>
          {/if}
          <button class="button primary open-selected" disabled={loading} onclick={() => activate(selected!)}>
{r.missing ? "Locate Repository…" : "Open Repository"}
          </button>
        </div>
      {:else}<p class="empty">{busy ? "Reading repositories…" : "Select a repository to see its details."}</p>{/if}
    </aside>{/if}</div>
{#snippet identity()}
  <div class="identity" aria-label="Commit identity">
    <div class="identity-head">
      <span class="identity-title">Before your first commit</span>
      <span class="identity-sub">Your name and email go into every commit. Oxbow saves them to ~/.gitconfig, so the git CLI uses them too.</span>
    </div>
    <div class="identity-grid">
      <span>Name</span>
      <input bind:value={name} placeholder={info?.name ?? "Jane Appleseed"} aria-label="Name" spellcheck="false" autocomplete="name" />
      <span>Email</span>
      <input bind:value={email} type="email" placeholder={info?.email ?? "jane@example.com"} aria-label="Email" spellcheck="false" autocomplete="email" />
    </div>
    <div class="identity-foot">
      <span class:error={!!identityError}>{identityError ?? "Can be overridden per repository later."}</span>
      <button
        class="save"
        onclick={saveIdentity}
        disabled={savingIdentity || !(name.trim() || info?.name) || !(email.trim() || info?.email) || !(name.trim() || email.trim())}>Save</button
      >
    </div>
  </div>
{/snippet}


{#snippet branch(s: RepoOverview)}
  <span class="branch" style:background={tint(s.color, "label")} style:color={plate(s.color)} title={s.branch ?? s.commit ?? "No commits"}>{s.branch ?? (s.commit ? "Detached HEAD" : "No commits")}</span>
{/snippet}

{#if menu}<Menu x={menu.x} y={menu.y} label={menu.label} entries={menu.entries} onClose={() => (menu = null)} />{/if}
<style>
  .welcome {
    height: 100%;
    display: flex;
    background: var(--win);
    color: var(--text);
  }
  .drag {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 44px;
  }
  .start {
    position: relative;
    width: 330px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 48px 28px 20px;
  }
  .app-icon {
    width: 96px;
    height: 96px;
    filter: drop-shadow(0 6px 14px rgba(40, 60, 80, 0.28));
  }
  .app-name {
    font-size: var(--text-display);
    font-weight: 700;
    letter-spacing: -0.4px;
    margin-top: 10px;
  }
  .version {
    min-height: 16px;
    font-size: var(--text-small);
    color: var(--text2);
    margin-top: 2px;
  }
  .actions {
    align-self: stretch;
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 22px;
  }
  .action {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 42px;
    padding: 0 12px 0 8px;
    border-radius: var(--radius-l);
    background: var(--field);
    text-align: left;
  }
  .action:disabled {
    opacity: 0.6;
  }
  .action-icon {
    width: 28px;
    height: 28px;
    border-radius: var(--radius-m);
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent);
    flex-shrink: 0;
  }
  .action-label {
    flex-grow: 1;
    font-weight: 500;
  }
  .keys {
    font-size: var(--text-small);
    color: var(--text2);
  }
  .opening {
    margin: 12px 0 0;
    color: var(--text2);
    font-size: var(--text-small);
  }
  .error {
    margin: 12px 0 0;
    color: var(--red);
    font-size: var(--text-small);
    line-height: 1.4;
    overflow-wrap: anywhere;
    align-self: stretch;
  }
  .grow {
    flex-grow: 1;
  }
  .env {
    align-self: stretch;
    margin-top: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 10px 12px;
    border-radius: var(--radius-l);
    border: 0.5px solid var(--sep);
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .env-row {
    display: flex;
    align-items: center;
    gap: 7px;
    min-height: 18px;
  }
  .dot {
    width: 14px;
    height: 14px;
    border-radius: var(--radius-pill);
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--green-soft);
    color: var(--green);
    flex-shrink: 0;
  }
  .warn .dot {
    background: var(--orange-soft);
    color: var(--orange);
  }
  .dot svg {
    width: 10px;
    height: 10px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .env-text {
    flex-grow: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .warn .env-text {
    color: var(--orange);
  }
  .env-action {
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--accent-text);
  }
  .launch {
    align-self: flex-start;
    display: flex;
    align-items: center;
    gap: 7px;
    margin-top: 12px;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .box {
    width: 14px;
    height: 14px;
    border-radius: var(--radius-xs);
    border: 1px solid var(--text2);
    display: flex;
    align-items: center;
    justify-content: center;
    color: transparent;
  }
  .box.on {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  .box svg {
    width: 10px;
    height: 10px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2.4;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .repos {
    position: relative;
    flex-grow: 1;
    min-width: 0;
    background: var(--side);
    border-left: 1px solid var(--side-border);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .top-drag {
    height: 12px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 52px;
    padding: 0 12px 0 18px;
    flex-shrink: 0;
  }
  .title {
    flex-grow: 1;
    font-size: var(--text-body);
    font-weight: 700;
  }
  .filter {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 200px;
    height: 30px;
    padding: 0 10px;
    border-radius: var(--radius-pill);
    background: var(--field);
    color: var(--text2);
  }
  .filter:focus-within {
    box-shadow: var(--focus-ring);
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
  .list {
    flex-grow: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0 8px 8px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 11px;
    height: 54px;
    flex-shrink: 0;
    padding: 0 12px 0 10px;
    border-radius: var(--radius-l);
    text-align: left;
  }
  .row.on {
    background: var(--side-sel);
  }
  .row:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .row.missing .folder,
  .row.missing .words {
    opacity: 0.55;
  }
  .folder {
    width: 32px;
    height: 32px;
    border-radius: var(--radius-m);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }
  .words {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    line-height: 1.25;
  }
  .name {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .where {
    font-size: var(--text-caption);
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .side {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 3px;
    flex-shrink: 0;
    max-width: 45%;
    line-height: 1.25;
  }
  .branch {
    display: block;
    height: 18px;
    padding: 0 8px;
    border-radius: var(--radius-pill);
    font-size: var(--text-caption);
    font-weight: 500;
    line-height: 18px;
    max-width: 100%;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: var(--text-caption);
    color: var(--text2);
    white-space: nowrap;
  }
  .status.warn {
    color: var(--orange);
  }
  .status svg {
    width: 11px;
    height: 11px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .locate {
    font-weight: 600;
    color: var(--accent-text);
    margin-left: 4px;
    cursor: default;
  }
  .none {
    margin: 12px 10px;
    font-size: var(--text-small);
    color: var(--text2);
  }
  .foot {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 40px;
    flex-shrink: 0;
    font-size: var(--text-caption);
    color: var(--text2);
    border-top: 0.5px solid var(--sep);
  }
  .empty {
    flex-grow: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 0 44px;
    text-align: center;
  }
  .empty-icon {
    width: 56px;
    height: 56px;
    border-radius: var(--radius-xl);
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--field);
    color: var(--text2);
  }
  .empty-icon svg {
    width: 28px;
    height: 28px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .empty-title {
    font-size: var(--text-heading);
    font-weight: 700;
    margin-top: 14px;
  }
  .empty-sub {
    font-size: var(--text-small);
    color: var(--text2);
    line-height: 1.45;
    margin-top: 4px;
    max-width: 360px;
  }
  .identity {
    align-self: center;
    width: min(560px, calc(100% - 32px));
    margin: 0 0 16px;
    flex-shrink: 0;
    padding: 16px;
    border-radius: var(--radius-xl);
    background: var(--win);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--panel-shadow);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .identity-head {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .identity-title {
    font-weight: 700;
  }
  .identity-sub {
    font-size: var(--text-small);
    color: var(--text2);
    line-height: 1.4;
  }
  .identity-grid {
    display: grid;
    grid-template-columns: 52px 1fr;
    align-items: center;
    gap: 8px 10px;
    font-size: var(--text-small);
  }
  .identity-grid span {
    color: var(--text2);
    text-align: right;
  }
  .identity-grid input {
    height: 30px;
    padding: 0 10px;
    border-radius: var(--radius-m);
    border: 0.5px solid var(--glass-border);
    background: var(--field);
    outline: 0;
    font: inherit;
    font-size: var(--text-body);
    color: var(--text);
    user-select: text;
    -webkit-user-select: text;
  }
  .identity-grid input:focus {
    box-shadow: var(--focus-ring);
  }
  .identity-foot {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .identity-foot span {
    flex-grow: 1;
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .identity-foot span.error {
    color: var(--red);
  }
  .save {
    height: 28px;
    padding: 0 14px;
    border-radius: var(--radius-pill);
    background: var(--accent);
    color: #fff;
    font-weight: 600;
  }
  .save:disabled {
    opacity: 0.5;
  }

  .back { position: absolute; top: 14px; right: 16px; }
  .head .filter { width: 180px; min-width: 80px; }
  .head .filter input { width: 100%; }
  .head .title { white-space: nowrap; }
  .side { min-width: 0; }
  .status { max-width: 100%; overflow: hidden; text-overflow: ellipsis; display: block; }
  .status svg { vertical-align: -1px; margin-right: 4px; }
  .foot { text-align: center; padding: 0 8px; font-size: var(--text-caption); }
  aside { width: clamp(300px, 28vw, 380px); flex-shrink: 0; border-left: 0.5px solid var(--sep); background: var(--win); overflow: auto; }
  .inspector { padding: 32px 28px; }
  h2 { font-size: var(--text-large); font-weight: 600; margin: 0 0 8px; overflow-wrap: anywhere; }
  .full-path { margin: 0; color: var(--text2); font-size: var(--text-small); overflow-wrap: anywhere; user-select: text; -webkit-user-select: text; }
  .branch-line { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin-top: 16px; }
  .inspector .branch { max-width: 100%; }
  .inspector > .warn { color: var(--orange); margin: 12px 0 0; font-size: var(--text-small); }
  .conflicts { display: flex; align-items: center; gap: 8px; }
  .conflicts .icon { width: 14px; height: 14px; flex-shrink: 0; }
  .facts { margin-top: 24px; border-radius: var(--radius-l); background: var(--panel); border: 0.5px solid var(--panel-border); }
  .snapshot { margin-top: 16px; color: var(--text2); font-size: var(--text-caption); line-height: 1.5; }
  .snapshot summary { cursor: pointer; }
  .snapshot p { margin: 8px 0 0; }
  .note { margin: 0; color: var(--text2); font-size: var(--text-small); line-height: 1.5; overflow-wrap: anywhere; }
  .inspector > .note { margin-top: 8px; }
  .open-selected { margin-top: 24px; }
  .problem { margin-top: 24px; }
  .problem .error { margin: 0 0 8px; }
  .empty { padding: 48px 24px; color: var(--text2); font-size: var(--text-small); }
  @media (max-width: 1150px) { .start { width: 290px; padding-left: 20px; padding-right: 20px; } .inspector { padding: 24px 16px; } .head { gap: 8px; } .head .filter { width: 120px; } }
</style>
