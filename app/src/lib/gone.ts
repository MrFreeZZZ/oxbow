// What the screen of a repository folder that isn't there says: why, what is safe and what may
// not be, and which way out comes first. The folder's commit message and Conflicts draft stay
// with it: Open Main Folder never takes them to the main folder's branch.

import type { Gone } from "./types";

export type GoneAction = "main" | "find" | "retry";

export interface GoneFact {
  /** "safe": nothing to do. "warn": may be lost. "note": what is kept, or what to do later. */
  tone: "safe" | "warn" | "note";
  text: string;
}

export interface GoneScreen {
  title: string;
  reason: string;
  facts: GoneFact[];
  /** The way out that fits the reason best: the filled button. */
  primary: GoneAction;
  /** Open Main Folder is offered: a worktree whose main folder is there. */
  main: string | null;
}

/** What the folder keeps in Oxbow: its commit message (summary) and a Conflicts draft. */
export interface GoneDrafts {
  message: string | null;
  conflicts: boolean;
}

/** The screen for `gone`. `shown` writes a path as the screen shows it (`~/src/acme`), `mac`
 *  picks the system's words. */
export function goneScreen(gone: Gone, drafts: GoneDrafts, shown: (path: string) => string, mac: boolean): GoneScreen {
  const name = gone.name;
  const wt = gone.worktree;
  const main = wt?.main ?? null;
  const mainName = main ? (main.split(/[/\\]/).filter(Boolean).pop() ?? main) : null;
  const facts: GoneFact[] = [];
  let title: string;
  let reason: string;
  let primary: GoneAction;

  switch (gone.why) {
    case "disk":
      title = `${name} is on a disk that isn’t connected`;
      reason = `The folder is on “${gone.disk}”. Connect it, then choose Retry: nothing is lost while it is away.`;
      primary = "retry";
      break;
    case "trash":
      title = `${name} is in the Trash`;
      reason = mac
        ? `Put it back (in the Trash, Put Back), then choose Retry.`
        : `Restore it from the Trash, then choose Retry.`;
      primary = "retry";
      break;
    case "moved":
      title = `${name} was moved`;
      reason = `It is at ${shown(gone.found ?? "")} now, and git still looks for it where it was. Find Folder… reconnects it there.`;
      primary = "find";
      break;
    case "noAccess":
      title = `Oxbow can’t read ${name}`;
      reason = mac
        ? "macOS doesn’t let Oxbow into this folder. Allow Oxbow in System Settings › Privacy & Security › Files and Folders, then choose Retry."
        : "The system doesn’t let Oxbow into this folder. Check its permissions, then choose Retry.";
      primary = "retry";
      break;
    default:
      title = `${name} isn’t there any more`;
      reason = wt
        ? "It was deleted, or moved somewhere Oxbow didn’t look. If it was moved, Find Folder… reconnects it there."
        : "It was deleted, or moved somewhere Oxbow didn’t look. If it was moved, Find Folder… opens it there.";
      primary = wt && main ? "main" : "find";
  }

  if (wt && mainName) {
    const what = wt.branch ? `The branch ${wt.branch} and its commits are` : wt.head ? `Its commit ${wt.head.slice(0, 7)} is` : "Its commits are";
    facts.push({ tone: "safe", text: `${what} safe in ${mainName}.` });
  }
  if (gone.why === "missing") {
    facts.push({
      tone: "warn",
      text: wt
        ? "Changes that weren’t committed were only in the folder: if it was deleted, they went with it."
        : "Changes that weren’t committed, and commits that weren’t pushed, were only in the folder: if it was deleted, they went with it.",
    });
  } else if (gone.why === "trash" || gone.why === "moved") {
    facts.push({ tone: "safe", text: "Changes that weren’t committed are in the folder, where it is now." });
  }

  const kept = [drafts.message !== null && `the commit message “${drafts.message}”`, drafts.conflicts && "the choices on the Conflicts screen"].filter(
    (part): part is string => !!part,
  );
  if (kept.length) {
    const [them, come] = kept.length === 1 ? ["it", "it comes"] : ["them", "they come"];
    const branch = main && wt ? ` Open Main Folder doesn’t move ${them} to ${wt.mainBranch ?? `${mainName}’s branch`}.` : "";
    facts.push({ tone: "note", text: `Oxbow keeps ${kept.join(" and ")} for this folder, and ${come} back with it.${branch}` });
  }

  if (wt && main && gone.why === "missing") {
    facts.push({
      tone: "note",
      text:
        wt.locked !== null
          ? `It is locked${wt.locked ? ` (“${wt.locked}”)` : ""}, so git keeps it until it is unlocked.`
          : `Once you are sure it is gone, Prune under Worktrees in ${mainName} makes git forget it${wt.branch ? ` and frees ${wt.branch}` : ""}.`,
    });
  }

  return { title, reason, facts, primary, main: main && wt ? main : null };
}

/** The summary of a commit message saved as a draft, for the screen to name; null when none. */
export function messageOf(saved: unknown): string | null {
  if (!saved || typeof saved !== "object") return null;
  const { summary, description } = saved as { summary?: unknown; description?: unknown };
  const text = typeof summary === "string" && summary.trim() ? summary.trim() : typeof description === "string" && description.trim() ? description.trim().split("\n")[0] : "";
  if (!text) return null;
  return text.length > 48 ? text.slice(0, 47) + "…" : text;
}
