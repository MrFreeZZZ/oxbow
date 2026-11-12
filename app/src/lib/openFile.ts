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
