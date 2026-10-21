// Worktrees: more working folders for one repository, each with its own branch checked out.
// The New Worktree sheet, removing, locking, pruning and reconnecting a moved folder, and what
// checking out a branch that another worktree has does.

import { open } from "@tauri-apps/plugin-dialog";
import { homeDir } from "@tauri-apps/api/path";
import { api } from "./api";
import type { Part, Recovery, Request } from "./confirm.svelte";
import { nameProblem, type BranchContext } from "./branches";
import { checking } from "./folders";
import { shortId } from "./format";
import { tilde as tildeIn } from "./term";
import type { Failure, History, Worktree } from "./types";

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

/** "a, b and c" */
function list(names: string[]): string {
  return names.length < 2 ? (names[0] ?? "") : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

let home = "";
homeDir().then(
  (dir) => (home = dir),
  () => {},
);

/** A path with the home folder as ~, the way people read it. */
export const tilde = (path: string) => tildeIn(path, home);

/** A typed path with ~ for the home folder, in full. */
function untilde(path: string): string {
  if (!home || !(path === "~" || path.startsWith("~/"))) return path;
  return home.replace(/[/\\]$/, "") + path.slice(1);
}

const isAbsolute = (path: string) => path.startsWith("/") || /^[A-Za-z]:[\\/]/.test(path) || path.startsWith("\\\\");

const parentOf = (path: string) => path.replace(/[/\\][^/\\]*[/\\]?$/, "");

/** The other worktree `branch` is checked out in, if any. */
export function worktreeOf(history: History, branch: string): Worktree | null {
  return history.worktrees.find((t) => !t.current && t.branch === branch) ?? null;
}

/** Where a worktree's branch is, in a capsule; a detached one by its commit. */
function branchPart(ctx: BranchContext, wt: Worktree): Part {
  return wt.branch ? { branch: wt.branch, color: ctx.colorOf(wt.branch) } : { code: wt.head ? shortId(wt.head) : "HEAD" };
}

interface NewWorktree {
  /** The branch to check out, or the name of the new one with `create`. */
  branch: string;
  create: boolean;
  /** Where a new branch starts; null for HEAD. */
  from: string | null;
  /** The folder as typed, maybe with ~. */
  folder: string;
  /** The folder was typed or chosen, so it stops following the branch. */
  edited: boolean;
  open: boolean;
  /** What is typed in the search box Other… opens; null while it is closed. */
  search: string | null;
}

const PREFIXES = ["feature/", "fix/"];

/** Branches no worktree has checked out, the latest committed first. */
function freeBranches(history: History): string[] {
  const busy = new Set(history.worktrees.map((t) => t.branch));
  if (history.head.branch) busy.add(history.head.branch);
  const time = new Map(history.rows.map((r) => [r.id, r.time]));
  return history.refs
    .filter((r) => r.kind === "local" && !busy.has(r.name))
    .sort((a, b) => (time.get(b.target) ?? 0) - (time.get(a.target) ?? 0) || a.name.localeCompare(b.name))
    .map((r) => r.name);
}

/** A folder next to this one named after the repository and the branch: acme-api-billing-webhooks. */
function folderFor(repoPath: string, branch: string): string {
  const repo = repoPath.replace(/[/\\]$/, "");
  const last = branch.split("/").filter(Boolean).pop() ?? "worktree";
  return `${parentOf(repo)}/${repo.split(/[/\\]/).pop()}-${last.replace(/[^\w.-]+/g, "-")}`;
}

/** `folderFor`, with -2, -3… when the folder is taken. */
async function freeFolder(history: History, repoPath: string, branch: string): Promise<string> {
  const base = folderFor(repoPath, branch);
  for (let n = 1; n < 20; n++) {
    const path = n === 1 ? base : `${base}-${n}`;
    if (history.worktrees.some((t) => t.path === path)) continue;
    const state = await api.folderState(path).catch(() => "missing" as const);
    if (state === "missing" || state === "empty") return path;
  }
  return base;
}

/** The New Worktree sheet; `branch` is the one it was opened for, if any. */
export async function newWorktreeRequest(ctx: BranchContext, repoPath: string, branch?: string): Promise<Request> {
  const free = freeBranches(ctx.history);
  const first = branch && free.includes(branch) ? branch : free[0];
  const state: NewWorktree = first
    ? { branch: first, create: false, from: null, folder: "", edited: false, open: true, search: null }
    : { branch: "", create: true, from: null, folder: "", edited: false, open: true, search: null };
  state.folder = tilde(await freeFolder(ctx.history, repoPath, state.branch || "new"));
  const shown = [...new Set([first, ...free].filter((b): b is string => !!b))].slice(0, 3);
  return newWorktreeSheet(ctx, repoPath, shown, free, state);
}

/** Branch chips shown while searching. */
const FOUND = 8;

function newWorktreeSheet(ctx: BranchContext, repoPath: string, shown: string[], free: string[], state: NewWorktree): Request {
  const again = (change: Partial<NewWorktree>) => newWorktreeSheet(ctx, repoPath, shown, free, { ...state, ...change });
  /** Pick another branch: the folder follows it unless it was typed. */
  const pickBranch = async (change: Partial<NewWorktree>) => {
    const next = { ...state, ...change };
    if (!next.edited) next.folder = tilde(await freeFolder(ctx.history, repoPath, next.branch || "new"));
    return newWorktreeSheet(ctx, repoPath, shown, free, next);
  };
  const { history } = ctx;
  const name = state.branch.trim();
  const path = untilde(state.folder.trim()).replace(/[/\\]$/, "");
  const folderName = path.split(/[/\\]/).pop() ?? "";
  const nextTo = parentOf(path) === parentOf(repoPath.replace(/[/\\]$/, ""));
  const here = history.head.branch;
  const fromName = state.from ?? here ?? (history.head.commit ? shortId(history.head.commit) : "HEAD");

  let invalid: string | null = null;
  let nameError: string | undefined;
  if (state.create) {
    const problem = nameProblem(ctx, name);
    invalid = problem;
    if (name && problem) nameError = problem;
  } else if (!name) invalid = "Every branch is checked out in a worktree already: pick New branch.";
  if (!invalid && state.search !== null) invalid = "Pick one of the branches found.";
  let folderError: string | undefined;
  if (!invalid) {
    if (!path) invalid = "Choose a folder for it.";
    else if (!isAbsolute(path)) invalid = folderError = "Type the full path of the folder, or choose one.";
    else if (history.worktrees.some((t) => t.path === path)) invalid = folderError = "That folder is a worktree already.";
  }

  const body: Part[] = ["Makes a second working folder, ", { code: folderName || "a new folder" }, nextTo ? ", next to this one, with " : ", with "];
  if (state.create) body.push("a new branch ", ...(name && !nameError ? [{ branch: name, color: ctx.colorOf(fromName) }, " "] : []), "from ", { branch: fromName, color: ctx.colorOf(fromName) });
  else if (name) body.push({ branch: name, color: ctx.colorOf(name) });
  body.push(" checked out. Both folders share one repository: commit in either and it shows up in both.");
  if (ctx.uncommitted) body.push(` Your ${plural(ctx.uncommitted, "uncommitted file")} stay${ctx.uncommitted === 1 ? "s" : ""} here.`);

  const busy = [here, ...history.worktrees.filter((t) => !t.current).map((t) => t.branch)].filter((b): b is string => !!b);
  const notes: string[] = [];
  if (busy.length) notes.push(`${list([...new Set(busy)])} ${busy.length === 1 ? "is" : "are"} already checked out in a worktree.`);

  // Other… opens a search box over every free branch; a branch picked there joins the chips.
  const searching = state.search !== null;
  const query = (state.search ?? "").trim().toLowerCase();
  const found = free.filter((b) => b.toLowerCase().includes(query));
  const picked = !state.create && state.branch && !shown.includes(state.branch) ? [state.branch] : [];
  const names = searching ? found.slice(0, FOUND) : [...shown, ...picked];
  // While searching no chip is on, so a click on any of them picks it and closes the search.
  const branchChip = (b: string) => ({ label: b, on: !searching && !state.create && state.branch === b, pick: () => pickBranch({ branch: b, create: false, search: null }) });
  if (searching) {
    notes.length = 0;
    if (!found.length) notes.push(`No free branch matches “${state.search?.trim()}”.`);
    else if (found.length > FOUND) notes.push(`${found.length} branches match: type more of the name.`);
  }
  const fields: NonNullable<Request["fields"]> = [
    {
      label: "Branch",
      text: searching
        ? { value: state.search ?? "", placeholder: `Find a branch: ${free.length} are free`, focus: true, edit: (value) => again({ search: value }) }
        : undefined,
      chips: [
        ...names.map(branchChip),
        ...(free.length > shown.length && !searching ? [{ label: "Other…", on: false, pick: () => again({ search: "" }) }] : []),
        { label: "New branch", on: !searching && state.create, pick: () => pickBranch({ branch: state.create ? state.branch : "", create: true, search: null }) },
      ],
      note: notes.join(" ") || undefined,
    },
  ];
  if (state.create) {
    const withPrefix = (prefix: string) => {
      const bare = PREFIXES.reduce((n, p) => (n.startsWith(p) ? n.slice(p.length) : n), state.branch);
      return state.branch.startsWith(prefix) ? bare : prefix + bare;
    };
    const follow = (branch: string): Partial<NewWorktree> => ({ branch, ...(state.edited ? {} : { folder: tilde(folderFor(repoPath, branch || "new")) }) });
    fields.push({
      label: "Name",
      text: { value: state.branch, placeholder: "feature/csv-export", edit: (value) => again(follow(value)) },
      chips: PREFIXES.map((p) => ({ label: p, on: state.branch.startsWith(p), pick: () => again(follow(withPrefix(p))) })),
      error: nameError,
    });
    const froms = [...new Set([here, history.trunk].filter((b): b is string => !!b))];
    if (froms.length > 1) fields.push({ label: "From", chips: froms.map((b) => ({ label: b, on: fromName === b, pick: () => again({ from: b === here ? null : b }) })) });
  }
  fields.push({
    label: "Folder",
    text: { value: state.folder, placeholder: "~/Developer/project-feature", edit: (value) => again({ folder: value, edited: true }) },
    chips: [
      {
        label: "Choose…",
        on: false,
        pick: async () => {
          const picked = await open({ directory: true, multiple: false, title: "Folder for the new worktree", defaultPath: parentOf(path || repoPath) || undefined });
          return typeof picked === "string" ? again({ folder: tilde(picked), edited: true }) : again({});
        },
      },
    ],
    error: folderError,
  });

  return {
    title: "New worktree",
    body,
    icon: "worktree",
    button: "Create Worktree",
    fields,
    options: [{ label: "Open in a new window", sub: "Each worktree gets its own Oxbow window", on: state.open, toggle: () => again({ open: !state.open }) }],
    invalid,
    note: "Remove it any time: the branch and its commits stay.",
    status: `Creating ${folderName}…`,
    done: state.open ? `Created ${folderName}. It opens in a new window.` : `Created ${folderName}, with ${name} checked out.`,
    after: state.open ? () => void api.openWindow(path).catch((err) => console.error(err)) : undefined,
    action: { kind: "worktreeAdd", path, branch: name, create: state.create, start: state.create ? state.from : null },
  };
}

/** Remove a worktree; with `stash`, its uncommitted changes go to the stash first. */
export function removeWorktreeRequest(ctx: BranchContext, wt: Worktree, stash = false): Request {
  const dirty = (wt.changed ?? 0) > 0;
  /** `git status` failed there, or is still to come: changes or not isn't known, so it isn't forced. */
  const unknown = wt.changed === null;
  const changed = wt.changed ?? 0;
  const body: Part[] = ["Deletes the folder ", { code: tilde(wt.path) }, ". "];
  if (wt.branch) body.push(branchPart(ctx, wt), " and all its commits stay in this repository.");
  else body.push("HEAD there is detached at ", branchPart(ctx, wt), ": commits only it has are left on no branch.");
  if (unknown && stash) body.push(" Any uncommitted changes there go to the stash first, under Stashes, so they aren’t lost.");
  else if (unknown && checking(wt)) body.push(" Oxbow is still checking it for uncommitted changes, so it isn’t forced: git stops if there are any.");
  else if (unknown) body.push(" Oxbow couldn’t check it for uncommitted changes (git status failed there), so it isn’t forced: git stops if there are any.");
  else if (dirty && stash) body.push(` Its ${plural(changed, "uncommitted file")} go${changed === 1 ? "es" : ""} to the stash first, under Stashes, so ${changed === 1 ? "it isn’t" : "they aren’t"} lost.`);
  else if (dirty) body.push(` The folder has ${plural(changed, "changed file")} that ${changed === 1 ? "was" : "were"} never committed; removing deletes ${changed === 1 ? "it" : "them"} too.`);
  else body.push(" Files git ignores there, like build output, go too.");
  if (wt.locked !== null) body.push(" It is locked, so it is unlocked first.");
  return {
    title: `Remove the ${wt.name} worktree?`,
    body,
    icon: stash ? "stash" : "drop",
    button: stash ? "Stash and Remove" : "Remove",
    danger: !stash,
    alt: (dirty || unknown) && !stash ? { label: "Stash Them First", request: () => removeWorktreeRequest(ctx, wt, true) } : undefined,
    note: (dirty || unknown) && !stash ? "Stash Them First keeps them, under Stashes." : "The branch stays: make a worktree for it again any time.",
    status: `Removing ${wt.name}…`,
    done: `Removed ${wt.name}.${dirty && stash ? " Its changes are in the stash." : ""}`,
    after: () => void api.closeWindowOf(wt.path).catch(() => {}),
    action: { kind: "worktreeRemove", path: wt.path, force: dirty, stash: (dirty || unknown) && stash },
  };
}

/** Lock a worktree, e.g. one on a drive that isn't always connected. */
export function lockRequest(wt: Worktree, reason = ""): Request {
  return {
    title: `Lock ${wt.name}?`,
    body: ["Git won’t prune or remove a locked worktree, so it stays known even while its folder can’t be found, e.g. on a drive that isn’t connected."],
    icon: "worktree",
    button: "Lock",
    fields: [{ label: "Reason", text: { value: reason, placeholder: "On the external drive (optional)", edit: (value) => lockRequest(wt, value) } }],
    status: `Locking ${wt.name}…`,
    done: `Locked ${wt.name}.`,
    action: { kind: "worktreeLock", path: wt.path, reason: reason.trim() || null },
  };
}

export function unlockRequest(wt: Worktree): Request {
  return {
    title: `Unlock ${wt.name}?`,
    body: [wt.locked ? `It was locked: “${wt.locked}”. ` : "", "Prune and Remove work on it again."],
    icon: "worktree",
    button: "Unlock",
    status: `Unlocking ${wt.name}…`,
    done: `Unlocked ${wt.name}.`,
    action: { kind: "worktreeUnlock", path: wt.path },
  };
}

/** Forget the worktrees whose folders are gone. */
export function pruneWorktreesRequest(ctx: BranchContext): Request {
  const missing = ctx.history.worktrees.filter((t) => t.missing);
  const gone = missing.filter((t) => t.locked === null);
  const kept = missing.filter((t) => t.locked !== null);
  const body: Part[] = [`Git forgets ${gone.length === 1 ? "the worktree" : "the worktrees"} whose folder${gone.length === 1 ? " is" : "s are"} gone: `];
  gone.forEach((t, i) => {
    if (i) body.push(i === gone.length - 1 ? " and " : ", ");
    body.push({ code: t.name });
  });
  body.push(". Branches and commits stay, and a branch that was checked out there can be checked out again.");
  if (kept.length) body.push(` Locked ones stay: ${list(kept.map((t) => t.name))}.`);
  return {
    title: gone.length === 1 ? `Forget ${gone[0].name}?` : `Forget ${gone.length} missing worktrees?`,
    body,
    icon: "worktree",
    button: "Prune",
    status: "Pruning worktrees…",
    done: gone.length === 1 ? `Forgot ${gone[0].name}.` : `Forgot ${gone.length} worktrees.`,
    action: { kind: "worktreePrune" },
  };
}

/** Locate Folder… on a missing worktree: pick where it went, then reconnect it. */
export async function locateRequest(ctx: BranchContext, wt: Worktree): Promise<Request | null> {
  const picked = await open({ directory: true, multiple: false, title: `Locate ${wt.name}`, defaultPath: parentOf(wt.path) || undefined });
  if (typeof picked !== "string") return null;
  const name = picked.split(/[/\\]/).filter(Boolean).pop() ?? picked;
  return {
    title: `Reconnect ${wt.name}?`,
    body: ["Git and the folder ", { code: tilde(picked) }, " are told about each other again, so ", branchPart(ctx, wt), " works there as before."],
    icon: "worktree",
    button: "Reconnect",
    status: `Reconnecting ${name}…`,
    done: `Reconnected ${name}.`,
    action: { kind: "worktreeRepair", path: picked },
  };
}

/** Checking out `branch`, which another worktree has: git refuses, so offer that folder instead. */
export function elsewhereRequest(ctx: BranchContext, branch: string, wt: Worktree): Request {
  const chip: Part = { branch, color: ctx.colorOf(branch) };
  if (wt.missing) {
    return {
      title: `${branch} is checked out in a missing worktree`,
      body: ["Git won’t check out ", chip, " here: the worktree ", { code: wt.name }, " has it, and its folder is gone. Prune forgets that worktree, then the branch is free."],
      icon: "worktree",
      tone: "warn",
      button: "Prune",
      invalid: wt.locked !== null ? `${wt.name} is locked: unlock it in the sidebar first.` : null,
      note: "Git keeps a branch in one folder at a time.",
      status: "Pruning worktrees…",
      done: `Forgot ${wt.name}. ${branch} can be checked out now.`,
      action: { kind: "worktreePrune" },
    };
  }
  return {
    title: `${branch} is open in another worktree`,
    body: ["Git won’t check out ", chip, " here: it is already checked out in ", { code: wt.name }, ". Open that folder, or remove the worktree first."],
    icon: "worktree",
    button: `Open ${wt.name}`,
    note: "Git keeps a branch in one folder at a time.",
    status: `Opening ${wt.name}…`,
    local: {
      commands: [`git switch ${branch}`],
      comment: `git refuses: ${branch} is already used by the worktree at ${tilde(wt.path)}`,
      run: async () => {
        await api.openWindow(wt.path);
        return `Opened ${wt.name}.`;
      },
    },
  };
}

/** git refused a checkout because another worktree has the branch: offer that folder. */
export function refusedForWorktree(failure: Failure, ctx: BranchContext): Recovery | null {
  const said = /'([^']+)' is already (?:used by worktree|checked out) at '([^']+)'/.exec(failure.output);
  if (!said) return null;
  const [, branch, path] = said;
  const wt = ctx.history.worktrees.find((t) => t.path === path);
  const name = wt?.name ?? path.split(/[/\\]/).pop() ?? path;
  return {
    title: `${branch} is open in another worktree`,
    body: ["Git won’t check out ", { branch, color: ctx.colorOf(branch) }, " here: it is already checked out in ", { code: name }, ". Open that folder, or remove the worktree first."],
    icon: "worktree",
    tone: "warn",
    act: wt && !wt.missing ? { label: `Open ${name}`, run: () => api.openWindow(wt.path).then(() => `Opened ${name}.`) } : undefined,
    note: "Git keeps a branch in one folder at a time.",
    close: "Cancel",
  };
}
