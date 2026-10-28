// The right-click menu of a file in the Changes panel's lists, or of several picked files, and of
// a file of a commit.

import { api } from "./api";
import { ignoreChoices, ignoreRequest, stashFilesRequest, stashMessageFor, type StashCandidate } from "./changes";
import { stashStagedRequest } from "./stashLines";
import { commitFileActions, commitFileRequest, type CommitFile } from "./commitFiles";
import { confirm, type Request } from "./confirm.svelte";
import { splitPath } from "./format";
import { menuIcons, type MenuEntry, type SubEntry } from "./Menu.svelte";
import { nav } from "./nav.svelte";
import type { FileChange, Side } from "./types";

/** What the menu's items do, in the panel. */
export interface FileMenuActions {
  stage: (files: FileChange[]) => unknown;
  unstage: (files: FileChange[]) => unknown;
  discard: (files: FileChange[]) => unknown;
  act: (request: Request) => unknown;
  /** Stage or unstage the picked files of the list, or discard them. */
  actOnPicked: (how?: "move" | "discard") => unknown;
  /** Every file with changes once, for a stash of a few of them. */
  stashCandidates: () => StashCandidate[];
  /** How many files are staged, for Stash Staged. */
  stagedCount: () => number;
}

/** Stash Staged Changes…, in the menus of staged files. */
const stashStagedItem = (a: FileMenuActions): MenuEntry => ({ kind: "item", label: "Stash Staged Changes…", icon: menuIcons.stash, run: () => a.act(stashStagedRequest(a.stagedCount(), "")) });

export interface FileMenu {
  entries: MenuEntry[];
  label: string;
}

const revealLabel = navigator.platform.startsWith("Mac") ? "Reveal in Finder" : navigator.platform.startsWith("Win") ? "Show in Explorer" : "Open Containing Folder";

/** Copy Path/Reference, as in JetBrains IDEs: the path from the disk's root, the name, or the
 *  path in the repository, of one file or of several on lines of their own. */
function copyMenu(repo: string, paths: string[]): MenuEntry {
  // Windows paths come with backslashes.
  const sep = repo.includes("\\") && !repo.includes("/") ? "\\" : "/";
  const absolute = (path: string) => repo.replace(/[\\/]+$/, "") + sep + path.replaceAll("/", sep);
  const one = paths.length === 1;
  const item = (label: string, texts: string[]): SubEntry => ({
    label,
    hint: one ? texts[0] : undefined,
    run: () => navigator.clipboard.writeText(texts.join("\n")).then(() => confirm.say(one ? `Copied ${texts[0]}.` : `Copied ${paths.length} paths.`, "done"), () => {}),
  });
  return {
    kind: "sub",
    label: "Copy Path/Reference",
    icon: menuIcons.copy,
    entries: [
      item(one ? "Absolute Path" : "Absolute Paths", paths.map(absolute)),
      item(one ? "File Name" : "File Names", paths.map((p) => splitPath(p).name)),
      item(one ? "Path from Repository Root" : "Paths from Repository Root", paths),
    ],
  };
}

/** The menu of several picked files of one list. */
export function filesMenu(repo: string, several: FileChange[], group: Side | "untracked", a: FileMenuActions): FileMenu {
  const n = several.length;
  const fresh = several.every((f) => f.status === "untracked");
  const entries: MenuEntry[] = [{ kind: "header", label: `${n} selected files` }];
  if (group === "staged") entries.push({ kind: "item", label: `Unstage ${n} Files`, icon: menuIcons.unstage, run: () => a.actOnPicked() });
  else entries.push({ kind: "item", label: `Stage ${n} Files`, icon: menuIcons.stage, run: () => a.actOnPicked() });
  entries.push({ kind: "item", label: "Stash These Files…", icon: menuIcons.stash, run: () => a.act(stashFilesRequest(a.stashCandidates(), several.map((f) => f.path), stashMessageFor(several[0].path))) });
  if (group === "staged") entries.push(stashStagedItem(a));
  entries.push({ kind: "sep" });
  entries.push(copyMenu(repo, several.map((f) => f.path)));
  if (group !== "staged") {
    entries.push({ kind: "sep" });
    entries.push({ kind: "item", label: fresh ? `Delete ${n} Files…` : `Discard ${n} Files…`, icon: menuIcons.drop, danger: true, run: () => a.actOnPicked("discard") });
  }
  return { entries, label: `${n} files menu` };
}

/** The menu of one file. */
export function fileMenu(repo: string, file: FileChange, group: Side | "untracked" | "conflicted", a: FileMenuActions): FileMenu {
  const side = group === "untracked" ? "unstaged" : group;
  const name = splitPath(file.path).name;
  const what = { modified: "modified", added: "new file", untracked: "untracked", deleted: "deleted", renamed: "renamed", copied: "copied", conflicted: "conflicted" }[file.status];
  const entries: MenuEntry[] = [{ kind: "header", label: `${name} · ${what}${side === "staged" ? ", staged" : ""}` }];
  if (side === "staged") entries.push({ kind: "item", label: "Unstage File", icon: menuIcons.unstage, run: () => a.unstage([file]) });
  else if (side === "unstaged") entries.push({ kind: "item", label: "Stage File", icon: menuIcons.stage, run: () => a.stage([file]) });
  if (side !== "conflicted")
    entries.push({ kind: "item", label: "Stash This File…", icon: menuIcons.stash, run: () => a.act(stashFilesRequest(a.stashCandidates(), [file.path], stashMessageFor(file.path))) });
  if (side === "staged") entries.push(stashStagedItem(a));
  if (file.status !== "conflicted" && file.status !== "deleted")
    entries.push({
      kind: "sub",
      label: "Add to .gitignore",
      icon: menuIcons.ignore,
      entries: ignoreChoices(file.path, file.status !== "untracked").map((choice) => ({ label: choice.label, hint: choice.pattern, run: () => a.act(ignoreRequest(file.path, choice)) })),
    });
  entries.push({ kind: "sep" });
  if (file.status !== "deleted")
    entries.push({ kind: "item", label: "Open in Editor", icon: menuIcons.open, run: () => api.openInEditor(file.path, null).catch((err) => confirm.fail(err)) });
  if (file.status !== "deleted")
    entries.push({ kind: "item", label: revealLabel, icon: menuIcons.reveal, run: () => api.revealFile(file.path).catch((err) => confirm.fail(err)) });
  entries.push(copyMenu(repo, [file.path]));
  if (file.status !== "untracked" && file.status !== "added") entries.push({ kind: "item", label: "File History", icon: menuIcons.history, run: () => nav.openFile(file.path) });
  if (side === "unstaged") {
    entries.push({ kind: "sep" });
    entries.push({ kind: "item", label: file.status === "untracked" ? "Delete File…" : "Discard Changes…", icon: menuIcons.drop, danger: true, run: () => a.discard([file]) });
  }
  return { entries, label: `${name} menu` };
}

/** What the menu of a file of a commit needs from the view it is in. */
export interface CommitFileContext {
  /** The commit's summary, for the sheets. */
  summary: string | null;
  /** Whether the commit is in the checked-out commit's history; null when that isn't known. */
  inHead: boolean | null;
  /** Compare with Working Copy, in the view. */
  compare: (file: CommitFile) => void;
  /** Split Commit with the file picked, for a commit of the checked-out branch's own. */
  split?: ((path: string) => void) | null;
}

/** The menu of a file of a commit: what can be done with it, as with a file in Changes, but
 *  nothing to stage, stash or discard; and its change on its own, undone or copied. */
export function commitFileMenu(repo: string, file: FileChange, commit: string, ctx: CommitFileContext): FileMenu {
  const name = splitPath(file.path).name;
  const what = { modified: "modified", added: "added", untracked: "added", deleted: "deleted", renamed: "renamed", copied: "copied", conflicted: "conflicted" }[file.status];
  const entries: MenuEntry[] = [{ kind: "header", label: `${name} · ${what} in this commit` }];
  const actions = commitFileEntries(file, commit, ctx);
  const split = ctx.split;
  if (split) actions.push({ kind: "item", label: "Split into a New Commit…", icon: menuIcons.split, run: () => split(file.path) });
  if (actions.length) entries.push(...actions, { kind: "sep" });
  if (file.status !== "deleted") {
    entries.push({ kind: "item", label: "Open in Editor", icon: menuIcons.open, run: () => api.openInEditor(file.path, null).catch((err) => confirm.fail(err)) });
    entries.push({ kind: "item", label: revealLabel, icon: menuIcons.reveal, run: () => api.revealFile(file.path).catch((err) => confirm.fail(err)) });
  }
  entries.push(copyMenu(repo, [file.path]));
  entries.push({ kind: "item", label: "File History", icon: menuIcons.history, run: () => nav.openFile(file.path, commit) });
  return { entries, label: `${name} menu` };
}

const actionIcons = { revertFile: menuIcons.revert, cherryPickFile: menuIcons.cherry, takeVersion: menuIcons.checkout, compare: menuIcons.compare };

/** Revert This File, Cherry-Pick This File, Take This Version and Compare with Working Copy, as
 *  menu items; the same in File History. */
export function commitFileEntries(file: CommitFile, commit: string, ctx: CommitFileContext): MenuEntry[] {
  return commitFileActions(file, ctx.inHead).map((action) => ({
    kind: "item",
    label: action.label,
    icon: actionIcons[action.id],
    run: () => {
      if (action.id === "compare") ctx.compare(file);
      else confirm.runner?.(commitFileRequest(action.id, file, commit, ctx.summary));
    },
  }));
}
