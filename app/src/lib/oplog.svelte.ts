// The Operation Log: every step Oxbow ran in this repository, newest first, and the way back to
// the moment before any of them, or right after one.

import { api } from "./api";
import type { Icon, Request } from "./confirm.svelte";
import { relativeTime } from "./format";
import { actionIcons } from "./icons";
import type { HistoryRow, OpEntry, ReflogEntry } from "./types";

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

class OplogState {
  entries = $state<OpEntry[]>([]);
  /** The popover under the toolbar button. */
  open = $state(false);
  /** Compare with Now: the step, and which side of it (`after` for Restore to Here), shown in
   *  the details panel of History. */
  compare = $state<{ entry: OpEntry; after: boolean } | null>(null);
  /** The popover's tab: Oxbow's own steps, or git's reflog. */
  tab = $state<"steps" | "reflog">("steps");
  /** Where HEAD and the local branches have been, from git's reflog, newest first. */
  reflog = $state<ReflogEntry[]>([]);
  /** Every move in the reflog, not only the commits no branch has. */
  allMoves = $state(false);
  /** A commit from the reflog that History doesn't have, shown in its details panel. */
  lost = $state<ReflogEntry | null>(null);
  #asked = 0;
  #askedReflog = 0;

  async load() {
    const mine = ++this.#asked;
    try {
      const entries = await api.operationLog();
      if (mine === this.#asked) this.entries = entries;
    } catch {
      if (mine === this.#asked) this.entries = [];
    }
    if (this.open && this.tab === "reflog") this.loadReflog();
  }

  async loadReflog() {
    const mine = ++this.#askedReflog;
    try {
      const entries = await api.reflogEntries();
      if (mine === this.#askedReflog) this.reflog = entries;
    } catch {
      if (mine === this.#askedReflog) this.reflog = [];
    }
  }

  forget() {
    this.#asked++;
    this.#askedReflog++;
    this.entries = [];
    this.reflog = [];
    this.open = false;
    this.compare = null;
    this.lost = null;
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

/** Revert, offered on the Undo of a commit the remote already has: a new commit that takes it
 *  back for everyone, where Undo only changes this copy. */
export function revertInsteadRequest(id: string, summary: string, branch: string): Request {
  const sha = id.slice(0, 7);
  return {
    title: `Revert ${sha} instead?`,
    body: [
      "Adds a new commit on top of ",
      { code: branch },
      " that undoes ",
      { code: sha },
      " ",
      { quote: summary },
      ". The original commit stays in history, so this is safe on a branch that is already pushed.",
    ],
    icon: "revert",
    button: "Revert",
    note: "Nothing is pushed.",
    status: `Reverting ${sha}…`,
    done: `Reverted ${sha} on ${branch}.`,
    resolveConflicts: true,
    action: { kind: "revert", commit: id },
  };
}

/** "until 14 Nov": the last day a step can be gone back to. */
export function keptUntil(entry: Pick<OpEntry, "expires">): string {
  const date = new Date(entry.expires * 1000);
  const sameYear = date.getFullYear() === new Date().getFullYear();
  return `until ${date.toLocaleDateString(undefined, sameYear ? { day: "numeric", month: "short" } : { day: "numeric", month: "short", year: "numeric" })}`;
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

/** The reflog's list: lost commits only, a newer lost one standing for those it keeps, or every move. */
export function reflogRows(entries: ReflogEntry[], all: boolean): ReflogEntry[] {
  return all ? entries : entries.filter((e) => e.lost && !e.within);
}

/** What git wrote for the move, without the commit's subject it repeats: "commit (amend)",
 *  "reset: moving to HEAD~2". */
export function moveText(entry: ReflogEntry): string {
  const tail = `: ${entry.summary}`;
  return entry.summary && entry.message.endsWith(tail) ? entry.message.slice(0, -tail.length) : entry.message;
}

/** The icon of a reflog move; an amend is a commit, anything unknown gets the undo arrow. */
export function moveIcon(entry: ReflogEntry): string {
  const kind = entry.kind === "amend" ? "commit" : entry.kind;
  return actionIcons[kind as Icon] ?? actionIcons.undo;
}

/** "until 14 Nov" for a lost commit; "may go soon" once git's expiry passed but no clean-up ran. */
export function lostUntil(entry: ReflogEntry): string {
  if (entry.expires === null) return "";
  if (entry.expires * 1000 < Date.now()) return "may go soon";
  return keptUntil({ expires: entry.expires });
}

/** A History row for a commit History doesn't show, so the commit panel can show it. */
export function lostRow(entry: ReflogEntry, color: number): HistoryRow {
  return {
    id: entry.id,
    summary: entry.summary,
    authorName: entry.authorName,
    authorEmail: entry.authorEmail,
    time: entry.time,
    parents: entry.parents,
    labels: [],
    unpushed: false,
    noBranch: true,
    graph: { column: 0, color, forkColors: [], mergeColors: [], segments: [], width: 0, branch: null },
    worktree: null,
  };
}
