// The Operation Log: every step Oxbow ran in this repository, newest first, and the way back to
// the moment before any of them, or right after one.

import { api } from "./api";
import type { Icon, Request } from "./confirm.svelte";
import { relativeTime } from "./format";
import { actionIcons } from "./icons";
import type { OpEntry } from "./types";

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

class OplogState {
  entries = $state<OpEntry[]>([]);
  /** The popover under the toolbar button. */
  open = $state(false);
  #asked = 0;

  async load() {
    const mine = ++this.#asked;
    try {
      const entries = await api.operationLog();
      if (mine === this.#asked) this.entries = entries;
    } catch {
      if (mine === this.#asked) this.entries = [];
    }
  }

  forget() {
    this.#asked++;
    this.entries = [];
    this.open = false;
  }
}

export const oplog = new OplogState();

/** The icon of a step; an unknown kind gets the undo arrow. */
export function entryIcon(entry: OpEntry): string {
  return actionIcons[entry.kind as Icon] ?? actionIcons.undo;
}

/** "Undo “Commit to main”" undone again is a redo. */
export function isRedo(entry: OpEntry): boolean {
  return undoneTitle(entry) !== null;
}

function undoneTitle(entry: OpEntry): string | null {
  const m = /^Undo “(.*)”$/.exec(entry.title);
  return entry.kind === "undo" && m ? m[1] : null;
}

/** "2 min ago", "just now", "yesterday 17:05", "on 12 Mar". */
function when(time: number): string {
  const text = relativeTime(time);
  if (/^(Just|Yesterday)/.test(text)) return text[0].toLowerCase() + text.slice(1);
  return /ago$/.test(text) ? text : `on ${text}`;
}

/** Back to before the newest step: Undo in the popover, ⌘Z, and Undo on a toast. */
export function undoRequest(entry: OpEntry, later = 0): Request {
  const redo = undoneTitle(entry);
  const what = redo ?? entry.title;
  const action = { kind: "restore", id: entry.id, after: false } as const;
  if (redo !== null)
    return {
      title: `Redo “${what}”?`,
      body: [`Takes back the undo from ${when(entry.time)}, so “${what}” is done again: branches, tags, stashes and files go back to how it left them.`],
      icon: "undo",
      button: "Redo",
      status: "Redoing…",
      done: `Redid “${what}”.`,
      action,
    };
  return {
    title: `Undo “${what}”?`,
    body: [
      `Puts the repository back as it was before this step, ${when(entry.time)}: branches, tags, stashes and every file, staged or not.`,
      later ? ` The ${plural(later, "later step")} ${later === 1 ? "is" : "are"} undone too.` : "",
      " The undo is a step of the log itself, so it can be taken back.",
    ],
    icon: "undo",
    button: "Undo",
    status: "Undoing…",
    done: `Undid “${what}”.`,
    action,
  };
}

/** Restore to Here: the repository as it was right after `entry`, which is `later` steps back. */
export function restoreRequest(entry: OpEntry, later: number): Request {
  return {
    title: `Restore to after “${entry.title}”?`,
    body: [
      `Puts the repository back as it was right after this step, ${when(entry.time)}: branches, tags, stashes and every file, staged or not. `,
      later === 1 ? "The step after it is undone." : `The ${plural(later, "step")} after it are undone.`,
      " Changes you made since are kept in the log, so this can be undone too.",
    ],
    icon: "undo",
    tone: "warn",
    button: "Restore",
    status: "Restoring…",
    done: `Restored to after “${entry.title}”.`,
    action: { kind: "restore", id: entry.id, after: true },
  };
}

export function clearRequest(count: number): Request {
  return {
    title: "Clear the Operation Log?",
    body: [`Removes ${count === 1 ? "the only step" : `all ${count} steps`} from the log, so Undo can't go back past this point. Your branches and files do not change.`],
    icon: "discard",
    button: "Clear Log",
    danger: true,
    note: "Git deletes the snapshots in its next clean-up.",
    done: "Cleared the Operation Log.",
    action: { kind: "clearOperationLog" },
  };
}

/** Why a step can't be gone back to, or null when it can. */
export function cannotRestore(entry: OpEntry, after: boolean): string | null {
  const trees = after ? entry.after : entry.before;
  return trees.index ? null : `The repository was in the middle of a merge or rebase ${after ? "right after" : "before"} this step`;
}
