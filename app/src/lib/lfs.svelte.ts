// Git LFS: which files the repository keeps on the LFS server, and the confirmations that
// change that. Big files get a note in Changes with a way into LFS; a repository that uses LFS
// while git-lfs isn't installed gets a banner, since its files are only pointers then.

import { api } from "./api";
import type { Part, Request } from "./confirm.svelte";
import type { LfsPattern, LfsStatus } from "./types";
import { bytes } from "./unusual";
import { splitPath } from "./format";

export { BIG_FILE } from "./unusual";

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

/** Whether a .gitattributes pattern matches `path`: without a slash it matches a name in any
 *  folder, with one a path from the top. */
export function matches(pattern: string, path: string): boolean {
  const anchored = pattern.replace(/\/$/, "").includes("/");
  const target = anchored ? path : path.slice(path.lastIndexOf("/") + 1);
  return globRegex(pattern.replace(/^\//, "")).test(target);
}

function globRegex(glob: string): RegExp {
  let out = "";
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i];
    if (c === "*" && glob[i + 1] === "*") {
      // `**/` is any folders, a trailing `/**` everything inside.
      if (glob[i + 2] === "/") {
        out += "(?:.*/)?";
        i += 2;
      } else {
        out += ".*";
        i += 1;
      }
    } else if (c === "*") out += "[^/]*";
    else if (c === "?") out += "[^/]";
    else if (c === "[") {
      const end = glob.indexOf("]", i + 1);
      if (end < 0) out += "\\[";
      else {
        out += `[${glob.slice(i + 1, end).replace(/^!/, "^").replace(/\\/g, "\\\\")}]`;
        i = end;
      }
    } else out += c.replace(/[.+^${}()|\\]/g, "\\$&");
  }
  return new RegExp(`^${out}$`);
}

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
    action: { kind: "lfsTrack", pattern, paths: stagePaths },
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

/** Download the files of one pattern, or all, in place of their pointers. */
export function downloadRequest(pattern: LfsPattern | null): Request {
  const count = pattern ? pattern.missing : lfs.patterns.reduce((n, p) => n + p.missing, 0);
  const what = pattern ? `${plural(count, "file")} of ${pattern.pattern}` : count ? `${plural(count, "LFS file")}` : "LFS files";
  return {
    title: `Download ${what}?`,
    body: [
      pattern ? ["Downloads the files ", { code: pattern.pattern }, " matches"] : ["Downloads the LFS files of the checked-out commit"],
      " from the LFS server and puts them in place of their pointers. Nothing is committed.",
    ].flat() as Part[],
    icon: "fetch",
    button: "Download",
    status: `Downloading ${what}…`,
    done: pattern ? `Downloaded the files of ${pattern.pattern}.` : "Downloaded the LFS files.",
    action: { kind: "lfsPull", include: pattern?.pattern ?? null },
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
