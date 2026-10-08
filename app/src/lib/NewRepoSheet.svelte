<script lang="ts">
  // New Repository: git init in a new or existing folder, starter files, and the first commit,
  // with the exact commands. A folder that is a repository already is opened instead.

  import { open as chooseFolder } from "@tauri-apps/plugin-dialog";
  import { api } from "./api";
  import { prefs } from "./prefs.svelte";
  import { publishCommands, publishNote, repositoryName } from "./publish";
  import { join, start } from "./start.svelte";
  import TermBlock, { type TermLine } from "./TermBlock.svelte";
  import { tilde } from "./term";
  import type { GitHubOwners, NewRepoPlan, Publish } from "./types";

  let {
    folder: initialFolder,
    onDone,
    onClose,
  }: { folder: string | null; onDone: (path: string) => void; onClose: () => void } = $props();

  const mac = navigator.platform.startsWith("Mac");

  /** `path` split into the folder it is in and its own name. */
  function split(path: string): { parent: string; name: string } {
    const trimmed = path.replace(/[/\\]+$/, "");
    const at = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
    return at < 0 ? { parent: "", name: trimmed } : { parent: trimmed.slice(0, at) || "/", name: trimmed.slice(at + 1) };
  }

  // svelte-ignore state_referenced_locally
  const given = initialFolder ? split(initialFolder) : null;
  let name = $state(given?.name ?? "");
  let parent = $state(given?.parent ?? "");
  let branch = $state("");
  let readme = $state(true);
  let gitignore = $state<string | null>(null);
  let license = $state<string | null>(null);
  let commit = $state(true);
  let plan = $state<NewRepoPlan | null>(null);
  let planError = $state<string | null>(null);
  let running = $state(false);
  let failure = $state<string | null>(null);
  let nameInput = $state<HTMLInputElement>();

  // Publish to GitHub once the first commit is made; needs a sign-in.
  let owners = $state<GitHubOwners | null>(null);
  let publish = $state(false);
  let owner = $state("");
  let isPrivate = $state(true);
  /** The repository made here when publishing it failed: Open goes there. */
  let made = $state<string | null>(null);
  let phase = $state<"creating" | "publishing">("creating");
  api.githubAccount().then(
    (account) =>
      account &&
      api.githubOwners().then(
        (o) => ((owners = o), (owner = o.login)),
        () => (owners = { login: account.login, orgs: [] }),
      ),
    () => {},
  );

  const info = $derived(start.info);
  const home = $derived(info?.home ?? "");

  $effect(() => {
    if (!start.info) start.loadInfo();
    nameInput?.focus();
  });

  $effect(() => {
    if (!parent) parent = prefs.get("oxbow.clone.folder") || info?.projects || "";
  });
  $effect(() => {
    if (!branch && info) branch = info.defaultBranch || "main";
  });

  const path = $derived(parent && name.trim() ? join(parent, name.trim()) : "");
  const branches = $derived([...new Set(["main", "master", ...(info?.defaultBranch ? [info.defaultBranch] : [])])]);
  const options = $derived({ path, branch: branch || "main", readme, gitignore, license, commit });

  // Look at the folder again whenever something that changes the plan changes.
  $effect(() => {
    const o = options;
    if (!o.path) {
      plan = null;
      planError = null;
      return;
    }
    const timer = setTimeout(() => {
      api.planNewRepo(o).then(
        (p) => {
          if (options === o) {
            plan = p;
            planError = null;
          }
        },
        (err) => {
          if (options === o) {
            plan = null;
            planError = String(err);
          }
        },
      );
    }, 150);
    return () => clearTimeout(timer);
  });

  const hasFiles = $derived(!!plan && plan.entries > 0);
  const publishing = $derived(publish && commit && !!owners);
  const toPublish = $derived<Publish | null>(
    owners ? { owner: owner || owners.login, personal: (owner || owners.login) === owners.login, name: repositoryName(name), private: isPrivate, description: "", branch: options.branch } : null,
  );
  const nameOf = (key: string | null, table: [string, string][] | undefined) => table?.find(([k]) => k === key)?.[1] ?? "";

  async function choose() {
    const picked = await chooseFolder({ directory: true, multiple: false, title: "Choose a folder for the repository", defaultPath: parent || undefined });
    if (typeof picked !== "string") return;
    ({ parent, name } = split(picked));
  }

  const invalid = $derived(
    !name.trim() ? "Name the repository" : /[/\\]/.test(name.trim()) ? "The name can't have slashes" : planError ? planError : !plan ? "…" : null,
  );

  async function go() {
    if (running) return;
    if (plan?.repository || made) {
      onDone(plan?.repository ?? made!);
      return;
    }
    if (invalid) return;
    running = true;
    failure = null;
    phase = "creating";
    let path: string | null = null;
    try {
      path = await api.createRepo(options);
      if (publishing && toPublish) {
        phase = "publishing";
        await api.githubPublish(path, toPublish);
      }
      onDone(path);
    } catch (err) {
      if (path) {
        made = path;
        failure = `The repository is made here, but publishing to GitHub failed: ${String(err).trim()}`;
        return;
      }
      const message = String(err);
      failure = message.includes("failed: ") ? message.slice(message.indexOf("failed: ") + 8).replace(/^(fatal|error): /, "") : message;
    } finally {
      running = false;
    }
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape" && !running) {
      event.preventDefault();
      onClose();
    } else if (event.key === "Enter" && !(event.target instanceof HTMLButtonElement) && !(event.target instanceof HTMLSelectElement)) {
      event.preventDefault();
      go();
    }
  }

  /** The commands, with what Oxbow does itself between them as comments. */
  const lines = $derived.by<TermLine[]>(() => {
    if (!plan || plan.repository) return [];
    const shown = tilde(plan.path, home);
    const out: TermLine[] = [];
    const [init, ...rest] = plan.commands;
    if (plan.exists) {
      out.push({ kind: "cmd", text: `cd ${shown}` });
      out.push({ kind: "cmd", text: init.display });
      out.push({ kind: "comment", text: `makes the folder a repository, first branch ${options.branch}; its files stay as they are` });
    } else {
      out.push({ kind: "cmd", text: init.display.endsWith(" " + plan.path) ? init.display.slice(0, -plan.path.length) + shown : init.display });
      out.push({ kind: "comment", text: `creates the folder with an empty repository, first branch ${options.branch}` });
      out.push({ kind: "cmd", text: `cd ${shown}` });
    }
    if (plan.writes.length) {
      const what = plan.writes.map((f) => (f === ".gitignore" && gitignore ? `.gitignore (${gitignore === "macos" ? "macOS" : `${nameOf(gitignore, info?.gitignores)} + macOS`})` : f));
      out.push({ kind: "comment", text: `Oxbow writes ${what.join(", ")}` });
    }
    for (const command of rest) out.push({ kind: "cmd", text: command.display });
    if (publishing && toPublish) {
      out.push({ kind: "comment", text: publishNote(toPublish) });
      for (const text of publishCommands(toPublish)) out.push({ kind: "cmd", text });
    }
    return out;
  });

  const commitNote = $derived(
    !plan ? "" : plan.exists && plan.entries > 0 ? "everything in the folder" : `${plan.writes.length} ${plan.writes.length === 1 ? "file" : "files"}`,
  );
  const noIdentity = $derived(!!info && commit && (!info.name || !info.email));
</script>

<svelte:window onkeydown={onKey} />

<div class="dim">
  <div class="sheet" role="dialog" aria-modal="true" aria-labelledby="new-title" aria-busy={running}>
    <div class="head">
      <span class="title" id="new-title">New Repository</span>
      <span class="sub">Starts version control in a new folder, or in a folder you already have.</span>
    </div>

    <div class="grid">
      <span class="label">Name</span>
      <input class="text" bind:this={nameInput} bind:value={name} placeholder="my-project" aria-label="Name" spellcheck="false" autocomplete="off" />

      <span class="label">Where</span>
      <div class="row">
        <div class="where" title={path}>
          <svg class="icon" viewBox="0 0 16 16"><path d="M1.5 4.5a1 1 0 0 1 1-1H6l1.5 1.5h6a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" /></svg>
          <span class="clip"><span class="grey">{tilde(parent, home)}{parent.endsWith("/") || parent.endsWith("\\") ? "" : "/"}</span>{name.trim()}</span>
        </div>
        <button class="btn" onclick={choose}>Choose…</button>
      </div>

      {#if plan?.repository}
        <span></span>
        <div class="note warn">
          <svg viewBox="0 0 16 16"><path d="M8 2.5 14 13H2zM8 6.5v3M8 11.5v.1" /></svg>
          <span>{plan.repository === plan.path ? "This folder is a Git repository already." : `This folder is inside the repository ${tilde(plan.repository, home)}.`} Open it instead.</span>
        </div>
      {:else}
        <span class="label">Branch</span>
        <div class="row">
          <div class="seg" role="radiogroup" aria-label="First branch">
            {#each branches as b (b)}
              <button role="radio" aria-checked={branch === b} class:on={branch === b} onclick={() => (branch = b)}>{b}</button>
            {/each}
          </div>
          {#if info?.defaultBranch}<span class="hint">From Settings → Git</span>{/if}
        </div>

        <span class="label top">Start with</span>
        <div class="starts">
          <div class="row wrap">
            <button class="check" role="checkbox" aria-checked={readme && !hasFiles} disabled={hasFiles} onclick={() => (readme = !readme)} title={hasFiles ? "The folder has files already" : undefined}>
              <span class="box" class:on={readme && !hasFiles}><svg viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg></span>
              README.md
            </button>
            <label class="popup">
              <span class="grey">.gitignore</span>
              <select bind:value={gitignore} aria-label=".gitignore">
                <option value={null}>None</option>
                {#each info?.gitignores ?? [] as [key, label] (key)}<option value={key}>{label}</option>{/each}
              </select>
            </label>
            <label class="popup">
              <span class="grey">License</span>
              <select bind:value={license} aria-label="License">
                <option value={null}>None</option>
                {#each info?.licenses ?? [] as [key, label] (key)}<option value={key}>{label}</option>{/each}
              </select>
            </label>
          </div>
          {#if plan?.kept.length}
            <span class="hint">The folder keeps its own {plan.kept.join(" and ")}.</span>
          {/if}
          <button class="check" role="checkbox" aria-checked={commit} onclick={() => (commit = !commit)}>
            <span class="box" class:on={commit}><svg viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg></span>
            Make the first commit
            {#if commitNote}<span class="grey">· {commitNote}</span>{/if}
          </button>
          {#if noIdentity}
            <span class="hint warn-text">Commits need your name and email: save them on the Welcome window first.</span>
          {/if}
        </div>

        <span class="label top">GitHub</span>
        <div class="starts">
          {#if owners}
            <div class="row wrap">
              <button class="check" role="checkbox" aria-checked={publishing} disabled={!commit} onclick={() => (publish = !publish)} title={commit ? undefined : "Publishing needs the first commit"}>
                <span class="box" class:on={publishing}><svg viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg></span>
                Publish to GitHub
              </button>
              {#if owners.orgs.length}
                <label class="popup" class:off={!publishing}>
                  <span class="grey">Owner</span>
                  <select bind:value={owner} aria-label="Owner" disabled={!publishing}>
                    {#each [owners.login, ...owners.orgs] as o (o)}<option value={o}>{o}</option>{/each}
                  </select>
                </label>
              {/if}
              <div class="seg" class:off={!publishing} role="radiogroup" aria-label="Visibility">
                <button role="radio" aria-checked={isPrivate} class:on={isPrivate} disabled={!publishing} onclick={() => (isPrivate = true)}>Private</button>
                <button role="radio" aria-checked={!isPrivate} class:on={!isPrivate} disabled={!publishing} onclick={() => (isPrivate = false)}>Public</button>
              </div>
            </div>
            {#if publishing && toPublish && toPublish.name !== name.trim()}
              <span class="hint">On GitHub it is called {toPublish.name}.</span>
            {/if}
          {:else}
            <span class="hint">Sign in to GitHub in Settings › Accounts to publish from here. <button class="link" onclick={() => api.openSettings()}>Open Settings</button></span>
          {/if}
        </div>
      {/if}
    </div>

    {#if failure}<div class="failed" role="alert">{failure}</div>{/if}
    {#if planError && name.trim()}<div class="failed" role="alert">{planError}</div>{/if}

    {#if lines.length && prefs.get("oxbow.confirm.showCommand")}
      <TermBlock where="zsh" {lines} copy={lines.filter((l) => l.kind === "cmd").map((l) => l.text)} />
    {/if}

    <div class="foot">
      <span class="foot-note"
        >{publishing ? (isPrivate ? "Private on GitHub: only you can see it." : "Public on GitHub: anyone can see it.") : mac ? "Nothing leaves this Mac." : "Nothing leaves this computer."}</span
      >
      <button class="btn" onclick={onClose} disabled={running}>Cancel</button>
      <button class="btn go" onclick={go} disabled={running || (!plan?.repository && !made && !!invalid)} title={plan?.repository || made ? undefined : (invalid ?? undefined)}>
        {plan?.repository || made ? "Open" : running ? (phase === "publishing" ? "Publishing…" : "Creating…") : publishing ? "Create and Publish" : "Create"}
      </button>
    </div>
  </div>
</div>

<style>
  .dim {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: var(--dim);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 12px;
  }
  .sheet {
    width: min(560px, calc(100vw - 48px));
    max-height: calc(100vh - 36px);
    overflow-y: auto;
    padding: 18px 20px 16px;
    border-radius: 22px;
    background: var(--sheet);
    border: 0.5px solid var(--glass-border);
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.28);
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .head {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .title {
    font-size: 15px;
    font-weight: 700;
  }
  .sub {
    font-size: 12px;
    color: var(--text2);
  }
  .grid {
    display: grid;
    grid-template-columns: 74px 1fr;
    align-items: center;
    gap: 10px 10px;
  }
  .label {
    font-size: 12px;
    color: var(--text2);
    text-align: right;
  }
  .label.top {
    align-self: start;
    padding-top: 3px;
  }
  .text {
    height: 30px;
    padding: 0 10px;
    border-radius: 8px;
    background: var(--win);
    border: 1px solid var(--sep);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    outline: none;
    user-select: text;
    -webkit-user-select: text;
  }
  .text:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .row.wrap {
    flex-wrap: wrap;
    gap: 8px 18px;
  }
  .where {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 10px;
    border-radius: 8px;
    background: var(--field);
    border: 0.5px solid var(--glass-border);
  }
  .where .icon {
    color: var(--accent);
    flex-shrink: 0;
  }
  .clip {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .grey {
    color: var(--text2);
  }
  .off {
    opacity: 0.5;
  }
  .link {
    color: var(--accent);
    font-size: inherit;
  }
  .seg {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: 9px;
    background: var(--field);
  }
  .seg button {
    height: 24px;
    min-width: 72px;
    padding: 0 12px;
    border-radius: 7px;
    font-size: 12px;
  }
  .seg button.on {
    background: var(--glass);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
    font-weight: 600;
  }
  .hint {
    font-size: 11px;
    color: var(--text2);
  }
  .warn-text {
    color: var(--orange);
  }
  .starts {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    text-align: left;
  }
  .check:disabled {
    opacity: 0.45;
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
    flex-shrink: 0;
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
  .popup {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }
  .popup select {
    height: 24px;
    padding: 0 6px;
    border-radius: 7px;
    border: 0.5px solid var(--glass-border);
    background: var(--glass);
    box-shadow: var(--glass-shadow);
    color: var(--text);
    font: inherit;
    font-size: 12px;
  }
  .note {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    font-size: 12px;
    line-height: 1.4;
  }
  .note.warn {
    color: var(--orange);
  }
  .note svg {
    width: 13px;
    height: 13px;
    margin-top: 1px;
    flex-shrink: 0;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .failed {
    font-size: 12px;
    line-height: 1.4;
    color: var(--red);
    overflow-wrap: anywhere;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .foot-note {
    flex-grow: 1;
    font-size: 11px;
    color: var(--text2);
  }
  .btn {
    flex-shrink: 0;
    min-width: 84px;
    height: 30px;
    padding: 0 16px;
    border-radius: 15px;
    text-align: center;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    font-weight: 500;
  }
  .btn.go {
    background: var(--accent);
    border-color: transparent;
    color: #fff;
    font-weight: 600;
  }
  .btn:disabled {
    opacity: 0.5;
  }
</style>
