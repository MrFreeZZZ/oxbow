// Git LFS: which files the repository keeps on the LFS server, and the confirmations that
// change that. Big files get a note in Changes with a way into LFS; a repository that uses LFS
// while git-lfs isn't installed gets a banner, since its files are only pointers then.

import { api } from "./api";
import { confirm, type Part, type Recovery, type Request } from "./confirm.svelte";
import type { Failure, LfsFile, LfsPattern, LfsStatus } from "./types";
import { bytes } from "./unusual";
import { splitPath } from "./format";
import { errorLine } from "./reads";
import { matches } from "./lfsGlob";

export { BIG_FILE } from "./unusual";
export { matches } from "./lfsGlob";

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

class LfsState {
  status = $state<LfsStatus | null>(null);
  /** Not Now on the banner, until another repository is opened. */
  dismissed = $state(false);
  #loads = 0;

  /** git-lfs is installed and turned on. */
  get ready(): boolean {
    return !!this.status?.version && this.status.filters;
  }

  get patterns(): LfsPattern[] {
    return this.status?.patterns ?? [];
  }

  /** Files the repository keeps in LFS. */
  get files(): number {
    return this.patterns.reduce((n, p) => n + p.files, 0);
  }

  /** The repository has LFS files that git-lfs can't fetch here. */
  get missing(): boolean {
    return !this.ready && this.files > 0;
  }

  async load() {
    const mine = ++this.#loads;
    try {
      const next = await api.lfsStatus();
      if (mine === this.#loads) this.status = next;
    } catch {
      if (mine === this.#loads) this.status = null;
    }
  }

  forget() {
    this.#loads++;
    this.status = null;
    this.dismissed = false;
  }

  /** The rule that sends `path` to LFS: the last one that matches, as in .gitattributes. */
  patternOf(path: string): string | null {
    return [...this.patterns].reverse().find((p) => matches(p.pattern, path))?.pattern ?? null;
  }
}

export const lfs = new LfsState();

/** Patterns to offer for a file: its kind, its folder, the file itself. */
export function patternChoices(path: string): string[] {
  const { dir, name } = splitPath(path);
  const dot = name.lastIndexOf(".");
  const out: string[] = [];
  if (dot > 0) out.push(`*${name.slice(dot)}`);
  if (dir) out.push(`${dir.replace(/\/$/, "")}/**`);
  out.push(path);
  return out;
}

function patternProblem(pattern: string): string | null {
  if (!pattern) return "Type a pattern, like *.psd.";
  if (/\s/.test(pattern)) return "Patterns can’t contain spaces here.";
  if (lfs.patterns.some((p) => p.pattern === pattern)) return `${pattern} is already tracked with Git LFS.`;
  return null;
}

interface Track {
  pattern: string;
  /** Stage the file and .gitattributes right away. */
  stage: boolean;
}

/** Track with Git LFS: for a big file in Changes (`path`), or a new pattern from the sidebar. */
export function lfsTrackRequest(path: string | null, size: number | null, state?: Track): Request {
  const choices = path ? patternChoices(path) : [];
  const s: Track = state ?? { pattern: choices[0] ?? "", stage: true };
  const again = (change: Partial<Track>) => lfsTrackRequest(path, size, { ...s, ...change });
  const pattern = s.pattern.trim();
  const problem = patternProblem(pattern);
  const name = path ? splitPath(path).name : null;
  const misses = !!path && !!pattern && !problem && !matches(pattern, path);
  const stagePaths = path && s.stage && !misses ? [path] : [];

  const body: Part[] = ["Adds ", { code: pattern || "…" }, " to .gitattributes, so Git LFS stores "];
  if (name) body.push({ code: name }, " on the LFS server and the commit gets a small pointer", size ? ` instead of ${bytes(size)}` : "", ".");
  else body.push("matching files on the LFS server and commits get small pointers instead.");
  body.push(" Everyone who pulls gets the rule too, as .gitattributes is committed.");

  const field: NonNullable<Request["fields"]>[number] = {
    label: "Pattern",
    text: { value: s.pattern, placeholder: "*.psd", edit: (value) => again({ pattern: value }) },
    chips: choices.map((c) => ({ label: c, mono: true, on: pattern === c, pick: () => again({ pattern: c }) })),
    error: pattern && problem ? problem : misses ? `${pattern} doesn’t match ${path}.` : undefined,
    note: describePattern(pattern),
  };
  return {
    title: `Track ${pattern || "files"} with Git LFS?`,
    body,
    icon: "lfs",
    button: "Track",
    fields: [field],
    options: [
      {
        label: name && !misses ? `Stage ${name} and .gitattributes` : "Stage .gitattributes",
        sub: "Ready for the next commit",
        on: s.stage,
        toggle: () => again({ stage: !s.stage }),
      },
    ],
    note: "Commits you already made are not changed.",
    invalid: problem,
    status: `Tracking ${pattern}…`,
    done: `Git LFS now tracks ${pattern}.`,
    action: { kind: "lfsTrack", pattern, paths: stagePaths, stage: s.stage },
  };
}

/** What a pattern covers, in words, under the field. */
function describePattern(pattern: string): string | undefined {
  if (!pattern) return "Like .gitignore: *.psd, assets/video/**, a single path.";
  const ext = /^\*(\.[^/*?[]+)$/.exec(pattern);
  if (ext) return `Every ${ext[1]} file in the repository`;
  const folder = /^(.+)\/\*\*$/.exec(pattern);
  if (folder) return `Everything in ${folder[1]}`;
  if (!/[*?[]/.test(pattern)) return "Only this file";
  return undefined;
}

/** Stop Tracking: new versions of matching files go into git itself again. */
export function untrackRequest(pattern: string): Request {
  const entry = lfs.patterns.find((p) => p.pattern === pattern);
  const body: Part[] = ["Removes ", { code: pattern }, " from .gitattributes. "];
  body.push(
    entry?.files
      ? `The ${plural(entry.files, "file")} already in LFS stay there; new versions of matching files are stored by git itself, full size.`
      : "New matching files are stored by git itself, full size.",
  );
  return {
    title: `Stop tracking ${pattern} with Git LFS?`,
    body,
    icon: "lfs",
    tone: "warn",
    button: "Stop Tracking",
    note: "Commits you already made are not changed.",
    status: `Untracking ${pattern}…`,
    done: `Git LFS no longer tracks ${pattern}.`,
    action: { kind: "lfsUntrack", pattern },
  };
}

/** Files listed in the sheet; the rest are counted in one more row. */
const LISTED = 300;

/** The files not downloaded here that `pattern` matches, those at `paths`, or all. */
export function missingFiles(pattern: LfsPattern | null, paths?: string[]): LfsFile[] {
  const all = lfs.status?.missingFiles ?? [];
  if (paths) return all.filter((f) => paths.includes(f.path));
  return pattern ? all.filter((f) => matches(pattern.pattern, f.path)) : all;
}

/** Download the files of one pattern, the ones at `paths`, or all, in place of their pointers.
 *  The sheet lists them with their sizes, then shows how far each one is; with `always` it
 *  shows even when downloads don't ask, for the list of what isn't downloaded. */
export function downloadRequest(pattern: LfsPattern | null, paths?: string[], always = false): Request {
  const files = missingFiles(pattern, paths);
  // The status lists at most 5000; the patterns count them all.
  const count = paths ? files.length : pattern ? Math.max(pattern.missing, files.length) : Math.max(lfs.patterns.reduce((n, p) => n + p.missing, 0), files.length);
  const size = files.reduce((n, f) => n + f.size, 0);
  const one = count === 1 && files.length === 1 ? splitPath(files[0].path).name : null;
  const what = one ?? (pattern ? `${plural(count, "file")} of ${pattern.pattern}` : count ? plural(count, "LFS file") : "LFS files");
  const list = files.slice(0, LISTED).map((f) => ({ name: f.path, meta: bytes(f.size) }));
  if (files.length > LISTED) {
    const rest = files.slice(LISTED);
    list.push({ name: `and ${plural(rest.length, "more file")}`, meta: bytes(rest.reduce((n, f) => n + f.size, 0)) });
  }
  const body: Part[] = one
    ? ["Downloads ", { code: files[0].path }, " from the LFS server and puts it in place of its pointer"]
    : pattern
      ? ["Downloads the files ", { code: pattern.pattern }, " matches from the LFS server and puts them in place of their pointers"]
      : count
        ? ["Downloads these files from the LFS server in place of the pointers git checked out"]
        : ["Downloads the LFS files of the checked-out commit and puts them in place of their pointers"];
  body.push(size ? `: ${bytes(size)} in all. Nothing is committed.` : ". Nothing is committed.");
  return {
    title: `Download ${what}?`,
    body,
    icon: "fetch",
    button: "Download",
    always,
    list,
    status: `Downloading ${what}…`,
    done: one ? `Downloaded ${one}.` : pattern ? `Downloaded the files of ${pattern.pattern}.` : "Downloaded the LFS files.",
    past: `Downloaded ${what}`,
    partWhy: lfsWhy,
    recover: downloadRecovery,
    action: { kind: "lfsPull", include: pattern?.pattern ?? null, paths: paths ?? [] },
  };
}

/** Why git-lfs couldn't download a file, in words, from what it printed. */
export function lfsWhy(error: string): string {
  if (/\[404\]|does not exist/i.test(error)) return "The LFS server doesn’t have it; whoever committed it may not have pushed it.";
  if (/\[40[13]\]|permission|credentials|authenticat/i.test(error)) return "The LFS server didn’t allow it with this account.";
  if (/dial tcp|connection (refused|reset)|timed? ?out|no such host|could not resolve|network|EOF/i.test(error)) return "Couldn’t reach the LFS server.";
  if (/no space left/i.test(error)) return "There is no space left on the disk.";
  return errorLine(error);
}

/** After a download where some files failed: which, why, and Try Again for just those. */
function downloadRecovery(failure: Failure): Recovery | null {
  if (failure.kind !== "partlyDone") return null;
  const parts = confirm.parts;
  const failed = parts.filter((p) => p.state === "failed");
  if (!failed.length) return null;
  const done = parts.filter((p) => p.state === "done").length;
  const name = (path: string) => splitPath(path).name;
  const one = failed.length === 1 ? name(failed[0].name) : null;
  const reasons = new Set(failed.map((p) => lfsWhy(p.error ?? "")));
  const body: Part[] = [];
  failed.slice(0, 3).forEach((p, i) => {
    if (i) body.push(i === failed.length - 1 ? " and " : ", ");
    body.push({ code: name(p.name) });
  });
  if (failed.length > 3) body.push(` and ${plural(failed.length - 3, "more file")}`);
  body.push(failed.length === 1 ? " is still a pointer" : " are still pointers");
  body.push(reasons.size === 1 ? `: ${[...reasons][0].replace(/\.$/, "").replace(/^./, (c) => c.toLowerCase())}.` : ". Each one says why below.");
  if (done) body.push(` The other ${plural(done, "file")} ${done === 1 ? "is" : "are"} in place.`);
  const paths = failed.map((p) => p.name);
  return {
    title: done ? `Downloaded ${done} of ${plural(parts.length, "file")}` : parts.length === 1 ? `Couldn’t download ${name(parts[0].name)}` : `None of the ${plural(parts.length, "file")} could be downloaded`,
    body,
    icon: "fetch",
    tone: "warn",
    button: {
      label: one ? "Try Again" : `Try ${failed.length} Again`,
      action: { kind: "lfsPull", include: null, paths },
      status: one ? `Downloading ${one}…` : `Downloading ${plural(failed.length, "file")}…`,
      done: one ? `Downloaded ${one}.` : "Downloaded the LFS files.",
      recover: downloadRecovery,
    },
  };
}

/** Free Up Space: local copies of old versions go, once the server is known to have them. */
export function pruneRequest(preview: string | null): Request {
  return {
    title: "Free up Git LFS space?",
    body: [
      "Deletes local copies of LFS files that the checked-out commit and recent branches don’t need, and only ones the LFS server has. They download again if you check out an older commit.",
    ],
    icon: "lfs",
    button: "Free Up Space",
    note: preview ? `Right now: ${preview}.` : "Nothing old to delete right now; recent versions are kept.",
    status: "Checking the LFS server and deleting…",
    done: "Freed up Git LFS space.",
    action: { kind: "lfsPrune" },
  };
}

/** Install Git LFS with Homebrew, or turn on one that is installed, then download the files. */
export function installRequest(repoName: string): Request | null {
  const status = lfs.status;
  if (!status) return null;
  const installed = !!status.version;
  if (!installed && !status.brew) return null;
  const files = lfs.files;
  const pull = files > 0;
  const body: Part[] = installed
    ? [`git-lfs is installed but not turned on, so git leaves LFS files as pointers.`]
    : [`${repoName} keeps ${plural(files, "file")} in Git LFS, but git-lfs isn’t installed on this computer.`];
  body.push(" Until it is, those files are small pointer text files: images don’t open, videos don’t play, tests that read fixtures fail.");
  return {
    title: installed ? "Turn on Git LFS?" : "Install Git LFS?",
    body,
    icon: "lfs",
    button: installed ? "Turn On" : "Install",
    note: !installed && status.brew ? `Needs Homebrew. Oxbow found it in ${status.brew.replace(/\/bin\/brew$/, "")}.` : undefined,
    status: installed ? "Turning on Git LFS…" : "Installing Git LFS…",
    done: pull ? "Git LFS is ready, and the files are downloaded." : "Git LFS is ready.",
    action: { kind: "lfsInstall", brew: installed ? null : status.brew, pull },
  };
}

/** `12 files · 340 MB`, or `3 pointers · not downloaded` while git-lfs can't fetch them. */
export function patternNote(p: LfsPattern): string {
  if (!p.files) return p.new ? "no files yet" : "no files";
  if (!lfs.ready && p.missing) return `${plural(p.missing, "pointer")} · not downloaded`;
  return `${plural(p.files, "file")} · ${p.new ? "uploads on push" : bytes(p.size)}`;
}
