// Edit Stack's templates: plans made in one go that the list then shows, to adjust or rebase.
// Squash WIP folds work-in-progress commits into the commits they continue, Fix Messages points
// at subjects that say little, and Move to Branch carries a commit over the next branch line.

import type { Draft, Item } from "./stack";
import type { StepAction } from "./types";

type Commit = { id: string; summary: string };

/** What a subject says about the commit: a fixup or squash of an earlier one, or work in progress. */
export function wipOf(summary: string): { fold: "fixup" | "squash"; of: string } | "wip" | null {
  const s = summary.trim();
  const marked = /^(fixup|amend|squash)! (.+)$/.exec(s);
  if (marked) {
    // "fixup! fixup! x" is a fixup of x too.
    let of = marked[2];
    while (/^(fixup|amend|squash)! /.test(of)) of = of.replace(/^(fixup|amend|squash)! /, "");
    return { fold: marked[1] === "squash" ? "squash" : "fixup", of };
  }
  if (/^(wip|tmp|temp)\b/i.test(s) || /^\.+$/.test(s)) return "wip";
  return null;
}

/** Squash WIP: each `fixup!`/`squash!` commit moves right above the commit it names and folds into
 *  it; any other WIP commit folds into the commit below it in its branch. `left` lists the WIP
 *  commits with nothing below them in their branch to fold into. */
export function squashWip(draft: Draft, commits: Commit[]): { draft: Draft; folded: number; left: string[] } {
  const summary = new Map(commits.map((c) => [c.id, c.summary]));
  const items = [...draft.items];
  const acts: Record<string, StepAction> = { ...draft.acts };
  const live = (id: string) => (acts[id] ?? "pick") !== "drop";
  let folded = 0;

  // Marked ones first: they may move into another branch, next to the commit they name.
  for (const id of commits.map((c) => c.id)) {
    const wip = wipOf(summary.get(id) ?? "");
    if (!wip || wip === "wip" || !live(id)) continue;
    const at = items.findIndex((i) => i.kind === "commit" && i.id === id);
    const target = items.findIndex((i, k) => k > at && i.kind === "commit" && live(i.id) && !wipOf(summary.get(i.id) ?? "") && summary.get(i.id) === wip.of);
    if (target < 0) continue;
    const [item] = items.splice(at, 1);
    items.splice(target - 1, 0, item);
    acts[id] = wip.fold;
    folded++;
  }

  // The rest fold into the commit below them, as long as it is in the same branch.
  const left: string[] = [];
  items.forEach((item, k) => {
    if (item.kind !== "commit" || !live(item.id) || acts[item.id] || wipOf(summary.get(item.id) ?? "") !== "wip") return;
    const below = items.slice(k + 1).find((i) => i.kind === "branch" || live(i.id));
    if (below?.kind === "commit") {
      acts[item.id] = "fixup";
      folded++;
    } else left.push(item.id);
  });
  return { draft: { ...draft, items, acts }, folded, left };
}

/** Why a subject could say more, for Fix Messages; null when it reads fine. */
export function messageNote(summary: string): string | null {
  const s = summary.trim();
  if (!s) return "No message";
  const wip = wipOf(s);
  if (wip === "wip") return "Work in progress: say what it changes";
  if (wip) return "Meant to be squashed: Squash WIP folds it in";
  if (/^(fix|fixes|fixed|update|updates|updated|change|changes|stuff|misc|cleanup|refactor|test|tests|more|done)$/i.test(s)) return "Says little about the change";
  if ([...s].length > 72) return `Subject is ${[...s].length} characters, over 72`;
  if (/[^.]\.$/.test(s)) return "Subject ends with a period";
  return null;
}

/** The nearest branch line above the item at `index`; -1 when there is none. */
function lineAbove(items: Item[], index: number): number {
  for (let k = index - 1; k >= 0; k--) if (items[k].kind === "branch") return k;
  return -1;
}

/** The branch a commit moves into with `dir`: the one below it (+1) or above it (-1); null when
 *  there is none. */
export function neighborBranch(items: Item[], id: string, dir: 1 | -1): string | null {
  const at = items.findIndex((i) => i.kind === "commit" && i.id === id);
  if (at < 0) return null;
  if (dir === 1) {
    const line = items.findIndex((i, k) => k > at && i.kind === "branch");
    return line < 0 ? null : (items[line] as { name: string }).name;
  }
  const own = lineAbove(items, at);
  if (own <= 0) return null;
  const above = lineAbove(items, own);
  return above < 0 ? null : (items[above] as { name: string }).name;
}

/** Move to Branch: the commit becomes the newest one of the branch below (+1), or the oldest one
 *  of the branch above (-1). Null when there is no branch that way. */
export function moveToBranch(items: Item[], id: string, dir: 1 | -1): Item[] | null {
  if (!neighborBranch(items, id, dir)) return null;
  const at = items.findIndex((i) => i.kind === "commit" && i.id === id);
  const next = [...items];
  const [item] = next.splice(at, 1);
  if (dir === 1) {
    const line = next.findIndex((i, k) => k >= at && i.kind === "branch");
    next.splice(line + 1, 0, item);
  } else {
    const own = lineAbove(next, at);
    next.splice(own, 0, item);
  }
  return next;
}
