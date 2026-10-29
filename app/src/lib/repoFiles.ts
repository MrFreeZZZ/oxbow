// Settings › Repository Files, its pure part: what each shared file is for, the words of a
// row and the editor's banner, a fix put into the text, and a message's `code` split out.

import type { FileFix, FileProblem, LineNote, RepoFile, RepoFileKind } from "./types";

/** What the file is for, in a few words: the row's sub line. */
export function purpose(kind: RepoFileKind, gitlab: boolean): string {
  switch (kind) {
    case "gitignore":
      return "Files git leaves out";
    case "gitattributes":
      return "Line endings, binary files and Git LFS";
    case "mailmap":
      return "Joins one person’s names and e-mails in History and Blame";
    case "blameIgnoreRevs":
      return "Commits Blame skips, such as reformatting";
    case "lfsconfig":
      return "The Git LFS server everyone uses";
    case "gitmodules":
      return "Submodules. Changed only by the submodule commands";
    case "codeowners":
      return `Who ${gitlab ? "GitLab" : "GitHub"} asks to review each folder`;
    case "editorconfig":
      return "Indentation and line endings for every editor";
  }
}

/** The row's sub line: what it is for and how much it holds, or its first error. */
export function rowSub(file: RepoFile, gitlab: boolean): string {
  if (file.problem) return file.problem.line ? `Line ${file.problem.line}: ${plain(file.problem.message)}` : plain(file.problem.message);
  const what = purpose(file.kind, gitlab);
  if (!file.exists || !file.rules) return what;
  const unit: Partial<Record<RepoFileKind, [string, string]>> = {
    gitignore: ["rule", "rules"],
    gitattributes: ["rule", "rules"],
    codeowners: ["rule", "rules"],
    blameIgnoreRevs: ["commit", "commits"],
    mailmap: ["line", "lines"],
  };
  const words = unit[file.kind];
  return words ? `${what} · ${file.rules} ${file.rules === 1 ? words[0] : words[1]}` : what;
}

/** One sentence above the editor. */
export function banner(kind: RepoFileKind, gitlab: boolean): string {
  switch (kind) {
    case "gitignore":
      return "Files and folders git leaves out. Gray notes say what each line matches in the working copy now.";
    case "gitattributes":
      return "How git treats files: line endings, binary files, Git LFS, diff and merge drivers. Gray notes say how many files each line covers.";
    case "mailmap":
      return "One person under one name and e-mail in History and Blame. Gray notes say how many commits each line covers.";
    case "blameIgnoreRevs":
      return "Commits Blame skips, such as reformatting, so each line shows who really wrote it. One full commit hash per line.";
    case "lfsconfig":
      return "Where Git LFS keeps the large files, for everyone who clones the repository. Settings of your own go in git config instead.";
    case "gitmodules":
      return "The submodules of the repository: their folders and where they come from.";
    case "codeowners":
      return `Who ${gitlab ? "GitLab" : "GitHub"} asks to review each part of the repository. A later line wins over an earlier one.`;
    case "editorconfig":
      return "Indentation and line endings, the same in every editor that reads .editorconfig.";
  }
}

/** The text with a fix put in: line `line` (from 1) replaced, or taken out. */
export function applyFix(text: string, fix: Extract<FileFix, { kind: "replace" }>): string {
  const lines = text.split("\n");
  const at = fix.line - 1;
  if (at < 0 || at >= lines.length) return text;
  const cr = lines[at].endsWith("\r") ? "\r" : "";
  if (fix.text === null) lines.splice(at, 1);
  else lines[at] = fix.text + cr;
  return lines.join("\n");
}

/** A message as words and `code`. */
export function messageParts(message: string): { text: string; code: boolean }[] {
  return message
    .split(/(`[^`]*`)/)
    .filter((part) => part !== "")
    .map((part) => (part.length > 1 && part.startsWith("`") && part.endsWith("`") ? { text: part.slice(1, -1), code: true } : { text: part, code: false }));
}

/** A message without its backticks, for one line of text. */
export const plain = (message: string) => message.replace(/`([^`]*)`/g, "$1");

/** Where line `line` (from 1) starts in `text`, in UTF-16 units. */
export function lineStart(text: string, line: number): number {
  let at = 0;
  for (let i = 1; i < line; i++) {
    const next = text.indexOf("\n", at);
    if (next < 0) return text.length;
    at = next + 1;
  }
  return at;
}

/** What the editor paints of a line: pieces in order, each with the problem it underlines. */
export interface Piece {
  text: string;
  level: "error" | "warn" | null;
  comment: boolean;
}

/** Whether a line is a comment in `kind`'s syntax. */
export function isComment(kind: RepoFileKind, line: string): boolean {
  const t = line.trimStart();
  if (t.startsWith("#")) return true;
  return (kind === "lfsconfig" || kind === "gitmodules" || kind === "editorconfig") && t.startsWith(";");
}

/** A line cut where problems start and end; an error's underline wins over a warning's. */
export function paintLine(kind: RepoFileKind, line: string, problems: FileProblem[]): Piece[] {
  const comment = isComment(kind, line);
  const cuts = new Set([0, line.length]);
  const marks = problems.filter((p) => p.to > p.from).map((p) => ({ from: Math.min(p.from, line.length), to: Math.min(p.to, line.length), level: p.level }));
  for (const m of marks) cuts.add(m.from).add(m.to);
  const points = [...cuts].sort((a, b) => a - b);
  const pieces: Piece[] = [];
  for (let i = 0; i + 1 < points.length; i++) {
    const [from, to] = [points[i], points[i + 1]];
    if (from === to) continue;
    const over = marks.filter((m) => m.from <= from && to <= m.to);
    const level = over.some((m) => m.level === "error") ? "error" : over.length ? "warn" : null;
    pieces.push({ text: line.slice(from, to), level, comment });
  }
  return pieces;
}

/** The note of each line, by line. */
export function notesByLine(notes: LineNote[]): Map<number, LineNote> {
  return new Map(notes.map((n) => [n.line, n]));
}

/** The bar under the editor: what stops saving, or where the file stands. */
export function status(problems: FileProblem[], dirty: boolean, exists: boolean, uncommitted: boolean): { text: string; error: boolean } {
  const errors = problems.filter((p) => p.level === "error").length;
  if (errors) return { text: `Fix ${errors === 1 ? "1 error" : `${errors} errors`} to save.${errors < problems.length ? " Warnings don’t stop saving." : ""}`, error: true };
  if (!exists) return { text: "Not created yet", error: false };
  if (dirty) return { text: "Not saved yet", error: false };
  return { text: uncommitted ? "Saved, not committed yet: commit it in Changes" : "Same as the last commit", error: false };
}
