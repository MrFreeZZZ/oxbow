<script lang="ts">
  import { keys } from "./keys";
  // The Settings window (⌘,): sections on the left, grouped rows of controls on the right, in
  // the layout of the Settings design. Oxbow's own settings save at once to settings.json and
  // reach the main window through prefs; Git's own (identity, pull, default branch…) are written
  // to ~/.gitconfig or .git/config with `git config`, so a terminal sees the same.

  import { listen } from "@tauri-apps/api/event";
  import { open as openFile } from "@tauri-apps/plugin-dialog";
  import { api } from "./api";
  import ConfirmSheet from "./ConfirmSheet.svelte";
  import { confirm } from "./confirm.svelte";
  import DiffView from "./DiffView.svelte";
  import Menu, { menuIcons, type MenuEntry } from "./Menu.svelte";
  import { defaults, effectiveLook, prefs, type PrefKey } from "./prefs.svelte";
  import { addRemoteRequest, optimizeRequest, removeRemoteRequest, setUrlRequest } from "./repoSettings";
  import { paletteOf, THEMES, themeById, type Palette } from "./themes";
  import type { ConfigScope, FileDiff, GitSettings, OpenApp, RemoteInfo, RepoSettings } from "./types";

  const GEAR =
    "M12.78 6.52L14.44 6.56L14.44 9.44L12.78 9.48L12.42 10.33L13.57 11.54L11.54 13.57L10.33 12.42L9.48 12.78L9.44 14.44L6.56 14.44L6.52 12.78L5.67 12.42L4.46 13.57L2.43 11.54L3.58 10.33L3.22 9.48L1.56 9.44L1.56 6.56L3.22 6.52L3.58 5.67L2.43 4.46L4.46 2.43L5.67 3.58L6.52 3.22L6.56 1.56L9.44 1.56L9.48 3.22L10.33 3.58L11.54 2.43L13.57 4.46L12.42 5.67zM8 5.8a2.2 2.2 0 1 0 0 4.4a2.2 2.2 0 1 0 0-4.4";

  type Value = string | number | boolean;
  type Option = { value: Value; label: string };
  type Text = string | (() => string);

  /** Where a row's value lives, for its gear menu. */
  type Setting =
    | { kind: "pref"; key: PrefKey }
    | { kind: "git"; scope: ConfigScope; keys: string[]; commands: () => string[]; reset: () => Promise<void> };

  type Control =
    | { type: "seg" | "popup"; options: () => Option[]; get: () => Value; set: (value: Value) => void }
    | { type: "switch"; get: () => boolean; set: (on: boolean) => void }
    /** A value with Edit, which turns it into a text box. */
    | { type: "edit"; get: () => string; shown?: () => string; placeholder?: string; save: (value: string) => void }
    | { type: "button"; label: Text; run: () => void; danger?: boolean };

  type Row = {
    id: string;
    label: Text;
    sub?: Text;
    mono?: boolean;
    badge?: () => string | null;
    control?: Control;
    setting?: Setting;
    /** Greyed out while another setting makes this one irrelevant. */
    off?: () => boolean;
    /** More items for its gear menu. */
    more?: () => MenuEntry[];
  };
  type Group = { title?: string; foot?: Text; rows: Row[] };
  type Section = { id: string; label: string; color: string; icon: string; groups: () => Group[]; head?: string };

  const text = (t: Text | undefined) => (typeof t === "function" ? t() : (t ?? ""));
  const opts = (...values: (string | number)[]): Option[] => values.map((value) => ({ value, label: String(value) }));
  /** The options plus the current value when it is not one of them, as one typed into
   *  settings.json can be, so the popup never shows empty. */
  function withCurrent(options: Option[], current: Value): Option[] {
    if (current === "" || options.some((o) => String(o.value) === String(current))) return options;
    const extra = { value: current as string | number, label: String(current) };
    const at = typeof current === "number" ? options.findIndex((o) => typeof o.value === "number" && o.value > current) : -1;
    return at < 0 ? [...options, extra] : [...options.slice(0, at), extra, ...options.slice(at)];
  }

  /** Monospaced font families installed here, for the Font popup. */
  let fonts = $state<string[]>([]);
  api.monospaceFonts().then((list) => (fonts = list), () => {});

  // Oxbow's own settings.
  /** Installed apps as popup options; an empty popup says so. */
  const apps = (list: OpenApp[] | undefined): Option[] => (list?.length ? list.map((app) => ({ value: app.id, label: app.name })) : [{ value: "", label: "None found" }]);
  const prefRow = (key: PrefKey, label: Text, control: "seg" | "popup" | "switch", more: Partial<Row> & { options?: () => Option[] } = {}): Row => ({
    id: key,
    label,
    setting: { kind: "pref", key },
    control:
      control === "switch"
        ? { type: "switch", get: () => !!prefs.get(key), set: (on) => prefs.set(key, on as never) }
        : { type: control, options: more.options ?? (() => []), get: () => prefs.get(key) as Value, set: (value) => prefs.set(key, value as never) },
    ...more,
  });

  // Git's own settings, read from `git config --list` (whose keys are lower case).
  let git = $state<GitSettings | null>(null);
  let repo = $state<RepoSettings | null>(null);
  let problem = $state<string | null>(null);

  async function load() {
    const [g, r] = await Promise.all([api.gitSettings().catch(() => null), api.repoSettings().catch(() => null)]);
    git = g;
    repo = r;
  }
  load();
  listen("repo-changed", () => load()).catch(() => {});

  function config(scope: ConfigScope, key: string): string | undefined {
    const map = scope === "global" ? git?.global : repo?.local;
    return map?.[key.toLowerCase()];
  }

  const truthy = (value: string | undefined) => value !== undefined && !["false", "no", "off", "0", ""].includes(value.toLowerCase());

  /** Write Git settings in order; `null` removes a value. */
  async function writeGit(scope: ConfigScope, values: [string, string | null][]) {
    problem = null;
    const map = scope === "global" ? git?.global : repo?.local;
    for (const [key, value] of values) {
      try {
        await api.setGitConfig(scope, key, value);
        if (map) {
          if (value === null) delete map[key.toLowerCase()];
          else map[key.toLowerCase()] = value;
        }
      } catch (err) {
        problem = String(err);
        break;
      }
    }
    load();
  }

  const flag = (scope: ConfigScope) => (scope === "global" ? "--global" : "--local");
  const quote = (value: string) => (/^[\w@%+=:,./~-]+$/.test(value) ? value : `"${value.replace(/(["\\$`])/g, "\\$1")}"`);
  const configLine = (scope: ConfigScope, key: string, value: string | null) =>
    value === null ? `git config ${flag(scope)} --unset ${key}` : `git config ${flag(scope)} ${key} ${quote(value)}`;

  function gitSetting(scope: ConfigScope, writes: () => [string, string | null][], unset: string[]): Setting {
    return {
      kind: "git",
      scope,
      keys: unset,
      commands: () => writes().map(([key, value]) => configLine(scope, key, value)),
      reset: () => writeGit(scope, unset.map((key) => [key, null])),
    };
  }

  /** A Git switch; `fallback` is what Oxbow does while the key is not set. */
  function gitSwitch(scope: ConfigScope, key: string | string[], label: Text, fallback: boolean, more: Partial<Row> = {}): Row {
    const keys = Array.isArray(key) ? key : [key];
    const get = () => {
      const value = config(scope, keys[0]);
      return value === undefined ? fallback : truthy(value);
    };
    return {
      id: `git:${keys[0]}`,
      label,
      control: { type: "switch", get, set: (on) => writeGit(scope, keys.map((k) => [k, String(on)])) },
      setting: gitSetting(scope, () => keys.map((k) => [k, String(get())]), keys),
      ...more,
    };
  }

  /** A Git text value with Edit. */
  function gitEdit(scope: ConfigScope, key: string, label: Text, more: Partial<Row> & { shown?: () => string; placeholder?: string } = {}): Row {
    const get = () => config(scope, key) ?? "";
    return {
      id: `git:${scope}:${key}`,
      label,
      control: { type: "edit", get, shown: more.shown, placeholder: more.placeholder, save: (value) => writeGit(scope, [[key, value.trim() || null]]) },
      setting: gitSetting(scope, () => [[key, get() || null]], [key]),
      ...more,
    };
  }

  // Pull: rebase unless pull.rebase is false (merge) or pull.ff is only.
  const pullMode = () => (config("global", "pull.ff") === "only" ? "ff" : config("global", "pull.rebase") !== undefined && !truthy(config("global", "pull.rebase")) ? "merge" : "rebase");
  const pullWrites = (mode: string): [string, string | null][] =>
    mode === "ff" ? [["pull.ff", "only"], ["pull.rebase", null]] : mode === "merge" ? [["pull.ff", null], ["pull.rebase", "false"]] : [["pull.ff", null], ["pull.rebase", "true"]];

  // Signing: SSH signing with a key from ~/.ssh.
  const signingKey = () => config("global", "user.signingkey");
  const sshFormat = () => config("global", "gpg.format") === "ssh";
  const firstKey = () => git?.sshKeys.find((k) => k.name === "id_ed25519") ?? git?.sshKeys[0] ?? null;
  const keyName = (path: string | undefined) => git?.sshKeys.find((k) => k.path === path)?.name ?? path?.split("/").pop() ?? "";

  function setSigning(on: boolean) {
    if (!on) return writeGit("global", [["commit.gpgsign", "false"]]);
    const writes: [string, string | null][] = [];
    if (!signingKey() && firstKey()) writes.push(["gpg.format", "ssh"], ["user.signingkey", firstKey()!.path]);
    writes.push(["commit.gpgsign", "true"]);
    return writeGit("global", writes);
  }

  // This repository's own identity: on while its .git/config names an author.
  const ownIdentity = () => config("local", "user.name") !== undefined || config("local", "user.email") !== undefined;
  const who = () => [config("global", "user.name"), config("global", "user.email")].filter(Boolean).join(" · ") || "nobody yet: set a name in Accounts";

  let copied = $state<string | null>(null);
  function copy(id: string, value: string) {
    navigator.clipboard.writeText(value).then(() => {
      copied = id;
      setTimeout(() => copied === id && (copied = null), 1500);
    });
  }

  async function chooseGit() {
    const path = await openFile({ multiple: false, directory: false, title: "Choose the git to run" });
    if (typeof path !== "string") return;
    await prefs.set("oxbow.git.path", path);
    load();
  }

  function size(bytes: number): string {
    if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
    if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(bytes < 10 * 1024 ** 2 ? 1 : 0)} MB`;
    return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  }

  async function runRepo(request: Parameters<typeof confirm.run>[0]) {
    await confirm.run(request);
    load();
  }

  const confirmOff = () => !prefs.get("oxbow.confirm.enabled");
  const look = $derived(effectiveLook());

  const sections: Section[] = [
    {
      id: "general",
      label: "General",
      color: "var(--section-grey)",
      icon: GEAR,
      groups: () => [
        {
          title: "Language",
          rows: [
            prefRow("oxbow.git.englishOutput", "Show git output in English", "switch", {
              sub: () =>
                prefs.get("oxbow.git.englishOutput")
                  ? "Git errors are easier to look up in English. Commands are never translated."
                  : "Terminal blocks show git’s own translation, in your system’s language",
            }),
          ],
        },
        {
          title: "Appearance",
          rows: [
            prefRow("oxbow.appearance", "Appearance", "seg", {
              options: () => [
                { value: "system", label: "System" },
                { value: "light", label: "Light" },
                { value: "dark", label: "Dark" },
              ],
            }),
            prefRow("oxbow.history.rowStyle", "History rows", "seg", {
              sub: "Two lines show branch labels, author and time under the message",
              options: () => [
                { value: "twoLines", label: "Two lines" },
                { value: "compact", label: "Compact" },
              ],
            }),
          ],
        },
        {
          title: "Startup",
          rows: [prefRow("oxbow.startup.reopenRepository", "Reopen the repository from the last session", "switch", { sub: "Otherwise Oxbow starts on the Welcome window" })],
        },
        {
          title: "Background",
          rows: [
            prefRow("oxbow.fetch.auto", "Fetch remotes automatically", "switch", { sub: "Keeps ahead and behind counts current. It never changes your branches." }),
            prefRow("oxbow.fetch.interval", "Fetch every", "popup", {
              off: () => !prefs.get("oxbow.fetch.auto"),
              options: () => [
                { value: 5, label: "5 minutes" },
                { value: 15, label: "15 minutes" },
                { value: 30, label: "30 minutes" },
                { value: 60, label: "hour" },
              ],
            }),
          ],
        },
        {
          title: "Confirmations",
          foot: "Background fetch and read-only actions such as Copy never ask.",
          rows: [
            prefRow("oxbow.confirm.enabled", "Confirm actions before running them", "switch", { sub: "Describes the action with branch and commit names before anything changes" }),
            prefRow("oxbow.confirm.scope", "Ask for", "seg", {
              sub: "Risky: reset, drop, discard, delete, rebase, abort, force push",
              off: confirmOff,
              options: () => [
                { value: "all", label: "All actions" },
                { value: "risky", label: "Risky only" },
              ],
            }),
            prefRow("oxbow.confirm.showCommand", "Show the git command", "switch", { sub: "A terminal block with the exact commands, to learn git as you go", off: confirmOff }),
          ],
        },
        {
          title: "Safety",
          rows: [
            prefRow("oxbow.undo.keepDays", "Keep undo history for", "seg", {
              sub: "Every action in the Operation Log can be undone",
              options: () => [
                { value: 7, label: "7 days" },
                { value: 30, label: "30 days" },
                { value: 90, label: "90 days" },
              ],
            }),
            prefRow("oxbow.push.confirmForce", "Ask before force pushing", "switch", { sub: "Oxbow always force pushes with --force-with-lease" }),
          ],
        },
      ],
    },
    {
      id: "theme",
      label: "Themes",
      color: "var(--section-pink)",
      icon: "M8 1.8a6.2 6.2 0 1 0 0 12.4c1 0 1.4-.6 1.4-1.3 0-.9-.8-1.2-.8-2s.7-1.3 1.6-1.3h1.4a2.6 2.6 0 0 0 2.6-2.6C14.2 4.2 11.4 1.8 8 1.8M4.9 7.3v.1M6.9 4.7v.1M9.9 4.7v.1",
      groups: () => {
        const theme = themeById(prefs.get("oxbow.theme"));
        return [
          {
            rows: [
              prefRow("oxbow.theme.variant", "Variant", "seg", {
                sub: () => (theme.light && theme.dark ? "Auto follows Appearance in General" : `${theme.name} has only a ${theme.dark ? "dark" : "light"} version, so it is used in both`),
                off: () => !(theme.light && theme.dark),
                options: () => [
                  { value: "auto", label: "Auto" },
                  { value: "light", label: "Light" },
                  { value: "dark", label: "Dark" },
                ],
              }),
            ],
          },
        ];
      },
    },
    {
      id: "accounts",
      label: "Accounts",
      color: "var(--lane-0)",
      icon: "M8 8a2.6 2.6 0 1 0 0-5.2a2.6 2.6 0 1 0 0 5.2M3 13.8c.6-2.6 2.6-3.6 5-3.6s4.4 1 5 3.6",
      groups: () => [
        {
          title: "Commit identity",
          foot: () => `Saved in ~/.gitconfig.${repo ? ` A repository can use its own identity, see ${repo.name} in the sidebar.` : ""}`,
          rows: [
            gitEdit("global", "user.name", "Name", { shown: () => config("global", "user.name") ?? "Not set", placeholder: "Your Name" }),
            gitEdit("global", "user.email", "Email", { shown: () => config("global", "user.email") ?? "Not set", placeholder: "you@example.com" }),
          ],
        },
        {
          title: "SSH",
          foot: "Add the public key to GitHub or GitLab to fetch and push over SSH.",
          rows: git?.sshKeys.length
            ? git.sshKeys.map((key) => ({
                id: `ssh:${key.path}`,
                label: key.name,
                mono: true,
                sub: [key.fingerprint, key.comment].filter(Boolean).join(" · "),
                badge: () => (signingKey() === key.path ? "Signs commits" : null),
                control: { type: "button", label: () => (copied === key.path ? "Copied" : "Copy Key"), run: () => copy(key.path, key.public) },
              }))
            : [{ id: "ssh:none", label: "No SSH keys in ~/.ssh", sub: "Create one in Terminal with ssh-keygen -t ed25519" }],
        },
        {
          title: "Signing",
          rows: [
            {
              ...gitSwitch("global", "commit.gpgsign", "Sign commits", false),
              control: { type: "switch", get: () => truthy(config("global", "commit.gpgsign")), set: setSigning },
              off: () => !signingKey() && !firstKey(),
              sub: () =>
                truthy(config("global", "commit.gpgsign"))
                  ? `Signed with ${sshFormat() ? keyName(signingKey()) : (signingKey() ?? "your GPG key")}. Add it to GitHub as a signing key to show Verified.`
                  : !signingKey() && !firstKey()
                    ? "Needs an SSH key in ~/.ssh"
                    : "Uses your SSH key to sign commits",
            },
            ...(sshFormat() && (git?.sshKeys.length ?? 0) > 1
              ? [
                  {
                    id: "git:user.signingkey",
                    label: "Signing key",
                    control: {
                      type: "popup" as const,
                      options: () => git!.sshKeys.map((k) => ({ value: k.path, label: k.name })),
                      get: () => signingKey() ?? "",
                      set: (value: Value) => writeGit("global", [["user.signingkey", String(value)]]),
                    },
                    setting: gitSetting("global", () => [["user.signingkey", signingKey() ?? null]], ["user.signingkey"]),
                  },
                ]
              : []),
          ],
        },
      ],
    },
    {
      id: "git",
      label: "Git",
      color: "var(--section-orange)",
      icon: "M4.5 2v12M4.5 11c0-3 7-2 7-5.5M11.5 5.5a1.6 1.6 0 1 0 0-.1",
      groups: () => [
        {
          title: "Git",
          foot: "Oxbow reads history, graphs and diffs with its built-in engine and calls your Git for everything that changes the repository, so hooks and credentials behave as in Terminal.",
          rows: [
            {
              id: "oxbow.git.path",
              label: () => git?.git.path ?? "git",
              mono: true,
              sub: () => (!git ? "Looking…" : git.git.version ? `git ${git.git.version} · commit, rebase, push, hooks` : "This git does not run. Choose another one."),
              setting: { kind: "pref", key: "oxbow.git.path" },
              control: { type: "button", label: "Change…", run: chooseGit },
            },
          ],
        },
        {
          title: "Pull",
          rows: [
            {
              id: "git:pull",
              label: "When pulling",
              control: {
                type: "seg",
                options: () => [
                  { value: "merge", label: "Merge" },
                  { value: "rebase", label: "Rebase" },
                  { value: "ff", label: "Fast-forward" },
                ],
                get: pullMode,
                set: (mode) => writeGit("global", pullWrites(String(mode))),
              },
              setting: gitSetting("global", () => pullWrites(pullMode()), ["pull.rebase", "pull.ff"]),
            },
            gitSwitch("global", ["rebase.autoStash", "merge.autoStash"], "Stash my changes before pulling and bring them back after", true),
          ],
        },
        {
          title: "Branches and stacks",
          rows: [
            gitEdit("global", "init.defaultBranch", "Default branch for new repositories", { mono: false, shown: () => config("global", "init.defaultBranch") ?? "master (Git’s default)", placeholder: "main" }),
            gitSwitch("global", "rebase.updateRefs", "Move stacked branches along when rebasing", true, { sub: "git rebase --update-refs: branches built on the rebased one follow it" }),
            gitSwitch("global", "fetch.prune", "Remove branches deleted on the remote when fetching", true),
          ],
        },
        {
          title: "Commits",
          rows: [
            prefRow("oxbow.git.runHooks", "Run Git hooks", "switch", {
              sub: () => (prefs.get("oxbow.git.runHooks") ? "pre-commit, commit-msg and pre-push from .git/hooks" : "Commits, merges and pushes add --no-verify"),
            }),
            prefRow("oxbow.commit.subjectGuide", "Subject line guide", "popup", {
              sub: "A soft marker in the commit message field",
              options: () => [
                { value: 50, label: "50 characters" },
                { value: 72, label: "72 characters" },
                { value: 0, label: "Off" },
              ],
            }),
          ],
        },
      ],
    },
    {
      id: "diff",
      label: "Diff & Text",
      color: "var(--section-green)",
      icon: "M4.5 2.5v5M2 5h5M9 11.5h5M2.5 13.5 13.5 2.5",
      groups: () => [
        {
          title: "Diff",
          rows: [
            prefRow("oxbow.diff.view", "Show", "seg", {
              sub: "The default for History, Changes and Stashes",
              options: () => [
                { value: "changes", label: "Changes only" },
                { value: "full", label: "Full file" },
              ],
            }),
            prefRow("oxbow.diff.contextLines", "Context lines", "popup", { sub: "Unchanged lines kept around each change", options: () => opts(1, 3, 5, 10) }),
            prefRow("oxbow.diff.wordHighlight", "Highlight changed words", "switch"),
            prefRow("oxbow.diff.ignoreWhitespace", "Ignore whitespace changes", "switch", {
              sub: "In commits and stashes. Changes stays exact, so staging matches what you see",
            }),
            prefRow("oxbow.diff.files", "Files of a commit", "seg", {
              sub: "Smart folds long, generated, deleted and binary files",
              options: () => [
                { value: "smart", label: "Smart" },
                { value: "expanded", label: "Expanded" },
                { value: "collapsed", label: "Collapsed" },
              ],
            }),
            prefRow("oxbow.diff.foldOver", "Fold diffs longer than", "popup", {
              off: () => prefs.get("oxbow.diff.files") !== "smart",
              options: () => [100, 300, 1000].map((value) => ({ value, label: `${value} lines` })),
            }),
          ],
        },
        {
          title: "Text",
          rows: [
            prefRow("oxbow.text.font", "Font", "popup", {
              sub: "Monospaced fonts installed on this computer",
              options: () => opts(...new Set(["SF Mono", ...fonts])),
            }),
            prefRow("oxbow.text.fontSize", "Size", "popup", { options: () => opts(10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 22, 24) }),
            prefRow("oxbow.text.tabWidth", "Tab width", "popup", { options: () => opts(2, 4, 8) }),
          ],
        },
      ],
    },
    {
      id: "integrations",
      label: "Integrations",
      color: "var(--section-purple)",
      icon: "M6.5 9.5l3-3M7 4.5l1-1a2.5 2.5 0 0 1 3.5 3.5l-1 1M9 11.5l-1 1a2.5 2.5 0 0 1-3.5-3.5l1-1",
      groups: () => [
        {
          title: "Open in",
          rows: [
            prefRow("oxbow.openIn.editor", "Editor", "popup", {
              sub: () => (git && !git.editors.length ? "None of Visual Studio Code, Zed, Sublime Text or Xcode is installed" : "The ↗ button on a file’s diff opens it at its first change"),
              off: () => !git?.editors.length,
              control: {
                type: "popup",
                options: () => apps(git?.editors),
                get: () => prefs.get("oxbow.openIn.editor") || git?.editors[0]?.id || "",
                set: (value) => prefs.set("oxbow.openIn.editor", String(value)),
              },
            }),
            prefRow("oxbow.openIn.terminal", "Terminal", "popup", {
              sub: () => (git && !git.terminals.length ? "No terminal Oxbow knows is installed" : "Right-click the repository in the sidebar › Open in Terminal"),
              off: () => !git?.terminals.length,
              control: {
                type: "popup",
                options: () => apps(git?.terminals),
                get: () => prefs.get("oxbow.openIn.terminal") || git?.terminals[0]?.id || "",
                set: (value) => prefs.set("oxbow.openIn.terminal", String(value)),
              },
            }),
          ],
        },
      ],
    },
    {
      id: "repo",
      head: "This Repository",
      label: "",
      color: "var(--section-teal)",
      icon: "M1.8 4.5a1 1 0 0 1 1-1H6l1.5 1.5h5.7a1 1 0 0 1 1 1v6.2a1 1 0 0 1-1 1H2.8a1 1 0 0 1-1-1z",
      groups: () => {
        if (!repo) return [];
        const r = repo;
        return [
          {
            title: "Identity",
            rows: [
              {
                id: "git:local:identity",
                label: "Use a different identity in this repository",
                sub: () => (ownIdentity() ? `Saved in ${r.name}/.git/config` : `Commits use ${who()}`),
                control: {
                  type: "switch",
                  get: ownIdentity,
                  set: (on) =>
                    writeGit(
                      "local",
                      on
                        ? [
                            ["user.name", config("global", "user.name") ?? ""],
                            ["user.email", config("global", "user.email") ?? ""],
                          ]
                        : [
                            ["user.name", null],
                            ["user.email", null],
                          ],
                    ),
                },
                setting: gitSetting(
                  "local",
                  () => [
                    ["user.name", config("local", "user.name") ?? null],
                    ["user.email", config("local", "user.email") ?? null],
                  ],
                  ["user.name", "user.email"],
                ),
              },
              { ...gitEdit("local", "user.name", "Name", { shown: () => config("local", "user.name") ?? "", placeholder: "Your Name" }), off: () => !ownIdentity() },
              { ...gitEdit("local", "user.email", "Email", { shown: () => config("local", "user.email") ?? "", placeholder: "you@work.example" }), off: () => !ownIdentity() },
            ],
          },
          {
            title: "Remotes",
            rows: [
              ...r.remotes.map(
                (remote: RemoteInfo): Row => ({
                  id: `remote:${remote.name}`,
                  label: remote.name,
                  mono: true,
                  sub: remote.pushUrl && remote.pushUrl !== remote.fetchUrl ? `${remote.fetchUrl} · pushes to ${remote.pushUrl}` : remote.fetchUrl,
                  badge: () => (remote.pushUrl && remote.pushUrl !== remote.fetchUrl ? "fetch" : "fetch · push"),
                  control: { type: "button", label: "Edit", run: () => runRepo(setUrlRequest(remote)) },
                  more: () => [
                    { kind: "item", label: "Copy Address", icon: menuIcons.copy, run: () => copy(remote.name, remote.fetchUrl) },
                    { kind: "sep" },
                    { kind: "item", label: `Remove ${remote.name}…`, icon: menuIcons.drop, danger: true, run: () => runRepo(removeRemoteRequest(remote)) },
                  ],
                }),
              ),
              {
                id: "remote:add",
                label: r.remotes.length ? "Add a remote" : "No remotes yet",
                sub: r.remotes.length ? undefined : "Add one to fetch, pull and push",
                control: { type: "button", label: "Add…", run: () => runRepo(addRemoteRequest(r.remotes.map((x) => x.name))) },
              },
            ],
          },
          {
            title: "Storage",
            rows: [
              {
                id: "storage:lfs",
                label: "Git LFS",
                sub: () => {
                  const patterns = r.storage?.lfsPatterns ?? [];
                  return patterns.length ? `${patterns.length} ${patterns.length === 1 ? "pattern" : "patterns"}: ${patterns.join(", ")}` : "Not used: .gitattributes hands no files to Git LFS";
                },
              },
              ...(r.storage
                ? [
                    {
                      id: "storage:size",
                      label: `${size(r.storage.bytes)} · ${r.storage.objects.toLocaleString("en-US")} objects`,
                      sub: r.storage.loose ? `${r.storage.loose.toLocaleString("en-US")} loose objects can be packed` : "Everything is packed",
                      control: { type: "button" as const, label: "Optimize", run: () => runRepo(optimizeRequest(r.storage!.loose)) },
                    },
                  ]
                : []),
            ],
          },
        ];
      },
    },
  ];

  const shownSections = $derived(sections.filter((s) => s.id !== "repo" || repo));
  const labelOf = (section: Section) => (section.id === "repo" ? (repo?.name ?? "") : section.label);

  const mac = navigator.platform.startsWith("Mac");

  // Back and Forward walk the sections visited, like System Settings.
  let visited = $state(["general"]);
  let at = $state(0);
  const current = $derived(shownSections.find((s) => s.id === visited[at]) ?? shownSections[0]);

  function go(id: string) {
    query = "";
    json = false;
    if (id === visited[at]) return;
    visited = [...visited.slice(0, at + 1), id];
    at = visited.length - 1;
  }

  let query = $state("");
  const needle = $derived(query.trim().toLowerCase());

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

  function saveEdit(row: Row) {
    if (!editing || row.control?.type !== "edit") return;
    row.control.save(editing.value);
    editing = null;
  }

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
  let jsonError = $state<string | null>(null);

  async function openJson() {
    menu = null;
    jsonText = jsonSaved = await api.settingsText().catch(() => "{}\n");
    jsonError = null;
    json = true;
  }

  const unknownKeys = $derived.by(() => {
    try {
      const parsed = JSON.parse(jsonText);
      return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? Object.keys(parsed).filter((key) => !(key in defaults) && key !== "oxbow.history.detailsWidth") : [];
    } catch {
      return [];
    }
  });

  async function saveJson() {
    try {
      const parsed = JSON.parse(jsonText);
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error("settings.json must be one object: { \"oxbow.…\": … }");
    } catch (err) {
      jsonError = err instanceof Error ? err.message : String(err);
      return;
    }
    try {
      await api.saveSettingsText(jsonText);
      jsonSaved = jsonText;
      jsonError = null;
      load();
    } catch (err) {
      jsonError = String(err);
    }
  }

  // Themes: a sample diff drawn by the real code view, in the chosen theme.
  const sample: FileDiff = {
    file: { path: "src/config.rs", oldPath: null, status: "modified", additions: 1, deletions: 1, binary: false },
    tooLarge: false,
    hunks: [
      {
        header: "@@ -18,7 +18,7 @@",
        oldStart: 18,
        oldLines: 7,
        newStart: 18,
        newLines: 7,
        lines: [
          { kind: "context", oldLine: 18, newLine: 18, text: "impl Default for Config {", words: null },
          { kind: "context", oldLine: 19, newLine: 19, text: "    fn default() -> Self {", words: null },
          { kind: "context", oldLine: 20, newLine: 20, text: "        // Give up on a slow login after this long", words: null },
          {
            kind: "removed",
            oldLine: 21,
            newLine: null,
            text: "        Self { login_timeout: Duration::from_secs(10), name: \"oxbow\" }",
            words: [
              { text: "        Self { login_timeout: Duration::from_secs(", changed: false },
              { text: "10", changed: true },
              { text: "), name: \"oxbow\" }", changed: false },
            ],
          },
          {
            kind: "added",
            oldLine: null,
            newLine: 21,
            text: "        Self { login_timeout: Duration::from_secs(30), name: \"oxbow\" }",
            words: [
              { text: "        Self { login_timeout: Duration::from_secs(", changed: false },
              { text: "30", changed: true },
              { text: "), name: \"oxbow\" }", changed: false },
            ],
          },
          { kind: "context", oldLine: 22, newLine: 22, text: "    }", words: null },
          { kind: "context", oldLine: 23, newLine: 23, text: "}", words: null },
        ],
      },
    ],
  };

  /** Bars of a theme card: a few lines of code in the theme's colors. */
  function bars(palette: Palette) {
    return [
      { x: 10, y: 10, w: 14, c: palette.kw },
      { x: 28, y: 10, w: 22, c: palette.fn },
      { x: 54, y: 10, w: 10, c: palette.fg },
      { x: 18, y: 21, w: 34, c: palette.com },
      { x: 18, y: 32, w: 12, c: palette.type },
      { x: 34, y: 32, w: 26, c: palette.str },
      { x: 64, y: 32, w: 8, c: palette.num },
      { x: 6, y: 43, w: 74, c: palette.add, soft: true },
    ];
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
    const ctx = ([n, line]: [number, string]): Line => ({ kind: "ctx", old: n, new: n, before: line });
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

<svelte:window onfocus={load} />

<div class="settings" class:mac>
  <nav aria-label="Settings sections">
    <div class="lights" data-tauri-drag-region></div>
    <label class="search">
      <svg class="icon small" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5" /><path d="M10.5 10.5 14 14" /></svg>
      <!-- svelte-ignore a11y_autofocus -->
      <input type="search" aria-label="Search settings" placeholder="Search" bind:value={query} spellcheck="false" autofocus />
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
      <span class="spacer" data-tauri-drag-region></span>
      {#if json}
        <button class="pill" onclick={() => (json = false)} title="Back to the Settings window">
          <svg class="icon small" viewBox="0 0 16 16"><path d="M2.5 4.5h11M2.5 8h11M2.5 11.5h11" /></svg>Open Settings UI
        </button>
      {:else}
        <button class="pill" onclick={openJson} title="The same settings as a JSON file">
          <svg class="icon small" viewBox="0 0 16 16"
            ><path
              d="M5.5 2.5c-1.6 0-2 .7-2 2v1.6c0 .9-.5 1.5-1.5 1.9 1 .4 1.5 1 1.5 1.9v1.6c0 1.3.4 2 2 2M10.5 2.5c1.6 0 2 .7 2 2v1.6c0 .9.5 1.5 1.5 1.9-1 .4-1.5 1-1.5 1.9v1.6c0 1.3-.4 2-2 2"
            /></svg
          >Edit in settings.json
        </button>
      {/if}
    </header>

    {#if json}
      <div class="json">
        <p class="banner">
          Only values that differ from the defaults are listed. Git’s own settings (identity, pull, default branch, signing) stay in ~/.gitconfig, where Settings writes them with
          <code>git config</code>.
        </p>
        <textarea
          class="editor"
          bind:value={jsonText}
          spellcheck="false"
          aria-label="settings.json"
          onkeydown={(event) => {
            if ((event.metaKey || event.ctrlKey) && event.key === "s") {
              event.preventDefault();
              saveJson();
            } else if (event.key === "Tab") {
              event.preventDefault();
              const box = event.currentTarget;
              box.setRangeText("  ", box.selectionStart, box.selectionEnd, "end");
              jsonText = box.value;
            }
          }}
        ></textarea>
        <div class="json-bar">
          {#if jsonError}
            <span class="error">{jsonError}</span>
          {:else if unknownKeys.length}
            <span class="warn">Oxbow does not know {unknownKeys.join(", ")}</span>
          {:else}
            <span class="sub">{jsonText === jsonSaved ? "Saved" : "Not saved yet"}</span>
          {/if}
          <span class="spacer"></span>
          <button class="plain" disabled={jsonText === jsonSaved} onclick={() => ((jsonText = jsonSaved), (jsonError = null))}>Revert</button>
          <button class="primary" disabled={jsonText === jsonSaved} onclick={saveJson}>Save <span class="keys">{keys("Mod+S")}</span></button>
        </div>
      </div>
    {:else}
      <div class="scroll">
        {#if problem}<p class="problem" role="alert">{problem}</p>{/if}

        {#if !needle && current.id === "theme"}
          <section>
            <h2>Preview</h2>
            <div class="theme-preview">
              <DiffView diffs={[sample]} color={0} whole={false} onToggleWhole={() => {}} openable={false} />
            </div>
          </section>
        {/if}

        {#each shown as group (group.key)}
          <section>
            {#if group.title}<h2>{group.title}</h2>{/if}
            <div class="panel">
              {#each group.rows as row (row.id)}
                {@const off = row.off?.()}
                {@const control = row.control}
                {@const badge = row.badge?.()}
                <div
                  class="row"
                  role="group"
                  aria-label={text(row.label)}
                  oncontextmenu={(event) => {
                    event.preventDefault();
                    openMenu(row, event.clientX, event.clientY);
                  }}
                >
                  {#if row.setting || row.more}
                    <button
                      class="gear"
                      class:on={menu?.row.id === row.id}
                      aria-label="More actions for {text(row.label)}"
                      onclick={(event) => {
                        const box = event.currentTarget.getBoundingClientRect();
                        openMenu(row, box.left, box.bottom + 4);
                      }}
                    >
                      <svg class="icon" viewBox="0 0 16 16"><path d={GEAR} /></svg>
                    </button>
                  {/if}
                  <span class="text" class:off>
                    <span class="name" class:mono={row.mono}>{text(row.label)}</span>
                    {#if text(row.sub)}<span class="sub">{text(row.sub)}</span>{/if}
                  </span>
                  {#if badge}<span class="chip">{badge}</span>{/if}
                  {#if control?.type === "seg"}
                    <span class="seg" class:off role="radiogroup" aria-label={text(row.label)}>
                      {#each control.options() as option (option.value)}
                        <button role="radio" aria-checked={control.get() === option.value} disabled={off} onclick={() => control.set(option.value)}>{option.label}</button>
                      {/each}
                    </span>
                  {:else if control?.type === "popup"}
                    {@const options = withCurrent(control.options(), control.get())}
                    <span class="popup" class:off>
                      <select
                        aria-label={text(row.label)}
                        disabled={off}
                        value={String(control.get())}
                        onchange={(event) => {
                          const option = options.find((o) => String(o.value) === event.currentTarget.value);
                          if (option) control.set(option.value);
                        }}
                      >
                        {#each options as option (option.value)}
                          <option value={String(option.value)}>{option.label}</option>
                        {/each}
                      </select>
                      <svg class="icon" viewBox="0 0 16 16"><path d="M5 6.5 8 3.5l3 3M5 9.5l3 3 3-3" /></svg>
                    </span>
                  {:else if control?.type === "switch"}
                    <button class="switch" class:off role="switch" aria-checked={control.get()} aria-label={text(row.label)} disabled={off} onclick={() => control.set(!control.get())}>
                      <span></span>
                    </button>
                  {:else if control?.type === "edit"}
                    {#if editing?.id === row.id}
                      <!-- svelte-ignore a11y_autofocus -->
                      <input
                        class="field"
                        bind:value={editing.value}
                        placeholder={control.placeholder}
                        spellcheck="false"
                        autofocus
                        onkeydown={(event) => {
                          if (event.key === "Enter") saveEdit(row);
                          else if (event.key === "Escape") editing = null;
                        }}
                      />
                      <button class="small-button" onclick={() => (editing = null)}>Cancel</button>
                      <button class="small-button primary" onclick={() => saveEdit(row)}>Save</button>
                    {:else}
                      <span class="value" class:off>{control.shown?.() ?? control.get()}</span>
                      <button class="small-button" disabled={off} onclick={() => (editing = { id: row.id, value: control.get() })}>Edit</button>
                    {/if}
                  {:else if control?.type === "button"}
                    <button class="small-button" class:danger={control.danger} disabled={off} onclick={control.run}>{text(control.label)}</button>
                  {/if}
                </div>
              {/each}
            </div>
            {#if text(group.foot)}<p class="foot">{text(group.foot)}</p>{/if}
          </section>
        {:else}
          {#if needle}<p class="empty">No settings match “{query.trim()}”.</p>{/if}
        {/each}

        {#if !needle && current.id === "theme"}
          <section>
            <div class="title-line">
              <h2>Popular themes</h2>
              <span class="sub">by VS Code Marketplace installs</span>
            </div>
            <div class="cards" role="radiogroup" aria-label="Theme">
              {#each THEMES as theme (theme.id)}
                {@const { palette } = paletteOf(theme, prefs.get("oxbow.theme.variant"), look)}
                {@const on = prefs.get("oxbow.theme") === theme.id}
                <button class="card" role="radio" aria-checked={on} onclick={() => prefs.set("oxbow.theme", theme.id)} title={theme.name}>
                  <span class="swatch" class:on style:background={palette.bg}>
                    {#each bars(palette) as bar, i (i)}
                      <span style:left="{bar.x}px" style:top="{bar.y}px" style:width="{bar.w}px" style:background={bar.c} style:opacity={bar.soft ? 0.25 : 1}></span>
                    {/each}
                  </span>
                  <span class="card-name">
                    {#if theme.rank}<span class="rank">{theme.rank}</span>{/if}
                    <span class:strong={on}>{theme.name}</span>
                  </span>
                  <span class="card-sub">{theme.installs ? `${theme.installs} · ` : "Default · "}{theme.light && theme.dark ? "Light, Dark" : theme.dark ? "Dark" : "Light"}</span>
                </button>
              {/each}
            </div>
            <p class="foot">Themes color code: diffs, conflicts and stashes. The window keeps its own look.</p>
          </section>
        {/if}

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
    {/if}
  </div>
</div>

{#if menu}
  <Menu x={menu.x} y={menu.y} label="Setting {text(menu.row.label)}" entries={menuEntries(menu.row)} onClose={() => (menu = null)} />
{/if}

<ConfirmSheet repo={repo?.name ?? ""} branch={null} color={0} />

<style>
  .settings {
    --section-grey: #8e8e93;
    --section-green: #6aae80;
    --section-pink: #d58aa5;
    --section-orange: #d99a62;
    --section-purple: #a77bc9;
    --section-teal: #5baeb6;
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
    border-radius: 18px;
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
    border-radius: 15px;
    background: var(--field);
    color: var(--text2);
    flex-shrink: 0;
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

  .head {
    font-size: 11px;
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
    border-radius: 9px;
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

  .pill {
    display: flex;
    align-items: center;
    gap: 7px;
    height: 30px;
    padding: 0 14px 0 11px;
    border-radius: 15px;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    font-size: 12px;
    font-weight: 500;
  }

  .pill .icon {
    color: var(--icon);
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
    width: 14px;
    height: 14px;
    stroke-width: 1.3;
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

  .name.mono {
    font-family: var(--mono);
    font-size: 12px;
  }

  .sub {
    font-size: 11px;
    color: var(--text2);
  }

  .off {
    opacity: 0.4;
  }

  .chip {
    flex-shrink: 0;
    height: 20px;
    line-height: 20px;
    padding: 0 9px;
    border-radius: 10px;
    font-size: 11px;
    font-weight: 500;
    background: var(--field);
    color: var(--text2);
    white-space: nowrap;
  }

  .value {
    flex-shrink: 0;
    max-width: 50%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--text2);
  }

  .field {
    flex: 0 1 260px;
    min-width: 120px;
    height: 26px;
    padding: 0 9px;
    border-radius: 8px;
    border: 0.5px solid var(--glass-border);
    background: var(--win);
    color: var(--text);
    font: inherit;
    font-size: 12px;
    outline: 2px solid var(--accent);
    outline-offset: 0;
  }

  .small-button {
    flex-shrink: 0;
    min-width: 72px;
    height: 26px;
    padding: 0 12px;
    border-radius: 13px;
    text-align: center;
    background: var(--field);
    font-size: 12px;
    font-weight: 500;
  }

  .small-button.primary {
    background: var(--accent);
    color: #ffffff;
  }

  .small-button.danger {
    color: var(--danger);
  }

  .small-button:disabled {
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

  .problem {
    margin: 0;
    padding: 8px 12px;
    border-radius: 10px;
    background: var(--danger-soft);
    color: var(--danger);
    font-size: 12px;
  }

  /* Themes */
  .theme-preview {
    border-radius: 12px;
    border: 0.5px solid var(--panel-border);
    padding-top: 8px;
    background: var(--win);
  }

  .cards {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 10px 12px;
  }

  .card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }

  .swatch {
    position: relative;
    display: block;
    width: 100%;
    height: 56px;
    border-radius: 10px;
    border: 0.5px solid var(--panel-border);
    overflow: hidden;
  }

  .swatch.on {
    box-shadow: 0 0 0 2px var(--accent);
  }

  .swatch span {
    position: absolute;
    height: 5px;
    border-radius: 3px;
  }

  .card-name {
    display: flex;
    align-items: baseline;
    gap: 5px;
    padding: 0 2px;
    font-size: 12px;
    min-width: 0;
  }

  .card-name span:last-child {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .rank {
    color: var(--text2);
    font-size: 11px;
  }

  .strong {
    font-weight: 600;
  }

  .card-sub {
    margin-top: -4px;
    padding: 0 2px;
    font-size: 11px;
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* settings.json */
  .json {
    flex-grow: 1;
    min-height: 0;
    padding: 6px 30px 20px 28px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .banner {
    margin: 0;
    padding: 9px 12px;
    border-radius: 12px;
    background: var(--panel);
    border: 0.5px solid var(--panel-border);
    font-size: 12px;
    line-height: 1.4;
    color: var(--text2);
  }

  .banner code {
    font-family: var(--mono);
    font-size: 11px;
  }

  .editor {
    flex-grow: 1;
    min-height: 0;
    resize: none;
    padding: 10px 12px;
    border-radius: 12px;
    border: 0.5px solid var(--panel-border);
    background: var(--code-bg, var(--win));
    color: var(--code-fg, var(--code));
    font-family: var(--code-font);
    font-size: var(--code-size);
    line-height: var(--code-line);
    tab-size: 2;
    outline: 0;
    user-select: text;
    -webkit-user-select: text;
  }

  .editor:focus-visible {
    border-color: var(--accent);
  }

  .json-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }

  .json-bar .error {
    color: var(--danger);
  }

  .json-bar .warn {
    color: var(--orange);
  }

  .plain,
  .primary {
    height: 28px;
    padding: 0 14px;
    border-radius: 14px;
    font-size: 12px;
    font-weight: 500;
    background: var(--field);
  }

  .json-bar .primary {
    background: var(--accent);
    color: #ffffff;
  }

  .plain:disabled,
  .primary:disabled {
    opacity: 0.4;
  }

  .keys {
    opacity: 0.7;
    font-weight: 400;
    margin-left: 4px;
  }

  /* Diff & Text preview */
  .preview {
    border-radius: 12px;
    background: var(--code-bg, var(--win));
    border: 0.5px solid var(--panel-border);
    padding: 6px 0 8px;
    font-family: var(--code-font);
    font-size: var(--code-size);
    tab-size: var(--tab);
    color: var(--code-fg, var(--code));
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
    background: var(--fold-bg, var(--field));
    color: var(--code-dim, var(--text2));
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
    color: var(--code-dim, var(--text2));
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
