// Mirrors the serde types of oxbow-core.

export type RefKind = "local" | "remote" | "tag" | "stash";

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
  branch: string | null;
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
  /** Set on the row of uncommitted changes, whose id is WORKTREE_ID. */
  worktree: WorktreeSummary | null;
}

export const WORKTREE_ID = "worktree";

export interface WorktreeSummary {
  files: number;
  staged: number;
  unstaged: number;
  conflicted: number;
}

export interface StashInfo {
  /** n in stash@{n} */
  index: number;
  id: string;
  message: string;
}

export interface History {
  head: HeadInfo;
  trunk: string | null;
  trunkTipRow: number | null;
  refs: RefInfo[];
  remotes: string[];
  stashes: StashInfo[];
  rows: HistoryRow[];
  truncated: boolean;
}

export interface Person {
  name: string;
  email: string;
  time: number;
  offset: number;
}

export type FileStatus = "added" | "deleted" | "modified" | "renamed" | "copied" | "untracked" | "conflicted";

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
  /** The `@@ -a,b +c,d @@` line; it names the hunk when it is staged on its own. */
  header: string;
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

export interface WorkingTree {
  staged: FileChange[];
  unstaged: FileChange[];
  conflicted: FileChange[];
}

export type Side = "staged" | "unstaged";

export type Action =
  | { kind: "stage"; paths: string[] }
  | { kind: "unstage"; paths: string[] }
  | { kind: "discard"; paths: string[] }
  | { kind: "stageHunk"; path: string; header: string }
  | { kind: "unstageHunk"; path: string; header: string }
  | { kind: "discardHunk"; path: string; header: string }
  | { kind: "commit"; message: string; amend: boolean };

export interface GitCommand {
  args: string[];
  comment: string | null;
  /** The command as typed in a shell. */
  display: string;
}

export interface Plan {
  commands: GitCommand[];
}
