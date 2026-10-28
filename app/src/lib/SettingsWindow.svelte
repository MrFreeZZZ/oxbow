<script lang="ts">
  // The Settings window (⌘,): sections on the left, grouped rows of controls on the right, in
  // the layout of the Settings design. Oxbow's own settings save at once to settings.json and
  // reach the main window through prefs; Git's own (identity, pull, default branch…) are written
  // to ~/.gitconfig or .git/config with `git config`, so a terminal sees the same. The sections
  // are in settingsSections.ts, settingsGit.ts and settingsKeys.svelte.ts, a row's control in
  // SettingsRow.svelte.

  import { listen } from "@tauri-apps/api/event";
  import { api } from "./api";
  import ConfirmSheet from "./ConfirmSheet.svelte";
  import { confirm } from "./confirm.svelte";
  import BranchPreview from "./BranchPreview.svelte";
  import DiffPreview from "./DiffPreview.svelte";
  import GitHubSignIn from "./GitHubSignIn.svelte";
  import GitLabSignIn from "./GitLabSignIn.svelte";
  import Menu, { menuIcons, type MenuEntry } from "./Menu.svelte";
  import { defaults, prefs, type PrefKey } from "./prefs.svelte";
  import { accountsSection, gitSection, repoSection } from "./settingsGit";
  import SettingsJson from "./SettingsJson.svelte";
  import { keysSection } from "./settingsKeys.svelte";
  import { config, copy, load, model } from "./settingsModel.svelte";
  import SettingsRow from "./SettingsRow.svelte";
  import { text, type Group, type Row, type Section } from "./settingsRows";
  import { diffSection, generalSection, integrationsSection, themeSection } from "./settingsSections";
  import ShortcutSearch from "./ShortcutSearch.svelte";
  import { shortcutEdit } from "./shortcutEdit.svelte";
  import ThemeCards from "./ThemeCards.svelte";
  import ThemeSample from "./ThemeSample.svelte";

  api.monospaceFonts().then((list) => (model.fonts = list), () => {});
  load();
  listen("repo-changed", () => load()).catch(() => {});
  listen("account-changed", () => load()).catch(() => {});
  listen("trust-changed", () => load()).catch(() => {});

  const sections: Section[] = [generalSection, themeSection, accountsSection, gitSection, diffSection, keysSection, integrationsSection, repoSection];

  const shownSections = $derived(sections.filter((s) => s.id !== "repo" || model.repo));
  const labelOf = (section: Section) => (section.id === "repo" ? (model.repo?.name ?? "") : section.label);

  const mac = navigator.platform.startsWith("Mac");

  // Back and Forward walk the sections visited, like System Settings.
  let visited = $state(["general"]);
  let at = $state(0);
  const current = $derived(shownSections.find((s) => s.id === visited[at]) ?? shownSections[0]);

  function go(id: string) {
    model.query = "";
    json = false;
    shortcutEdit.cancel();
    if (id === visited[at]) return;
    visited = [...visited.slice(0, at + 1), id];
    at = visited.length - 1;
  }

  const needle = $derived(model.query.trim().toLowerCase());

  function matches(section: Section, group: Group, row: Row): boolean {
    const words = [labelOf(section), group.title ?? "", text(row.label), text(row.sub), row.id];
    return words.some((t) => t.toLowerCase().includes(needle));
  }

  /** Groups to show: the current section, or every row that matches the search. */
  const shown = $derived.by(() => {
    if (!needle) return current.groups().map((group, i) => ({ ...group, key: `${current.id}:${i}`, section: current }));
    return shownSections.flatMap((section) =>
      section
        .groups()
        .map((group, i) => ({
          ...group,
          key: `${section.id}:${i}`,
          title: group.title ? `${labelOf(section)} · ${group.title}` : labelOf(section),
          foot: undefined,
          section,
          rows: group.rows.filter((row) => matches(section, group, row)),
        }))
        .filter((group) => group.rows.length),
    );
  });
  const hits = $derived(new Set(shown.map((group) => group.section.id)));

  // A text value being edited.
  let editing = $state<{ id: string; value: string } | null>(null);

  // The gear beside a row, or a right click on it.
  let menu = $state<{ x: number; y: number; row: Row } | null>(null);

  function openMenu(row: Row, x: number, y: number) {
    if (!row.setting && !row.more) return;
    menu = { x, y, row };
  }

  function menuEntries(row: Row): MenuEntry[] {
    const entries: MenuEntry[] = [];
    const setting = row.setting;
    if (setting?.kind === "pref") {
      const key = setting.key;
      entries.push(
        { kind: "item", label: "Copy Setting ID", icon: menuIcons.copy, run: () => copy(key, key) },
        { kind: "item", label: "Copy Setting as JSON", icon: menuIcons.copy, run: () => copy(key, `"${key}": ${JSON.stringify(prefs.get(key))}`) },
        { kind: "item", label: "Edit in settings.json", icon: menuIcons.edit, run: openJson },
      );
      if (prefs.changed(key)) {
        const control = row.control;
        const fallback =
          control && (control.type === "seg" || control.type === "popup")
            ? (control.options().find((o) => o.value === defaults[key])?.label ?? String(defaults[key]))
            : typeof defaults[key] === "boolean"
              ? defaults[key]
                ? "On"
                : "Off"
              : String(defaults[key]) || "automatic";
        entries.push({ kind: "sep" }, { kind: "item", label: `Reset to Default (${fallback})`, icon: menuIcons.reset, run: () => prefs.reset(key) });
      }
    } else if (setting?.kind === "git") {
      const commands = setting.commands();
      entries.push(
        { kind: "note", label: `${setting.scope === "global" ? "Git’s own setting, in ~/.gitconfig" : "Git’s own setting, in .git/config"}:` },
        ...commands.map((line): MenuEntry => ({ kind: "note", label: `$ ${line}` })),
        { kind: "item", label: commands.length > 1 ? "Copy Commands" : "Copy Command", icon: menuIcons.copy, run: () => copy(row.id, commands.join("\n")) },
      );
      if (setting.keys.some((key) => config(setting.scope, key) !== undefined)) {
        entries.push({ kind: "sep" }, { kind: "item", label: "Reset to Git’s Default", icon: menuIcons.reset, run: () => setting.reset() });
      }
    }
    const more = row.more?.() ?? [];
    if (more.length && entries.length) entries.push({ kind: "sep" });
    return [...entries, ...more];
  }


  // settings.json as text, like VS Code.
  let json = $state(false);
  let jsonText = $state("");
  let jsonSaved = $state("");

  async function openJson() {
    menu = null;
    jsonText = jsonSaved = await api.settingsText().catch(() => "{}\n");
    json = true;
  }

  /** What the Settings window calls an Oxbow setting, for the note when it couldn't be saved. */
  function prefLabel(key: PrefKey): string | null {
    for (const section of sections)
      for (const group of section.groups())
        for (const row of group.rows) if (row.setting?.kind === "pref" && row.setting.key === key) return text(row.label);
    return null;
  }
  const failedLabel = $derived(prefs.failure ? prefLabel(prefs.failure.key) : null);

  const revealLabel = mac ? "Reveal in Finder" : navigator.platform.startsWith("Win") ? "Show in Explorer" : "Open Containing Folder";

  function reveal() {
    api.revealSettings().catch((err) => confirm.fail(err));
  }

  /** A path typed into a shell, with ~ left unquoted. */
  const shellPath = (path: string) => path.replace(/[^\w@%+=:,./~-]/g, "\\$&");

  /** Whether there is anything to restore; a file Oxbow can't read counts too. */
  // settings.json that can't be read can still be moved aside.
  const customized = $derived(prefs.customized || !!prefs.unreadable || (json && jsonSaved.trim() !== "{}"));

  async function restoreDefaults() {
    const where = await api.settingsLocation().catch(() => ({ file: "settings.json", backup: "settings.backup.json" }));
    const reopen = async () => {
      if (json) jsonText = jsonSaved = await api.settingsText().catch(() => "{}\n");
      await prefs.reload();
    };
    await confirm.run({
      title: "Restore the default settings?",
      body: [
        "Every Oxbow setting goes back to its default. Git’s own settings (identity, pull, default branch, signing) stay as they are in ~/.gitconfig and .git/config. The current file is kept as ",
        { code: "settings.backup.json" },
        ".",
      ],
      icon: "reset",
      button: "Restore Defaults",
      danger: true,
      status: "Restoring the default settings…",
      local: {
        commands: [`mv ${shellPath(where.file)} ${shellPath(where.backup)}`],
        comment: "Oxbow does this itself; no git command runs",
        run: async () => {
          await api.restoreDefaultSettings();
          await reopen();
          return "Settings are back to their defaults.";
        },
      },
      undo: () => ({
        title: "Put back the settings you had?",
        body: ["The settings from before Restore Defaults come back from ", { code: "settings.backup.json" }, "."],
        icon: "undo",
        button: "Put Back",
        status: "Putting the settings back…",
        local: {
          commands: [`mv ${shellPath(where.backup)} ${shellPath(where.file)}`],
          comment: "Oxbow does this itself; no git command runs",
          run: async () => {
            await api.restoreSettingsBackup();
            await reopen();
            return "Your settings are back.";
          },
        },
      }),
    });
  }
</script>

<svelte:window onfocus={load} />

<div class="settings" class:mac>
  <nav aria-label="Settings sections">
    <div class="lights" data-tauri-drag-region></div>
    <label class="search">
      <svg class="icon small" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5" /><path d="M10.5 10.5 14 14" /></svg>
      <!-- svelte-ignore a11y_autofocus -->
      <input type="search" aria-label="Search settings" placeholder="Search" bind:value={model.query} spellcheck="false" autofocus />
    </label>
    {#each shownSections as section (section.id)}
      {#if section.head}<div class="head">{section.head}</div>{/if}
      <button
        class="section"
        class:current={!needle && !json && section.id === current.id}
        class:miss={needle && !hits.has(section.id)}
        aria-current={!needle && section.id === current.id ? "page" : undefined}
        onclick={() => go(section.id)}
      >
        <span class="badge" style:background={section.color}>
          <svg class="icon" viewBox="0 0 16 16"><path d={section.icon} /></svg>
        </span>
        <span class="label">{labelOf(section)}</span>
      </button>
    {/each}
  </nav>

  <div class="content">
    <header data-tauri-drag-region>
      <div class="nav-buttons">
        <button aria-label="Back" disabled={at === 0 || !!needle || json} onclick={() => (at -= 1)}>
          <svg class="icon small" viewBox="0 0 16 16"><path d="M10 3.5 5.5 8l4.5 4.5" /></svg>
        </button>
        <button aria-label="Forward" disabled={at === visited.length - 1 || !!needle || json} onclick={() => (at += 1)}>
          <svg class="icon small" viewBox="0 0 16 16"><path d="M6 3.5 10.5 8 6 12.5" /></svg>
        </button>
      </div>
      <h1 data-tauri-drag-region>{json ? "settings.json" : needle ? "Search" : labelOf(current)}</h1>
      {#if json}
        <button class="folder" onclick={reveal} title={revealLabel} aria-label={revealLabel}>
          <svg class="icon small" viewBox="0 0 16 16"><path d={menuIcons.reveal} /></svg>
        </button>
      {/if}
      <span class="spacer" data-tauri-drag-region></span>
      <button class="button pill" onclick={restoreDefaults} disabled={!customized} title={customized ? "Put every Oxbow setting back to its default. Git’s own settings stay as they are" : "Every Oxbow setting is at its default"}>
        <svg class="icon small" viewBox="0 0 16 16"><path d={menuIcons.reset} /></svg>Restore defaults
      </button>
      {#if json}
        <button class="button pill" onclick={() => (json = false)} title="Back to the Settings window">
          <svg class="icon small" viewBox="0 0 16 16"><path d="M2.5 4.5h11M2.5 8h11M2.5 11.5h11" /></svg>Open Settings UI
        </button>
      {:else}
        <button class="button pill" onclick={openJson} title="The same settings as a JSON file">
          <svg class="icon small" viewBox="0 0 16 16"
            ><path
              d="M5.5 2.5c-1.6 0-2 .7-2 2v1.6c0 .9-.5 1.5-1.5 1.9 1 .4 1.5 1 1.5 1.9v1.6c0 1.3.4 2 2 2M10.5 2.5c1.6 0 2 .7 2 2v1.6c0 .9.5 1.5 1.5 1.9-1 .4-1.5 1-1.5 1.9v1.6c0 1.3-.4 2-2 2"
            /></svg
          >Edit in settings.json
        </button>
      {/if}
    </header>

    {#if json}
      <SettingsJson bind:text={jsonText} bind:saved={jsonSaved} />
    {:else}
      <div class="scroll">
        {#if model.problem}<p class="problem" role="alert">{model.problem}</p>{/if}
        {#if prefs.failure}
          <p class="problem with-actions" role="alert">
            <span class="grow">Couldn’t save {failedLabel ? `“${failedLabel}”` : prefs.failure.key}, so it is back to how it was. {prefs.failure.error}</span>
            <button class="button small" onclick={() => prefs.retry()}>Try Again</button>
            <button class="close" onclick={() => (prefs.failure = null)} aria-label="Close" title="Close">
              <svg class="icon small" viewBox="0 0 16 16"><path d="M4 4l8 8M12 4l-8 8" /></svg>
            </button>
          </p>
        {:else if prefs.unreadable}
          <p class="problem with-actions" role="alert">
            <span class="grow">Oxbow is using the default settings. {prefs.unreadable}</span>
            <button class="button small" onclick={openJson}>Edit in settings.json</button>
          </p>
        {/if}

        {#if !needle && current.id === "keys"}
          <ShortcutSearch empty={!shown.length} />
        {/if}
        {#each shown as group (group.key)}
          <section>
            {#if group.title}<h2>{group.title}</h2>{/if}
            {#if !needle && group.key === "theme:0"}
              <ThemeSample />
            {:else if !needle && group.key === "theme:1"}
              <BranchPreview colors={prefs.get("oxbow.branchPalette") === "oxbow" ? 9 : 7} />
            {/if}
            <div class="panel">
              {#each group.rows as row (row.id)}
                <SettingsRow {row} menuOn={menu?.row.id === row.id} onMenu={(x, y) => openMenu(row, x, y)} bind:editing />
              {/each}
            </div>
            {#if text(group.foot)}<p class="foot">{text(group.foot)}</p>{/if}
          </section>
          {#if !needle && group.key === "diff:1"}
            <section>
              <h2>Preview</h2>
              <DiffPreview />
            </section>
          {/if}
          {#if !needle && group.key === "diff:2"}
            <section>
              <h2>Preview</h2>
              <div class="term-preview" aria-label="Git commands">
                <div><span class="dim">%</span> <span class="git">git</span> push origin main</div>
                <div class="dim">To github.com:acme/api.git</div>
                <div class="dim">{"   3f2a1c9..7c1e9a2  main -> main"}</div>
              </div>
            </section>
          {/if}
          {#if !needle && group.key === "theme:0"}
            <section>
              <div class="title-line">
                <h2>Popular themes</h2>
                <span class="sub">by VS Code Marketplace installs</span>
              </div>
              <ThemeCards />
              <p class="foot">Themes color code: diffs, conflicts and stashes. The window keeps its own look.</p>
            </section>
          {/if}
        {:else}
          {#if needle}<p class="empty">No settings match “{model.query.trim()}”.</p>{/if}
        {/each}
      </div>
    {/if}
  </div>
</div>

{#if menu}
  <Menu x={menu.x} y={menu.y} label="Setting {text(menu.row.label)}" entries={menuEntries(menu.row)} onClose={() => (menu = null)} />
{/if}

<ConfirmSheet repo={model.repo?.name ?? ""} branch={null} color={0} />
{#if model.signIn}
  <GitHubSignIn
    tokenFirst={model.signIn.tokenFirst}
    onClose={() => (model.signIn = null)}
    onDone={(signedIn) => {
      model.account = signedIn;
      model.signIn = null;
    }}
  />
{/if}
{#if model.gitlabSignIn}
  <GitLabSignIn
    server={model.gitlabSignIn.server}
    onClose={() => (model.gitlabSignIn = null)}
    onDone={() => {
      model.gitlabSignIn = null;
      load();
    }}
  />
{/if}

<style>
  .settings {
    --section-grey: #8e8e93;
    --section-green: #6aae80;
    --section-pink: #d58aa5;
    --section-orange: #d99a62;
    --section-purple: #a77bc9;
    --section-teal: #5baeb6;
    --section-blue: #6f8fae;
    position: relative;
    height: 100%;
    display: flex;
    background: var(--win);
  }

  :global(:root[data-theme="dark"]) .settings {
    --section-grey: #7c7c82;
    --section-green: #73be8c;
    --section-pink: #db8aa6;
    --section-orange: #d9965a;
    --section-purple: #b48acf;
    --section-teal: #69b9c4;
  }

  .icon.small {
    width: 14px;
    height: 14px;
  }

  nav {
    margin: 8px 0 8px 8px;
    width: 222px;
    flex-shrink: 0;
    border-radius: var(--radius-xl);
    background: var(--side);
    border: 0.5px solid var(--side-border);
    box-shadow: var(--panel-shadow);
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow: hidden auto;
  }

  /* Room for the traffic lights, which macOS draws over the sidebar. */
  .lights {
    height: 10px;
    flex-shrink: 0;
  }

  .mac .lights {
    height: 44px;
  }

  .search {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    margin: 0 10px 8px;
    padding: 0 10px;
    border-radius: var(--radius-pill);
    background: var(--field);
    color: var(--text2);
    flex-shrink: 0;
  }
  .search:focus-within {
    box-shadow: var(--focus-ring);
  }

  .search input {
    flex-grow: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    font: inherit;
    color: var(--text);
    box-shadow: none;
  }

  .head {
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
    padding: 14px 18px 4px;
  }

  .section {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 32px;
    margin: 0 8px;
    padding: 0 8px;
    border-radius: var(--radius-m);
    flex-shrink: 0;
  }

  .section.current {
    background: var(--side-sel);
    font-weight: 600;
  }

  .section.miss {
    opacity: 0.4;
  }

  .badge {
    width: 22px;
    height: 22px;
    border-radius: var(--radius-s);
    color: #ffffff;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .badge .icon {
    width: 14px;
    height: 14px;
    stroke-width: 1.5;
  }

  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .content {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  header {
    height: 52px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 20px 0 18px;
    flex-shrink: 0;
  }

  .spacer {
    flex-grow: 1;
    align-self: stretch;
  }

  .nav-buttons {
    display: flex;
    align-items: center;
    height: 30px;
    padding: 0 2px;
    border-radius: var(--radius-pill);
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    color: var(--icon);
  }

  .nav-buttons button {
    width: 30px;
    height: 30px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-pill);
  }

  .nav-buttons button:disabled {
    opacity: 0.35;
  }

  .pill {
    gap: 7px;
    padding: 0 14px 0 11px;
  }

  .pill .icon {
    color: var(--icon);
  }

  h1 {
    margin: 0;
    font-size: var(--text-heading);
    font-weight: 700;
    white-space: nowrap;
  }

  .scroll {
    flex-grow: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 6px 30px 20px 28px;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  section {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  h2 {
    margin: 0;
    padding: 0 12px;
    font-size: var(--text-body);
    font-weight: 600;
  }

  .title-line {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .title-line h2 {
    flex-grow: 1;
  }

  .title-line .sub {
    padding-right: 12px;
  }

  .panel {
    border-radius: var(--radius-l);
    background: var(--panel);
    border: 0.5px solid var(--panel-border);
  }

  .sub {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: var(--text-caption);
    color: var(--text2);
  }

  .button.small {
    min-width: 72px;
  }

  .foot,
  .empty {
    margin: 0;
    padding: 0 12px;
    font-size: var(--text-caption);
    line-height: 1.4;
    color: var(--text2);
  }

  .empty {
    padding-top: 8px;
    font-size: var(--text-body);
  }


  .problem {
    margin: 0;
    padding: 8px 12px;
    border-radius: var(--radius-l);
    background: var(--danger-soft);
    color: var(--danger);
    font-size: var(--text-small);
  }
  .problem.with-actions {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .problem .grow {
    flex-grow: 1;
    min-width: 0;
  }
  .problem .close {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: var(--radius-s);
    color: inherit;
  }

  .folder {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: var(--radius-pill);
    color: var(--icon);
  }

  .folder:hover {
    background: var(--field);
  }


  /* Diff & Text preview */
  .term-preview {
    border-radius: var(--radius-l);
    background: var(--term);
    border: 0.5px solid var(--term-border);
    padding: 10px 14px;
    color: var(--term-text);
    font-family: var(--term-font);
    font-size: var(--term-size);
    line-height: var(--term-line);
  }
  .term-preview div {
    white-space: pre-wrap;
  }
  .term-preview .dim {
    color: var(--term-dim);
  }
  .term-preview .git {
    color: var(--term-git);
    font-weight: 600;
  }
</style>
