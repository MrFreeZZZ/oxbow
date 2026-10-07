// Keyboard shortcuts as people read them, the way VS Code shows them: "⇧⌘C" on the Mac,
// "Ctrl+Shift+C" elsewhere. Combos are written "Mod+Shift+C", where Mod is ⌘ on the Mac and Ctrl
// elsewhere.

export const mac = typeof navigator !== "undefined" && navigator.platform.startsWith("Mac");

const macSymbols: Record<string, string> = { Ctrl: "⌃", Alt: "⌥", Shift: "⇧", Mod: "⌘", Enter: "↩", Up: "↑", Down: "↓", Escape: "Esc" };
const otherNames: Record<string, string> = { Mod: "Ctrl", Up: "↑", Down: "↓", Escape: "Esc" };
/** The Mac's order of modifiers. */
const macOrder = ["Ctrl", "Alt", "Shift", "Mod"];

export function keys(combo: string): string {
  const parts = combo.split("+");
  const key = parts.pop()!;
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
