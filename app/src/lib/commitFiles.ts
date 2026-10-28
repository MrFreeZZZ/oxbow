// What can be done with one file of a commit, in the menus of the commit's files and of File
// History: undo or copy just its change, take the file as the commit has it, or compare it with
// the file on disk. Every one changes only the files on disk, uncommitted, through the
// Operation Log like any other step.

import type { Part, Request } from "./confirm.svelte";
import type { FileStatus } from "./types";

// Type imports only, so the tests run on this file as it is.
const shortId = (id: string) => id.slice(0, 7);
const splitPath = (path: string) => ({ name: path.slice(path.lastIndexOf("/") + 1) });

/** A file of a commit, as the commit's list and File History know it. */
export interface CommitFile {
  path: string;
  oldPath?: string | null;
  status: FileStatus;
  submodule?: unknown;
}

export type CommitFileActionId = "revertFile" | "cherryPickFile" | "takeVersion" | "compare";

export interface CommitFileAction {
  id: CommitFileActionId;
  label: string;
}

/** The actions for `file` of a commit, in menu order. `inHead`: the commit is in the history of
 *  the checked-out commit, so its change is there to undo; otherwise there to copy. Null when
 *  that isn't known: both are offered. A submodule has none, a file the commit deleted only
 *  Revert and Cherry-Pick: there is no version of it to take or compare. */
export function commitFileActions(file: CommitFile, inHead: boolean | null): CommitFileAction[] {
  if (file.submodule) return [];
  const out: CommitFileAction[] = [];
  if (inHead !== false) out.push({ id: "revertFile", label: "Revert This File" });
  if (inHead !== true) out.push({ id: "cherryPickFile", label: "Cherry-Pick This File" });
  if (file.status !== "deleted") {
    out.push({ id: "takeVersion", label: "Take This Version…" });
    out.push({ id: "compare", label: "Compare with Working Copy" });
  }
  return out;
}

const commitPart = (commit: string, summary: string | null): Part[] => (summary ? [{ code: shortId(commit) }, " ", { quote: summary }] : [{ code: shortId(commit) }]);

/** Revert This File: what the commit changed in the file comes out of it, uncommitted. */
export function revertFileRequest(file: CommitFile, commit: string, summary: string | null): Request {
  const sha = shortId(commit);
  const name = splitPath(file.path).name;
  const what: Part[] =
    file.status === "added"
      ? ["The commit added ", { code: file.path }, ", so it is deleted from your files."]
      : file.status === "deleted"
        ? ["The commit deleted ", { code: file.path }, ", so it comes back."]
        : ["Takes what ", ...commitPart(commit, summary), " changed in ", { code: file.path }, " out of the file on disk."];
  return {
    title: `Revert ${name} of ${sha}?`,
    body: [...what, " Its other files and the commit itself stay as they are; nothing is committed."],
    icon: "revert",
    button: "Revert File",
    note: "Undo brings the file back, from the toast or the Operation Log.",
    status: `Reverting ${name}…`,
    done: `Reverted ${name} of ${sha}. Review it in Changes, then commit.`,
    action: { kind: "revertFile", commit, path: file.path, oldPath: file.oldPath ?? null },
  };
}

/** Cherry-Pick This File: the commit's change to the file goes into it, uncommitted. */
export function cherryPickFileRequest(file: CommitFile, commit: string, summary: string | null): Request {
  const sha = shortId(commit);
  const name = splitPath(file.path).name;
  return {
    title: `Cherry-pick ${name} from ${sha}?`,
    body: ["Brings what ", ...commitPart(commit, summary), " changed in ", { code: file.path }, " into your files, uncommitted. The commit's other files stay out."],
    icon: "cherry",
    button: "Cherry-Pick File",
    note: "Undo takes it back out, from the toast or the Operation Log.",
    status: `Cherry-picking ${name}…`,
    done: `Cherry-picked ${name} from ${sha}. Review it in Changes, then commit.`,
    action: { kind: "cherryPickFile", commit, path: file.path, oldPath: file.oldPath ?? null },
  };
}

/** Take This Version: the file on disk becomes the commit's version; nothing is staged. */
export function takeVersionRequest(file: CommitFile, commit: string, summary: string | null): Request {
  const sha = shortId(commit);
  const name = splitPath(file.path).name;
  return {
    title: `Do you really want to take ${name} as it is in ${sha}?`,
    body: ["Writes ", { code: file.path }, " on disk as it is in ", ...commitPart(commit, summary), ". Your edits to it since are replaced; nothing is staged or committed."],
    icon: "checkout",
    button: "Take This Version",
    tone: "warn",
    note: "Undo brings your version back, from the toast or the Operation Log.",
    status: `Taking ${name} from ${sha}…`,
    done: `${name} is now as it is in ${sha}.`,
    action: { kind: "takeVersion", commit, path: file.path },
  };
}

/** The request of an action that runs as a step; Compare with Working Copy opens a view instead. */
export function commitFileRequest(id: Exclude<CommitFileActionId, "compare">, file: CommitFile, commit: string, summary: string | null): Request {
  if (id === "revertFile") return revertFileRequest(file, commit, summary);
  if (id === "cherryPickFile") return cherryPickFileRequest(file, commit, summary);
  return takeVersionRequest(file, commit, summary);
}
