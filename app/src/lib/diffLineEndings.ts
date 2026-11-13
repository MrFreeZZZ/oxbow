import type { FileDiff } from "./types";

export function diffLineEndingLabel(diff: Pick<FileDiff, "lineEndings"> & { file: Pick<FileDiff["file"], "status"> }) {
  const [oldEnding, newEnding] = diff.lineEndings ?? [];
  const status = diff.file.status;
  if (status === "deleted") return oldEnding === "Empty" ? null : oldEnding;
  if (status === "added" || status === "untracked" || status === "copied") {
    return newEnding === "Empty" ? null : newEnding;
  }
  if (!oldEnding || !newEnding) return null;
  if (oldEnding === newEnding) return oldEnding === "Empty" ? null : oldEnding;
  return `${oldEnding} → ${newEnding}`;
}
