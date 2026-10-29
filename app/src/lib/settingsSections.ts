// The Settings window's sections for Oxbow's own settings, which save at once to settings.json
// and reach the main window through prefs: General, Themes, Diff & Text and Integrations.

import { BRANCH_PALETTES, branchPaletteById } from "./branchPalettes";
import { shortcut } from "./keys";
import { prefs } from "./prefs.svelte";
import { model, prefRow } from "./settingsModel.svelte";
import { GEAR, opts, type Option, type Section } from "./settingsRows";
import { themeById } from "./themes";
import { tips, TIP_COUNT } from "./tips.svelte";
import type { OpenApp } from "./types";

const confirmOff = () => !prefs.get("oxbow.confirm.enabled");

/** Installed apps as popup options; an empty popup says so. */
const apps = (list: OpenApp[] | undefined): Option[] => (list?.length ? list.map((app) => ({ value: app.id, label: app.name })) : [{ value: "", label: "None found" }]);

export const generalSection: Section = {
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
      foot: "With Risky only, the rest runs at once, and Activity (right-click Fetch) lists its git commands. Background fetch and read-only actions such as Copy never ask.",
      rows: [
        prefRow("oxbow.confirm.enabled", "Confirm actions before running them", "switch", { sub: "Describes the action with branch and commit names before anything changes" }),
        prefRow("oxbow.confirm.scope", "Ask for", "seg", {
          sub: "Risky: discard, reset, delete, abort, rewriting commits, push",
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
      title: "Learning",
      foot: "In any terminal block, click a subcommand or flag to see what it does, or a branch name or SHA to see its commit.",
      rows: [
        prefRow("oxbow.tips.show", "Show Git tips", "switch", { sub: "A short card the first time an idea matters, like a partly staged file or a detached HEAD" }),
        {
          id: "tips-again",
          label: "Closed tips",
          sub: () => `${tips.closed} of ${TIP_COUNT} closed with Got It`,
          control: { type: "button", label: "Show Again", run: () => tips.again() },
          off: () => !prefs.get("oxbow.tips.show") || !tips.closed,
        },
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
};

export const themeSection: Section = {
  id: "theme",
  label: "Themes",
  color: "var(--section-pink)",
  icon: "M8 1.8a6.2 6.2 0 1 0 0 12.4c1 0 1.4-.6 1.4-1.3 0-.9-.8-1.2-.8-2s.7-1.3 1.6-1.3h1.4a2.6 2.6 0 0 0 2.6-2.6C14.2 4.2 11.4 1.8 8 1.8M4.9 7.3v.1M6.9 4.7v.1M9.9 4.7v.1",
  groups: () => {
    const theme = themeById(prefs.get("oxbow.theme"));
    return [
      {
        title: "Code",
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
      {
        title: "Branches & graph",
        foot: "Light and dark follow Appearance in General. Branch names pick their color; the production branch is set per repository in This Repository.",
        rows: [
          prefRow("oxbow.branchPalette", "Branch palette", "seg", {
            sub: () => branchPaletteById(prefs.get("oxbow.branchPalette")).note,
            options: () => BRANCH_PALETTES.map((palette) => ({ value: palette.id, label: palette.name })),
          }),
        ],
      },
    ];
  },
};

export const diffSection: Section = {
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
        prefRow("oxbow.diff.layout", "Layout", "seg", {
          sub: "Side by Side puts the old lines next to the new ones",
          options: () => [
            { value: "unified", label: "Unified" },
            { value: "split", label: "Side by Side" },
          ],
        }),
        prefRow("oxbow.diff.contextLines", "Context lines", "popup", { sub: "Unchanged lines kept around each change", options: () => opts(1, 3, 5, 10) }),
        prefRow("oxbow.diff.wordHighlight", "Highlight changed words", "switch"),
        prefRow("oxbow.diff.wrap", "Wrap long lines", "switch", { sub: () => `Off, long lines run on and scroll sideways${shortcut("wrap") ? ` · ${shortcut("wrap")}` : ""}` }),
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
        prefRow("oxbow.diff.maxLines", "Hide diffs longer than", "popup", {
          sub: "A bigger diff waits for Show Diff Anyway, so History stays fast",
          options: () => [
            ...[1000, 5000, 20000].map((value) => ({ value, label: `${value.toLocaleString("en-US")} lines` })),
            { value: 0, label: "Never" },
          ],
        }),
      ],
    },
    {
      title: "Blame",
      rows: [
        prefRow("oxbow.blame.ignoreWhitespace", "Ignore whitespace changes", "switch", {
          sub: "A line whose only change was whitespace keeps the commit before it (git blame -w)",
        }),
        prefRow("oxbow.blame.ignoreRevs", "Skip the commits of .git-blame-ignore-revs", "switch", {
          sub: "Reformatting and the like listed there, as GitHub does; or the file blame.ignoreRevsFile names",
        }),
      ],
    },
    {
      title: "Text",
      rows: [
        prefRow("oxbow.text.font", "Font", "popup", {
          sub: "Monospaced fonts installed on this computer",
          options: () => opts(...new Set(["SF Mono", ...model.fonts])),
        }),
        prefRow("oxbow.text.fontSize", "Size", "popup", { options: () => opts(10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 22, 24) }),
        prefRow("oxbow.text.lineHeight", "Line height", "popup", {
          sub: () => `${Math.round(prefs.get("oxbow.text.fontSize") * prefs.get("oxbow.text.lineHeight"))} px per line at size ${prefs.get("oxbow.text.fontSize")}`,
          options: () => opts(1.2, 1.4, 1.5, 1.6, 1.8, 2),
        }),
        prefRow("oxbow.text.tabWidth", "Tab width", "popup", { options: () => opts(2, 4, 8) }),
      ],
    },
    {
      title: "Console & Activity",
      rows: [
        prefRow("oxbow.console.font", "Font", "popup", {
          sub: "Of git commands and their output, also in the sheets",
          options: () => [{ value: "", label: "Same as Text" }, ...opts(...new Set(["SF Mono", ...model.fonts]))],
        }),
        prefRow("oxbow.console.fontSize", "Size", "popup", { options: () => opts(10, 11, 12, 13, 14, 15, 16, 17, 18) }),
        prefRow("oxbow.console.lineHeight", "Line height", "popup", {
          sub: () => `${Math.round(prefs.get("oxbow.console.fontSize") * prefs.get("oxbow.console.lineHeight"))} px per line at size ${prefs.get("oxbow.console.fontSize")}`,
          options: () => opts(1.2, 1.4, 1.5, 1.6, 1.8, 2),
        }),
      ],
    },
  ],
};

export const integrationsSection: Section = {
  id: "integrations",
  label: "Integrations",
  color: "var(--section-purple)",
  icon: "M6.5 9.5l3-3M7 4.5l1-1a2.5 2.5 0 0 1 3.5 3.5l-1 1M9 11.5l-1 1a2.5 2.5 0 0 1-3.5-3.5l1-1",
  groups: () => [
    {
      title: "Open in",
      rows: [
        prefRow("oxbow.openIn.editor", "Editor", "popup", {
          sub: () => (model.git && !model.git.editors.length ? "None of Visual Studio Code, Zed, Sublime Text, Xcode or the JetBrains IDEs is installed" : "The ↗ button on a file’s diff opens it at its first change; a click on a line number opens it at that line"),
          off: () => !model.git?.editors.length,
          control: {
            type: "popup",
            options: () => apps(model.git?.editors),
            get: () => prefs.get("oxbow.openIn.editor") || model.git?.editors[0]?.id || "",
            set: (value) => prefs.set("oxbow.openIn.editor", String(value)),
          },
        }),
        prefRow("oxbow.openIn.terminal", "Terminal", "popup", {
          sub: () => (model.git && !model.git.terminals.length ? "No terminal Oxbow knows is installed" : "Right-click the repository in the sidebar › Open in Terminal"),
          off: () => !model.git?.terminals.length,
          control: {
            type: "popup",
            options: () => apps(model.git?.terminals),
            get: () => prefs.get("oxbow.openIn.terminal") || model.git?.terminals[0]?.id || "",
            set: (value) => prefs.set("oxbow.openIn.terminal", String(value)),
          },
        }),
      ],
    },
  ],
};
