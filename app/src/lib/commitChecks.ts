// Checks before commit, as Settings › This Repository lists them: the commands they are kept
// with in .git/config, and the list after an edit. Oxbow runs them itself (oxbow-core checks.rs).

import type { Recovery, Request } from "./confirm.svelte";
import type { Action, CommitCheck, Failure } from "./types";

/** The setting that keeps them, one value per check; `#` before a command turns it off. */
export const CHECK_KEY = "oxbow.check";

const quote = (value: string) => (/^[\w@%+=:,./~-]+$/.test(value) ? value : `"${value.replace(/(["\\$`])/g, "\\$1")}"`);

/** The `git config` commands that write `list` whole, as Settings shows them. */
export function checkCommands(list: CommitCheck[]): string[] {
  return [
    `git config --local --unset-all ${CHECK_KEY}`,
    ...list.filter((c) => c.command.trim()).map((c) => `git config --local --add ${CHECK_KEY} ${quote(c.on ? c.command.trim() : `# ${c.command.trim()}`)}`),
  ];
}

/** Why `command` can't be the check at `index` (`null` for a new one), if it can't. */
export function checkProblem(list: CommitCheck[], index: number | null, command: string): string | null {
  const text = command.trim();
  if (!text) return "Type a command";
  if (text.startsWith("#")) return "A command can’t start with #: that is how a check turned off is kept";
  if (list.some((c, i) => i !== index && c.command === text)) return "This check is in the list already";
  return null;
}

/** `list` with the check at `index` replaced by `command` (`null` adds it at the end, on). */
export function withCheck(list: CommitCheck[], index: number | null, command: string): CommitCheck[] {
  const text = command.trim();
  if (index === null) return [...list, { command: text, on: true }];
  return list.map((c, i) => (i === index ? { ...c, command: text } : { ...c }));
}

/** `list` with the check at `index` moved `by` places, or as it is at the ends. */
export function movedCheck(list: CommitCheck[], index: number, by: number): CommitCheck[] {
  const to = index + by;
  const next = list.map((c) => ({ ...c }));
  if (to < 0 || to >= next.length || index < 0 || index >= next.length) return next;
  const [check] = next.splice(index, 1);
  next.splice(to, 0, check);
  return next;
}

/** What a check's row says under it. */
export function checkSub(check: CommitCheck, restricted: boolean): string {
  if (restricted) return "Doesn’t run: this repository is Restricted";
  return check.on ? "Runs before each commit; when it fails, nothing is committed" : "Off: commits go through without it";
}

/** What every sheet that commits says when a check before commit failed: `doing` is what it
 *  was about to do, e.g. "committing". Its output is in the sheet's commands. */
export function checkFailed(failure: Failure, doing: string): Pick<Recovery, "title" | "body" | "icon" | "tone" | "checked"> {
  return {
    title: "A check before commit failed",
    body: ["Oxbow ran ", { code: failure.check ?? "a check" }, ` before ${doing}, and it failed, so nothing changed. Its output is below: fix what it reports and try again.`],
    icon: "hook",
    tone: "err",
    checked: [{ kind: "hint", text: "# Checks before commit are set in Settings › This Repository; git did not run" }],
  };
}

type CommitAction = Action & { kind: "commit" };
/** The way out of a commit that fails again, with the action it ran. */
type Again = (failure: Failure, action: CommitAction) => Recovery | null;

/** The way out when a check before commit stopped a commit: Try Again, or Commit Anyway,
 *  which skips the checks this once. */
export function checkRecovery(failure: Failure, action: CommitAction, summary: string, branch: string, again: Again): Recovery {
  return {
    ...checkFailed(failure, action.amend ? "amending" : "committing"),
    button: { label: "Try Again", action, status: `Committing to ${branch}…`, done: "Committed.", recover: (next) => again(next, action) },
    alt: { label: action.amend ? "Amend Anyway…" : "Commit Anyway…", danger: true, request: () => skipChecksRequest(action, summary, branch, failure.check ?? "", again) },
    note: "Your message is kept.",
    close: "Close",
  };
}

function skipChecksRequest(action: CommitAction, summary: string, branch: string, command: string, again: Again): Request {
  const next = { ...action, skipChecks: true };
  return {
    title: action.amend ? "Amend without the checks?" : "Commit without the checks?",
    body: ["Skips the checks before commit this once, so ", { quote: summary }, " goes in although ", { code: command || "a check" }, " failed."],
    icon: "hook",
    button: action.amend ? "Amend Anyway" : "Commit Anyway",
    danger: true,
    note: "The checks run again on your next commit.",
    status: `Committing to ${branch}…`,
    done: `Committed to ${branch} without the checks.`,
    recover: (failure) => again(failure, next),
    action: next,
  };
}
