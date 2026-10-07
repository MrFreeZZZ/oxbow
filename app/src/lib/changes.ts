// The finer actions of the working copy: single lines of a hunk, ignoring a file, stashing a
// few files, and the way out when a commit hook says no.

import type { Part, Recovery, Request } from "./confirm.svelte";
import { splitPath } from "./format";
import type { Action, Failure, Hunk } from "./types";

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
}

/** What the Ignore submenu offers for an untracked file: the file, files like it, its folder. */
export function ignoreChoices(path: string): IgnoreChoice[] {
  const { dir, name } = splitPath(path);
  const choices: IgnoreChoice[] = [{ label: `Ignore “${name}”`, pattern: `/${globEscape(path)}`, wide: false }];
  // .env.local → .env.*, debug.log → *.log
  const dotted = /^(\.[^.]+)\.[^.]+/.exec(name);
  const ext = /^[^.].*(\.[^.]+)$/.exec(name);
  if (dotted) choices.push({ label: `Ignore All ${dotted[1]}.* Files`, pattern: `${globEscape(dotted[1])}.*`, wide: true });
  else if (ext) choices.push({ label: `Ignore All *${ext[1]} Files`, pattern: `*${globEscape(ext[1])}`, wide: true });
  if (dir) choices.push({ label: `Ignore Folder “${dir}”`, pattern: `/${globEscape(dir)}`, wide: true });
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
  body.push(" as a change. .gitignore itself is staged, so the rule reaches your team with the next commit.");
  if (/^\.env/.test(name)) body.push(" Good call: it usually holds secrets.");
  return {
    title: `Ignore ${what}?`,
    body,
    icon: "ignore",
    button: "Ignore",
    status: "Adding the rule to .gitignore…",
    done: `Added ${choice.pattern} to .gitignore.`,
    action: { kind: "ignore", pattern: choice.pattern },
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

/** "“a”, “b” and 3 more" for the picked lines. */
function quoted(texts: string[]): Part[] {
  const shown = texts.slice(0, 3);
  const parts: Part[] = [];
  shown.forEach((text, i) => {
    if (i) parts.push(i === shown.length - 1 && texts.length === shown.length ? " and " : ", ");
    const t = text.trim();
    parts.push({ quote: t.length > 48 ? `${t.slice(0, 47)}…` : t || " " });
  });
  if (texts.length > shown.length) parts.push(` and ${texts.length - shown.length} more`);
  return parts;
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
  const action: Action = { kind, path, header: hunk.header, lines };
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

/** The way out when a pre-commit or commit-msg hook stopped a commit. */
export function commitRecovery(failure: Failure, action: Action & { kind: "commit" }, summary: string, branch: string): Recovery | null {
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

function noVerifyRequest(action: Action & { kind: "commit" }, summary: string, branch: string, hook: string): Request {
  return {
    title: `Commit without the ${hook.includes(" or ") ? "commit hooks" : `${hook} hook`}?`,
    body: ["Skips the hook this once, so ", { quote: summary }, " is committed without the checks it runs."],
    icon: "hook",
    button: action.amend ? "Amend Anyway" : "Commit Anyway",
    danger: true,
    note: "The hook runs again on your next commit.",
    status: `Committing to ${branch}…`,
    done: `Committed to ${branch} without the hook.`,
    action: { ...action, noVerify: true },
  };
}
