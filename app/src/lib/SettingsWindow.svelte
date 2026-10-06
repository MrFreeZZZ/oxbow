<script lang="ts">
  // The Settings window (⌘,): sections on the left, grouped rows of controls on the right, in
  // the layout of the Settings design. Every change is saved at once and reaches the main
  // window through prefs.

  import Menu, { menuIcons, type MenuEntry } from "./Menu.svelte";
  import { defaults, prefs, type PrefKey } from "./prefs.svelte";

  type Option = { value: string | number | boolean; label: string };
  type Row = {
    key: PrefKey;
    label: string;
    sub?: string;
    control: "seg" | "popup" | "switch";
    options?: Option[];
    /** Greyed out while another setting makes this one irrelevant. */
    off?: () => boolean;
  };
  type Group = { title: string; foot?: string; rows: Row[] };
  type Section = { id: string; label: string; color: string; icon: string; groups: Group[] };

  const opts = (...values: (string | number)[]): Option[] => values.map((value) => ({ value, label: String(value) }));
  const confirmOff = () => !prefs.get("oxbow.confirm.enabled");

  const sections: Section[] = [
    {
      id: "general",
      label: "General",
      color: "var(--section-grey)",
      icon: "M8 5.6a2.4 2.4 0 1 0 0 4.8a2.4 2.4 0 1 0 0-4.8M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4",
      groups: [
        {
          title: "Appearance",
          rows: [
            {
              key: "oxbow.appearance",
              label: "Appearance",
              control: "seg",
              options: [
                { value: "system", label: "System" },
                { value: "light", label: "Light" },
                { value: "dark", label: "Dark" },
              ],
            },
            {
              key: "oxbow.history.rowStyle",
              label: "History rows",
              sub: "Two lines show branch labels, author and time under the message",
              control: "seg",
              options: [
                { value: "twoLines", label: "Two lines" },
                { value: "compact", label: "Compact" },
              ],
            },
          ],
        },
        {
          title: "Startup",
          rows: [
            {
              key: "oxbow.startup.reopenRepository",
              label: "Reopen the repository from the last session",
              sub: "Otherwise Oxbow starts on the Welcome window",
              control: "switch",
            },
          ],
        },
        {
          title: "Confirmations",
          foot: "Read-only actions such as Fetch or Copy never ask.",
          rows: [
            {
              key: "oxbow.confirm.enabled",
              label: "Confirm actions before running them",
              sub: "Describes the action with branch and commit names before anything changes",
              control: "switch",
            },
            {
              key: "oxbow.confirm.scope",
              label: "Ask for",
              sub: "Risky: reset, drop, discard, delete, rebase, abort, force push",
              control: "seg",
              options: [
                { value: "all", label: "All actions" },
                { value: "risky", label: "Risky only" },
              ],
              off: confirmOff,
            },
            {
              key: "oxbow.confirm.showCommand",
              label: "Show the git command",
              sub: "A terminal block with the exact commands, to learn git as you go",
              control: "switch",
              off: confirmOff,
            },
          ],
        },
        {
          title: "Safety",
          rows: [
            {
              key: "oxbow.push.confirmForce",
              label: "Ask before force pushing",
              sub: "Oxbow always force pushes with --force-with-lease",
              control: "switch",
            },
          ],
        },
      ],
    },
    {
      id: "diff",
      label: "Diff & Text",
      color: "var(--section-green)",
      icon: "M4.5 2.5v5M2 5h5M9 11.5h5M2.5 13.5 13.5 2.5",
      groups: [
        {
          title: "Diff",
          rows: [
            {
              key: "oxbow.diff.view",
              label: "Show",
              sub: "The default for History, Changes and Stashes",
              control: "seg",
              options: [
                { value: "changes", label: "Changes only" },
                { value: "full", label: "Full file" },
              ],
            },
            {
              key: "oxbow.diff.contextLines",
              label: "Context lines",
              sub: "Unchanged lines kept around each change",
              control: "popup",
              options: opts(1, 3, 5, 10),
            },
            { key: "oxbow.diff.wordHighlight", label: "Highlight changed words", control: "switch" },
            {
              key: "oxbow.diff.ignoreWhitespace",
              label: "Ignore whitespace changes",
              sub: "In commits and stashes. Changes stays exact, so staging matches what you see",
              control: "switch",
            },
          ],
        },
        {
          title: "Text",
          rows: [
            { key: "oxbow.text.font", label: "Font", control: "popup", options: opts("SF Mono", "Menlo", "JetBrains Mono") },
            { key: "oxbow.text.fontSize", label: "Size", control: "popup", options: opts(11, 12, 13, 14) },
            { key: "oxbow.text.tabWidth", label: "Tab width", control: "popup", options: opts(2, 4, 8) },
          ],
        },
      ],
    },
  ];

  const mac = navigator.platform.startsWith("Mac");

  // Back and Forward walk the sections visited, like System Settings.
  let visited = $state(["general"]);
  let at = $state(0);
  const current = $derived(sections.find((s) => s.id === visited[at]) ?? sections[0]);

  function go(id: string) {
    query = "";
    if (id === visited[at]) return;
    visited = [...visited.slice(0, at + 1), id];
    at = visited.length - 1;
  }

  let query = $state("");
  const needle = $derived(query.trim().toLowerCase());

  function matches(section: Section, group: Group, row: Row): boolean {
    const text = [section.label, group.title, row.label, row.sub ?? "", row.key];
    return text.some((t) => t.toLowerCase().includes(needle));
  }

  /** Groups to show: the current section, or every row that matches the search. */
  const shown = $derived.by(() => {
    if (!needle) return current.groups.map((group) => ({ ...group, section: current }));
    return sections.flatMap((section) =>
      section.groups
        .map((group) => ({ ...group, title: `${section.label} · ${group.title}`, foot: undefined, section, rows: group.rows.filter((row) => matches(section, group, row)) }))
        .filter((group) => group.rows.length),
    );
  });
  const hits = $derived(new Set(shown.map((group) => group.section.id)));

  function pick(row: Row, value: string) {
    const option = row.options?.find((o) => String(o.value) === value);
    if (option) prefs.set(row.key, option.value as never);
  }

  // The gear beside a row, or a right click on it.
  let menu = $state<{ x: number; y: number; row: Row } | null>(null);
  let menuFor = $state<PrefKey | null>(null);

  function openMenu(row: Row, x: number, y: number) {
    menuFor = row.key;
    menu = { x, y, row };
  }

  function menuEntries(row: Row): MenuEntry[] {
    const copy = (text: string) => () => navigator.clipboard.writeText(text).catch(() => {});
    const entries: MenuEntry[] = [
      { kind: "item", label: "Copy Setting ID", icon: menuIcons.copy, run: copy(row.key) },
      { kind: "item", label: "Copy Setting as JSON", icon: menuIcons.copy, run: copy(`"${row.key}": ${JSON.stringify(prefs.get(row.key))}`) },
    ];
    if (prefs.changed(row.key)) {
      const fallback = row.options?.find((o) => o.value === defaults[row.key])?.label ?? (defaults[row.key] ? "On" : "Off");
      entries.push({ kind: "sep" }, { kind: "item", label: `Reset to Default (${fallback})`, icon: menuIcons.reset, run: () => prefs.reset(row.key) });
    }
    return entries;
  }

  // Diff & Text preview: a small change drawn with the same code styles as the real diffs.
  type Line = { kind: "ctx" | "add" | "del" | "fold"; old?: number; new?: number; before: string; word?: string; after?: string };
  const preview = $derived.by((): Line[] => {
    const full = prefs.get("oxbow.diff.view") === "full";
    const context = full ? 3 : Math.min(3, prefs.get("oxbow.diff.contextLines"));
    const above: [number, string][] = [
      [19, "\tfn default() -> Self {"],
      [20, "\t\tSelf {"],
      [21, "\t\t\trequest_timeout: Duration::from_secs(10),"],
    ];
    const below: [number, string][] = [
      [23, "\t\t\tretries: 2,"],
      [24, "\t\t}"],
      [25, "\t}"],
    ];
    const ctx = ([n, text]: [number, string]): Line => ({ kind: "ctx", old: n, new: n, before: text });
    const fold = (n: number): Line => ({ kind: "fold", before: `⋯  ${n} unchanged lines` });
    return [
      ...(full ? [] : [fold(18 + 3 - context)]),
      ...above.slice(3 - context).map(ctx),
      { kind: "del", old: 22, before: "\t\t\tlogin_timeout: Duration::from_secs(", word: "10", after: ")," },
      { kind: "add", new: 22, before: "\t\t\tlogin_timeout: Duration::from_secs(", word: "30", after: ")," },
      ...below.slice(0, context).map(ctx),
      ...(full ? [] : [fold(9 + 3 - context)]),
    ];
  });
</script>

<div class="settings" class:mac>
  <nav aria-label="Settings sections">
    <div class="lights" data-tauri-drag-region></div>
    <label class="search">
      <svg class="icon small" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5" /><path d="M10.5 10.5 14 14" /></svg>
      <!-- svelte-ignore a11y_autofocus -->
      <input type="search" aria-label="Search settings" placeholder="Search" bind:value={query} spellcheck="false" autofocus />
    </label>
    {#each sections as section (section.id)}
      <button
        class="section"
        class:current={!needle && section.id === current.id}
        class:miss={needle && !hits.has(section.id)}
        aria-current={!needle && section.id === current.id ? "page" : undefined}
        onclick={() => go(section.id)}
      >
        <span class="badge" style:background={section.color}>
          <svg class="icon" viewBox="0 0 16 16"><path d={section.icon} /></svg>
        </span>
        <span class="label">{section.label}</span>
      </button>
    {/each}
  </nav>

  <div class="content">
    <header data-tauri-drag-region>
      <div class="nav-buttons">
        <button aria-label="Back" disabled={at === 0 || !!needle} onclick={() => (at -= 1)}>
          <svg class="icon small" viewBox="0 0 16 16"><path d="M10 3.5 5.5 8l4.5 4.5" /></svg>
        </button>
        <button aria-label="Forward" disabled={at === visited.length - 1 || !!needle} onclick={() => (at += 1)}>
          <svg class="icon small" viewBox="0 0 16 16"><path d="M6 3.5 10.5 8 6 12.5" /></svg>
        </button>
      </div>
      <h1 data-tauri-drag-region>{needle ? "Search" : current.label}</h1>
    </header>

    <div class="scroll">
      {#each shown as group (group.title)}
        <section>
          <h2>{group.title}</h2>
          <div class="panel">
            {#each group.rows as row (row.key)}
              {@const off = row.off?.()}
              <div
                class="row"
                role="group"
                aria-label={row.label}
                oncontextmenu={(event) => {
                  event.preventDefault();
                  openMenu(row, event.clientX, event.clientY);
                }}
              >
                <button
                  class="gear"
                  class:on={menuFor === row.key && menu}
                  aria-label="More actions for {row.label}"
                  title="More actions"
                  onclick={(event) => {
                    const box = event.currentTarget.getBoundingClientRect();
                    openMenu(row, box.left, box.bottom + 4);
                  }}
                >
                  <svg class="icon" viewBox="0 0 16 16"><path d="M8 5.6a2.4 2.4 0 1 0 0 4.8a2.4 2.4 0 1 0 0-4.8M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4" /></svg>
                </button>
                <span class="text" class:off>
                  <span class="name">{row.label}</span>
                  {#if row.sub}<span class="sub">{row.sub}</span>{/if}
                </span>
                {#if row.control === "seg"}
                  <span class="seg" class:off role="radiogroup" aria-label={row.label}>
                    {#each row.options ?? [] as option (option.value)}
                      <button
                        role="radio"
                        aria-checked={prefs.get(row.key) === option.value}
                        disabled={off}
                        onclick={() => prefs.set(row.key, option.value as never)}>{option.label}</button
                      >
                    {/each}
                  </span>
                {:else if row.control === "popup"}
                  <span class="popup" class:off>
                    <select
                      aria-label={row.label}
                      disabled={off}
                      value={String(prefs.get(row.key))}
                      onchange={(event) => pick(row, event.currentTarget.value)}
                    >
                      {#each row.options ?? [] as option (option.value)}
                        <option value={String(option.value)}>{option.label}</option>
                      {/each}
                    </select>
                    <svg class="icon" viewBox="0 0 16 16"><path d="M5 6.5 8 3.5l3 3M5 9.5l3 3 3-3" /></svg>
                  </span>
                {:else}
                  <button
                    class="switch"
                    class:off
                    role="switch"
                    aria-checked={!!prefs.get(row.key)}
                    aria-label={row.label}
                    disabled={off}
                    onclick={() => prefs.set(row.key, !prefs.get(row.key) as never)}
                  >
                    <span></span>
                  </button>
                {/if}
              </div>
            {/each}
          </div>
          {#if group.foot}<p class="foot">{group.foot}</p>{/if}
        </section>
      {:else}
        <p class="empty">No settings match “{query.trim()}”.</p>
      {/each}

      {#if !needle && current.id === "diff"}
        <section>
          <h2>Preview</h2>
          <div class="preview" class:words={prefs.get("oxbow.diff.wordHighlight")}>
            {#each preview as line, i (i)}
              <div class="line {line.kind}" class:first={line.kind !== preview[i - 1]?.kind} class:last={line.kind !== preview[i + 1]?.kind}>
                {#if line.kind === "fold"}
                  <span class="fold">{line.before}</span>
                {:else}
                  <span class="n">{line.old ?? ""}</span>
                  <span class="n">{line.new ?? ""}</span>
                  <span class="code">{line.before}{#if line.word}<span class="word">{line.word}</span>{line.after}{/if}</span>
                {/if}
              </div>
            {/each}
          </div>
        </section>
      {/if}
    </div>
  </div>
</div>

{#if menu}
  <Menu
    x={menu.x}
    y={menu.y}
    label="Setting {menu.row.label}"
    entries={menuEntries(menu.row)}
    onClose={() => {
      menu = null;
      menuFor = null;
    }}
  />
{/if}

<style>
  .settings {
    --section-grey: #8e8e93;
    --section-green: #6aae80;
    position: relative;
    height: 100%;
    display: flex;
    background: var(--win);
  }

  :global(:root[data-theme="dark"]) .settings {
    --section-grey: #7c7c82;
    --section-green: #73be8c;
  }

  .icon.small {
    width: 14px;
    height: 14px;
  }

  nav {
    margin: 8px 0 8px 8px;
    width: 222px;
    flex-shrink: 0;
    border-radius: 18px;
    background: var(--side);
    border: 0.5px solid var(--side-border);
    box-shadow: var(--panel-shadow);
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow: hidden;
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
    border-radius: 15px;
    background: var(--field);
    color: var(--text2);
  }

  .search input {
    flex-grow: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    font: inherit;
    color: var(--text);
  }

  .search input::-webkit-search-cancel-button {
    -webkit-appearance: none;
  }

  .section {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 32px;
    margin: 0 8px;
    padding: 0 8px;
    border-radius: 9px;
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
    border-radius: 6px;
    color: #ffffff;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .badge .icon {
    width: 14px;
    height: 14px;
    stroke-width: 1.7;
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

  .nav-buttons {
    display: flex;
    align-items: center;
    height: 30px;
    padding: 0 2px;
    border-radius: 15px;
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
    border-radius: 15px;
  }

  .nav-buttons button:disabled {
    opacity: 0.35;
  }

  h1 {
    margin: 0;
    font-size: 15px;
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
    font-size: 13px;
    font-weight: 600;
  }

  .panel {
    border-radius: 12px;
    background: var(--panel);
    border: 0.5px solid var(--panel-border);
  }

  .row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 44px;
    padding: 7px 12px 7px 14px;
  }

  .row + .row {
    border-top: 0.5px solid var(--sep);
  }

  .gear {
    position: absolute;
    left: -27px;
    top: 50%;
    margin-top: -10px;
    width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 10px;
    color: var(--text2);
    opacity: 0;
  }

  .gear .icon {
    width: 13px;
    height: 13px;
  }

  .row:hover .gear,
  .gear.on,
  .gear:focus-visible {
    opacity: 1;
  }

  .gear.on {
    background: var(--field);
  }

  .text {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    line-height: 1.3;
  }

  .name,
  .sub {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .sub {
    font-size: 11px;
    color: var(--text2);
  }

  .off {
    opacity: 0.4;
  }

  .seg {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: 9px;
    background: var(--field);
    flex-shrink: 0;
  }

  .seg button {
    min-width: 72px;
    height: 24px;
    padding: 0 12px;
    border-radius: 7px;
    text-align: center;
    font-size: 12px;
  }

  .seg button[aria-checked="true"] {
    background: var(--seg-on);
    box-shadow: var(--seg-shadow);
    font-weight: 600;
  }

  .popup {
    position: relative;
    flex-shrink: 0;
    display: flex;
    align-items: center;
  }

  .popup select {
    -webkit-appearance: none;
    appearance: none;
    height: 26px;
    padding: 0 26px 0 11px;
    border-radius: 8px;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--ctrl-shadow);
    color: var(--text);
    font: inherit;
    font-size: 12px;
    outline: 0;
  }

  .popup select:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .popup .icon {
    position: absolute;
    right: 7px;
    width: 12px;
    height: 12px;
    color: var(--text2);
    pointer-events: none;
  }

  .switch {
    width: 36px;
    height: 20px;
    border-radius: 10px;
    background: var(--switch-off);
    position: relative;
    flex-shrink: 0;
    transition: background 0.15s;
  }

  .switch span {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 8px;
    background: #ffffff;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.25);
    transition: left 0.15s;
  }

  .switch[aria-checked="true"] {
    background: var(--accent);
  }

  .switch[aria-checked="true"] span {
    left: 18px;
  }

  .foot,
  .empty {
    margin: 0;
    padding: 0 12px;
    font-size: 11px;
    line-height: 1.4;
    color: var(--text2);
  }

  .empty {
    padding-top: 8px;
    font-size: 13px;
  }

  .preview {
    border-radius: 12px;
    background: var(--win);
    border: 0.5px solid var(--panel-border);
    padding: 6px 0 8px;
    font-family: var(--code-font);
    font-size: var(--code-size);
    tab-size: var(--tab);
    color: var(--code);
    overflow: hidden;
  }

  .line {
    display: flex;
    align-items: center;
    min-height: var(--code-line);
    margin: 0 8px;
  }

  .line.first {
    margin-top: 2px;
  }

  .line.add {
    background: var(--add);
  }

  .line.del {
    background: var(--del);
  }

  .line.add.first,
  .line.del.first {
    border-top-left-radius: 9px;
    border-top-right-radius: 9px;
  }

  .line.add.last,
  .line.del.last {
    border-bottom-left-radius: 9px;
    border-bottom-right-radius: 9px;
  }

  .line.del + .line.add {
    margin-top: 3px;
  }

  .line.fold {
    margin-top: 4px;
    border-radius: 9px;
    background: var(--field);
    color: var(--text2);
  }

  .line.fold:first-child {
    margin-top: 2px;
  }

  .fold {
    padding-left: 72px;
  }

  .n {
    width: 30px;
    text-align: right;
    color: var(--text2);
    opacity: 0.7;
    flex-shrink: 0;
  }

  .n + .n {
    padding-right: 12px;
    width: 42px;
  }

  .code {
    white-space: pre;
  }

  .words .add .word {
    background: var(--add-word);
  }

  .words .del .word {
    background: var(--del-word);
  }

  .words .word {
    border-radius: 5px;
    padding: 0 2px;
  }
</style>
