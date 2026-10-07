// Which files of a commit start folded in its diff. "Smart" keeps a small commit readable at a
// glance and stops a big one from turning into one long scroll: big diffs, lock and generated
// files, deleted and binary files start folded, the rest open.

import { prefs } from "./prefs.svelte";
import type { FileDiff } from "./types";

/** Written by tools, rarely read in a review. */
const generated = [
  /(^|\/)(package-lock\.json|npm-shrinkwrap\.json|yarn\.lock|pnpm-lock\.yaml|bun\.lockb?|Cargo\.lock|Gemfile\.lock|poetry\.lock|uv\.lock|Pipfile\.lock|composer\.lock|go\.sum|Podfile\.lock|Package\.resolved|flake\.lock|mix\.lock|pubspec\.lock)$/,
  /\.min\.(js|css)$/,
  /\.(js|css)\.map$/,
  /\.snap$/,
];

export function isGenerated(path: string): boolean {
  return generated.some((re) => re.test(path));
}

/** Why the file starts folded in Smart mode, or null when it starts open. */
export function foldReason(diff: FileDiff, over = prefs.get("oxbow.diff.foldOver")): string | null {
  const f = diff.file;
  if (f.binary) return "Binary file";
  if (diff.tooLarge) return "Too large to show";
  if (f.status === "deleted") return "Deleted";
  if (isGenerated(f.path)) return "Generated file";
  if (f.additions + f.deletions > over) return `${f.additions + f.deletions} changed lines`;
  return null;
}

export function startsFolded(diff: FileDiff): boolean {
  const mode = prefs.get("oxbow.diff.files");
  if (mode === "expanded") return false;
  if (mode === "collapsed") return true;
  return foldReason(diff) !== null;
}
