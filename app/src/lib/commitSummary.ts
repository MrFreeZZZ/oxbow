import type { CommitDetail, FileChange, RemoteInfo, RowLayout } from "./types";

const groupNames = ["Added", "Modified", "Deleted"] as const;
type GroupName = typeof groupNames[number];

interface SummaryFile {
  path: string;
  origin: string;
  counts: { additions: number; deletions: number } | null;
}

export interface SummaryContext {
  repository: string;
  path: string;
  branches: string;
  remotes: RemoteInfo[];
  publication: string;
  url: string | null;
}

export interface CommitSummary {
  id: string;
  branch: string;
  title: string;
  description: string;
  author: string;
  date: string;
  groups: { name: GroupName; files: SummaryFile[] }[];
  empty: string;
}

export function commitSummary(
  detail: CommitDetail,
  loaded: ReadonlyMap<string, FileChange>,
  date: string,
  graph: Pick<RowLayout, "branch" | "color">,
  trunk: string | null,
): CommitSummary {
  const groups: Record<GroupName, SummaryFile[]> = { Added: [], Modified: [], Deleted: [] };
  for (const [index, file] of detail.files.entries()) {
    const { path, oldPath, status } = file;
    const group = status === "added" || status === "copied" ? "Added" : status === "deleted" ? "Deleted" : "Modified";
    const origin = status === "renamed" ? "Renamed from" : status === "copied" ? "Copied from" : "";
    const counted = detail.counted && index >= detail.countedFrom ? file : loaded.get(path);
    groups[group].push({
      path,
      origin: origin && oldPath !== null ? `${origin} ${oldPath}` : status === "renamed" ? "Renamed" : "",
      counts: counted && !counted.binary && !counted.submodule
        ? { additions: counted.additions, deletions: counted.deletions }
        : null,
    });
  }
  return {
    id: detail.id,
    branch: graph.branch ?? (graph.color === 0 ? trunk : null) ?? "Unknown",
    title: detail.summary,
    description: detail.body || "No description.",
    author: detail.author.email ? `${detail.author.name} <${detail.author.email}>` : detail.author.name,
    date,
    groups: groupNames.filter((name) => groups[name].length).map((name) => ({ name, files: groups[name] })),
    empty: detail.shallowEnd ? "The full file list is unavailable in this shallow clone." : "No changes.",
  };
}

export function summaryText(summary: CommitSummary, context: SummaryContext): string {
  const repository = [
    `Repository: ${context.repository}`,
    `Path: ${context.path}`,
    `Branches: ${context.branches}`,
    `Commit branch: ${summary.branch}`,
    ...context.remotes.flatMap(({ name, fetchUrl, pushUrl }) => [
      `Remote · ${name}: ${fetchUrl}`,
      ...(pushUrl !== fetchUrl ? [`Push: ${pushUrl}`] : []),
    ]),
    ...(context.remotes.length ? [] : ["No remotes"]),
    context.publication,
    ...(context.url ? [`Commit link: ${context.url}`] : []),
  ].join("\n");
  const changes = summary.groups.map(({ name, files }) => [
    name,
    ...files.map(({ path, origin, counts }) => [
      path,
      origin && `(${origin})`,
      counts && `+${counts.additions} −${counts.deletions}`,
    ].filter(Boolean).join("  ")),
  ].join("\n")).join("\n\n");
  return [repository, summary.title, `Commit: ${summary.id}\nAuthor: ${summary.author}\nDate: ${summary.date}`,
    summary.description, changes || summary.empty].join("\n\n");
}
