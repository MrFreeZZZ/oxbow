// Oxbow's commands in the macOS menu bar. The menus put each command where a Mac app has it;
// a command the window can't run now is greyed out, and its key is the one the Command Palette
// shows. Keys typed into text fields (a letter with ⌥, a key alone) stay out of the menus, or
// the menu bar would take them from the field.

import type { ShortcutId } from "./shortcuts";

/** A command, a separator ("-"), or Open Recent with its list of repositories ("recent"). */
type Entry = ShortcutId | "-" | "recent";

/** The menus and their commands. File, Edit and View are the system's, with these added. */
export const MENUS: { title: string; items: Entry[] }[] = [
  { title: "File", items: ["newRepo", "clone", "open", "recent", "-", "openWorktree", "openSubmodule", "-", "reload", "closeRepo", "-"] },
  { title: "Edit", items: ["find", "findCode", "findNext", "findPrevious"] },
  {
    title: "View",
    items: ["palette", "-", "sidebar", "details", "expand", "focusOnly", "-", "history", "changes", "stashes", "oplog", "activity", "console", "-", "diffLayout", "wrap", "whitespace", "-", "zoomIn", "zoomOut", "zoomReset"],
  },
  { title: "Go", items: ["back", "forward", "-", "quickOpen", "goToBranch", "goToTag", "goToStash", "-", "nextChange", "previousChange", "-", "nextTab", "previousTab"] },
  {
    title: "Repository",
    items: ["fetch", "pull", "pullRebase", "pullMerge", "push", "publish", "publishGitHub", "-", "commit", "stash", "stashStaged", "-", "branch", "checkout", "newWorktree", "editStack", "splitCommit", "resolve", "-", "savePatches", "applyPatches", "-", "fixMistake"],
  },
];

/** Shorter titles than the Command Palette's, where the menu already says what they are. */
const TITLES: Partial<Record<ShortcutId, string>> = {
  newRepo: "New Repository…",
  open: "Open…",
  palette: "Command Palette…",
  find: "Find in History",
  console: "Console",
  zoomReset: "Actual Size",
  back: "Back",
  forward: "Forward",
  nextChange: "Next Change",
  previousChange: "Previous Change",
  nextTab: "Next Tab",
  previousTab: "Previous Tab",
};

/** The ones the window has now: the title it gives them, if any. */
export type Available = Map<string, string | undefined>;

export type MenuCommand = { id: string; title: string; enabled: boolean; accelerator: string | null };
export type MenuPart = { title: string; items: MenuCommand[] };

/** The menus as the menu bar shows them now, from each command's registry title and keys. A
 *  title the window gives a command now ("Hide Sidebar") wins over the menu's own, but for "Go
 *  Back to Changes", too long for a menu. */
export function menuBar(available: Available, titleOf: (id: ShortcutId) => string, keysOf: (id: ShortcutId) => string[]): MenuPart[] {
  return MENUS.map((menu) => ({
    title: menu.title,
    items: menu.items.map((entry) => {
      if (entry === "-" || entry === "recent") return { id: entry, title: "", enabled: true, accelerator: null };
      const own = available.get(entry);
      const said = own !== titleOf(entry) && entry !== "back" && entry !== "forward" ? own : undefined;
      const title = said ?? TITLES[entry] ?? titleOf(entry);
      return { id: entry, title, enabled: available.has(entry), accelerator: menuKey(keysOf(entry)) };
    }),
  }));
}

/** The first of the keys a menu can show and take: held with ⌘ or ⌃, or a function key; a
 *  chord never. As a menu accelerator, "CmdOrCtrl+Shift+P". */
export function menuKey(combos: string[]): string | null {
  for (const combo of combos) {
    if (combo.includes(" ")) continue;
    const parts = combo.split("+");
    const key = parts[parts.length - 1];
    const held = parts.slice(0, -1);
    if (!held.includes("Mod") && !held.includes("Ctrl") && !/^F\d+$/.test(key)) continue;
    return [...held.map((m) => (m === "Mod" ? "CmdOrCtrl" : m)), key].join("+");
  }
  return null;
}
