export type WorkingFileStatus =
  | { kind: "text" | "binary" }
  | { kind: "unavailable"; reason: string };

export function workingFileAction(status: WorkingFileStatus | undefined, historical: boolean) {
  switch (status?.kind) {
    case "text":
    case "binary": {
      const label = status.kind === "text" ? "Open in Editor" : "Open in Default App";
      return { label: historical ? `${label} (Working Copy)` : label, reason: undefined };
    }
    case "unavailable":
      return { label: "Open Working Copy", reason: status.reason };
    default:
      return { label: "Open Working Copy", reason: "Checking the file on disk…" };
  }
}

/** Keep only successful batch results, including per-file unavailable statuses. */
export async function loadWorkingFileStatuses(
  files: Iterable<string>,
  known: ReadonlyMap<string, WorkingFileStatus>,
  read: (paths: string[]) => Promise<WorkingFileStatus[]>,
) {
  const missing = [...files].filter((path) => !known.has(path));
  const results = await read(missing);
  const next = new Map(known);
  missing.forEach((path, i) => next.set(path, results[i]));
  return next;
}
