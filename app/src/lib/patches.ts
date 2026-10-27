// Commits as patch files: Save as Patches writes them with git format-patch, a file each or all
// in one; Apply Patches shows what patch files bring, then runs git am (or git apply for a plain
// diff), which can stop on conflicts like a rebase.

import { open, save } from "@tauri-apps/plugin-dialog";
import { desktopDir, homeDir } from "@tauri-apps/api/path";
import { api } from "./api";
import type { Part, Recovery, Request } from "./confirm.svelte";
import type { BranchContext } from "./branches";
import { stoppedOnConflicts } from "./merge";
import { shortId } from "./format";
import { tilde as tildeIn } from "./term";
import type { CommitBrief, Failure, PatchPreview } from "./types";

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : /(ch|sh|s|x)$/.test(word) ? "es" : "s"}`;
const chip = (ctx: BranchContext, name: string): Part => ({ branch: name, color: ctx.colorOf(name) });

/** Where patches were saved last, as the folder to start from next time. */
const FOLDER_KEY = "oxbow.patches.folder";

let home = "";
let desktop = "";
homeDir().then(
  (dir) => (home = dir.replace(/[/\\]$/, "")),
  () => {},
);
desktopDir().then(
  (dir) => (desktop = dir.replace(/[/\\]$/, "")),
  () => {},
);

const tilde = (path: string) => tildeIn(path, home);
function untilde(path: string): string {
  if (!home || !(path === "~" || path.startsWith("~/"))) return path;
  return home + path.slice(1);
}
const isAbsolute = (path: string) => path.startsWith("/") || /^[A-Za-z]:[\\/]/.test(path) || path.startsWith("\\\\");
const parentOf = (path: string) => path.replace(/[/\\][^/\\]*[/\\]?$/, "");
const nameOf = (path: string) => path.replace(/[/\\]$/, "").replace(/^.*[/\\]/, "");
const join = (folder: string, name: string) => `${folder.replace(/[/\\]$/, "")}/${name}`;

function lastFolder(): string {
  try {
    return localStorage.getItem(FOLDER_KEY) ?? "";
  } catch {
    return "";
  }
}

/** A name for files and folders: `auth/3-ui` is `auth-3-ui`, as format-patch names its files. */
const slug = (text: string) =>
  text
    .replace(/[^A-Za-z0-9._-]+/g, "-")
    .replace(/^[-.]+|[-.]+$/g, "")
    .slice(0, 52) || "patches";

/** The commits to save: those of `to` that `since` doesn't have (a branch's own), or the
 *  newest `count` up to `to`; `name` names the files, e.g. the branch. */
export interface PatchTarget {
  to: string;
  since: string | null;
  count: number;
  name: string;
  /** What the sheet calls them, e.g. "The commits of auth/3-ui that main doesn’t have". */
  what: Part[];
}

interface SaveState {
  oneFile: boolean;
  /** As typed or chosen, with ~ for the home folder; empty until edited. */
  place: string;
  edited: boolean;
}

/** Save as Patches: the commits listed, where they go, and the command. */
export async function savePatchesRequest(target: PatchTarget): Promise<Request> {
  const commits = await api.patchRange(target.to, target.since, target.count);
  return saveSheet(target, commits, { oneFile: false, place: "", edited: false });
}

function saveSheet(target: PatchTarget, commits: CommitBrief[], state: SaveState): Request {
  const again = (change: Partial<SaveState>) => saveSheet(target, commits, { ...state, ...change });
  const one = commits.length === 1;
  const start = lastFolder() || desktop || home;
  // A file each goes into a folder of its own, but for a single commit: its one file needs none.
  const place = state.edited ? state.place : tilde(state.oneFile ? join(start, `${slug(target.name)}.patch`) : one ? start : join(start, `${slug(target.name)}-patches`));
  const path = untilde(place.trim());
  let invalid: string | null = null;
  let error: string | undefined;
  if (!commits.length) invalid = "There are no commits to save: merges are left out.";
  else if (!path) invalid = state.oneFile ? "Choose a file to save to." : "Choose a folder to save to.";
  else if (!isAbsolute(path)) invalid = error = `Type the full path of the ${state.oneFile ? "file" : "folder"}, or choose one.`;

  const first = commits[0];
  const fileName = first ? `0001-${slug(first.summary.replace(/\s+/g, "-"))}.patch` : "";
  return {
    title: one ? "Save the commit as a patch" : `Save ${plural(commits.length, "commit")} as patches`,
    body: [...target.what, one ? ", as a file to send or to apply in another repository." : ", as files to send or to apply in another repository."],
    icon: "patch",
    button: one ? "Save Patch" : "Save Patches",
    note: "Nothing in the repository changes.",
    status: "Saving patches…",
    done: state.oneFile || one ? `Saved ${nameOf(state.oneFile ? path : join(path, fileName))}.` : `Saved ${plural(commits.length, "patch")} in ${nameOf(path)}.`,
    list: commits.map((c) => ({ name: c.summary, meta: `${shortId(c.id)} · ${c.authorName}`, words: true })),
    fields: [
      ...(one
        ? []
        : [
            {
              label: "Save as",
              chips: [
                { label: "A File Each", on: !state.oneFile, pick: () => again({ oneFile: false, edited: false }) },
                { label: "One File", on: state.oneFile, pick: () => again({ oneFile: true, edited: false }) },
              ],
              note: state.oneFile ? "One file with every commit, in order: git am applies it whole" : `${fileName} and so on, numbered in order`,
            },
          ]),
      {
        label: state.oneFile ? "File" : "Folder",
        text: { value: place, placeholder: state.oneFile ? "~/Desktop/changes.patch" : "~/Desktop/patches", edit: (value) => again({ place: value, edited: true }) },
        chips: [
          {
            label: "Choose…",
            on: false,
            pick: async () => {
              const picked = state.oneFile
                ? await save({ title: "Save patches as", defaultPath: path || undefined, filters: [{ name: "Patch", extensions: ["patch"] }] })
                : await open({ directory: true, multiple: false, title: "Save patches in", defaultPath: (one ? path : parentOf(path)) || undefined });
              return typeof picked === "string" ? again({ place: tilde(picked), edited: true }) : again({});
            },
          },
        ],
        note: one && !state.oneFile ? `${fileName} goes into this folder` : undefined,
        error,
      },
    ],
    invalid,
    after: () => {
      try {
        localStorage.setItem(FOLDER_KEY, one && !state.oneFile ? path : parentOf(path));
      } catch {
        // Next time starts from the Desktop again.
      }
    },
    action: { kind: "savePatches", to: target.to, since: target.since, count: target.count, into: path || "/", oneFile: state.oneFile },
  };
}

/** Pick patch files for Apply Patches; null when none were picked. */
export async function choosePatchFiles(): Promise<string[] | null> {
  const picked = await open({
    multiple: true,
    directory: false,
    title: "Apply patches",
    filters: [
      { name: "Patches", extensions: ["patch", "diff", "mbox", "eml", "txt"] },
      { name: "All files", extensions: ["*"] },
    ],
  });
  if (!picked) return null;
  const paths = Array.isArray(picked) ? picked : [picked];
  // 0001-…, 0002-…: in the order format-patch numbered them.
  return paths.length ? [...paths].sort((a, b) => nameOf(a).localeCompare(nameOf(b), undefined, { numeric: true })) : null;
}

/** Apply Patches: what the files bring and whether it fits, before git am runs. */
export async function applyPatchesRequest(ctx: BranchContext, paths: string[]): Promise<Request> {
  return applySheet(ctx, paths, await api.patchPreview(paths));
}

function applySheet(ctx: BranchContext, paths: string[], preview: PatchPreview): Request {
  const branch = ctx.history.head.branch;
  const here: Part = branch ? chip(ctx, branch) : { code: "HEAD" };
  const hereName = branch ?? "HEAD";
  const count = preview.commits.length + preview.more;
  const files = preview.files.length;
  const what = paths.length === 1 ? nameOf(paths[0]) : plural(paths.length, "file");
  const list = [
    ...preview.commits.map((c) => ({
      name: c.summary,
      meta: [c.id ? shortId(c.id) : null, c.authorName].filter(Boolean).join(" · "),
      words: true,
      group: plural(count, "commit"),
    })),
    ...(preview.more ? [{ name: `and ${preview.more} more`, meta: "", words: true, group: plural(count, "commit") }] : []),
    ...preview.files.map((f) => ({
      name: f.path,
      meta: f.added === null || f.removed === null ? "binary" : `+${f.added} −${f.removed}`,
      group: plural(files, "file"),
    })),
  ];

  let invalid: string | null = null;
  let warning: Request["warning"];
  if (ctx.history.operation) {
    invalid = "Finish or abort what git is in the middle of first.";
  } else if (preview.inTheWay.length) {
    const named = preview.inTheWay.slice(0, 3);
    invalid = "Commit or stash these changes first.";
    warning = {
      title: "Uncommitted changes are in the way",
      body: [
        ...named.flatMap((p, i): Part[] => [i ? (i === named.length - 1 ? " and " : ", ") : "", { code: p }]),
        preview.inTheWay.length > named.length ? ` and ${preview.inTheWay.length - named.length} more` : "",
        preview.inTheWay.length === 1 ? " has" : " have",
        " uncommitted changes: git am starts only with nothing staged and no edits in the files the patches change. Commit or stash them, then apply the patches.",
      ],
    };
  } else if (preview.plain && (preview.notClean.length || preview.fails)) {
    invalid = "The diff doesn’t fit the files as they are.";
    warning = {
      title: "The diff doesn’t fit",
      body: [
        ...(preview.notClean.length ? [{ code: preview.notClean[0] } as Part, preview.notClean.length > 1 ? ` and ${preview.notClean.length - 1} more don’t` : " doesn’t"] : ["It doesn’t"]),
        " match the lines the diff expects, so git apply would change nothing. A diff made with git format-patch carries its commits and can be merged three ways.",
      ],
    };
  } else if (!preview.plain && (preview.notClean.length || preview.fails)) {
    const first = preview.notClean[0];
    warning = {
      title: first ? `${first}${preview.notClean.length > 1 ? ` and ${preview.notClean.length - 1} more` : ""} ${preview.notClean.length > 1 ? "don’t" : "doesn’t"} apply as ${preview.notClean.length > 1 ? "they are" : "it is"}` : "Some changes don’t apply as they are",
      body: [here, " changed ", preview.notClean.length > 1 ? "them" : first ? "it" : "those files", " too, so git merges three ways. If both changed the same lines, Conflicts opens and you go on from there, or skip that patch."],
    };
  }

  if (preview.plain) {
    return {
      title: `Apply ${what} to your files?`,
      body: ["A plain diff, without commits: its changes to ", plural(files, "file"), " go into your files on ", here, ", uncommitted, for you to look over and commit."],
      icon: "patch",
      button: "Apply",
      always: true,
      list,
      warning,
      invalid,
      status: "Applying the diff…",
      done: `Applied ${what}. The changes are uncommitted.`,
      action: { kind: "applyPatches", paths },
    };
  }
  return {
    title: count === 1 ? `Apply the patch to ${hereName}?` : `Apply ${plural(count, "patch")} to ${hereName}?`,
    body: [count === 1 ? "The patch becomes a commit on " : "Each patch becomes a commit on ", here, ", with its own author, date and message."],
    icon: "patch",
    button: count === 1 ? "Apply Patch" : "Apply Patches",
    always: true,
    list,
    warning,
    invalid,
    note: "Nothing is pushed.",
    status: count === 1 ? "Applying the patch…" : "Applying patches…",
    done: count === 1 ? `Applied the patch to ${hereName}.` : `Applied ${plural(count, "patch")} to ${hereName}.`,
    resolveConflicts: true,
    recover: (failure) => stoppedOnConflicts(failure, "patches") ?? stoppedWithout(failure),
    action: { kind: "applyPatches", paths },
  };
}

/** git am stopped on a patch it couldn't merge at all, with no conflicts to resolve. */
function stoppedWithout(failure: Failure): Recovery | null {
  if (!/Patch failed at|patch does not apply/.test(failure.output)) return null;
  return {
    title: "A patch doesn’t apply",
    body: ["Git couldn’t fit one of the patches, even merging three ways: the files it changes are too different here. Skip that patch, or abort to put everything back."],
    icon: "warn",
    tone: "warn",
    button: { label: "Abort Patches", action: { kind: "abort" }, status: "Aborting…", done: "Everything is back where it was." },
    close: "Later",
  };
}
