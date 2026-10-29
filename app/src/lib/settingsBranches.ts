// Settings › Git › Branches: how new branches are named and where they start, the prefixes the
// Name box offers, and git's own settings for branches.

import { confirm, type Request } from "./confirm.svelte";
import { menuIcons, type MenuEntry } from "./Menu.svelte";
import { prefs } from "./prefs.svelte";
import { BRANCH_PREFIXES, type BranchPrefix } from "./branchPrefixes";
import { config, gitEdit, gitSwitch, prefRow } from "./settingsModel.svelte";
import type { Row, Section } from "./settingsRows";

const prefixes = () => prefs.get("oxbow.branch.prefixes");
const saved = (list: BranchPrefix[]) => prefs.set("oxbow.branch.prefixes", list.map((p) => ({ ...p })));
const sameList = (a: BranchPrefix[], b: BranchPrefix[]) => JSON.stringify(a) === JSON.stringify(b);

/** The sheet that adds a prefix, or edits the one at `index`. */
function prefixRequest(index: number | null, draft: BranchPrefix): Request {
  const list = prefixes();
  const again = (change: Partial<BranchPrefix>) => prefixRequest(index, { ...draft, ...change });
  const typed = draft.prefix.trim();
  const prefix = typed && !typed.endsWith("/") ? `${typed}/` : typed;
  let invalid: string | null = null;
  if (!prefix) invalid = "Type a prefix";
  else if (/[\s~^:?*[\\]/.test(prefix) || prefix.startsWith("/") || prefix.includes("//")) invalid = "Git can’t take that in a branch name";
  else if (list.some((p, i) => i !== index && p.prefix === prefix)) invalid = "This prefix is in the list already";
  const item: BranchPrefix = { prefix, words: draft.words.trim(), ...(draft.from ? { from: draft.from } : {}) };
  return {
    title: index === null ? "Add a prefix" : `Edit ${list[index].prefix}`,
    body: ["The pop-up at the start of every Name box for a new branch offers it, with what it is for."],
    icon: "branch",
    button: index === null ? "Add Prefix" : "Save",
    always: true,
    invalid,
    fields: [
      {
        label: "Prefix",
        text: { value: draft.prefix, placeholder: "spike/", focus: true, edit: (value) => again({ prefix: value }) },
        error: typed && invalid ? invalid : undefined,
      },
      { label: "For", text: { value: draft.words, placeholder: "Trying an idea out", edit: (value) => again({ words: value }) } },
      {
        label: "Starts from",
        chips: [
          { label: "Where you are", on: !draft.from, pick: () => again({ from: undefined }) },
          { label: "Production branch", on: draft.from === "production", pick: () => again({ from: "production" }) },
        ],
        note: draft.from ? "main or master, or the one set in This Repository, at its latest commit" : "The current branch, or the branch or commit New Branch was opened on",
      },
    ],
    status: "Saving the prefix…",
    local: {
      commands: [],
      run: async () => {
        await prefs.save("oxbow.branch.prefixes", index === null ? [...list, item] : list.map((p, i) => (i === index ? item : { ...p })));
        return index === null ? `Added ${prefix}.` : `Saved ${prefix}.`;
      },
    },
  };
}

function moved(list: BranchPrefix[], index: number, by: number): BranchPrefix[] {
  const copy = list.map((p) => ({ ...p }));
  const [item] = copy.splice(index, 1);
  copy.splice(index + by, 0, item);
  return copy;
}

/** A row for each prefix, the row that adds one, and the way back to the default list. */
function prefixRows(): Row[] {
  const list = prefixes();
  const rows = list.map(
    (p, i): Row => ({
      id: `prefix:${p.prefix}`,
      label: p.prefix,
      mono: true,
      sub: () => [p.words, p.from === "production" && prefs.get("oxbow.branch.prefixStart") ? "starts from the production branch" : ""].filter(Boolean).join(" · "),
      control: { type: "button", label: "Edit…", run: () => confirm.run(prefixRequest(i, { ...p })) },
      more: () => {
        const entries: MenuEntry[] = [];
        if (i > 0) entries.push({ kind: "item", label: "Move Up", icon: menuIcons.up, run: () => saved(moved(list, i, -1)) });
        if (i < list.length - 1) entries.push({ kind: "item", label: "Move Down", icon: menuIcons.down, run: () => saved(moved(list, i, 1)) });
        if (entries.length) entries.push({ kind: "sep" });
        entries.push({ kind: "item", label: "Remove Prefix", icon: menuIcons.drop, danger: true, run: () => saved(list.filter((_, j) => j !== i)) });
        return entries;
      },
    }),
  );
  rows.push({
    id: "prefix:add",
    label: list.length ? "Add a prefix" : "No prefixes",
    sub: list.length ? undefined : "Name boxes have no pop-up, only the name",
    control: { type: "button", label: "Add…", run: () => confirm.run(prefixRequest(null, { prefix: "", words: "" })) },
  });
  if (!sameList(list, BRANCH_PREFIXES)) {
    rows.push({
      id: "prefix:default",
      label: "Default list",
      sub: BRANCH_PREFIXES.map((p) => p.prefix).join(" "),
      control: { type: "button", label: "Use", run: () => saved(BRANCH_PREFIXES) },
    });
  }
  return rows;
}

const cleanOff = () => !prefs.get("oxbow.branch.cleanNames");

export const branchesSection: Section = {
  id: "branches",
  label: "Branches",
  color: "var(--section-orange)",
  icon: "M3 3.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0-3 0M3 12.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0-3 0M10 5.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0-3 0M4.5 5v6M11.5 7c0 3-7 2-7 4",
  child: true,
  groups: () => [
    {
      title: "New branch names",
      rows: [
        prefRow("oxbow.branch.cleanNames", "Clean up names as you type", "switch", {
          sub: () =>
            prefs.get("oxbow.branch.cleanNames") ? "CSV Export becomes csv-export: spaces turn into dashes, and characters git refuses go" : "Names stay as typed; Create Branch waits until git can take one",
        }),
        prefRow("oxbow.branch.lowercase", "Make names lowercase", "switch", { sub: "Login-Timeout becomes login-timeout", off: cleanOff }),
        prefRow("oxbow.branch.firstPrefix", "A new name starts with", "popup", {
          sub: "The prefix in the Name box when the sheet opens",
          options: () => [{ value: "last", label: "The prefix used last" }, ...prefixes().map((p) => ({ value: p.prefix, label: p.prefix })), { value: "", label: "No prefix" }],
        }),
      ],
    },
    {
      title: "Prefixes",
      foot: "The pop-up at the start of the Name box offers them in this order, each with what it is for. A prefix typed by hand works too.",
      rows: [
        ...prefixRows(),
        prefRow("oxbow.branch.repoPrefixes", "Also offer the prefixes this repository uses", "switch", { sub: "Such as renovate/ or a teammate’s own, under the list, with how many branches have them" }),
      ],
    },
    {
      title: "Where a new branch starts",
      rows: [
        prefRow("oxbow.branch.prefixStart", "Let the prefix choose", "switch", {
          sub: () => {
            const from = prefixes().filter((p) => p.from === "production").map((p) => p.prefix);
            return from.length ? `${from.join(" and ")} start from the production branch, the rest where you are` : "No prefix starts from the production branch: set it with Edit";
          },
        }),
        prefRow("oxbow.branch.fetchFirst", "Fetch first", "switch", {
          sub: () =>
            prefs.get("oxbow.branch.fetchFirst")
              ? "A branch from master starts at origin/master’s latest commit, unless master has commits of its own"
              : "A branch starts where the local branch is, even when the remote has moved on",
        }),
        prefRow("oxbow.branch.uncommitted", "Uncommitted files", "seg", {
          sub: () =>
            prefs.get("oxbow.branch.uncommitted") === "bring"
              ? "They come along to the new branch, as with git switch -c"
              : "They stay behind in a stash named after the branch you leave; Apply it when you come back",
          options: () => [
            { value: "bring", label: "Come along" },
            { value: "stash", label: "Stay behind" },
          ],
        }),
      ],
    },
    {
      title: "Git",
      foot: "Saved in ~/.gitconfig. The production branch and protected branches are set for each repository in This Repository; branch colors in Themes.",
      rows: [
        gitEdit("global", "init.defaultBranch", "Default branch for new repositories", { mono: false, shown: () => config("global", "init.defaultBranch") ?? "master (Git’s default)", placeholder: "main" }),
        gitSwitch("global", "rebase.updateRefs", "Move stacked branches along when rebasing", true, { sub: "git rebase --update-refs: branches built on the rebased one follow it" }),
        gitSwitch("global", "fetch.prune", "Remove branches deleted on the remote when fetching", true),
      ],
    },
  ],
};
