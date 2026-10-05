// Mirrors the serde types of oxbow-core.

export type RefKind = "local" | "remote" | "tag";

export interface HeadInfo {
  branch: string | null;
  commit: string | null;
}

export interface RefInfo {
  name: string;
  kind: RefKind;
  target: string;
  remote: string | null;
}

export interface Segment {
  from: number;
  to: number;
  color: number;
  dashed: boolean;
}

export interface RowLayout {
  column: number;
  color: number;
  forkColors: number[];
  mergeColors: number[];
  segments: Segment[];
  width: number;
}

export interface Label {
  name: string;
  kind: RefKind;
  color: number;
  head: boolean;
}

export interface HistoryRow {
  id: string;
  summary: string;
  authorName: string;
  authorEmail: string;
  time: number;
  parents: string[];
  labels: Label[];
  unpushed: boolean;
  graph: RowLayout;
}

export interface History {
  head: HeadInfo;
  trunk: string | null;
  trunkTipRow: number | null;
  refs: RefInfo[];
  remotes: string[];
  rows: HistoryRow[];
  truncated: boolean;
}

export interface Person {
  name: string;
  email: string;
  time: number;
  offset: number;
}

export type FileStatus = "added" | "deleted" | "modified" | "renamed" | "copied";

export interface FileChange {
  path: string;
  oldPath: string | null;
  status: FileStatus;
  additions: number;
  deletions: number;
  binary: boolean;
}

export interface CommitDetail {
  id: string;
  summary: string;
  body: string;
  author: Person;
  committer: Person;
  parents: string[];
  files: FileChange[];
}

export type LineKind = "context" | "added" | "removed";

export interface WordPart {
  text: string;
  changed: boolean;
}

export interface DiffLine {
  kind: LineKind;
  oldLine: number | null;
  newLine: number | null;
  text: string;
  words: WordPart[] | null;
}

export interface Hunk {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  lines: DiffLine[];
}

export interface FileDiff {
  file: FileChange;
  hunks: Hunk[];
  tooLarge: boolean;
}

export interface RepoSummary {
  name: string;
  path: string;
}
