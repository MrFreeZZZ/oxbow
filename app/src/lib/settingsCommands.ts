// Custom commands use the same rows, menus and edit sheet as Branches › Prefixes.
import { confirm, type Request } from "./confirm.svelte";
import { menuIcons, type MenuEntry } from "./Menu.svelte";
import { prefs } from "./prefs.svelte";
import type { Row, Section } from "./settingsRows";
import { commandsProblem, type UserCommand } from "./userCommands";

const commands = () => prefs.get("oxbow.commands");
const saved = (list: UserCommand[]) => prefs.set("oxbow.commands", list.map((c) => ({ ...c })));

function commandRequest(draft: UserCommand, adding: boolean): Request {
  const again = (change: Partial<UserCommand>) => commandRequest({ ...draft, ...change }, adding);
  const item = { ...draft, name: draft.name.trim(), repository: draft.repository.trim() };
  const invalid = !item.name ? "Type a name" : !item.script.trim() ? "Type a script" : commandsProblem([item]);
  return {
    title: adding ? "Add a command" : `Edit ${commands().find((c) => c.id === draft.id)?.name ?? draft.name}`,
    body: ["Save a personal command for explicit runs from Commands or the Command Palette."],
    icon: "edit",
    button: adding ? "Add Command" : "Save",
    always: true,
    invalid,
    fields: [
      { label: "Name", text: { value: draft.name, placeholder: "Run tests", focus: true, edit: (name) => again({ name }) } },
      { label: "Script", text: { value: draft.script, placeholder: "cargo test", multiline: true, edit: (script) => again({ script }) }, note: 'Quote context variables: "$OXBOW_REPOSITORY", "$OXBOW_BRANCH", "$OXBOW_COMMIT".' },
      { label: "Repository", text: { value: draft.repository, placeholder: "All repositories", edit: (repository) => again({ repository }) }, note: "Leave empty for all repositories, or enter the exact repository root path." },
    ],
    note: "Runs in the repository root without checkout. Restricted mode blocks commands. Script changes have no Oxbow Undo.",
    status: "Saving the command…",
    local: {
      commands: [],
      run: async () => {
        const list = commands();
        if (!adding && !list.some((c) => c.id === item.id)) throw new Error("This command was removed. Add it again to keep your changes.");
        const next = adding ? [...list, item] : list.map((c) => c.id === item.id ? item : { ...c });
        const problem = commandsProblem(next);
        if (problem) throw new Error(problem);
        await prefs.save("oxbow.commands", next);
        return `Saved ${item.name}.`;
      },
    },
  };
}

function moved(list: UserCommand[], index: number, by: number): UserCommand[] {
  const next = list.map((c) => ({ ...c }));
  const [item] = next.splice(index, 1);
  next.splice(index + by, 0, item);
  return next;
}

function commandRows(): Row[] {
  const list = commands();
  return [
    ...list.map((c, i): Row => ({
      id: `command:${c.id}`,
      label: c.name,
      sub: [c.script.replace(/\s+/g, " "), c.repository || "All repositories"].join(" · "),
      control: { type: "button", label: "Edit…", run: () => confirm.run(commandRequest({ ...c }, false)) },
      more: () => {
        const entries: MenuEntry[] = [];
        if (i > 0) entries.push({ kind: "item", label: "Move Up", icon: menuIcons.up, run: () => saved(moved(commands(), i, -1)) });
        if (i < list.length - 1) entries.push({ kind: "item", label: "Move Down", icon: menuIcons.down, run: () => saved(moved(commands(), i, 1)) });
        if (entries.length) entries.push({ kind: "sep" });
        entries.push({ kind: "item", label: "Remove Command", icon: menuIcons.drop, danger: true, run: () => saved(commands().filter((item) => item.id !== c.id)) });
        return entries;
      },
    })),
    {
      id: "command:add",
      label: list.length ? "Add a command" : "No commands",
      sub: list.length ? undefined : "Keep your own test, build and other scripts here",
      control: { type: "button", label: "Add…", run: () => confirm.run(commandRequest({ id: crypto.randomUUID(), name: "", script: "", repository: "" }, true)) },
    },
  ];
}

export const commandsSection: Section = {
  id: "commands",
  label: "Custom Commands",
  color: "var(--section-grey)",
  icon: "M3 4l4 4-4 4M9 12h4",
  groups: () => [{
    title: "Commands",
    rows: commandRows(),
    foot: "Run commands from Commands in the repository toolbar or the Command Palette. Assign keys in Keyboard Shortcuts. Output appears in Console; avoid secrets in scripts or output.",
  }],
};
