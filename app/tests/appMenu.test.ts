import { MENUS, menuBar, menuKey } from "../src/lib/appMenu.ts";
import { bindingsOf, commandOf, defaultsOf, type ShortcutId } from "../src/lib/shortcuts.ts";
import { readFileSync, writeFileSync } from "node:fs";
import assert from "node:assert/strict";

const title = (id: ShortcutId) => commandOf(id).title;

// A menu takes keys held with ⌘ or ⌃, and function keys; a chord or a letter with ⌥ never.
assert.equal(menuKey(["Mod+Shift+P", "F1"]), "CmdOrCtrl+Shift+P");
assert.equal(menuKey(["Ctrl+-", "Mod+["]), "Ctrl+-");
assert.equal(menuKey(["Alt+F5"]), "Alt+F5");
assert.equal(menuKey(["Alt+Z"]), null);
assert.equal(menuKey(["Mod+K F"]), null);
assert.equal(menuKey(["Mod+K Mod+M", "Ctrl+Mod+E"]), "Ctrl+CmdOrCtrl+E");
assert.equal(menuKey([]), null);

// Every command in the menus is one the registry has.
for (const menu of MENUS) for (const entry of menu.items) if (entry !== "-" && entry !== "recent") assert.ok(commandOf(entry as ShortcutId), entry);

// What the window can't run is greyed out; a title the window gives wins, but Back's long one.
{
  const bar = menuBar(new Map([["sidebar", "Hide Sidebar"], ["back", "Go Back to Changes"], ["fetch", "Fetch"]]), title, (id) => bindingsOf(id, {}, true));
  const item = (id: string) => bar.flatMap((m) => m.items).find((i) => i.id === id)!;
  assert.deepEqual(item("sidebar"), { id: "sidebar", title: "Hide Sidebar", enabled: true, accelerator: "CmdOrCtrl+B" });
  assert.equal(item("back").title, "Back");
  assert.equal(item("fetch").enabled, true);
  assert.equal(item("push").enabled, false);
  assert.equal(item("newRepo").title, "New Repository…");
  assert.equal(item("wrap").accelerator, null);
}
// Your own keys show in the menu.
{
  const bar = menuBar(new Map(), title, (id) => bindingsOf(id, { fetch: ["Mod+Shift+L"] }, true));
  assert.equal(bar.flatMap((m) => m.items).find((i) => i.id === "fetch")!.accelerator, "CmdOrCtrl+Shift+L");
}
// The menu bar has these until the first window loads (app/src-tauri/src/menu.json): every
// command greyed out, with the Mac's own keys. `OXBOW_WRITE_MENU=1 npm test` writes it again.
{
  const file = new URL("../src-tauri/src/menu.json", import.meta.url);
  const atStart = menuBar(new Map(), title, (id) => defaultsOf(id, true));
  const text = JSON.stringify(atStart, null, 1) + "\n";
  if (process.env.OXBOW_WRITE_MENU) writeFileSync(file, text);
  assert.equal(readFileSync(file, "utf8"), text, "menu.json differs from appMenu.ts: run OXBOW_WRITE_MENU=1 npm test");
}
