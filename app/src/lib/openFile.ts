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

export type WorkingFileStatuses = {
  /** What the actions show now. */
  shown: ReadonlyMap<string, WorkingFileStatus>;
  /** What was read from disk since it last changed; the rest is asked for again. */
  checked: ReadonlyMap<string, WorkingFileStatus>;
};

/** The statuses kept for the files now visible. A change on disk asks again for every file but
 *  shows the last answers until the new ones come; another repository starts with none. */
export function keepWorkingFileStatuses(
  last: WorkingFileStatuses,
  files: ReadonlySet<string>,
  change: "none" | "disk" | "repo",
): WorkingFileStatuses {
  if (change === "repo") return { shown: new Map(), checked: new Map() };
  const visible = (statuses: ReadonlyMap<string, WorkingFileStatus>) =>
    new Map([...statuses].filter(([path]) => files.has(path)));
  return { shown: visible(last.shown), checked: change === "disk" ? new Map() : visible(last.checked) };
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
