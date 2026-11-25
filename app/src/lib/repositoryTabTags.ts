export const REPOSITORY_TAB_TAGS = [
  { id: "red", label: "Red", color: "#ff3038" },
  { id: "orange", label: "Orange", color: "#fc871e" },
  { id: "yellow", label: "Yellow", color: "#f5bf00" },
  { id: "green", label: "Green", color: "#24bb45" },
  { id: "blue", label: "Blue", color: "#007bed" },
  { id: "purple", label: "Purple", color: "#ae24c3" },
  { id: "gray", label: "Gray", color: "#929397" },
] as const;

export type RepositoryTabTagId = (typeof REPOSITORY_TAB_TAGS)[number]["id"];

export const TAB_TAG_COLOR_KEY = "oxbow.repositoryTabTags.colors";
export const TAB_TAG_ASSIGNMENT_KEY = "oxbow.repositoryTabTags.assignments";
export type TabTagKey = typeof TAB_TAG_COLOR_KEY | typeof TAB_TAG_ASSIGNMENT_KEY;

const isTagId = (id: unknown) => REPOSITORY_TAB_TAGS.some(tag => tag.id === id);

export function tabTagEntryProblem(key: TabTagKey, entry: string, value: unknown): string | null {
  if (key === TAB_TAG_COLOR_KEY) {
    if (!isTagId(entry)) return "Unknown repository tab tag “" + entry + "”";
    return value === null || (typeof value === "string" && value.length === 7 && /^#[0-9a-f]{6}$/i.test(value))
      ? null : "Expected a hex color like #ff3038";
  }
  if (!/^(?:\/|\\\\|[a-z]:[\\/])/i.test(entry) || entry.includes("\0")) return "Expected an absolute working folder path";
  return value === null || isTagId(value) ? null : "Expected a repository tab tag ID";
}

export function tabTagSettingProblem(key: TabTagKey, value: Record<string, unknown>): string | null {
  for (const [entry, item] of Object.entries(value)) {
    if (item === null) return "Expected a value for “" + entry + "”";
    const problem = tabTagEntryProblem(key, entry, item);
    if (problem) return problem;
  }
  return null;
}
