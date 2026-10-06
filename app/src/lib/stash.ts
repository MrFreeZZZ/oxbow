// The stash: put uncommitted changes aside, bring them back, drop them, or turn one into a
// branch. Used by the Stashes screen and by the menus of stashes in History and the sidebar.

import { api } from "./api";
import type { Part, Request } from "./confirm.svelte";
import type { BranchContext } from "./branches";
import { nameProblem } from "./branches";
import { stoppedOnConflicts } from "./merge";
import { menuIcons, type MenuEntry } from "./Menu.svelte";
import type { StashCheck, StashInfo } from "./types";
import { shortId } from "./format";

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
const chip = (ctx: BranchContext, name: string): Part => ({ branch: name, color: ctx.colorOf(name) });
const here = (ctx: BranchContext): Part => (ctx.history.head.branch ? chip(ctx, ctx.history.head.branch) : { code: "HEAD" });
const hereName = (ctx: BranchContext) => ctx.history.head.branch ?? "HEAD";

export const stashName = (stash: StashInfo) => `stash@{${stash.index}}`;

/** The branch it was made on, as a chip, or the commit when that was no branch. */
function madeOn(ctx: BranchContext, stash: StashInfo): Part {
  return stash.branch ? chip(ctx, stash.branch) : { code: shortId(stash.base) };
}

/** "a.txt, b.txt and 3 more" */
function listFiles(paths: string[]): Part[] {
  const shown = paths.slice(0, 2);
  const parts: Part[] = [];
  shown.forEach((p, i) => {
    if (i) parts.push(i === shown.length - 1 && paths.length === shown.length ? " and " : ", ");
    parts.push({ code: p });
  });
  if (paths.length > shown.length) parts.push(` and ${paths.length - shown.length} more`);
  return parts;
}

/** Put every uncommitted change aside. */
export function stashRequest(ctx: BranchContext, message = "", untracked = true): Request {
  const files = ctx.uncommitted;
  return {
    title: `Stash ${plural(files, "changed file")}?`,
    body: [
      "Saves your uncommitted changes as a new stash and takes them out of your working copy, so ",
      here(ctx),
      " is back to its last commit. Bring them back from Stashes, on this branch or another one.",
    ],
    icon: "stash",
    button: "Stash",
    fields: [
      {
        label: "Message",
        text: { value: message, placeholder: "What these changes are, e.g. WIP: session banner", edit: (value) => stashRequest(ctx, value, untracked) },
      },
    ],
    options: [
      {
        label: "Include untracked files",
        sub: "New files git doesn’t track yet go into the stash too.",
        on: untracked,
        toggle: () => stashRequest(ctx, message, !untracked),
      },
    ],
    invalid: files ? null : "Nothing to stash: there are no uncommitted changes.",
    status: "Stashing your changes…",
    done: "Stashed. Your working copy is clean.",
    action: { kind: "stashPush", message: message.trim() || null, untracked },
  };
}

/** Bring a stash's changes back; `pop` deletes it afterwards. */
export async function applyRequest(ctx: BranchContext, stash: StashInfo, pop: boolean, keepIndex = false, check?: StashCheck): Promise<Request> {
  const known = check ?? (await api.stashCheck(stash.index, stash.id).catch(() => null));
  return applySheet(ctx, stash, pop, keepIndex, known);
}

function applySheet(ctx: BranchContext, stash: StashInfo, pop: boolean, keepIndex: boolean, check: StashCheck | null): Request {
  const name = stashName(stash);
  const verb = pop ? "Pop" : "Apply";
  const body: Part[] = [
    "Brings back the changes of ",
    { quote: stash.title },
    ", stashed on ",
    madeOn(ctx, stash),
    ", into your working copy on ",
    here(ctx),
    ".",
  ];
  body.push(pop ? " Then the stash is deleted." : " The stash stays in the list.");
  const conflicts = check?.conflicts ?? [];
  const inTheWay = check?.inTheWay ?? [];
  if (inTheWay.length) {
    body.push(" Your uncommitted changes to ", ...listFiles(inTheWay), " are in the way, so git would refuse. Commit or stash them first.");
  } else if (conflicts.length) {
    body.push(` ${plural(conflicts.length, "file")} will conflict: `, ...listFiles(conflicts), ". You pick what goes into each file in Conflicts.");
    if (pop) body.push(" The stash is deleted only once that is finished.");
  }
  return {
    title: `${verb} ${name}?`,
    body,
    icon: "stash",
    tone: conflicts.length || inTheWay.length ? "warn" : undefined,
    button: conflicts.length ? `${verb} and Resolve…` : verb,
    options: [
      {
        label: "Keep staged",
        sub: "Files that were staged when you stashed come back staged.",
        on: keepIndex,
        toggle: () => applySheet(ctx, stash, pop, !keepIndex, check),
      },
    ],
    invalid: inTheWay.length ? "Commit or stash your changes to these files first." : null,
    note: pop ? "Nothing is lost: until the apply finishes, the stash stays." : undefined,
    status: `${pop ? "Popping" : "Applying"} ${name}…`,
    done: pop ? `Popped “${stash.title}” onto ${hereName(ctx)}. The stash is deleted.` : `Applied “${stash.title}” to ${hereName(ctx)}. The stash is kept.`,
    resolveConflicts: conflicts.length > 0,
    recover: (failure) => stoppedOnConflicts(failure, "stash"),
    action: { kind: "stashApply", index: stash.index, id: stash.id, pop, keepIndex },
  };
}

/** Delete a stash; the toast offers to put it back. */
export function dropRequest(ctx: BranchContext, stash: StashInfo): Request {
  const name = stashName(stash);
  return {
    title: `Drop ${name}?`,
    body: [
      "Deletes ",
      { quote: stash.title },
      ", stashed on ",
      madeOn(ctx, stash),
      ", from the stash list without applying it. Later stashes move up one number.",
    ],
    icon: "drop",
    danger: true,
    button: "Drop",
    note: "Right after, Undo on the message puts it back.",
    status: `Dropping ${name}…`,
    done: `Dropped “${stash.title}”.`,
    undo: () => restoreRequest(stash),
    action: { kind: "stashDrop", index: stash.index, id: stash.id },
  };
}

/** Put a dropped stash back. */
function restoreRequest(stash: StashInfo): Request {
  return {
    title: "Put the stash back?",
    body: ["Stores ", { quote: stash.title }, " in the stash list again, as ", { code: "stash@{0}" }, "."],
    icon: "stash",
    button: "Put Back",
    status: "Putting the stash back…",
    done: `“${stash.title}” is back in the stash list.`,
    action: { kind: "stashStore", id: stash.id, message: stash.message },
  };
}

/** A new branch where the stash was made, with the stash applied there and dropped. */
export function stashBranchRequest(ctx: BranchContext, stash: StashInfo, typed = ""): Request {
  const name = typed.trim();
  const problem = name ? nameProblem(ctx, name) : null;
  const target: Part = name && !problem ? { branch: name, color: ctx.colorOf(name) } : "a new branch";
  return {
    title: "New branch from the stash",
    body: [
      "Creates ",
      target,
      " at ",
      { code: shortId(stash.base) },
      ", the commit the stash was made on, checks it out and applies ",
      { quote: stash.title },
      " there, so it can’t conflict. Then the stash is deleted.",
      ctx.uncommitted ? " Commit or stash your uncommitted changes first." : "",
    ],
    icon: "branch",
    button: "Create Branch",
    fields: [
      {
        label: "Name",
        text: { value: typed, placeholder: "feature/…", edit: (value) => stashBranchRequest(ctx, stash, value) },
        error: problem ?? undefined,
      },
    ],
    invalid: !name ? "Type a name for the branch." : problem,
    status: `Creating ${name}…`,
    done: `Created ${name} with the stash applied.`,
    action: { kind: "stashBranch", index: stash.index, id: stash.id, name },
  };
}

/** The menu of a stash, in History and in the sidebar. */
export function stashMenu(ctx: BranchContext, stash: StashInfo, run: (request: Request | Promise<Request>) => void, copy: (text: string) => void): MenuEntry[] {
  const busy = !!ctx.history.operation;
  const entries: MenuEntry[] = [{ kind: "header", label: `${stashName(stash)} · ${stash.title}` }];
  if (!busy) {
    entries.push(
      { kind: "item", label: `Apply to ${hereName(ctx)}…`, icon: menuIcons.stash, run: () => run(applyRequest(ctx, stash, false)) },
      { kind: "item", label: `Pop onto ${hereName(ctx)}…`, icon: menuIcons.pop, run: () => run(applyRequest(ctx, stash, true)) },
      { kind: "item", label: "New Branch from Stash…", icon: menuIcons.branch, run: () => run(stashBranchRequest(ctx, stash)) },
    );
  } else {
    entries.push({ kind: "note", label: "Finish or abort the operation in progress first." });
  }
  entries.push(
    { kind: "sep" },
    {
      kind: "sub",
      label: "Copy",
      icon: menuIcons.copy,
      entries: [
        { label: "Name", hint: stashName(stash), run: () => copy(stashName(stash)) },
        { label: "Message", run: () => copy(stash.title) },
        { label: "SHA", hint: shortId(stash.id), run: () => copy(stash.id) },
      ],
    },
    { kind: "sep" },
    { kind: "item", label: "Drop…", icon: menuIcons.drop, danger: true, run: () => run(dropRequest(ctx, stash)) },
  );
  return entries;
}
