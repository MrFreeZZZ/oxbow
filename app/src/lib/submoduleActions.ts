// What can be done with a submodule, worked out in one place: the sidebar's menu, the card shown
// in place of the commit's details and the sheets offer the same actions, in the same words,
// with the same reasons when one can't run or needs a second look.

import type { Submodule } from "./types";

export type SubmoduleActionId = "clone" | "update" | "latest";

export interface SubmoduleAction {
  id: SubmoduleActionId;
  /** As a button or a menu item, e.g. "Update to 1a2b3c4". */
  label: string;
  /** What it does, in one line: tells Update and Pull Latest apart. */
  about: string;
  /** The way to bring the folder in line: the card's blue button. */
  primary?: boolean;
  /** Why it can't run now: it is listed anyway, greyed, with this. */
  off?: string;
  /** What to know before it runs, said in the menu, on the card and in the sheet. */
  caution?: { title: string; text: string };
}

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

/** The branch Pull Latest takes, in words. */
export function followsWord(sub: { follows: string | null }): string {
  if (sub.follows === ".") return "the branch checked out here";
  return sub.follows ?? "its default branch";
}

/** Clone, Update and Pull Latest as they stand for `sub`. `checking`: what its folder has isn't
 *  read yet (folders.ts `checking`). */
export function submoduleActions(sub: Submodule, checking: boolean): SubmoduleAction[] {
  const wait = `Oxbow is still checking what ${sub.title}’s folder has.`;
  // Both check out another commit in the folder: commits made there on no branch would be left
  // where only its reflog finds them. Changed files are never lost: git stops instead.
  const caution =
    sub.cloned && !checking && sub.onNoBranch
      ? {
          title: `${plural(sub.onNoBranch, "commit")} on no branch`,
          text: `${sub.title} has ${sub.onNoBranch === 1 ? "a commit" : `${sub.onNoBranch} commits`} made with no branch checked out. After this only its reflog has ${sub.onNoBranch === 1 ? "it" : "them"}: open it in a new window and make a branch first.`,
        }
      : undefined;
  const out: SubmoduleAction[] = [];
  if (!sub.cloned) {
    out.push({ id: "clone", label: "Clone", about: "Clone it and check out the commit this repository records", primary: true });
  } else if (sub.recorded && (checking || sub.current !== sub.recorded)) {
    out.push({
      id: "update",
      label: `Update to ${sub.recorded.slice(0, 7)}`,
      about: sub.missing ? "Fetch and check out the commit this repository records" : "Back to the commit this repository records",
      primary: true,
      off: checking ? wait : undefined,
      caution,
    });
  }
  out.push({
    id: "latest",
    label: "Pull Latest",
    about: `The newest commit of ${followsWord(sub)}; commit here to record it`,
    off: !sub.cloned ? `Clone ${sub.title} first: Pull Latest works in its folder.` : checking ? wait : undefined,
    caution,
  });
  return out;
}
