// Local History: the versions of files saved between commits, kept a few days by the core
// (crates/oxbow-core/src/localhistory.rs). What File History and the Local History screen say
// about a version, and the request that restores one.

import type { Request } from "./confirm.svelte";
import type { LocalVersion } from "./types";

/** `nav.openFile`'s commit for File History to start on the newest saved version. */
export const NEWEST_SAVED = "local";

/** Versions of every file the core lists, the newest (MAX_LISTED_ALL in localhistory.rs). */
export const MAX_LISTED_ALL = 2000;

/** A version's key in a list that also holds commits. */
export const keyOf = (v: LocalVersion) => `local:${v.id}:${v.at}`;

const nameOf = (path: string) => path.slice(path.lastIndexOf("/") + 1);

const sameDay = (a: Date, b: Date) => a.toDateString() === b.toDateString();

/** "Today", "Yesterday" or "Monday, 27 October": the day a version was saved. */
export function dayOf(at: number, now = Date.now()): string {
  const date = new Date(at);
  const today = new Date(now);
  if (sameDay(date, today)) return "Today";
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  if (sameDay(date, yesterday)) return "Yesterday";
  return date.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });
}

/** "today at 14:32", "yesterday at 18:05" or "on Monday, 27 October at 09:12". */
export function whenSaved(at: number, now = Date.now()): string {
  const day = dayOf(at, now);
  const time = new Date(at).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  if (day === "Today") return `today at ${time}`;
  if (day === "Yesterday") return `yesterday at ${time}`;
  return `on ${day} at ${time}`;
}

/** The versions, newest first, under the day each was saved. */
export function byDay(versions: LocalVersion[], now = Date.now()): { day: string; versions: LocalVersion[] }[] {
  const out: { day: string; versions: LocalVersion[] }[] = [];
  for (const v of versions) {
    const day = dayOf(v.at, now);
    const last = out[out.length - 1];
    if (last && last.day === day) last.versions.push(v);
    else out.push({ day, versions: [v] });
  }
  return out;
}

/** What a version's lines are counted from, in File History. */
export function sinceWhat(v: LocalVersion): string {
  if (v.fromHead) return "since the last commit";
  if (v.base) return "since the version before";
  return "a new file";
}

/** The lines a version added and removed; null when there are none to tell, as for a binary file. */
export function countsOf(v: LocalVersion): { add: number; del: number } | null {
  return v.additions === null || v.deletions === null ? null : { add: v.additions, del: v.deletions };
}

/** Restore: the file on disk becomes the version; nothing is staged. `time` says when it was
 *  saved, as the list shows it. */
export function restoreRequest(v: LocalVersion, time: string): Request {
  const name = nameOf(v.path);
  return {
    title: `Do you really want to restore ${name} as it was saved ${time}?`,
    body: v.gone
      ? ["Writes ", { code: v.path }, " back on disk as it was saved ", time, ". Nothing is staged or committed."]
      : ["Writes ", { code: v.path }, " on disk as it was saved ", time, ". What the file has now is replaced; nothing is staged or committed."],
    icon: "checkout",
    button: "Restore",
    tone: "warn",
    note: "Undo brings the file back as it is now, from the toast or the Operation Log.",
    status: `Restoring ${name}…`,
    done: `${name} is back as it was saved ${time}.`,
    action: { kind: "restoreLocalVersion", path: v.path, id: v.id },
  };
}
