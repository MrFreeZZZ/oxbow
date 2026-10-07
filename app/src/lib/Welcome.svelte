<script lang="ts">
  // The Welcome window: clone, open or start a repository on the left with this computer's Git
  // setup under them; repositories opened before on the right, each with where it stands.

  import { open as chooseFolder } from "@tauri-apps/plugin-dialog";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { api } from "./api";
  import { plate, relativeTime, tint } from "./format";
  import Menu, { type MenuEntry, menuIcons } from "./Menu.svelte";
  import { prefs } from "./prefs.svelte";
  import { looksLikeUrl, start } from "./start.svelte";
  import { tilde } from "./term";
  import type { OperationKind, RecentRepo, RepoGlance } from "./types";

  let {
    loading,
    error,
    onOpen,
    onOpenPath,
  }: {
    loading: boolean;
    error: string | null;
    /** Choose a folder and open it. */
    onOpen: () => void;
    onOpenPath: (path: string) => void;
  } = $props();

  const mac = navigator.platform.startsWith("Mac");
  const cmd = mac ? "⌘" : "Ctrl+";
  const shift = mac ? "⇧" : "Shift+";

  let filter = $state("");
  let selected = $state<string | null>(null);
  let menu = $state<{ x: number; y: number; label: string; entries: MenuEntry[] } | null>(null);
  let list = $state<HTMLDivElement>();
  let name = $state("");
  let email = $state("");
  let savingIdentity = $state(false);
  let identityError = $state<string | null>(null);

  $effect(() => {
    start.loadInfo();
    // The newest repository is selected once the list is read again.
    start.loadRecent().then(() => (selected = start.recent?.[0]?.path ?? null));
  });

  const info = $derived(start.info);
  const home = $derived(info?.home ?? "");
  const needsIdentity = $derived(!!info && (!info.name || !info.email));

  const shown = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    const all = start.recent ?? [];
    return q ? all.filter((r) => r.name.toLowerCase().includes(q) || r.path.toLowerCase().includes(q)) : all;
  });

  // The first repository is selected, and a filter that hides the selected one moves it.
  $effect(() => {
    if (shown.length && !shown.some((r) => r.path === selected)) selected = shown[0].path;
  });

  const actions = [
    { label: "Clone Repository…", keys: `${shift}${cmd}C`, icon: "M8 2.5v8M5 7.5l3 3 3-3M3 13.5h10", run: () => start.clone() },
    { label: "Open Local Repository…", keys: `${cmd}O`, icon: "M1.5 4.5a1 1 0 0 1 1-1H6l1.5 1.5h6a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z", run: () => onOpen() },
    { label: "New Repository…", keys: `${cmd}N`, icon: "M8 3v10M3 8h10", run: () => start.newRepo() },
  ];

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

  const OPERATION: Record<OperationKind, string> = {
    merge: "Merge paused",
    squash: "Squash waiting to be committed",
    rebase: "Rebase paused",
    cherryPick: "Cherry-pick paused",
    revert: "Revert paused",
    stashApply: "Stash apply paused",
  };

  /** The second line on the right of a row: where the repository stands. */
  function status(repo: RecentRepo, glance: RepoGlance | null | undefined): { text: string; warn?: boolean } {
    if (repo.missing) return { text: "Folder not found" };
    if (glance === undefined) return { text: "" };
    if (glance === null) return { text: "Can't read it" };
    if (glance.operation) {
      const n = glance.conflicts;
      return { text: OPERATION[glance.operation] + (n ? ` · ${n} conflict${n === 1 ? "" : "s"}` : ""), warn: true };
    }
    const parts: string[] = [];
    if (glance.ahead) parts.push(`↑${glance.ahead}`);
    if (glance.behind) parts.push(glance.ahead || glance.changed ? `↓${glance.behind}` : `↓${glance.behind} behind ${glance.remote ?? "upstream"}`);
    if (glance.changed) parts.push(`${glance.changed} changed`);
    if (glance.stashes) parts.push(`${glance.stashes} stash${glance.stashes === 1 ? "" : "es"}`);
    if (parts.length) return { text: parts.join(" · ") };
    return { text: glance.upstream ? "Up to date" : glance.branch ? "Not published" : "Detached HEAD" };
  }

  async function locate(repo: RecentRepo) {
    const path = await chooseFolder({ directory: true, multiple: false, title: `Locate ${repo.name}`, defaultPath: home || undefined });
    if (typeof path !== "string") return;
    await api.forgetRepo(repo.path, path).catch(() => {});
    onOpenPath(path);
  }

  function activate(repo: RecentRepo) {
    if (repo.missing) locate(repo);
    else onOpenPath(repo.path);
  }

  async function forget(repo: RecentRepo) {
    await api.forgetRepo(repo.path).catch(() => {});
    start.recent = (start.recent ?? []).filter((r) => r.path !== repo.path);
  }

  function rowMenu(event: MouseEvent, repo: RecentRepo) {
    event.preventDefault();
    selected = repo.path;
    const entries: MenuEntry[] = [
      repo.missing
        ? { kind: "item", label: "Locate…", icon: menuIcons.checkout, run: () => locate(repo) }
        : { kind: "item", label: "Open", icon: menuIcons.checkout, run: () => onOpenPath(repo.path) },
      { kind: "item", label: "Copy Path", icon: menuIcons.copy, run: () => navigator.clipboard.writeText(repo.path).catch(() => {}) },
      { kind: "sep" },
      { kind: "item", label: "Remove from Recent", icon: menuIcons.drop, run: () => forget(repo) },
    ];
    menu = { x: event.clientX, y: event.clientY, label: repo.name, entries };
  }

  function onListKey(event: KeyboardEvent) {
    const at = shown.findIndex((r) => r.path === selected);
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const next = shown[Math.max(0, Math.min(shown.length - 1, at + (event.key === "ArrowDown" ? 1 : -1)))];
      if (next) {
        selected = next.path;
        list?.querySelector<HTMLElement>(`[data-path="${CSS.escape(next.path)}"]`)?.focus();
      }
    } else if (event.key === "Enter" && at >= 0) {
      event.preventDefault();
      activate(shown[at]);
    }
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
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "drop" && event.payload.paths[0] && !start.sheet) dropFolder(event.payload.paths[0]);
      })
      .then((unlisten) => (stop = unlisten))
      .catch(() => {});
    return () => stop?.();
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
</script>

<svelte:window onpaste={onPaste} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="welcome" ondragover={(e) => e.preventDefault()} ondrop={onDrop}>
  <section class="start" aria-label="Start">
    <div class="drag" data-tauri-drag-region></div>
    <img class="app-icon" src="/app-icon.png" alt="Oxbow app icon" width="112" height="112" />
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
    {#if error}<p class="error" role="alert">{error}</p>{/if}

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
    {#if start.recent && start.recent.length}
      <div class="head">
        <span class="title">Recent Repositories</span>
        <label class="filter">
          <svg class="icon" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5" /><path d="m10.5 10.5 3 3" /></svg>
          <input bind:value={filter} placeholder="Filter" aria-label="Filter repositories" spellcheck="false" autocomplete="off" />
        </label>
      </div>
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <div class="list" role="listbox" aria-label="Recent repositories" bind:this={list} onkeydown={onListKey}>
        {#each shown as repo (repo.path)}
          {@const glance = start.glances[repo.path]}
          {@const color = glance?.color ?? 0}
          {@const state = status(repo, glance)}
          <button
            class="row"
            class:on={repo.path === selected}
            class:missing={repo.missing}
            role="option"
            aria-selected={repo.path === selected}
            data-path={repo.path}
            onclick={() => (selected = repo.path)}
            ondblclick={() => activate(repo)}
            oncontextmenu={(e) => rowMenu(e, repo)}
          >
            <span
              class="folder"
              style:background={repo.missing || !glance ? "var(--field)" : tint(color, "label")}
              style:color={repo.missing || !glance ? "var(--text2)" : plate(color)}
            >
              <svg class="icon" viewBox="0 0 16 16"><path d="M1.5 4.5a1 1 0 0 1 1-1H6l1.5 1.5h6a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" /></svg>
            </span>
            <span class="words">
              <span class="name">{repo.name}</span>
              <span class="where">{tilde(repo.path, home)} · {relativeTime(repo.opened)}</span>
            </span>
            <span class="side">
              {#if glance?.branch}
                <span class="branch" style:background={tint(color, "label")} style:color={plate(color)}>{glance.branch}</span>
              {/if}
              <span class="status" class:warn={state.warn}>
                {#if state.warn}<svg viewBox="0 0 16 16"><path d="M8 2.5 14 13H2zM8 6.5v3M8 11.5v.1" /></svg>{/if}
                {state.text}
                {#if repo.missing}
                  <!-- svelte-ignore a11y_click_events_have_key_events -->
                  <span class="locate" role="button" tabindex="-1" onclick={(e) => (e.stopPropagation(), locate(repo))}>Locate…</span>
                {/if}
              </span>
            </span>
          </button>
        {:else}
          <p class="none">No repository matches “{filter}”.</p>
        {/each}
      </div>
      {#if needsIdentity}{@render identity()}{/if}
      <div class="foot">
        <svg class="icon" viewBox="0 0 16 16"><path d="M8 2.5v8M5 7.5l3 3 3-3M3 13.5h10" /></svg>
        Double-click to open · drop a folder or a Git URL anywhere in this window
      </div>
    {:else if start.recent}
      <div class="empty">
        <span class="empty-icon"><svg viewBox="0 0 16 16"><path d="M1.5 4.5a1 1 0 0 1 1-1H6l1.5 1.5h6a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" /></svg></span>
        <div class="empty-title">No repositories yet</div>
        <div class="empty-sub">Clone one, open a folder that already has Git, or drop it onto this window.</div>
      </div>
      {#if needsIdentity}{@render identity()}{/if}
    {/if}
  </section>
</div>

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

{#if menu}
  <Menu x={menu.x} y={menu.y} label={menu.label} entries={menu.entries} onClose={() => (menu = null)} />
{/if}

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
    padding: 66px 28px 22px;
  }
  .app-icon {
    width: 112px;
    height: 112px;
    filter: drop-shadow(0 6px 14px rgba(40, 60, 80, 0.28));
  }
  .app-name {
    font-size: 30px;
    font-weight: 700;
    letter-spacing: -0.4px;
    margin-top: 14px;
  }
  .version {
    min-height: 16px;
    font-size: 12px;
    color: var(--text2);
    margin-top: 2px;
  }
  .actions {
    align-self: stretch;
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 26px;
  }
  .action {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 42px;
    padding: 0 12px 0 8px;
    border-radius: 12px;
    background: var(--field);
    text-align: left;
  }
  .action:disabled {
    opacity: 0.6;
  }
  .action-icon {
    width: 28px;
    height: 28px;
    border-radius: 8px;
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
    font-size: 12px;
    color: var(--text2);
  }
  .opening {
    margin: 12px 0 0;
    color: var(--text2);
    font-size: 12px;
  }
  .error {
    margin: 12px 0 0;
    color: var(--red);
    font-size: 12px;
    line-height: 1.4;
    overflow-wrap: anywhere;
    align-self: stretch;
  }
  .grow {
    flex-grow: 1;
  }
  .env {
    align-self: stretch;
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 10px 12px;
    border-radius: 12px;
    border: 0.5px solid var(--sep);
    font-size: 11px;
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
    border-radius: 7px;
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
    font-size: 11px;
    font-weight: 600;
    color: var(--accent-text);
  }
  .launch {
    align-self: flex-start;
    display: flex;
    align-items: center;
    gap: 7px;
    margin-top: 12px;
    font-size: 12px;
    color: var(--text2);
  }
  .box {
    width: 14px;
    height: 14px;
    border-radius: 4px;
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
    margin: 8px 8px 8px 0;
    border-radius: 18px;
    background: var(--side);
    border: 0.5px solid var(--side-border);
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
    font-size: 13px;
    font-weight: 700;
  }
  .filter {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 200px;
    height: 30px;
    padding: 0 10px;
    border-radius: 15px;
    background: var(--field);
    color: var(--text2);
  }
  .filter input {
    flex-grow: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    font: inherit;
    font-size: 12px;
    color: var(--text);
    user-select: text;
    -webkit-user-select: text;
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
    border-radius: 11px;
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
    border-radius: 9px;
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
    font-size: 11px;
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
    border-radius: 9px;
    font-size: 11px;
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
    font-size: 11px;
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
    font-size: 12px;
    color: var(--text2);
  }
  .foot {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 40px;
    flex-shrink: 0;
    font-size: 11px;
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
    border-radius: 16px;
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
    font-size: 15px;
    font-weight: 700;
    margin-top: 14px;
  }
  .empty-sub {
    font-size: 12px;
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
    border-radius: 16px;
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
    font-size: 12px;
    color: var(--text2);
    line-height: 1.4;
  }
  .identity-grid {
    display: grid;
    grid-template-columns: 52px 1fr;
    align-items: center;
    gap: 8px 10px;
    font-size: 12px;
  }
  .identity-grid span {
    color: var(--text2);
    text-align: right;
  }
  .identity-grid input {
    height: 30px;
    padding: 0 10px;
    border-radius: 8px;
    border: 0.5px solid var(--glass-border);
    background: var(--field);
    outline: 0;
    font: inherit;
    font-size: 13px;
    color: var(--text);
    user-select: text;
    -webkit-user-select: text;
  }
  .identity-grid input:focus {
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent);
  }
  .identity-foot {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .identity-foot span {
    flex-grow: 1;
    font-size: 11px;
    color: var(--text2);
  }
  .identity-foot span.error {
    color: var(--red);
  }
  .save {
    height: 28px;
    padding: 0 14px;
    border-radius: 14px;
    background: var(--accent);
    color: #fff;
    font-weight: 600;
  }
  .save:disabled {
    opacity: 0.5;
  }
</style>
