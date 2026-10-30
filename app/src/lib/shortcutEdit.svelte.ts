// Settings › Keyboard Shortcuts: your own keys, saved to settings.json over VS Code's. A key is
// recorded by pressing it; one a command already has is taken from it on Save, after a warning
// that names the command.

import { confirm, type Request } from "./confirm.svelte";
import { keys } from "./keys";
import { prefs } from "./prefs.svelte";
import {
  assign,
  bindingsOf,
  changedKeys,
  commandIds,
  commandOf,
  conflicts,
  defaultsOf,
  refusal,
  resetKeys,
  scopeOf,
  unassign,
  type Custom,
  type ShortcutId,
} from "./shortcuts";

const KEY = "oxbow.keyboard.shortcuts";

/** Your keys as settings.json has them, a copy to change. */
export const mine = (): Custom => $state.snapshot(prefs.get(KEY)) as Custom;

/** "View: Toggle Sidebar". */
export const nameOf = (id: ShortcutId) => `${commandOf(id).category}: ${commandOf(id).title}`;

const listed = (combos: string[]) => combos.map((c) => keys(c)).join(", ");

/** A line under a command, while its key is recorded or once you changed its keys. */
export type Note = { text: string; tone?: "warn" | "error" };

class ShortcutEdit {
  /** The command whose key is being pressed: a new key, or one in place of `replacing`. */
  recording = $state<{ id: ShortcutId; replacing?: string; strokes: string[] } | null>(null);

  /** The combo pressed so far, "Mod+K Mod+M". */
  get combo(): string | null {
    return this.recording?.strokes.length ? this.recording.strokes.join(" ") : null;
  }

  /** Why the combo pressed can't be the command's, if it can't. */
  get problem(): string | null {
    const r = this.recording;
    return r && this.combo ? refusal(r.id, this.combo) : null;
  }

  /** The commands that would lose a key to the one pressed. */
  get lost(): { id: ShortcutId; combo: string }[] {
    const r = this.recording;
    if (!r || !this.combo || this.problem) return [];
    // Pressing the key it has changes nothing.
    return conflicts(r.id, this.combo, mine()).filter((c) => !(c.id === r.id && c.combo === r.replacing));
  }

  start(id: ShortcutId, replacing?: string) {
    this.recording = { id, replacing, strokes: [] };
  }

  cancel() {
    this.recording = null;
  }

  /** A key pressed while recording. A window's command takes a second one as a chord, as ⌘K
   *  ⌘M; a third starts over, and so does any key after one that can't start a chord. */
  press(stroke: string) {
    const r = this.recording;
    if (!r) return;
    const chord = scopeOf(r.id) === "window" && r.strokes.length === 1 && !this.problem;
    this.recording = { ...r, strokes: chord ? [...r.strokes, stroke] : [stroke] };
  }

  async save() {
    const r = this.recording;
    const combo = this.combo;
    if (!r || !combo || this.problem) return;
    const lost = this.lost;
    this.recording = null;
    if (r.replacing === combo) return;
    const saved = await prefs.set(KEY, assign(r.id, combo, mine(), undefined, r.replacing));
    if (saved && lost.length) confirm.say(`${keys(combo)} now runs ${nameOf(r.id)}; ${lost.map((l) => `${nameOf(l.id)} lost ${keys(l.combo)}`).join(", ")}.`);
  }

  remove(id: ShortcutId, combo: string) {
    void prefs.set(KEY, unassign(id, combo, mine()));
  }

  reset(id: ShortcutId) {
    if (this.recording?.id === id) this.recording = null;
    void prefs.set(KEY, resetKeys(id, mine()));
  }

  /** The line under a command: what to press, what is wrong with it, or its default keys. */
  note(id: ShortcutId): Note | null {
    const r = this.recording;
    if (r?.id === id) {
      if (!this.combo) return { text: scopeOf(id) === "window" ? `Press the keys, then Return. A second key makes a chord, as ${keys("Mod+K Mod+M")}` : "Press the key, then Return" };
      if (this.problem) return { text: `${keys(this.combo)} can’t be used. ${this.problem}`, tone: "error" };
      const lost = this.lost;
      if (lost.length) return { text: `Already used by ${lost.map((l) => `${nameOf(l.id)} (${keys(l.combo)})`).join(" and ")}. Saving takes ${lost.length === 1 ? "it" : "them"} from there`, tone: "warn" };
      return { text: "Return saves it, Esc cancels" };
    }
    if (!changedKeys(id)) return null;
    const fallback = defaultsOf(id);
    return { text: fallback.length ? `Default: ${listed(fallback)}` : "No key by default" };
  }
}

export const shortcutEdit = new ShortcutEdit();

/** Whether you changed any command's keys. */
export const anyChanged = () => commandIds().some((id) => changedKeys(id));

/** Every command back to VS Code's keys, with Undo. */
export function resetAllRequest(): Request {
  const before = mine();
  const count = commandIds().filter((id) => changedKeys(id, before)).length;
  return {
    title: "Reset every keyboard shortcut?",
    body: [`The ${count === 1 ? "command you gave your own keys goes" : `${count} commands you gave your own keys go`} back to the keys they came with, as in VS Code.`],
    icon: "reset",
    button: "Reset All",
    status: "Resetting the keyboard shortcuts…",
    local: {
      commands: [`"${KEY}": {}`],
      comment: "Oxbow writes this to settings.json; no git command runs",
      run: async () => {
        await prefs.save(KEY, {});
        return "Keyboard shortcuts are back to VS Code’s.";
      },
    },
    undo: () => ({
      title: "Put back your keyboard shortcuts?",
      body: [`The keys you had come back for ${count === 1 ? "1 command" : `${count} commands`}.`],
      icon: "undo",
      button: "Put Back",
      status: "Putting back your keyboard shortcuts…",
      local: {
        commands: [`"${KEY}": ${JSON.stringify(before)}`],
        comment: "Oxbow writes this to settings.json; no git command runs",
        run: async () => {
          await prefs.save(KEY, before);
          return "Your keyboard shortcuts are back.";
        },
      },
    }),
  };
}

/** Whether a command fits the section's search: words of its name, or the keys it has. */
export function fits(id: ShortcutId, query: string, mac: boolean): boolean {
  const words = query.trim().toLowerCase();
  if (!words) return true;
  const combos = bindingsOf(id);
  const text = [nameOf(id), id, ...combos.map((c) => keys(c)), ...combos.map((c) => c.replaceAll("Mod", mac ? "Cmd" : "Ctrl"))].join(" ").toLowerCase();
  return words.split(/\s+/).every((w) => text.includes(w));
}
