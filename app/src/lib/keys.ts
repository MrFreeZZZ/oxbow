// Keyboard shortcuts as people read them, the way VS Code shows them: "⇧⌘C" on the Mac,
// "Ctrl+Shift+C" elsewhere. Combos are written "Mod+Shift+C", where Mod is ⌘ on the Mac and Ctrl
// elsewhere; a chord is two of them with a space between, "Mod+K Mod+M".

import { combo, mac, type ShortcutId } from "./shortcuts";

export { mac };

const macSymbols: Record<string, string> = { Ctrl: "⌃", Alt: "⌥", Shift: "⇧", Mod: "⌘", Enter: "↩", Up: "↑", Down: "↓", Left: "←", Right: "→", Escape: "Esc" };
const otherNames: Record<string, string> = { Mod: "Ctrl", Up: "↑", Down: "↓", Left: "←", Right: "→", Escape: "Esc" };
/** The Mac's order of modifiers. */
const macOrder = ["Ctrl", "Alt", "Shift", "Mod"];

export function keys(combo: string): string {
  return combo.split(" ").map(stroke).join(" ");
}

function stroke(combo: string): string {
  const parts = combo.split("+");
  // "Mod++" is Mod and the plus key.
  const key = parts.pop() || "+";
  if (mac) {
    const mods = macOrder.filter((m) => parts.includes(m)).map((m) => macSymbols[m]);
    return mods.join("") + (macSymbols[key] ?? key);
  }
  const mods = ["Mod", "Ctrl", "Shift", "Alt"].filter((m) => parts.includes(m)).map((m) => otherNames[m] ?? m);
  return [...new Set(mods), otherNames[key] ?? key].join("+");
}

/** A tooltip with its shortcut: "Settings (⌘,)". */
export function withKeys(label: string, combo: string): string {
  return `${label} (${keys(combo)})`;
}

/** A window-wide shortcut as people read it: "⇧⌘P", "Ctrl+Shift+P". */
export function shortcut(id: ShortcutId): string {
  return keys(combo(id));
}
