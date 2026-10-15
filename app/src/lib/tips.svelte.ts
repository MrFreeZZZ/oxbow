// One-time Git tips: a short card at the moment a git idea first matters, e.g. the first time
// a file is partly staged. Got It closes a tip for good; Turn Off Tips closes them all, and
// Settings › General turns them back on.

import { confirm, type Part } from "./confirm.svelte";
import { prefs } from "./prefs.svelte";

export interface Tip {
  title: string;
  body: Part[];
}

export const TIPS = {
  partlyStaged: {
    title: "One file, two lists",
    body: [
      "Part of this file is staged and part isn’t. Git keeps two copies: the staging area, which the next commit takes, and your files. ",
      "Staged lines go in the commit; the rest waits here.",
    ],
  },
  fetchedOnly: {
    title: "Fetch downloads, it doesn’t change files",
    body: ["New commits landed in ", { code: "origin/…" }, " branches only. Your branch and files stay as they are until you pull or merge."],
  },
  localCommits: {
    title: "Your commits are on this computer only",
    body: ["Commits stay local until you push. Push sends them to the server, where others can pull them."],
  },
  detached: {
    title: "No branch is checked out",
    body: [
      "HEAD usually points at a branch, and each commit moves that branch forward. Now it points straight at a commit, so a new commit moves no branch: ",
      "only HEAD knows it, and switching away leaves it behind.",
    ],
  },
  rebaseConflict: {
    title: "A rebase goes one commit at a time",
    body: [
      "Git replays your commits on the new base one by one and stopped at this one. Resolve, then Continue moves to the next commit, and more conflicts may follow. ",
      "In a terminal, git calls the new base “ours” and your commit “theirs”, the other way round from a merge.",
    ],
  },
  revertReset: {
    title: "Revert or reset?",
    body: [
      { code: "git revert" },
      " adds a new commit that undoes the changes, and history stays, so it’s safe for pushed commits. ",
      { code: "git reset" },
      " moves the branch back and drops commits from it: use it only for commits nobody else has.",
    ],
  },
  forceLease: {
    title: "Why --force-with-lease?",
    body: ["Plain --force replaces the server’s branch whatever is there. With a lease, git refuses if someone pushed since your last fetch, so their commits can’t vanish by accident."],
  },
  amend: {
    title: "Amend makes a new commit",
    body: ["The old last commit is replaced by a new one with a new SHA. That’s fine before you push; after a push, the server still has the old one and needs a force push."],
  },
  stashApplyPop: {
    title: "Apply or Pop?",
    body: ["Both bring the stashed changes back into your files. Pop also deletes the stash once they apply without conflicts; Apply keeps it, so you can apply it again elsewhere."],
  },
} satisfies Record<string, Tip>;

export type TipId = keyof typeof TIPS;
export const TIP_COUNT = Object.keys(TIPS).length;

class Tips {
  /** Whether the tip may show: tips are on and it wasn't closed. */
  shows(id: TipId): boolean {
    return prefs.loaded && prefs.get("oxbow.tips.show") && !prefs.get("oxbow.tips.seen").includes(id);
  }

  /** Tips closed with Got It. */
  get closed(): number {
    return prefs.get("oxbow.tips.seen").filter((id) => id in TIPS).length;
  }

  gotIt(id: TipId) {
    const seen = prefs.get("oxbow.tips.seen");
    if (!seen.includes(id)) prefs.set("oxbow.tips.seen", [...seen, id]);
  }

  off() {
    prefs.set("oxbow.tips.show", false);
    confirm.say("Tips are off. Settings › General turns them back on.");
  }

  /** Every tip shows once more. */
  again() {
    prefs.reset("oxbow.tips.seen");
  }
}

export const tips = new Tips();
