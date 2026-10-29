// The actions of the working copy: whole files and hunks, single lines of a hunk, ignoring a
// file, stashing a few files, and the way out when a commit hook or the secrets check says no.

import type { Part, Recovery, Request } from "./confirm.svelte";
import { checkRecovery } from "./commitChecks";
import { quoted } from "./stashLines";
import { splitPath } from "./format";
import type { Action, Failure, FileChange, Hunk, SecretFinding } from "./types";

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

/** A .gitignore pattern that matches `text` literally. */
export function globEscape(text: string): string {
  return text
    .replace(/[\\*?[]/g, (c) => `\\${c}`)
    .replace(/^[#!]/, (c) => `\\${c}`)
    .replace(/ $/, "\\ ");
}

export interface IgnoreChoice {
  label: string;
  /** The line added to .gitignore. */
  pattern: string;
  /** Matches more than this one file. */
  wide: boolean;
  /** The tracked file or folder git stops tracking along with the rule; it stays on disk. */
  untrack?: string;
}

/** What Add to .gitignore offers for a file: the file, files like it, its folder. A tracked
 *  file is also taken out of git, so it is offered without the files like it, which git would
 *  go on tracking. */
export function ignoreChoices(path: string, tracked = false): IgnoreChoice[] {
  const { dir, name } = splitPath(path);
  const choices: IgnoreChoice[] = [{ label: `Ignore “${name}”`, pattern: `/${globEscape(path)}`, wide: false, untrack: tracked ? path : undefined }];
  // .env.local → .env.*, debug.log → *.log
  const dotted = /^(\.[^.]+)\.[^.]+/.exec(name);
  const ext = /^[^.].*(\.[^.]+)$/.exec(name);
  if (!tracked && dotted) choices.push({ label: `Ignore All ${dotted[1]}.* Files`, pattern: `${globEscape(dotted[1])}.*`, wide: true });
  else if (!tracked && ext) choices.push({ label: `Ignore All *${ext[1]} Files`, pattern: `*${globEscape(ext[1])}`, wide: true });
  if (dir) choices.push({ label: `Ignore Folder “${dir}”`, pattern: `/${globEscape(dir)}`, wide: true, untrack: tracked ? dir.slice(0, -1) : undefined });
  return choices;
}

export function ignoreRequest(path: string, choice: IgnoreChoice): Request {
  const { name } = splitPath(path);
  const folder = choice.pattern.endsWith("/");
  const what = folder ? `the folder ${splitPath(path).dir}` : choice.wide ? `all ${choice.pattern} files` : name;
  const body: Part[] = ["Adds the line ", { code: choice.pattern }, " to .gitignore, so git stops showing "];
  if (folder) body.push("everything in ", { code: splitPath(path).dir });
  else if (choice.wide) body.push("files like ", { code: name }, " anywhere in the repository");
  else body.push({ code: path });
  if (choice.untrack) {
    body.push(" and stops tracking it. It stays on your disk and leaves the repository with the next commit; when others pull that commit, git deletes it from their copies.");
    body.push(" .gitignore itself is staged too.");
  } else body.push(" as a change. .gitignore itself is staged, so the rule reaches your team with the next commit.");
  if (/^\.env/.test(name)) body.push(" Good call: it usually holds secrets.");
  return {
    title: `Ignore ${what}?`,
    body,
    icon: "ignore",
    button: "Ignore",
    // Taking a file out of git reaches the team: always shown first.
    always: !!choice.untrack,
    status: "Adding the rule to .gitignore…",
    done: choice.untrack ? `Added ${choice.pattern} to .gitignore and stopped tracking it.` : `Added ${choice.pattern} to .gitignore.`,
    action: { kind: "ignore", pattern: choice.pattern, untrack: choice.untrack },
  };
}

/** A file that can go into a stash of a few files. */
export interface StashCandidate {
  path: string;
  untracked: boolean;
  staged: boolean;
  unstaged: boolean;
}

/** A stash message from a file name: "WIP: session banner". */
export function stashMessageFor(path: string): string {
  return `WIP: ${splitPath(path).name.replace(/\..*$/, "").replace(/[_-]+/g, " ") || splitPath(path).name}`;
}

/** Stash the files that are ticked, with `message`. */
export function stashFilesRequest(files: StashCandidate[], on: string[], message: string): Request {
  const picked = files.filter((f) => on.includes(f.path));
  const untracked = picked.some((f) => f.untracked);
  const again = (next: string[], text = message) => stashFilesRequest(files, next, text);
  const one = picked.length === 1 ? splitPath(picked[0].path).name : null;
  return {
    title: `Stash ${one ?? plural(picked.length, "file")}?`,
    body: [
      `Saves the changes of ${picked.length === 1 ? "this file" : `these ${plural(picked.length, "file")}`} as a new stash and takes them out of your working copy. Everything else stays as it is. Bring them back from Stashes.`,
    ],
    icon: "stash",
    button: "Stash",
    fields: [
      {
        label: "Message",
        text: { value: message, placeholder: "What these changes are", edit: (value) => again(on, value) },
      },
    ],
    options: files.map((f) => ({
      label: f.path,
      sub: f.untracked ? "New file, so the stash needs --include-untracked" : f.staged && f.unstaged ? "Staged and unstaged changes" : f.staged ? "Staged" : undefined,
      on: on.includes(f.path),
      toggle: () => again(on.includes(f.path) ? on.filter((p) => p !== f.path) : [...on, f.path]),
    })),
    invalid: picked.length ? null : "Tick at least one file to stash.",
    status: "Stashing…",
    done: `Stashed ${one ?? plural(picked.length, "file")}${message.trim() ? ` as “${message.trim()}”` : ""}.`,
    action: { kind: "stashPush", message: message.trim() || null, untracked, paths: picked.map((f) => f.path) },
  };
}

const name = (files: FileChange[]): Part[] =>
  files.length === 1 ? [{ code: files[0].path }] : [`${files.length} files`];
const count = (files: FileChange[]) => (files.length === 1 ? splitPath(files[0].path).name : `${files.length} files`);

/** Stage whole files. */
export function stageRequest(files: FileChange[], branch: Part): Request {
  return {
    title: `Stage ${count(files)}?`,
    body: ["Adds the changes in ", ...name(files), " to the next commit on ", branch, "."],
    icon: "stage",
    button: "Stage",
    past: `Staged ${count(files)}`,
    action: { kind: "stage", paths: files.map((f) => f.path) },
  };
}

export function unstageRequest(files: FileChange[]): Request {
  return {
    title: `Unstage ${count(files)}?`,
    body: ["Takes ", ...name(files), " out of the next commit. Your edits stay in the files."],
    icon: "unstage",
    button: "Unstage",
    past: `Unstaged ${count(files)}`,
    action: { kind: "unstage", paths: files.map((f) => f.path) },
  };
}

/** Throw away the edits in files, or delete those git does not track yet. */
export function discardRequest(files: FileChange[]): Request {
  const fresh = files.filter((f) => f.status === "untracked");
  const body: Part[] =
    fresh.length === files.length
      ? ["Deletes ", ...name(files), ". Git does not track ", files.length === 1 ? "it" : "them", " yet."]
      : fresh.length
        ? ["Throws away your edits in ", ...name(files.filter((f) => !fresh.includes(f))), " and deletes ", ...name(fresh), ", which git does not track yet."]
        : ["Throws away your edits in ", ...name(files), " and puts back the staged or committed version."];
  return {
    title: `Discard changes in ${count(files)}?`,
    body,
    icon: "discard",
    button: "Discard",
    danger: true,
    note: "Undo brings it back, from the toast or the Operation Log.",
    done: `Discarded changes in ${count(files)}.`,
    action: { kind: "discard", paths: files.map((f) => f.path) },
  };
}

/** The changed lines of a hunk, without its context: new line numbers, or old ones when it only removes. */
function lines(hunk: Hunk): string {
  const added = hunk.lines.filter((l) => l.kind === "added").map((l) => l.newLine ?? 0);
  const numbers = added.length ? added : hunk.lines.filter((l) => l.kind === "removed").map((l) => l.oldLine ?? 0);
  if (!numbers.length) return "a hunk";
  const first = Math.min(...numbers);
  const last = Math.max(...numbers);
  return first === last ? `line ${first}` : `lines ${first}–${last}`;
}

/** Stage, unstage or discard one hunk of a file. */
export function hunkRequest(kind: LinesKind, path: string, hunk: Hunk): Request {
  const where: Part[] = [`${lines(hunk)} of `, { code: path }];
  const file = path.slice(path.lastIndexOf("/") + 1);
  const request: Record<typeof kind, Omit<Request, "action">> = {
    stageHunk: { title: "Stage this hunk?", body: ["Adds ", ...where, " to the next commit. The rest of the file stays unstaged."], icon: "stage", button: "Stage Hunk", past: `Staged ${lines(hunk)} of ${file}` },
    unstageHunk: { title: "Unstage this hunk?", body: ["Takes ", ...where, " out of the next commit. Your edits stay in the file."], icon: "unstage", button: "Unstage Hunk", past: `Unstaged ${lines(hunk)} of ${file}` },
    discardHunk: {
      title: "Discard this hunk?",
      body: ["Throws away your edits in ", ...where, ". The rest of the file stays as it is."],
      icon: "discard",
      button: "Discard",
      danger: true,
      note: "Undo brings it back, from the toast or the Operation Log.",
      done: `Discarded ${lines(hunk)} of ${file}.`,
    },
  };
  return { ...request[kind], action: { kind, path, header: hunk.header, check: hunk.check ?? "" } };
}

export type LinesKind = "stageHunk" | "unstageHunk" | "discardHunk";

/** Stage, unstage or discard only the picked lines of a hunk. */
export function linesRequest(kind: LinesKind, path: string, hunk: Hunk, lines: number[]): Request {
  const changed = hunk.lines.filter((l) => l.kind !== "context").length;
  const n = lines.length;
  const rest = changed - n;
  const texts = [...lines].sort((a, b) => a - b).map((i) => hunk.lines[i]?.text ?? "");
  const name = splitPath(path).name;
  const others = rest ? ` The other ${plural(rest, "changed line")} of this hunk ${rest === 1 ? "stays" : "stay"}` : "";
  const action: Action = { kind, path, header: hunk.header, check: hunk.check ?? "", lines };
  if (kind === "stageHunk")
    return {
      title: `Stage ${plural(n, "line")} of ${name}?`,
      body: ["Adds only the picked lines to the next commit: ", ...quoted(texts), `.${others ? `${others} unstaged.` : ""}`],
      icon: "lines",
      button: `Stage ${plural(n, "Line")}`,
      done: `Staged ${plural(n, "line")}.`,
      action,
    };
  if (kind === "unstageHunk")
    return {
      title: `Unstage ${plural(n, "line")} of ${name}?`,
      body: ["Takes only the picked lines out of the next commit: ", ...quoted(texts), `. Your file does not change.${others ? `${others} staged.` : ""}`],
      icon: "lines",
      button: `Unstage ${plural(n, "Line")}`,
      done: `Unstaged ${plural(n, "line")}.`,
      action,
    };
  return {
    title: `Discard ${plural(n, "line")} of ${name}?`,
    body: ["Throws away only the picked lines in your file: ", ...quoted(texts), `.${others ? `${others} as ${rest === 1 ? "it is" : "they are"}.` : ""}`],
    icon: "discard",
    button: "Discard",
    danger: true,
    note: "Undo brings it back, from the toast or the Operation Log.",
    done: `Discarded ${plural(n, "line")}.`,
    action,
  };
}

/** The way out when a pre-commit or commit-msg hook, the secrets check or a check before commit
 *  stopped a commit. */
export function commitRecovery(failure: Failure, action: Action & { kind: "commit" }, summary: string, branch: string): Recovery | null {
  if (failure.kind === "secrets") return secretsRecovery(failure.secrets ?? [], action, summary, branch);
  if (failure.kind === "check") return checkRecovery(failure, action, summary, branch, (again, next) => commitRecovery(again, next, summary, branch));
  if (failure.kind !== "hook") return null;
  const hook = failure.hook ?? "pre-commit";
  const both = hook.includes(" or ");
  return {
    title: both ? "A commit hook stopped the commit" : `The ${hook} hook stopped the commit`,
    body: [`The repository’s ${hook} hook found a problem, so nothing was committed. Its output is below: fix what it reports, stage the fixes and try again.`],
    icon: "hook",
    tone: "err",
    button: { label: "Try Again", action, status: `Committing to ${branch}…`, done: "Committed." },
    alt: { label: "Commit Without Hook…", danger: true, request: () => noVerifyRequest(action, summary, branch, hook) },
    note: "Your message is kept.",
    close: "Close",
  };
}

/** The way out when Oxbow found what looks like a password or key in the staged lines. */
function secretsRecovery(found: SecretFinding[], action: Action & { kind: "commit" }, summary: string, branch: string): Recovery {
  const paths = [...new Set(found.map((f) => f.path))];
  const files = paths.length === 1 ? splitPath(paths[0]).name : plural(paths.length, "file");
  // A changelist's lines aren't staged: they stay as they are, and the list is fixed in its files.
  const list = !!action.changelist;
  return {
    title: found.length === 1 ? `Possible ${found[0].kind} in ${files}` : `Possible secrets in ${files}`,
    body: [
      list ? "Oxbow checked the changelist’s lines before committing and stopped: " : "Oxbow checked the staged lines before committing and stopped: ",
      found.length === 1 ? "this one looks" : "these look",
      " like a password or key. Once pushed, a secret has to be revoked: deleting it from history is not enough.",
    ],
    icon: "key",
    tone: "warn",
    secrets: found,
    checked: [{ kind: "hint", text: "# Oxbow searched the lines this commit adds; git commit did not run" }],
    button: list
      ? undefined
      : {
          label: paths.length === 1 ? "Unstage File" : `Unstage ${plural(paths.length, "File")}`,
          action: { kind: "unstage", paths },
          status: "Unstaging…",
          done: `Unstaged ${paths.length === 1 ? files : plural(paths.length, "file")}. Your files do not change.`,
        },
    alt: { label: "Commit Anyway…", danger: true, request: () => allowSecretsRequest(action, summary, branch, found.length) },
    note: list ? "Your message is kept: fix the file, or move the line out of the changelist. A line with oxbow:allow is never flagged." : "Your message is kept. A line with oxbow:allow is never flagged.",
    close: "Close",
  };
}

function allowSecretsRequest(action: Action & { kind: "commit" }, summary: string, branch: string, n: number): Request {
  const next = { ...action, allowSecrets: true };
  return {
    title: `Commit with ${n === 1 ? "a possible secret" : `${n} possible secrets`}?`,
    body: ["Commits ", { quote: summary }, ` to ${branch} as it is. If ${n === 1 ? "it is a real secret" : "they are real secrets"}, revoke ${n === 1 ? "it" : "them"} before pushing.`],
    icon: "key",
    button: action.amend ? "Amend Anyway" : "Commit Anyway",
    danger: true,
    note: "Oxbow checks again on your next commit.",
    status: `Committing to ${branch}…`,
    done: `Committed to ${branch}.`,
    recover: (failure) => commitRecovery(failure, next, summary, branch),
    action: next,
  };
}

function noVerifyRequest(action: Action & { kind: "commit" }, summary: string, branch: string, hook: string): Request {
  const next = { ...action, noVerify: true };
  return {
    title: `Commit without the ${hook.includes(" or ") ? "commit hooks" : `${hook} hook`}?`,
    body: ["Skips the hook this once, so ", { quote: summary }, " is committed without the checks it runs."],
    icon: "hook",
    button: action.amend ? "Amend Anyway" : "Commit Anyway",
    danger: true,
    note: "The hook runs again on your next commit.",
    status: `Committing to ${branch}…`,
    done: `Committed to ${branch} without the hook.`,
    recover: (failure) => commitRecovery(failure, next, summary, branch),
    action: next,
  };
}
