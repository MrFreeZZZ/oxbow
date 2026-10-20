// Oxbow's window-wide keyboard shortcuts in one place: the same keys as VS Code wherever VS Code
// has the command, so its users need learn nothing new; Oxbow's own keys only where VS Code has
// nothing like it. The Command Palette, tooltips and menus show the first combo of each.
// Keys are matched by the letter typed, and by the key's place when the layout types no Latin
// letter (Russian, Greek…), so ⌘P is ⌘P whatever the layout, as in VS Code. keys.ts shows them.

export const mac = typeof navigator !== "undefined" && navigator.platform.startsWith("Mac");

export type ShortcutId =
  | "palette"
  | "quickOpen"
  | "settings"
  | "sidebar"
  | "details"
  | "expand"
  | "console"
  | "changes"
  | "find"
  | "findCode"
  | "findNext"
  | "findPrevious"
  | "undo"
  | "redo"
  | "fixMistake"
  | "reload"
  | "openRecent"
  | "open"
  | "clone"
  | "newRepo"
  | "closeRepo"
  | "nextTab"
  | "previousTab"
  | "zoomIn"
  | "zoomOut"
  | "zoomReset";

type Binding = { mac: string[]; other: string[] };

/** VS Code's name for the command each shortcut has there, for the list of keys. */
export const BINDINGS: Record<ShortcutId, Binding> = {
  // Show All Commands.
  palette: { mac: ["Mod+Shift+P", "F1"], other: ["Mod+Shift+P", "F1"] },
  // Go to File.
  quickOpen: { mac: ["Mod+P"], other: ["Mod+P"] },
  // Open Settings.
  settings: { mac: ["Mod+,"], other: ["Mod+,"] },
  // Toggle Primary Side Bar Visibility. ⌃⌘S, as in Mail, still works on the Mac.
  sidebar: { mac: ["Mod+B", "Ctrl+Mod+S"], other: ["Mod+B"] },
  // Toggle Secondary Side Bar Visibility: the details are the side bar on the right.
  details: { mac: ["Alt+Mod+B", "Ctrl+Mod+I"], other: ["Ctrl+Alt+B"] },
  // Toggle Maximize Editor Group.
  expand: { mac: ["Mod+K Mod+M", "Ctrl+Mod+E"], other: ["Mod+K Mod+M"] },
  // Show Output, where VS Code lists the git commands it ran; Toggle Terminal too, the other
  // place people look for them.
  console: { mac: ["Mod+Shift+U", "Ctrl+`", "Alt+Mod+C"], other: ["Mod+Shift+U", "Ctrl+`"] },
  // Show Source Control.
  changes: { mac: ["Ctrl+Shift+G"], other: ["Mod+Shift+G"] },
  // Find, Find in Files (here: in the changed code of every commit), Find Next, Find Previous.
  find: { mac: ["Mod+F"], other: ["Mod+F"] },
  findCode: { mac: ["Mod+Shift+F"], other: ["Mod+Shift+F"] },
  findNext: { mac: ["Mod+G"], other: ["F3"] },
  findPrevious: { mac: ["Mod+Shift+G"], other: ["Shift+F3"] },
  // Undo and Redo: the steps of the Operation Log, outside text fields.
  undo: { mac: ["Mod+Z"], other: ["Mod+Z"] },
  redo: { mac: ["Mod+Shift+Z"], other: ["Mod+Y", "Mod+Shift+Z"] },
  // Oxbow's own.
  fixMistake: { mac: ["Alt+Mod+Z"], other: ["Alt+Mod+Z"] },
  // Reload Window: ⌘R as in Safari; F5 elsewhere, where VS Code's Ctrl+R opens recent folders.
  reload: { mac: ["Mod+R"], other: ["F5"] },
  // Open Recent.
  openRecent: { mac: ["Ctrl+R"], other: ["Mod+R"] },
  // Open Folder, Git: Clone (Oxbow's own key), New File (here: a new repository), Close Folder.
  open: { mac: ["Mod+O"], other: ["Mod+O"] },
  clone: { mac: ["Mod+Shift+C"], other: ["Mod+Shift+C"] },
  newRepo: { mac: ["Mod+N"], other: ["Mod+N"] },
  closeRepo: { mac: ["Mod+K F"], other: ["Mod+K F"] },
  // Open Next Editor and Open Previous Editor: the tabs of the details panel.
  nextTab: { mac: ["Alt+Mod+Right", "Ctrl+Tab"], other: ["Mod+PageDown", "Ctrl+Tab"] },
  previousTab: { mac: ["Alt+Mod+Left", "Ctrl+Shift+Tab"], other: ["Mod+PageUp", "Ctrl+Shift+Tab"] },
  // Zoom In, Zoom Out, Reset Zoom.
  zoomIn: { mac: ["Mod+=", "Mod+Shift+="], other: ["Mod+=", "Mod+Shift+="] },
  zoomOut: { mac: ["Mod+-"], other: ["Mod+-"] },
  zoomReset: { mac: ["Mod+0"], other: ["Mod+0"] },
};

const ORDER = ["Ctrl", "Alt", "Shift", "Mod"];

/** A combo written the one way strokes are compared: modifiers in a fixed order, and Ctrl as
 *  Mod off the Mac, where they are the same key. */
function normal(combo: string, onMac: boolean): string {
  const parts = combo.split("+").map((part) => (!onMac && part === "Ctrl" ? "Mod" : part));
  const key = parts.pop() || "+";
  return [...ORDER.filter((m) => parts.includes(m)), key.length === 1 ? key.toUpperCase() : key].join("+");
}

const NAMED: Record<string, string> = { ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right", " ": "Space" };
const CODES: Record<string, string> = { Comma: ",", Period: ".", Equal: "=", Minus: "-", Backquote: "`", Slash: "/", Semicolon: ";", Quote: "'", BracketLeft: "[", BracketRight: "]", Backslash: "\\" };

/** The key of a key press as combos name it, or null for a modifier on its own. */
function keyOf(event: KeyboardEvent): string | null {
  const { key, code } = event;
  if (["Meta", "Control", "Alt", "Shift", "CapsLock", "Fn"].includes(key)) return null;
  // Shift+Tab comes as ISO_Left_Tab on Linux, which names no key; its code is still Tab.
  if (code === "Tab") return "Tab";
  if (/^[a-z]$/i.test(key)) return key.toUpperCase();
  // Another alphabet, or ⌥ turning the letter into a sign on the Mac: the key's place tells.
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1];
  const digit = /^Digit(\d)$/.exec(code);
  if (digit) return digit[1];
  if (/^[,.=\-`/;'[\]\\]$/.test(key)) return key;
  if (code in CODES) return CODES[code];
  return NAMED[key] ?? key;
}

/** A key press as a combo, "Shift+Mod+P", or null for a modifier on its own. */
export function strokeOf(event: KeyboardEvent, onMac = mac): string | null {
  const key = keyOf(event);
  if (!key) return null;
  const mods: string[] = [];
  if (onMac ? event.ctrlKey : false) mods.push("Ctrl");
  if (event.altKey) mods.push("Alt");
  if (event.shiftKey) mods.push("Shift");
  if (onMac ? event.metaKey : event.ctrlKey) mods.push("Mod");
  // The Windows key and the like are no part of any combo.
  if (!onMac && event.metaKey) mods.push("Meta");
  return [...mods, key].join("+");
}

/** What a key press asks for. "chord" is the first half of a chord, which waits for the second. */
export type Hit = { id: ShortcutId } | { chord: string } | { missed: string } | null;

export class Keymap {
  readonly onMac: boolean;
  #strokes: { id: ShortcutId; strokes: string[] }[];
  #pending: { stroke: string; at: number } | null = null;

  constructor(onMac = mac) {
    this.onMac = onMac;
    this.#strokes = (Object.keys(BINDINGS) as ShortcutId[]).flatMap((id) =>
      BINDINGS[id][onMac ? "mac" : "other"].map((combo) => ({ id, strokes: combo.split(" ").map((c) => normal(c, onMac)) })),
    );
  }

  /** The shortcut a key press is, if any. The second key of a chord must come within 3 seconds. */
  match(event: KeyboardEvent, now = Date.now()): Hit {
    const stroke = strokeOf(event, this.onMac);
    if (!stroke) return null;
    const pending = this.#pending && now - this.#pending.at < 3000 ? this.#pending.stroke : null;
    this.#pending = null;
    if (pending) {
      const done = this.#strokes.find((s) => s.strokes.length === 2 && s.strokes[0] === pending && s.strokes[1] === stroke);
      return done ? { id: done.id } : { missed: `${pending} ${stroke}` };
    }
    if (this.#strokes.some((s) => s.strokes.length === 2 && s.strokes[0] === stroke)) {
      this.#pending = { stroke, at: now };
      return { chord: stroke };
    }
    const single = this.#strokes.find((s) => s.strokes.length === 1 && s.strokes[0] === stroke);
    return single ? { id: single.id } : null;
  }
}

/** The combo shown for a shortcut, "Mod+Shift+P", for keys() and withKeys(). */
export function combo(id: ShortcutId): string {
  return BINDINGS[id][mac ? "mac" : "other"][0];
}
