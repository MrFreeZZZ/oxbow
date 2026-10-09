// Changes a line diff can't show well: pictures, binary files, huge diffs, a mode bit, line
// endings, renames. The file lists name them with a small badge, and DiffView swaps the code
// for a card that says what changed instead.

import { prefs } from "./prefs.svelte";
import type { FileChange, ModeChange } from "./types";

/** Files this big get a warning and the way into Git LFS: GitHub warns at 50 MB and refuses 100 MB. */
export const BIG_FILE = 50 * 1024 * 1024;

const IMAGE_TYPES: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  avif: "image/avif",
  bmp: "image/bmp",
  ico: "image/x-icon",
  svg: "image/svg+xml",
};

/** An SVG is text to git, but a picture to the people who change it. */
export const isSvg = (path: string) => path.toLowerCase().endsWith(".svg");

/** The picture type of `path` by its extension, or null when the webview can't draw it. */
export function imageType(path: string): string | null {
  const ext = path.slice(path.lastIndexOf(".") + 1).toLowerCase();
  return Object.hasOwn(IMAGE_TYPES, ext) ? IMAGE_TYPES[ext] : null;
}

/** `18.4 KB`, `6.1 MB`. */
export function bytes(n: number): string {
  const abs = Math.abs(n);
  if (abs < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB"];
  let value = n / 1024;
  let unit = 0;
  while (Math.abs(value) >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value.toFixed(Math.abs(value) < 100 ? 1 : 0)} ${units[unit]}`;
}

/** `+13.5 KB` or `−13.5 KB`. */
export function bytesDelta(n: number): string {
  return `${n < 0 ? "−" : "+"}${bytes(Math.abs(n))}`;
}

export const count = (n: number) => n.toLocaleString("en-US");

/** Whether the change makes the file executable, the other way round, or something else. */
export function modeWord(mode: ModeChange): "executable" | "notExecutable" | "link" | "other" {
  if (mode.new === "100755" && mode.old === "100644") return "executable";
  if (mode.new === "100644" && mode.old === "100755") return "notExecutable";
  if (mode.new === "120000") return "link";
  return "other";
}

/** Changed lines past which a diff waits for Show Diff Anyway; 0: none. */
export const maxLines = () => prefs.get("oxbow.diff.maxLines");

/** The badge and the short note a file list shows for an unusual change, or null for plain text. */
export function fileBadge(file: FileChange): { badge: string; note: string; title: string; warn?: boolean } | null {
  const sizes = file.oldSize !== undefined && file.newSize !== undefined;
  if (file.lfs) {
    const size = file.newSize ?? file.oldSize;
    return { badge: "LFS", note: size !== undefined ? bytes(size) : "", title: "Kept in Git LFS: git stores a small pointer" };
  }
  if ((file.newSize ?? 0) >= BIG_FILE) {
    return { badge: file.binary ? "Binary" : "Large", note: bytes(file.newSize!), title: "Too big for a git repository: GitHub refuses files over 100 MB", warn: true };
  }
  if (file.binary && imageType(file.path)) {
    const note = sizes ? `${bytes(file.oldSize!)} → ${bytes(file.newSize!)}` : bytes(file.newSize ?? file.oldSize ?? 0);
    return { badge: "Image", note, title: "A picture: compare the two versions in its diff" };
  }
  if (file.binary) {
    const note = sizes ? bytesDelta(file.newSize! - file.oldSize!) : bytes(file.newSize ?? file.oldSize ?? 0);
    return { badge: "Binary", note, title: "Binary file: git can't show it as text" };
  }
  const lines = file.additions + file.deletions;
  const max = maxLines();
  if (max > 0 && lines > max) {
    return { badge: "Large", note: `${count(lines)} lines`, title: `More than ${count(max)} changed lines: hidden until you ask for it` };
  }
  if (file.mode && lines === 0) {
    const word = modeWord(file.mode);
    const badge = word === "executable" ? "+x" : word === "notExecutable" ? "−x" : "Mode";
    return { badge, note: "mode only", title: `File mode ${file.mode.old} → ${file.mode.new}` };
  }
  if (file.eol) {
    const badge = `${file.eol.from} → ${file.eol.to}`;
    return { badge, note: `${count(file.eol.lines)} lines`, title: "Only line endings changed" };
  }
  return null;
}
