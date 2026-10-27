// Oxbow's commands and their keyboard shortcuts in one place: the same keys as VS Code wherever
// VS Code has the command, so its users need learn nothing new; Oxbow's own keys only where VS
// Code has nothing like it. Settings › Keyboard Shortcuts puts your own keys over these
// (oxbow.keyboard.shortcuts in settings.json); the Command Palette, tooltips and menus show the
// first key of each, yours if you set one.
// Keys are matched by the letter typed, and by the key's place when the layout types no Latin
// letter (Russian, Greek…), so ⌘P is ⌘P whatever the layout, as in VS Code. keys.ts shows them.

export const mac = typeof navigator !== "undefined" && navigator.platform.startsWith("Mac");

/** Where a command's key works: anywhere in the window, or only on one screen, where it may be a
 *  key alone, as G, B and S while bisecting. Keys of two screens never meet, so they may match. */
export type Scope = "window" | "changes" | "files" | "editStack" | "bisect";

type Command = {
  /** The Command Palette's "Category: Title", and the group in Settings. */
  category: string;
  title: string;
  /** The keys it comes with, the first one shown; none when only your own key runs it. */
  mac?: string[];
  other?: string[];
  scope?: Scope;
};

/** Every command a key can run. The comments name VS Code's command with the same key. */
export const COMMANDS = {
  // Show All Commands.
  palette: { category: "View", title: "Show All Commands", mac: ["Mod+Shift+P", "F1"], other: ["Mod+Shift+P", "F1"] },
  // Open Settings.
  settings: { category: "Preferences", title: "Open Settings", mac: ["Mod+,"], other: ["Mod+,"] },
  // Open Folder, Open Recent, Close Folder; Reload Window: ⌘R as in Safari, F5 elsewhere, where
  // VS Code's Ctrl+R opens recent folders.
  open: { category: "File", title: "Open Repository…", mac: ["Mod+O"], other: ["Mod+O"] },
  openRecent: { category: "File", title: "Open Recent…", mac: ["Ctrl+R"], other: ["Mod+R"] },
  closeRepo: { category: "File", title: "Close Repository", mac: ["Mod+K F"], other: ["Mod+K F"] },
  reload: { category: "File", title: "Reload Repository", mac: ["Mod+R"], other: ["F5"] },
  openWorktree: { category: "File", title: "Open Worktree…" },
  openSubmodule: { category: "File", title: "Open Submodule…" },
  // Go to File; Go to Symbol in Workspace: here the branches, tags, stashes, worktrees and submodules.
  quickOpen: { category: "Go", title: "Go to File History…", mac: ["Mod+P"], other: ["Mod+P"] },
  goToBranch: { category: "Go", title: "Go to Branch…", mac: ["Mod+T"], other: ["Mod+T"] },
  goToTag: { category: "Go", title: "Go to Tag…" },
  goToStash: { category: "Go", title: "Go to Stash…" },
  // Go Back and Go Forward: VS Code's keys, then ⌘[ and ⌘] as in Safari, Finder and Xcode.
  back: { category: "Go", title: "Go Back", mac: ["Ctrl+-", "Mod+["], other: ["Alt+Left", "Ctrl+Alt+-"] },
  forward: { category: "Go", title: "Go Forward", mac: ["Ctrl+Shift+-", "Mod+]"], other: ["Alt+Right", "Ctrl+Shift+-"] },
  // Go to Next Change and Go to Previous Change, of the diff on screen.
  nextChange: { category: "Go", title: "Go to Next Change", mac: ["Alt+F5"], other: ["Alt+F5"] },
  previousChange: { category: "Go", title: "Go to Previous Change", mac: ["Alt+Shift+F5"], other: ["Alt+Shift+F5"] },
  // Find, Find in Files (here: in the changed code of every commit), Find Next, Find Previous.
  find: { category: "Search", title: "Find in History", mac: ["Mod+F"], other: ["Mod+F"] },
  findCode: { category: "Search", title: "Find in Changed Code", mac: ["Mod+Shift+F"], other: ["Mod+Shift+F"] },
  findStashes: { category: "Search", title: "Find in Stashes" },
  findNext: { category: "Search", title: "Find Next", mac: ["Mod+G"], other: ["F3"] },
  findPrevious: { category: "Search", title: "Find Previous", mac: ["Mod+Shift+G"], other: ["Shift+F3"] },
  // Toggle Primary Side Bar Visibility; ⌃⌘S, as in Mail, still works on the Mac.
  sidebar: { category: "View", title: "Toggle Sidebar", mac: ["Mod+B", "Ctrl+Mod+S"], other: ["Mod+B"] },
  // Toggle Secondary Side Bar Visibility: the details are the side bar on the right.
  details: { category: "View", title: "Toggle Details", mac: ["Alt+Mod+B", "Ctrl+Mod+I"], other: ["Ctrl+Alt+B"] },
  // Toggle Maximize Editor Group.
  expand: { category: "View", title: "Toggle Expanded Details", mac: ["Mod+K Mod+M", "Ctrl+Mod+E"], other: ["Mod+K Mod+M"] },
  // Open Next Editor and Open Previous Editor: the tabs of the details panel.
  nextTab: { category: "View", title: "Open Next Tab", mac: ["Alt+Mod+Right", "Ctrl+Tab"], other: ["Mod+PageDown", "Ctrl+Tab"] },
  previousTab: { category: "View", title: "Open Previous Tab", mac: ["Alt+Mod+Left", "Ctrl+Shift+Tab"], other: ["Mod+PageUp", "Ctrl+Shift+Tab"] },
  // Show Output, where VS Code lists the git commands it ran; Toggle Terminal too, the other
  // place people look for them.
  console: { category: "View", title: "Show Console", mac: ["Mod+Shift+U", "Ctrl+`", "Alt+Mod+C"], other: ["Mod+Shift+U", "Ctrl+`"] },
  // Show Source Control.
  changes: { category: "View", title: "Show Changes", mac: ["Ctrl+Shift+G"], other: ["Mod+Shift+G"] },
  history: { category: "View", title: "Show History" },
  stashes: { category: "View", title: "Show Stashes" },
  oplog: { category: "View", title: "Show Operation Log" },
  activity: { category: "View", title: "Show Activity" },
  focusOnly: { category: "View", title: "Toggle Current Branch Only" },
  diffLayout: { category: "View", title: "Toggle Side by Side Diffs" },
  // View: Toggle Word Wrap, for the diffs.
  wrap: { category: "View", title: "Toggle Word Wrap", mac: ["Alt+Z"], other: ["Alt+Z"] },
  whitespace: { category: "View", title: "Toggle Ignore Whitespace" },
  // Zoom In, Zoom Out, Reset Zoom.
  zoomIn: { category: "View", title: "Zoom In", mac: ["Mod+=", "Mod+Shift+="], other: ["Mod+=", "Mod+Shift+="] },
  zoomOut: { category: "View", title: "Zoom Out", mac: ["Mod+-"], other: ["Mod+-"] },
  zoomReset: { category: "View", title: "Reset Zoom", mac: ["Mod+0"], other: ["Mod+0"] },
  fetch: { category: "Git", title: "Fetch" },
  pull: { category: "Git", title: "Pull" },
  pullRebase: { category: "Git", title: "Pull (Rebase)" },
  pullMerge: { category: "Git", title: "Pull (Merge)" },
  push: { category: "Git", title: "Push" },
  publish: { category: "Git", title: "Publish Branch…" },
  publishGitHub: { category: "Git", title: "Publish to GitHub…" },
  commit: { category: "Git", title: "Commit…" },
  stash: { category: "Git", title: "Stash" },
  checkout: { category: "Git", title: "Checkout to…" },
  branch: { category: "Git", title: "Create Branch…" },
  newWorktree: { category: "Git", title: "New Worktree…" },
  editStack: { category: "Git", title: "Edit Stack…" },
  resolve: { category: "Git", title: "Resolve Conflicts" },
  // Git: Clone (Oxbow's own key), and New File: here a new repository.
  clone: { category: "Git", title: "Clone…", mac: ["Mod+Shift+C"], other: ["Mod+Shift+C"] },
  newRepo: { category: "Git", title: "Initialize Repository…", mac: ["Mod+N"], other: ["Mod+N"] },
  // Undo and Redo: the steps of the Operation Log, outside text fields. Fix a Mistake is Oxbow's own.
  undo: { category: "Git", title: "Undo Last Step", mac: ["Mod+Z"], other: ["Mod+Z"] },
  redo: { category: "Git", title: "Redo", mac: ["Mod+Shift+Z"], other: ["Mod+Y", "Mod+Shift+Z"] },
  fixMistake: { category: "Git", title: "Fix a Mistake…", mac: ["Alt+Mod+Z"], other: ["Alt+Mod+Z"] },
  // The commit message's box: Git: Commit, as in VS Code's Source Control.
  commitNow: { category: "Changes", title: "Commit", mac: ["Mod+Enter"], other: ["Mod+Enter"], scope: "changes" },
  // The list of changed files, while it has the focus: ↑ and ↓ go through the files as in
  // Finder, Space stages the open file or takes it out, ⌥Space stages it and opens the next.
  nextFile: { category: "Changes", title: "Next File", mac: ["Down"], other: ["Down"], scope: "files" },
  previousFile: { category: "Changes", title: "Previous File", mac: ["Up"], other: ["Up"], scope: "files" },
  stageFile: { category: "Changes", title: "Stage or Unstage File", mac: ["Space"], other: ["Space"], scope: "files" },
  stageAndNext: { category: "Changes", title: "Stage & Next File", mac: ["Alt+Space"], other: ["Alt+Space"], scope: "files" },
  // Edit Stack: ⌘↩ runs it; ⌥↑ and ⌥↓ as VS Code's Move Line Up and Down; a letter per action,
  // as git's own todo list names them.
  stackRebase: { category: "Edit Stack", title: "Rebase Stack", mac: ["Mod+Enter"], other: ["Mod+Enter"], scope: "editStack" },
  stackUp: { category: "Edit Stack", title: "Move Commit Up", mac: ["Alt+Up"], other: ["Alt+Up"], scope: "editStack" },
  stackDown: { category: "Edit Stack", title: "Move Commit Down", mac: ["Alt+Down"], other: ["Alt+Down"], scope: "editStack" },
  stackPick: { category: "Edit Stack", title: "Pick", mac: ["P"], other: ["P"], scope: "editStack" },
  stackReword: { category: "Edit Stack", title: "Reword", mac: ["R"], other: ["R"], scope: "editStack" },
  stackEdit: { category: "Edit Stack", title: "Edit", mac: ["E"], other: ["E"], scope: "editStack" },
  stackSquash: { category: "Edit Stack", title: "Squash", mac: ["S"], other: ["S"], scope: "editStack" },
  stackFixup: { category: "Edit Stack", title: "Fixup", mac: ["F"], other: ["F"], scope: "editStack" },
  stackDrop: { category: "Edit Stack", title: "Drop", mac: ["D"], other: ["D"], scope: "editStack" },
  // Bisecting: the commit under test is good, bad, or can't be tested.
  bisectGood: { category: "Bisect", title: "Good", mac: ["G"], other: ["G"], scope: "bisect" },
  bisectBad: { category: "Bisect", title: "Bad", mac: ["B"], other: ["B"], scope: "bisect" },
  bisectSkip: { category: "Bisect", title: "Skip", mac: ["S"], other: ["S"], scope: "bisect" },
} satisfies Record<string, Command>;

export type ShortcutId = keyof typeof COMMANDS;

export const commandIds = Object.keys(COMMANDS) as ShortcutId[];
export const commandOf = (id: ShortcutId): Command => COMMANDS[id];
export const isCommand = (id: string): id is ShortcutId => Object.hasOwn(COMMANDS, id);
export const scopeOf = (id: ShortcutId): Scope => commandOf(id).scope ?? "window";

/** Your own keys by command, over the defaults: `[]` takes a command's keys away. */
export type Custom = Partial<Record<string, string[]>>;

let custom: () => Custom = () => ({});

/** Where your own keys come from: settings.json, set once by prefs. Read on every look, so a
 *  change shows at once in the tooltips, the palette and the keys that work. */
export function useCustom(read: () => Custom) {
  custom = read;
}

/** The keys a command comes with. */
export function defaultsOf(id: ShortcutId, onMac = mac): string[] {
  return commandOf(id)[onMac ? "mac" : "other"] ?? [];
}

/** The keys a command has now: yours if you set any, else its defaults. */
export function bindingsOf(id: ShortcutId, mine: Custom = custom(), onMac = mac): string[] {
  const own = Object.hasOwn(mine, id) ? mine[id] : undefined;
  return Array.isArray(own) ? own.filter((c) => validCombo(c)) : defaultsOf(id, onMac);
}

/** Whether you changed the command's keys. */
export function changedKeys(id: ShortcutId, mine: Custom = custom(), onMac = mac): boolean {
  return !same(bindingsOf(id, mine, onMac), defaultsOf(id, onMac));
}

const same = (a: string[], b: string[]) => a.length === b.length && a.every((c, i) => c === b[i]);

/** The combo shown for a command, "Mod+Shift+P", for keys() and withKeys(); null without one. */
export function combo(id: ShortcutId): string | null {
  return bindingsOf(id)[0] ?? null;
}

const ORDER = ["Ctrl", "Alt", "Shift", "Mod"];
const MODIFIERS = new Set([...ORDER, "Meta"]);
const NAMED_KEYS = new Set(["Up", "Down", "Left", "Right", "Enter", "Escape", "Tab", "Space", "Backspace", "Delete", "Home", "End", "PageUp", "PageDown", "Insert"]);

/** Whether a key, as combos name it, is one Oxbow can match. */
const knownKey = (key: string) => /^[A-Z0-9,.=\-`/;'[\]\\]$/.test(key) || /^F([1-9]|1\d|2[0-4])$/.test(key) || NAMED_KEYS.has(key);

/** Whether a combo is written the way Oxbow reads it: "Mod+Shift+P", or a chord "Mod+K Mod+M". */
export function validCombo(value: unknown): value is string {
  if (typeof value !== "string") return false;
  const strokes = value.split(" ");
  return (
    strokes.length >= 1 &&
    strokes.length <= 2 &&
    strokes.every((s) => {
      const parts = s.split("+");
      const key = parts.pop() ?? "";
      return knownKey(key.length === 1 ? key.toUpperCase() : key) && parts.every((m) => MODIFIERS.has(m)) && new Set(parts).size === parts.length;
    })
  );
}

/** A combo written the one way strokes are compared: modifiers in a fixed order, and Ctrl as
 *  Mod off the Mac, where they are the same key. */
function normal(combo: string, onMac: boolean): string {
  const parts = combo.split("+").map((part) => (!onMac && part === "Ctrl" ? "Mod" : part));
  const key = parts.pop() || "+";
  return [...[...ORDER, "Meta"].filter((m) => parts.includes(m)), key.length === 1 ? key.toUpperCase() : key].join("+");
}

const strokesOf = (combo: string, onMac: boolean) => combo.split(" ").map((c) => normal(c, onMac));

const NAMED: Record<string, string> = { ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right", " ": "Space" };
const CODES: Record<string, string> = { Comma: ",", Period: ".", Equal: "=", Minus: "-", Backquote: "`", Slash: "/", Semicolon: ";", Quote: "'", BracketLeft: "[", BracketRight: "]", Backslash: "\\" };

/** The key of a key press as combos name it, or null for a modifier on its own. */
function keyOf(event: KeyboardEvent): string | null {
  const { key, code } = event;
  if (["Meta", "Control", "Alt", "Shift", "CapsLock", "Fn"].includes(key)) return null;
  // Shift+Tab comes as ISO_Left_Tab on Linux, which names no key; its code is still Tab.
  if (code === "Tab") return "Tab";
  // ⌥Space types a no-break space on the Mac.
  if (code === "Space") return "Space";
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

/** The window's keys: the commands of scope "window", with your own keys over the defaults. */
export class Keymap {
  readonly onMac: boolean;
  #mine: () => Custom;
  #built: { from: Custom; strokes: { id: ShortcutId; strokes: string[] }[] } | null = null;
  #pending: { stroke: string; at: number } | null = null;

  constructor(onMac = mac, mine: () => Custom = () => custom()) {
    this.onMac = onMac;
    this.#mine = mine;
  }

  /** Built again whenever your keys change. */
  get #strokes() {
    const mine = this.#mine();
    if (this.#built?.from !== mine) {
      const strokes = commandIds
        .filter((id) => scopeOf(id) === "window")
        .flatMap((id) => bindingsOf(id, mine, this.onMac).map((c) => ({ id, strokes: strokesOf(c, this.onMac) })));
      this.#built = { from: mine, strokes };
    }
    return this.#built.strokes;
  }

  /** The shortcut a key press is, if any. The second key of a chord must come within 3 seconds. */
  match(event: KeyboardEvent, now = Date.now()): Hit {
    const stroke = strokeOf(event, this.onMac);
    if (!stroke) return null;
    const all = this.#strokes;
    const pending = this.#pending && now - this.#pending.at < 3000 ? this.#pending.stroke : null;
    this.#pending = null;
    if (pending) {
      const done = all.find((s) => s.strokes.length === 2 && s.strokes[0] === pending && s.strokes[1] === stroke);
      return done ? { id: done.id } : { missed: `${pending} ${stroke}` };
    }
    if (all.some((s) => s.strokes.length === 2 && s.strokes[0] === stroke)) {
      this.#pending = { stroke, at: now };
      return { chord: stroke };
    }
    const single = all.find((s) => s.strokes.length === 1 && s.strokes[0] === stroke);
    return single ? { id: single.id } : null;
  }
}

/** Whether a key press is one of a screen's commands, e.g. ⌘↩ in the commit message. */
export function pressed(id: ShortcutId, event: KeyboardEvent, mine: Custom = custom(), onMac = mac): boolean {
  const stroke = strokeOf(event, onMac);
  return !!stroke && bindingsOf(id, mine, onMac).some((c) => !c.includes(" ") && normal(c, onMac) === stroke);
}

/** Whether a combo starts with a key press, for finding commands by their keys. */
export function startsWith(combo: string, stroke: string, onMac = mac): boolean {
  return strokesOf(combo, onMac)[0] === normal(stroke, onMac);
}

// --- Your own keys ------------------------------------------------------------------------------

/** Keys the system or the menus keep, which no command can have. */
const RESERVED: { mac: Record<string, string>; other: Record<string, string> } = {
  mac: {
    "Mod+Q": "quits Oxbow",
    "Mod+H": "hides Oxbow",
    "Alt+Mod+H": "hides the other apps",
    "Mod+M": "minimizes the window",
    "Mod+W": "closes the window",
    "Ctrl+Mod+F": "turns full screen on and off",
    "Mod+C": "copies",
    "Mod+V": "pastes",
    "Mod+X": "cuts",
    "Mod+A": "selects all",
  },
  other: { "Alt+F4": "closes the window", "Mod+C": "copies", "Mod+V": "pastes", "Mod+X": "cuts", "Mod+A": "selects all" },
};

/** Why a combo can't be a command's key, or null when it can. */
export function refusal(id: ShortcutId, combo: string, onMac = mac): string | null {
  const strokes = strokesOf(combo, onMac);
  const reserved = Object.entries(RESERVED[onMac ? "mac" : "other"]).find(([c]) => strokes.includes(normal(c, onMac)));
  if (reserved) return `The system keeps it: it ${reserved[1]}`;
  if (scopeOf(id) !== "window" && strokes.length > 1) return "A screen’s keys are one press, not a chord";
  // Keys alone are for screens without a text field, such as Edit Stack.
  if (scopeOf(id) !== "window" && scopeOf(id) !== "changes") return null;
  const first = strokes[0].split("+");
  const key = first[first.length - 1];
  const held = first.some((m) => m === "Mod" || m === "Ctrl" || m === "Alt");
  if (!held && !/^F\d+$/.test(key)) return `Add ${onMac ? "⌘, ⌃ or ⌥" : "Ctrl or Alt"}: a key alone types in text fields`;
  return null;
}

/** Whether two commands' keys can meet: on the same screen, or one of them anywhere. */
const meet = (a: ShortcutId, b: ShortcutId) => scopeOf(a) === "window" || scopeOf(b) === "window" || scopeOf(a) === scopeOf(b);

/** Whether two combos clash: the same, or one is the first key of the other's chord. */
function clash(a: string, b: string, onMac: boolean): boolean {
  const x = strokesOf(a, onMac);
  const y = strokesOf(b, onMac);
  return x.length === y.length ? x.join(" ") === y.join(" ") : x[0] === y[0];
}

/** The other commands a combo would take a key from, with the key each loses. */
export function conflicts(id: ShortcutId, combo: string, mine: Custom = custom(), onMac = mac): { id: ShortcutId; combo: string }[] {
  return commandIds.flatMap((other) =>
    other === id || !meet(id, other)
      ? []
      : bindingsOf(other, mine, onMac)
          .filter((c) => clash(c, combo, onMac))
          .map((c) => ({ id: other, combo: c })),
  );
}

/** Your keys once a command's are `list`: kept only while they differ from its defaults. */
function withKeys(mine: Custom, id: ShortcutId, list: string[], onMac: boolean): Custom {
  const next = { ...mine };
  if (same(list, defaultsOf(id, onMac))) delete next[id];
  else next[id] = list;
  return next;
}

/** Your keys with `combo` given to a command, in place of `replacing` or after its keys, and
 *  taken from the commands it clashed with. */
export function assign(id: ShortcutId, combo: string, mine: Custom = custom(), onMac = mac, replacing?: string): Custom {
  let next = mine;
  for (const lost of conflicts(id, combo, mine, onMac)) {
    next = withKeys(next, lost.id, bindingsOf(lost.id, next, onMac).filter((c) => c !== lost.combo), onMac);
  }
  const current = bindingsOf(id, next, onMac);
  const at = replacing === undefined ? -1 : current.indexOf(replacing);
  const list = at < 0 ? [...current, combo] : current.map((c, i) => (i === at ? combo : c));
  return withKeys(next, id, list.filter((c, i) => list.indexOf(c) === i), onMac);
}

/** Your keys without one of a command's keys. */
export function unassign(id: ShortcutId, combo: string, mine: Custom = custom(), onMac = mac): Custom {
  return withKeys(mine, id, bindingsOf(id, mine, onMac).filter((c) => c !== combo), onMac);
}

/** Your keys with a command back to its defaults. */
export function resetKeys(id: ShortcutId, mine: Custom = custom()): Custom {
  const next = { ...mine };
  delete next[id];
  return next;
}

/** A combo as a menu accelerator, "CmdOrCtrl+,"; null for a chord, which menus can't show. */
export function accelerator(combo: string | null): string | null {
  if (!combo || combo.includes(" ")) return null;
  return combo
    .split("+")
    .map((part, i, parts) => (i < parts.length - 1 && part === "Mod" ? "CmdOrCtrl" : part))
    .join("+");
}
