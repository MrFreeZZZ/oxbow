// Mirrors the serde types of oxbow-core.

export type RefKind = "local" | "remote" | "tag" | "stash" | "head";

export interface HeadInfo {
  branch: string | null;
  commit: string | null;
}

export interface RefInfo {
  name: string;
  kind: RefKind;
  target: string;
  remote: string | null;
  /** For local branches: the upstream and how far apart the two are. */
  tracking: Tracking | null;
}

export interface Segment {
  from: number;
  to: number;
  color: number;
  dashed: boolean;
  /** The trunk's own line, drawn thicker. */
  thick: boolean;
}

export interface RowLayout {
  column: number;
  color: number;
  forkColors: number[];
  mergeColors: number[];
  segments: Segment[];
  width: number;
  branch: string | null;
  /** Lines to parents Current Branch Only leaves out: they stop half a row down. */
  cuts?: Segment[];
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
  /** Only a detached HEAD has the commit: no branch or tag keeps it. */
  noBranch: boolean;
  graph: RowLayout;
  /** Set on the row of uncommitted changes, whose id is WORKTREE_ID. */
  worktree: WorktreeSummary | null;
}

export const WORKTREE_ID = "worktree";

export interface WorktreeSummary {
  files: number;
  staged: number;
  unstaged: number;
  /** Of the unstaged files, the new ones git does not track yet. */
  untracked: number;
  conflicted: number;
  /** `git status` failed: the counts are 0 because they aren't known. */
  unknown: boolean;
}

/** The parts of the History read that can fail on their own, leaving the commits shown. */
export type ReadPart = "changes" | "tracking" | "operation" | "bisect" | "worktrees" | "submodules";

/** A part of the History read that failed, and git's words for why. */
export interface ReadProblem {
  part: ReadPart;
  error: string;
}

export interface StashInfo {
  /** n in stash@{n} */
  index: number;
  id: string;
  /** As git wrote it, e.g. "On main: Try a smaller pool". */
  message: string;
  /** The branch it was made on; null on a detached HEAD. */
  branch: string | null;
  /** The message without git's "On main: " prefix. */
  title: string;
  /** The commit it was made on. */
  base: string;
  /** The commit holding its untracked files. */
  untracked: string | null;
  time: number;
}

export interface StashCheck {
  /** Files that would conflict; null when git can't tell. */
  conflicts: string[] | null;
  /** Uncommitted files the stash changes too: git refuses to apply over them. */
  inTheWay: string[];
}

export interface Tracking {
  remote: string;
  /** Branch name on the remote. */
  branch: string;
  ahead: number;
  behind: number;
  gone: boolean;
  /** The upstream's tip when its extra commits are this branch's own, since rewritten:
   *  Push replaces them with --force-with-lease at this tip. */
  rewritten: string | null;
  /** With `rewritten`: commits someone pushed on top of the old versions that the branch has
   *  no copy of. Push copies them over first (`base` is where this clone last pushed). */
  replay: { base: string; commits: CommitBrief[] } | null;
}

export interface RemoteFetch {
  remote: string;
  /** Seconds since the epoch; null when nothing shows this clone ever fetched it. */
  time: number | null;
}

export interface History {
  head: HeadInfo;
  trunk: string | null;
  trunkTipRow: number | null;
  /** Gray dashed lines from the top of the graph down to a row, in an otherwise empty column. */
  leadIns: { column: number; row: number }[];
  /** While HEAD is detached, the branch checked out before. */
  previousBranch: string | null;
  /** A merge, rebase, cherry-pick or revert that stopped, waiting to be finished or aborted. */
  operation: Operation | null;
  /** A bisect in progress, or one that found its commit and waits to be finished. */
  bisect: Bisect | null;
  /** Every working folder of the repository, the main one first. */
  worktrees: Worktree[];
  /** In the order of `.gitmodules`. */
  submodules: Submodule[];
  /** The parts of this read that failed: what they show isn't known, which is not the same as
   *  none. See `settle` in reads.ts, which keeps the last good reading of each, marked stale. */
  problems: ReadProblem[];
  refs: RefInfo[];
  /** Tags the default remote doesn’t have yet, as far as Oxbow knows. */
  localTags: string[];
  remotes: string[];
  /** When each remote was last fetched, in the order of `remotes`. */
  fetched: RemoteFetch[];
  tracking: Tracking | null;
  defaultRemote: string | null;
  stashes: StashInfo[];
  rows: HistoryRow[];
  /** Straight lines over two rows or more, ordered by their first row. A row's own `segments`
   *  leave them out: see `segmentsAt` in graph.ts. */
  runs: Run[];
  /** More commits exist beyond `rows`. */
  truncated: boolean;
  /** Commits loaded, stashes aside: the limit that loads the same history again. */
  loaded: number;
  /** A shallow clone: its history stops where the fetched part ends. */
  shallow: boolean;
  /** The uncommitted changes the row of uncommitted changes counts; null when git could not list them. */
  changes: WorkingTree | null;
  /** Stands for the changed files on disk, by size and modification time. */
  changesStamp: string;
  /** Which reading of `changes` this is: the Changes panel asks for diffs of the same one. */
  snapshot: number;
  /** Stands for what the commit rows were built from: the same key, the same rows. */
  key: string;
  /** The rows of `key` the caller had were left out: `rows` holds only the row of uncommitted
   *  changes, if any, and `runs` and `leadIns` are empty. See `withRows`. */
  same: boolean;
  /** When not empty, the rows put together from the caller's (those of the key it asked with)
   *  and `rows`, which then holds only the others: runs of one or the other, in order. */
  pieces: Piece[];
  /** The git commands this read ran, as typed in a shell; commits and refs are read from .git
   *  directly. What Reload shows in Activity. */
  reads: string[];
  /** Read on a quiet refresh with nothing changed since the read before: no git command ran. */
  unchanged: boolean;
  /** Some loaded commits not marked unpushed may be: more were unpushed than the history walks
   *  while it loads. `settleUnpushed` tells, and the next load marks them. */
  unpushedPending?: boolean;
  /** Asked for with `focus`: the checked-out branch, its upstream and the trunk, laid out alone. */
  focus: Focus | null;
}

/** History as Current Branch Only shows it. */
export interface Focus {
  /** The rows shown, as indexes into `History.rows`. */
  rows: number[];
  /** The graph of each row shown, its straight lines in `runs`. */
  graph: RowLayout[];
  runs: Run[];
  leadIns: { column: number; row: number }[];
}

/** `len` of the caller's rows from `known` on, or with `known` null the next `len` of `rows`. */
export interface Piece {
  known: number | null;
  len: number;
}

/** A straight line down `column`, from row `first` to the row after `last`. */
export interface Run {
  column: number;
  color: number;
  dashed: boolean;
  thick: boolean;
  first: number;
  last: number;
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
  /** Sizes in bytes before and after, where known. */
  oldSize?: number;
  newSize?: number;
  /** The file mode changed, for example it became executable. */
  mode?: ModeChange;
  /** For renames and copies: how much of the old file is left, in percent. */
  similarity?: number;
  /** Only the line endings changed. */
  eol?: EolChange;
  /** Kept in Git LFS; the pointers before and after where they are known. */
  lfs?: LfsChange;
  /** A submodule, not a file: another commit is recorded or checked out (`moved`), or files in its folder changed. */
  submodule?: { moved: boolean; changed: boolean };
  /** A new file whose lines were not counted this time: many big new files are counted over a few reloads. */
  uncounted?: boolean;
}

/** A pointer file: the LFS object that stands in for a file. */
export interface LfsPointer {
  /** `sha256:4d7a…` */
  oid: string;
  size: number;
}

export interface LfsChange {
  old: LfsPointer | null;
  new: LfsPointer | null;
}

/** One `filter=lfs` rule of .gitattributes and the files it covers. */
export interface LfsPattern {
  pattern: string;
  files: number;
  /** Bytes: the real files', or what their pointers name. */
  size: number;
  /** Files that are still pointers here: not downloaded. */
  missing: number;
  /** Not in the last commit's .gitattributes yet. */
  new: boolean;
}

export interface LfsStatus {
  /** null when git-lfs is not installed. */
  version: string | null;
  /** `git lfs install` has turned on the filters that swap pointers and files. */
  filters: boolean;
  patterns: LfsPattern[];
  /** Homebrew, to install git-lfs with. */
  brew: string | null;
  /** The files that are still pointers here, by path; at most 5000. */
  missingFiles: LfsFile[];
}

/** A file kept in LFS that is only a pointer here: not downloaded. */
export interface LfsFile {
  path: string;
  /** The real file's size, as the pointer says. */
  size: number;
  oid: string;
}

/** Old and new mode in git's octal form: 100644, 100755, 120000. */
export interface ModeChange {
  old: string;
  new: string;
}

export interface EolChange {
  from: "CRLF" | "LF" | "mixed";
  to: "CRLF" | "LF" | "mixed";
  lines: number;
}

/** Where one side of a file diff can be read. */
export type Source = { kind: "blob"; id: string } | { kind: "worktree"; path: string };

/** Lines of one side of a file diff, for the unchanged lines between hunks. */
export interface SourceLines {
  lines: SourceLine[];
  /** How many lines the side has in all. */
  total: number;
}

export interface SourceLine {
  text: string;
  /** The line ends with CRLF; `text` leaves the CR out. */
  cr?: boolean;
}

export interface FileCommit {
  id: string;
  parents: string[];
  summary: string;
  authorName: string;
  authorEmail: string;
  time: number;
  /** The file's path after this commit. */
  path: string;
  /** Its path before, when this commit renamed or copied it. */
  oldPath: string | null;
  status: FileStatus;
  additions: number;
  deletions: number;
}

export interface FileHistory {
  path: string;
  commits: FileCommit[];
  /** Commits on other branches that changed the file. */
  elsewhere: FileCommit[];
  more: boolean;
}

export interface BlameCommit {
  id: string;
  summary: string;
  authorName: string;
  authorEmail: string;
  time: number;
  path: string;
  boundary: boolean;
}

export interface Blame {
  rev: string;
  path: string;
  commits: BlameCommit[];
  lines: { commit: number; text: string }[];
  command: string;
  /** The file is too long to blame whole: `lines` are its first ones. */
  cut: boolean;
}

export type CompareMode = "split" | "tips";

export interface CompareCommit {
  id: string;
  summary: string;
  authorName: string;
  time: number;
}

export interface CompareFile extends FileChange {
  /** The newest commit of the compared side that touched the file. */
  last: string | null;
  /** Tip to Tip only: the difference comes from the base side's newer commits alone. */
  onlyBase: boolean;
}

export interface Comparison {
  baseId: string;
  targetId: string;
  mergeBase: CompareCommit | null;
  ahead: CompareCommit[];
  behind: CompareCommit[];
  aheadCount: number;
  behindCount: number;
  mode: CompareMode;
  /** The commit the diff starts from: the split point, or the base's tip. */
  from: string;
  files: CompareFile[];
  command: string;
}

export interface CommitDetail {
  id: string;
  summary: string;
  body: string;
  author: Person;
  committer: Person;
  parents: string[];
  files: FileChange[];
  /** A shallow clone stops here: its parent isn't fetched yet, so `files` is empty. */
  shallowEnd: boolean;
  /** `files` have their line counts and sizes; without, only paths and how each changed. */
  counted: boolean;
  /** Files before this one are not counted though `counted` is set: their diffs have the counts. */
  countedFrom: number;
}

/** Diffs of a commit's files, a few at a time. */
export interface DiffPage {
  diffs: FileDiff[];
  /** Where the next page starts; null after the last file. */
  next: number | null;
  /** Changed files in all. */
  files: number;
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
  /** The line ends with CRLF; `text` leaves the CR out. */
  cr?: boolean;
  words: WordPart[] | null;
  /** An added line that looks like it holds a secret: what it looks like, e.g. "GitHub token". */
  secret?: string | null;
}

export interface Hunk {
  /** The `@@ -a,b +c,d @@` line; it names the hunk when it is staged on its own. */
  header: string;
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  lines: DiffLine[];
  /** Uncommitted changes only: fingerprint of the hunk's bytes, sent back with hunk actions. */
  check?: string;
}

export interface FileDiff {
  file: FileChange;
  hunks: Hunk[];
  tooLarge: boolean;
  /** More changed lines than Settings › Diff & Text allows: left out until Show Diff Anyway. */
  limited: boolean;
  old: Source | null;
  new: Source | null;
  /** For a submodule: how its recorded commit moved, in place of lines. */
  submodule?: SubmoduleMove;
}

/** How a submodule's commit moved. */
export type Moved = "forward" | "back" | "sideways" | "added" | "removed" | "same" | "unknown";

/** A submodule's change in a commit or in the working copy. */
export interface SubmoduleMove {
  path: string;
  title: string;
  url: string | null;
  /** The branch Pull Latest takes, null for the remote's default. */
  follows: string | null;
  old: string | null;
  new: string | null;
  /** `old` and `new` themselves, when the clone has them. */
  oldCommit: CommitBrief | null;
  newCommit: CommitBrief | null;
  moved: Moved;
  /** The commits between the two, newest first: gained, or dropped when it moved back. At most 50. */
  commits: CommitBrief[];
  count: number;
  /** Sideways: commits of `old` that `new` doesn't have. */
  dropped: number;
  /** A clone exists to read the commits from. */
  cloned: boolean;
  /** Working copy: files changed in its folder; null when git couldn't tell. */
  changed: number | null;
  /** `new` is on none of its remote branches yet. */
  unpushed: boolean;
}

export interface RepoSummary {
  name: string;
  path: string;
  /** Open Restricted: the repository's settings that git runs with off. */
  restricted: TrustFinding[];
  /** Nothing was opened: the repository's settings run programs, and the user hasn't said
   *  whether to trust it. */
  untrusted: TrustFinding[] | null;
}

/** What a setting of a repository makes git run, and so what Restricted turns off. */
export type TrustKind = "fsmonitor" | "hook" | "ssh" | "credential" | "filter" | "textconv" | "mergeDriver" | "mergeTool" | "signing" | "remote" | "command";

export interface TrustFinding {
  kind: TrustKind;
  /** As git names it (`core.sshCommand`), or `hooks/post-checkout` for a hook. */
  key: string;
  /** The command or path it gives. */
  value: string;
  /** The file it is in. */
  origin: string;
}

export interface TrustedRepo {
  path: string;
  /** Its settings that run programs, when last opened. */
  settings: string[];
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
  /** `lines`: only these changed lines of the hunk, as indexes into its lines. */
  | { kind: "stageHunk"; path: string; header: string; check: string; lines?: number[] | null }
  | { kind: "unstageHunk"; path: string; header: string; check: string; lines?: number[] | null }
  | { kind: "discardHunk"; path: string; header: string; check: string; lines?: number[] | null }
  | { kind: "ignore"; pattern: string; untrack?: string }
  | { kind: "commit"; message: string; amend: boolean; noVerify?: boolean; allowSecrets?: boolean }
  | { kind: "fetch"; remote: string | null }
  | { kind: "pull"; remote: string; branch: string; fetchFirst?: boolean; submodules?: boolean; mode?: "rebase" | "merge" | "fastForward" }
  | {
      kind: "push";
      remote: string;
      branch: string;
      upstream: string;
      setUpstream: boolean;
      force: boolean;
      lease: string | null;
      noVerify: boolean;
      /** Push the submodule commits it records that their remotes don't have, first. */
      submodules?: boolean;
      /** Ask GitLab to open a merge request of the branch with the push. */
      mergeRequest?: MergeRequest | null;
    }
  | { kind: "pullAndPush"; remote: string; branch: string; upstream: string; noVerify: boolean }
  | { kind: "replayAndPush"; remote: string; branch: string; upstream: string; base: string; lease: string; noVerify: boolean }
  | { kind: "abortRebase" }
  | { kind: "switch"; branch: string; stash: boolean; keep: string | null }
  | { kind: "detach"; commit: string; stash: boolean }
  | { kind: "track"; remote: string; branch: string; stash: boolean }
  | { kind: "createBranch"; name: string; start: string | null; switch: boolean; publish: string | null }
  | { kind: "renameBranch"; from: string; to: string; upstream: RemoteBranch | null }
  | { kind: "deleteBranch"; name: string; force: boolean; upstream: RemoteBranch | null }
  /** `expect`: only while the branch there is still at this commit (--force-with-lease). */
  | { kind: "deleteRemoteBranch"; remote: string; branch: string; expect?: string | null }
  | { kind: "merge"; branch: string; method: MergeMethod; message: string | null }
  | { kind: "continue"; message: string | null }
  | { kind: "abort" }
  | { kind: "skip" }
  | { kind: "resolve"; path: string; picks: Resolution[]; text?: string; stages?: string }
  | { kind: "takeFile"; path: string; side: ConflictSide }
  | { kind: "cherryPick"; commit: string }
  /** Commits of `to` that `since` doesn't have, or the newest `count` up to `to`, as patch
   *  files: a file each in the folder `into`, or with `oneFile` all in the file `into`. */
  | { kind: "savePatches"; to: string; since: string | null; count: number; into: string; oneFile: boolean }
  /** Mailed patches become commits (git am); a plain diff goes into the files (git apply). */
  | { kind: "applyPatches"; paths: string[] }
  | { kind: "revert"; commit: string }
  | { kind: "reset"; commit: string; mode: ResetMode }
  | { kind: "reword"; message: string; noVerify?: boolean }
  /** `paths`: only these files; every change when empty or missing. */
  | { kind: "stashPush"; message: string | null; untracked: boolean; paths?: string[] }
  | { kind: "stashApply"; index: number; id: string; pop: boolean; keepIndex: boolean }
  | { kind: "stashDrop"; index: number; id: string }
  | { kind: "stashStore"; id: string; message: string }
  | { kind: "stashBranch"; index: number; id: string; name: string }
  | { kind: "addRemote"; name: string; url: string }
  | { kind: "setRemoteUrl"; name: string; url: string }
  | { kind: "removeRemote"; name: string }
  | { kind: "addSshKey"; key: string }
  | { kind: "optimize" }
  | { kind: "createTag"; name: string; commit: string; message: string | null; push: string | null }
  | { kind: "pushTags"; remote: string; names: string[] }
  | { kind: "deleteTag"; name: string; remote: string | null }
  | { kind: "fetchTags"; remote: string }
  | { kind: "unshallow"; remote: string }
  /** Back to before step `id` of the Operation Log, or right after it with `after`. */
  | { kind: "restore"; id: string; after: boolean }
  | { kind: "clearOperationLog" }
  | { kind: "editStack"; plan: StackPlan }
  | { kind: "addToCommit"; commit: string; allowSecrets?: boolean }
  | { kind: "pushBranches"; remote: string; branches: { branch: string; upstream: string }[] }
  | { kind: "pullRequestMerged"; remote: string; base: string; branch: string }
  /** Only while `branch` is still at `sha`, the commit GitHub merged. */
  | { kind: "deleteMergedBranch"; base: string; branch: string; sha: string }
  /** `stage`: stage .gitattributes and `paths` too. */
  | { kind: "lfsTrack"; pattern: string; paths: string[]; stage: boolean }
  | { kind: "lfsUntrack"; pattern: string }
  | { kind: "lfsPull"; include: string | null; paths?: string[] }
  | { kind: "lfsPrune" }
  | { kind: "lfsInstall"; brew: string | null; pull: boolean }
  /** `command`: git runs it at every step and answers by its exit code. */
  | { kind: "bisectStart"; good: string; bad: string; stash: boolean; command: string | null }
  /** `commit`: the commit under test when null. */
  | { kind: "bisectMark"; mark: BisectMark; commit: string | null }
  | { kind: "bisectReset" }
  | { kind: "worktreeAdd"; path: string; branch: string; create: boolean; start: string | null }
  | { kind: "worktreeRemove"; path: string; force: boolean; stash: boolean }
  | { kind: "worktreeLock"; path: string; reason: string | null }
  | { kind: "worktreeUnlock"; path: string }
  | { kind: "worktreePrune" }
  | { kind: "worktreeRepair"; path: string }
  /** Empty `paths`: every submodule. `latest`: the newest commit of the branch each follows. */
  | { kind: "submoduleUpdate"; paths: string[]; latest: boolean }
  | { kind: "submoduleSync"; paths: string[] }
  | { kind: "submoduleAdd"; url: string; path: string; branch: string | null }
  | { kind: "submoduleRemove"; path: string }
  /** A way out of a typical mistake, planned when the repository looked like `expect`. */
  | { kind: "fix"; fix: Fix; expect: FixExpect };

/** One way out of a typical mistake (Fix a Mistake), with what was picked in its sheet. */
export type Fix =
  /** The last `count` commits go to `to`, a new branch with `create`; pushed ones are reverted and copied. */
  | { kind: "moveCommits"; count: number; to: string; create: boolean }
  | { kind: "addToLast"; paths: string[]; allowSecrets?: boolean }
  | { kind: "undoLast"; mode: ResetMode }
  | { kind: "revertPushed"; commit: string; push: boolean }
  /** `commit`: a commit only the reflog keeps; without it, where HEAD is. */
  | { kind: "saveDetached"; name: string; commit: string | null; switch: boolean };

/** How the repository looked when a fix was planned; it runs only while it still does. */
export interface FixExpect {
  head: string | null;
  branch: string | null;
  upstream: string | null;
  remotes: string;
  files: string;
}

export interface FixFile {
  path: string;
  status: "modified" | "new" | "deleted" | "renamed";
  /** Some changes are staged: they go into the commit anyway. */
  staged: boolean;
  /** Some changes are not staged: picking it stages them. */
  unstaged: boolean;
}

export interface FixState {
  expect: FixExpect;
  changed: FixFile[];
  /** Commits made without a branch that only the reflog keeps, newest first. */
  lost: ReflogEntry[];
}

/** A step of a fix's plan in words; `commands` of the plan's commands are its own. */
export type FixStep = { commands: number } & (
  | { kind: "stash"; files: number }
  | { kind: "createBranch"; name: string; at: string | null; switch: boolean }
  | { kind: "moveBranch"; branch: string; to: string }
  | { kind: "switch"; branch: string }
  | { kind: "copy"; count: number }
  | { kind: "revert"; count: number; branch: string | null }
  | { kind: "stage"; paths: string[] }
  | { kind: "amend" }
  | { kind: "reset"; mode: ResetMode }
  | { kind: "push"; remote: string; branch: string }
  | { kind: "pop" }
);

/** One working folder of the repository (`git worktree`). */
export interface Worktree {
  path: string;
  /** The folder's name. */
  name: string;
  /** null while HEAD is detached there. */
  branch: string | null;
  head: string | null;
  /** The folder that holds the repository; it can't be removed. */
  main: boolean;
  /** The folder open in this window. */
  current: boolean;
  /** The reason it was locked, "" without one; null when it isn't. */
  locked: string | null;
  /** Its folder is gone. */
  missing: boolean;
  /** Files with uncommitted changes there; 0 for this window's and a missing one. null when
   *  `git status` failed there, which is not the same as none: `statusError` says why. */
  changed: number | null;
  statusError: string | null;
  /** `changed` isn't read yet: it comes after the history, as a `FolderState`. */
  pending: boolean;
}

/** A submodule: another repository checked out in a folder here, at the commit this one records. */
export interface Submodule {
  name: string;
  path: string;
  /** The folder's name. */
  title: string;
  url: string | null;
  /** The branch Pull Latest takes (`.` for the one checked out here); null for the remote's default. */
  follows: string | null;
  /** The commit the index records, which the next commit records too. */
  recorded: string | null;
  /** The commit HEAD records. */
  committed: string | null;
  /** Checked out in its folder; null when not cloned. */
  current: string | null;
  /** null while detached, as `git submodule update` leaves it. */
  branch: string | null;
  cloned: boolean;
  /** Files with uncommitted changes in its folder; 0 when not cloned. null when `git status`
   *  failed there: `statusError` says why. */
  changed: number | null;
  statusError: string | null;
  /** Commits checked out that `recorded` doesn't have, and the other way round. */
  ahead: number;
  behind: number;
  /** The recorded commit isn't in its clone yet. */
  missing: boolean;
  /** Commits checked out with no branch there that no branch, remote branch or tag of its clone
   *  has: checking out another commit leaves them only in its reflog. */
  onNoBranch: number;
  /** What its folder has (`current`, `branch`, `changed`, `ahead`, `behind`, `missing`, `onNoBranch`) isn't
   *  read yet: it comes after the history, as a `FolderState`. */
  pending: boolean;
}

/** What a submodule's folder has checked out. */
export interface Checkout {
  current: string | null;
  branch: string | null;
  ahead: number;
  behind: number;
  missing: boolean;
  onNoBranch: number;
}

/** What one worktree's or submodule's folder has, read after the history. */
export interface FolderState {
  /** The worktree's path, or the submodule's path in the repository. */
  path: string;
  submodule: boolean;
  changed: number | null;
  statusError: string | null;
  /** For a submodule. */
  checkout: Checkout | null;
}

/** A submodule whose recorded commit is on none of its remotes. */
export interface UnpushedSubmodule {
  path: string;
  title: string;
  commit: string;
}

export type BisectMark = "good" | "bad" | "skip";

/** One answer of a bisect's journal. */
export interface BisectStep {
  commit: string;
  /** The subject, as git noted it with the answer. */
  summary: string;
  mark: BisectMark;
  /** Given before both a good and a bad commit were known: it set the range, it wasn't a step. */
  range: boolean;
}

/** git bisect, from git's own state, so one started in a terminal shows too. */
export interface Bisect {
  /** Where finishing goes back to: a branch, or a sha when HEAD was detached. */
  start: string;
  startIsBranch: boolean;
  /** The first good and bad answers, which set the range, and their names (a branch, tag or short sha). */
  firstGood: string | null;
  firstGoodLabel: string | null;
  firstBad: string | null;
  firstBadLabel: string | null;
  good: string[];
  bad: string[];
  skipped: string[];
  /** Commits the first bad one may still be; empty when unknown. */
  candidates: string[];
  /** Answers given after the range was set. */
  steps: number;
  /** git's numbers for the commit under test: revisions left after it, roughly how many steps. */
  left: number;
  stepsLeft: number;
  testing: string | null;
  found: string | null;
  /** Only skipped commits are left: it is one of these. */
  possible: string[];
  /** The last answer and what git said, as in a terminal. */
  output: string[];
  /** git bisect log. */
  log: string;
  /** Every answer so far, in the order given, the two that set the range first. */
  journal: BisectStep[];
  /** What it found, as text to paste into an issue: the commit, the range, the steps. Set once
   *  the first bad commit is found, or only skipped ones are left. */
  report: string | null;
  /** Oxbow stashed changes before starting; Finish brings them back. */
  stashed: boolean;
  termBad: string;
  termGood: string;
}

export type ResetMode = "soft" | "mixed" | "hard";

export type MergeMethod = "merge" | "squash" | "rebase" | "fastForward";
export type OperationKind = "merge" | "squash" | "rebase" | "cherryPick" | "revert" | "stashApply" | "am";
/** A side of a conflict as git names the index stages: ours is stage 2, theirs stage 3. */
export type ConflictSide = "ours" | "theirs";
export type Pick = "ours" | "theirs" | "oursThenTheirs" | "theirsThenOurs";
/** What replaces one conflict: a side, or lines written by hand (none: the conflict goes). */
export type Resolution = Pick | { lines: string[] };

export interface Operation {
  kind: OperationKind;
  /** The branch that changes: the checked-out one, or the one being rebased. */
  branch: string | null;
  /** The merged branch, the new base of a rebase, a short sha otherwise. */
  incoming: string | null;
  /** The commit being merged, replayed, picked or reverted. */
  commit: CommitBrief | null;
  incomingCount: number;
  /** For a rebase or git am: [commit or patch it stopped on, of how many], from 1. */
  step: [number, number] | null;
  conflicted: number;
  oursLabel: string;
  theirsLabel: string;
  /** The side with the user's own work: ours in a merge, theirs in a rebase. */
  yours: ConflictSide;
  message: string | null;
}

/** What applying patch files would do, before anything runs. */
export interface PatchPreview {
  /** The commits the patches make, in order; empty for a plain diff. */
  commits: CommitBrief[];
  /** More commits than are listed. */
  more: number;
  /** A diff without commits: its changes go into the files, uncommitted. */
  plain: boolean;
  files: PatchFile[];
  /** Files whose changes don't fit as they are. */
  notClean: string[];
  /** They don't fit and git said of no file why. */
  fails: boolean;
  /** Uncommitted changes git am won't work over. */
  inTheWay: string[];
}

/** A file a patch changes; null counts for a binary file. */
export interface PatchFile {
  path: string;
  added: number | null;
  removed: number | null;
}

export interface MergePreview {
  branch: string;
  incoming: CommitBrief[];
  incomingCount: number;
  ours: CommitBrief[];
  oursCount: number;
  base: CommitBrief | null;
  /** null when this git can't tell. */
  conflicts: string[] | null;
  files: number;
  touchedHere: number;
}

export type Chunk = { kind: "same"; lines: string[] } | { kind: "conflict"; ours: string[]; base: string[]; theirs: string[] };

export interface ConflictFile {
  path: string;
  base: boolean;
  ours: boolean;
  theirs: boolean;
  binary: boolean;
  /** A symbolic link or submodule: taken whole from one side. */
  link: boolean;
  /** Every version is UTF-8, so the result can be written by hand. */
  text: boolean;
  /** One side renamed the file to this path. */
  renamed: { from: string; side: ConflictSide } | null;
  chunks: Chunk[];
  /** No line break at the end of the file. */
  openEnd: boolean;
  /** Which versions the index holds, by blob id: decisions made for others are stale. */
  stages: string;
}

/** The merge tool git opens, from merge.guitool or merge.tool. */
export interface MergeTool {
  name: string;
  /** FileMerge for opendiff. */
  label: string;
  /** Runs in a terminal (vimdiff), which Oxbow can't give it. */
  terminal: boolean;
  gui: boolean;
}

export interface RemoteBranch {
  remote: string;
  branch: string;
}

/** What deleting a local branch would lose. */
export interface DeletionCheck {
  /** Commits on no other branch, remote branch or tag, the upstream included. */
  lost: CommitBrief[];
  /** The same when the upstream is deleted too. */
  lostWithUpstream: CommitBrief[];
}

export type ActionEvent =
  | { kind: "command"; display: string }
  | { kind: "line"; text: string; stderr: boolean; progress: boolean }
  /** The action ran, but its step couldn't be written to the Operation Log: Undo can't take it back. */
  | { kind: "notLogged"; reason: string }
  /** Another window of the repository is changing it: this starts once `title` there is done. */
  | { kind: "waiting"; title: string; window: string }
  /** The action goes over these one at a time, e.g. Update All over the submodules: one that fails doesn't stop the others. */
  | { kind: "parts"; names: string[] }
  /** One of the parts started or ended; a failed one has git's `error`, and `inner` names the
   *  nested submodule that failed when it wasn't this one. */
  | { kind: "part"; name: string; state: "started" | "done" | "failed"; error: string | null; inner: string | null }
  /** How much of one of the parts is done, in bytes, e.g. a file a download brings. */
  | { kind: "partProgress"; name: string; done: number; total: number };

/** What changed in the repository outside the window: in a terminal, an editor, another window. */
export interface OutsideChange {
  /** Branches, tags, HEAD, the stashes, what is staged, an operation under way, settings. */
  git: boolean;
  /** Files in the working copy, or what is ignored. */
  files: boolean;
  /** When the last of it happened, in milliseconds since the epoch. */
  at: number;
  /** The folder name of the repository's other window that changed it; null for a terminal or an editor. */
  window: string | null;
}

export type FailureKind =
  | "secrets"
  | "rejected"
  | "staleLease"
  | "auth"
  | "network"
  | "hook"
  | "conflict"
  | "localChanges"
  | "notMerged"
  | "cancelled"
  | "changed"
  | "stashKept"
  /** The state before couldn't be saved for Undo, so nothing ran. */
  | "noUndo"
  /** Some parts of an action that goes over several, e.g. Update All, failed; the rest ran. */
  | "partlyDone"
  | "other";

export interface CommitBrief {
  id: string;
  summary: string;
  authorName: string;
  time: number;
}

export interface Failure {
  kind: FailureKind;
  output: string;
  incoming: CommitBrief[];
  remoteTip: string | null;
  /** The hook that stopped it, e.g. "pre-commit", or "pre-commit or commit-msg". */
  hook: string | null;
  /** For a commit Oxbow stopped: what looks like a password or key in the staged lines. */
  secrets?: SecretFinding[];
  /** For a refused SSH connection: what Oxbow found out. */
  ssh?: SshCheck | null;
  /** For a rejected push of commits rewritten here: tracking after the fetch, with the lease
   *  and what others pushed on top of the old versions. */
  tracking?: Tracking | null;
}

/** An added line of a commit that looks like it holds a secret. */
export interface SecretFinding {
  path: string;
  /** 1-based, in the new file. */
  line: number;
  /** What it looks like, e.g. "GitHub token". */
  kind: string;
  /** The line with the secret masked, e.g. `TOKEN = "ghp_R8mZ••••••••Gt"`. */
  masked: string;
}

export type SshProblem = "agentEmpty" | "keyNotOnHost" | "noKey" | "unknownHost" | "works" | "other";

export interface SshCheck {
  problem: SshProblem;
  remote: string;
  url: string;
  /** e.g. `git@github.com`. */
  login: string;
  host: string;
  /** As `~/.ssh/id_ed25519`. */
  key: string | null;
  keyPath: string | null;
  /** What Oxbow ran to find out. */
  steps: { command: string; output: string; bad: boolean }[];
  httpsUrl: string | null;
}

export interface GitCommand {
  args: string[];
  comment: string | null;
  /** The command as typed in a shell. */
  display: string;
  /** What Oxbow feeds it on stdin, e.g. the patch of a few lines. */
  input: string | null;
  /** A step Oxbow does itself first, as typed in a shell. */
  before: string | null;
  /** The todo list Oxbow hands an interactive rebase. */
  todo: string | null;
}

export interface Plan {
  commands: GitCommand[];
  /** What the Operation Log can't bring back once the action ran. */
  notKept: NotKept[];
  /** For a fix: its steps, in order. */
  steps?: FixStep[];
}

/** A file and its size in bytes; a folder (ending in /) counts everything in it. */
export interface SizedFile {
  path: string;
  size: number;
}

/** Something an action changes that Undo can't bring back. */
export type NotKept =
  | { kind: "bigFiles"; files: SizedFile[]; more: number; limit: number }
  | { kind: "ignoredInTheWay"; files: SizedFile[]; more: number; bytes: number }
  | { kind: "unchecked"; folders: string[]; more: number; limit: number }
  | { kind: "ignored"; folder: string; files: SizedFile[]; more: number; bytes: number }
  | { kind: "worktree"; folder: string; branch: string | null; changed: number | null; removed: boolean }
  | { kind: "notLogged"; what: string }
  | { kind: "submoduleFolder"; folder: string }
  | { kind: "pushed"; branch: string; remote: string; commits: { id: string; summary: string }[]; more: number; revert: string | null };

/** Which config file a Git setting lives in: ~/.gitconfig or the repository's .git/config. */
export type ConfigScope = "global" | "local";

export interface OpenApp {
  id: string;
  name: string;
}

export interface SshKey {
  name: string;
  path: string;
  kind: string;
  fingerprint: string | null;
  comment: string;
  public: string;
}

/** The GitHub account signed in, from Settings › Accounts. */
export interface GitHubAccount {
  login: string;
  name: string | null;
  avatarUrl: string;
  htmlUrl: string;
  /** How it signed in: the browser (device flow) or a pasted token. */
  method: "browser" | "token";
  /** The token's scopes; null for a fine-grained token. */
  scopes: string[] | null;
}

/** A GitLab account: one per server, gitlab.com or your own. */
export interface GitLabAccount {
  /** `gitlab.com` or `git.example.com:8443`. */
  host: string;
  /** `https://gitlab.com`. */
  web: string;
  username: string;
  name: string | null;
  /** The account's picture as a `data:` address. */
  avatar: string | null;
  webUrl: string;
  /** The token's scopes; null when GitLab doesn't say. */
  scopes: string[] | null;
  /** The last day the token works, `2026-12-01`; null when it never expires. */
  expiresAt: string | null;
  /** The server's GitLab version, `17.5.1-ee`. */
  version: string;
  /** The credential store item with the token. */
  item: string;
}

/** What the GitLab sign-in sheet shows for the server typed so far. */
export interface GitLabSignInSetup {
  server: { host: string; web: string };
  /** The requests signing in makes, as curl. */
  requests: string[];
  newTokenUrl: string;
  scopes: string[];
  store: string;
  /** The oldest GitLab Oxbow signs in to, `16.0`. */
  oldest: string;
}

/** Where Publish can put a repository: the account and its organizations. */
export interface GitHubOwners {
  login: string;
  orgs: string[];
}

/** A repository to make on GitHub and push to. */
export interface Publish {
  owner: string;
  /** `owner` is the signed-in account, not an organization. */
  personal: boolean;
  name: string;
  private: boolean;
  description: string;
  branch: string;
}

/** What the sign-in sheet shows before it starts. */
export interface SignInSetup {
  /** Null: this build can't sign in with the browser, only with a token. */
  clientId: string | null;
  deviceRequests: string[];
  tokenRequest: string;
  newTokenUrl: string;
  scopes: string[];
  /** Keychain, Windows Credential Manager or keyring. */
  store: string;
}

/** The one-time code to approve on github.com/login/device. */
export interface DeviceCode {
  deviceCode: string;
  userCode: string;
  verificationUri: string;
  expiresIn: number;
  interval: number;
}

/** Oxbow's own settings, from settings.json. */
export interface AllSettings {
  /** The number of the last change in them, as `settings-changed` sends it. */
  revision: number;
  /** Every setting that differs from its default. */
  values: Record<string, unknown>;
  /** Why settings.json can't be read; the defaults are used meanwhile. */
  problem: string | null;
}

/** Settings read from Git's own config and the computer. */
export interface GitSettings {
  git: { path: string; version: string | null; custom: boolean };
  global: Record<string, string>;
  sshKeys: SshKey[];
  editors: OpenApp[];
  terminals: OpenApp[];
}

export interface RemoteInfo {
  name: string;
  fetchUrl: string;
  pushUrl: string;
}

/** Settings › This Repository. */
export interface RepoSettings {
  name: string;
  path: string;
  local: Record<string, string>;
  remotes: RemoteInfo[];
  storage: { bytes: number; objects: number; loose: number; lfsPatterns: string[] } | null;
  /** Production branch: the chosen full ref (`null` is Auto), whether it exists, what Auto
   *  picks, and the local and remote branches to choose from. */
  production: { chosen: string | null; found: boolean; auto: string | null; branches: string[] };
}

export type SearchMode = "message" | "code" | "author" | "file";

export interface SearchQuery {
  mode: SearchMode;
  text: string;
  branch: string | null;
  /** As `git log --since` reads it, e.g. `1.week.ago`. */
  since: string | null;
  author: string | null;
}

export interface SearchHit {
  id: string;
  /** For code and file searches, the files that matched. */
  files: string[];
}

export interface SearchResult {
  hits: SearchHit[];
  more: boolean;
  command: string;
}

/** A repository in the Welcome window's Recent Repositories. */
export interface RecentRepo {
  path: string;
  name: string;
  /** Last opened, in seconds since 1970. */
  opened: number;
  /** The folder is gone: moved, renamed or deleted. */
  missing: boolean;
  /** Why it is gone, when it is. */
  gone?: GoneFolder;
}

/** Why a repository's folder can't be opened: on a disk that isn't connected, in the Trash,
 *  moved (found next to where it was), deleted or moved where Oxbow didn't look, or there but
 *  not readable for Oxbow. */
export type GoneWhy = "disk" | "trash" | "moved" | "missing" | "noAccess";

/** A repository folder that isn't there any more, and what is still known about it. */
export interface GoneFolder {
  path: string;
  name: string;
  why: GoneWhy;
  /** The disk it is on, when that isn't connected. */
  disk: string | null;
  /** Where it is now, when Oxbow found it. */
  found: string | null;
  /** When it is a worktree of another folder: what the repository still has about it. */
  worktree: GoneWorktree | null;
}

export interface GoneWorktree {
  /** The main folder; null when it isn't there either. */
  main: string | null;
  /** The branch checked out in the main folder. */
  mainBranch: string | null;
  branch: string | null;
  /** The commit checked out there when HEAD was detached. */
  head: string | null;
  /** Locked, so Prune keeps it: the reason, empty when none was given. */
  locked: string | null;
}

/** The gone folder of the window, as its screen shows it. */
export interface Gone extends GoneFolder {
  /** The Conflicts screen has a draft saved for it. */
  conflictDraft: boolean;
}

/** Where a recent repository stands. */
export interface RepoGlance {
  branch: string | null;
  color: number;
  ahead: number;
  behind: number;
  upstream: boolean;
  remote: string | null;
  changed: number;
  stashes: number;
  operation: OperationKind | null;
  conflicts: number;
}

/** What the Welcome window says about this computer's Git setup. */
export interface WelcomeInfo {
  version: string;
  git: { path: string; version: string | null; custom: boolean };
  name: string | null;
  email: string | null;
  ssh: { label: string; ok: boolean };
  defaultBranch: string | null;
  home: string;
  /** Where clones and new repositories go unless another folder is picked. */
  projects: string;
  /** [key, name] of the starter .gitignore files and licenses. */
  gitignores: [string, string][];
  licenses: [string, string][];
}

export interface RemoteProbe {
  transport: string;
  defaultBranch: string | null;
  branches: number;
  /** Commits on the default branch, when the remote could tell without a clone. */
  commits: number | null;
}

export interface CloneOptions {
  url: string;
  path: string;
  submodules: boolean;
  shallow: boolean;
}

export interface NewRepoOptions {
  path: string;
  branch: string;
  readme: boolean;
  gitignore: string | null;
  license: string | null;
  commit: boolean;
}

export interface NewRepoPlan {
  path: string;
  exists: boolean;
  entries: number;
  /** Already a repository: open it instead. */
  repository: string | null;
  writes: string[];
  kept: string[];
  commands: GitCommand[];
}

/** One line git printed. */
export interface OutputLine {
  text: string;
  stderr: boolean;
  progress: boolean;
}

/** Where HEAD was: on a branch (maybe with no commit yet), or detached at a commit. */
export interface HeadState {
  branch: string | null;
  commit: string | null;
}

/** A ref a step created, moved or deleted, by full name. */
export interface RefMove {
  name: string;
  before: string | null;
  after: string | null;
}

export interface StashRecord {
  id: string;
  message: string;
}

/** The files at one moment: the working copy, and the staging area unless it had conflicts. */
export interface Trees {
  worktree: string;
  index: string | null;
}

/** One step of the Operation Log. */
export interface OpEntry {
  id: string;
  /** Seconds since the epoch. */
  time: number;
  /** The icon: commit, discard, reset, … */
  kind: string;
  title: string;
  detail: string;
  /** It failed or stopped half way, e.g. on conflicts. */
  failed: boolean;
  headBefore: HeadState;
  headAfter: HeadState;
  refs: RefMove[];
  stash: { before: StashRecord[]; after: StashRecord[] } | null;
  before: Trees;
  after: Trees;
  /** Until when the step can be gone back to, seconds since the epoch. */
  expires: number;
  /** Local branches (full names) the step rewrote rather than moved forward: a rebase, an amend. */
  rewrites: string[] | null;
}

/** A commit git's reflog of HEAD or a local branch points at: lost work, if no branch has it. */
export interface ReflogEntry {
  id: string;
  /** "HEAD" or the branch whose reflog has the entry. */
  source: string;
  /** As git names the entry, e.g. "HEAD@{3}". */
  selector: string;
  /** When the ref moved there, seconds since the epoch. */
  time: number;
  /** The icon: commit, amend, reset, checkout, rebase, merge, pull, cherry, revert, branch, other. */
  kind: string;
  /** What git wrote for the move, e.g. "reset: moving to HEAD~2". */
  message: string;
  summary: string;
  authorName: string;
  authorEmail: string;
  parents: string[];
  /** No branch, tag or remote branch has it, and HEAD is not on it. */
  lost: boolean;
  /** Commits only this one keeps: itself and the lost ones before it. */
  lostCount: number;
  /** A lost commit in the list whose history has this one. */
  within: string | null;
  /** For a lost commit, until when git keeps it, seconds since the epoch; null = never expires. */
  expires: number | null;
}

/** The repository now beside how it was at a step of the log. */
export interface SnapshotComparison {
  /** From the files then to the files now. */
  comparison: Comparison;
  /** Refs that point elsewhere now: before is then, after is now. */
  refs: RefMove[];
  headThen: HeadState;
  headNow: HeadState;
  stagingDiffers: boolean;
}

/** A stack of branches on top of the trunk, for Edit Stack. */
/** A branch that has a merged pull request, for a restack to move onto. */
export interface Landing {
  name: string;
  tip: CommitBrief;
}

/** What a restack after a merged pull request does, kept from its rebase until its last step, so
 *  a push or base change that failed goes on from there. */
export interface Restack {
  /** The merged pull request whose commits leave the stack. */
  merged: number;
  /** The branch it merged into, the new base of the pull requests that were built on it. */
  base: string;
  /** The branches the rebase moves. */
  branches: string[];
  /** The merged commits the rebase leaves out. */
  drop: string[];
  remote: string | null;
  push: { branch: string; upstream: string }[];
  /** Pull requests that went into the merged branch and go into `base` next. */
  retarget: number[];
  /** The step that failed last, until it runs again. */
  failed: { step: "push" | "base"; message: string } | null;
}

/** A kept restack and how far it got here. */
export interface RestackState {
  restack: Restack;
  /** The rebase stopped, e.g. on a conflict, and waits to be continued. */
  rebasing: boolean;
  /** Branches whose remote branch isn't at their tip yet. */
  unpushed: string[];
}

export interface Stack {
  /** The branch at the top, the one the rebase rewrites. */
  top: string;
  head: string | null;
  /** The branch the stack is built on, e.g. "main". */
  trunk: string;
  base: CommitBrief;
  trunkTip: CommitBrief;
  /** Commits on the trunk since the stack's base. */
  newer: number;
  /** Newest first. */
  commits: StackCommit[];
  /** Top first; a branch owns the commits under it down to the next one. */
  branches: StackBranch[];
  /** Branches that fork off the stack, so the rebase leaves them as they are. */
  leftBehind: string[];
  dirty: boolean;
  /** Merges of the trunk into the stack, oldest first: the rebase leaves them out. */
  merges: StackMerge[];
}

export interface StackMerge {
  id: string;
  summary: string;
  /** The branch of the stack it is on. */
  branch: string;
}

export interface StackCommit {
  id: string;
  summary: string;
  message: string;
  authorName: string;
  time: number;
  additions: number;
  deletions: number;
  branch: string;
  pushed: boolean;
}

export interface StackBranch {
  name: string;
  tip: string;
  /** The stack commit its line sits above: the tip, or the commit a merge at the tip was made on. */
  at: string;
  upstream: RemoteBranch | null;
  ahead: number;
}

export type StepAction = "pick" | "reword" | "edit" | "squash" | "fixup" | "drop";

/** Oldest first. */
export type StackStep = { kind: "commit"; id: string; action: StepAction; message: string | null } | { kind: "branch"; name: string };

export interface StackPlan {
  top: string;
  base: string;
  onto: string;
  ontoName: string | null;
  head: string | null;
  steps: StackStep[];
}

export interface StackPreview {
  changed: boolean;
  rewritten: number;
  squashed: number;
  dropped: number;
  reworded: number;
  moved: number;
  edits: number;
  emptied: string[];
  conflict: { id: string; summary: string; files: string[] } | null;
  /** Top first. */
  branches: BranchAfter[];
}

export interface BranchAfter {
  name: string;
  /** Its own commits afterwards. */
  commits: number;
  moves: boolean;
  /** The commit it points at afterwards, as it is now: a stack commit (with a new SHA when it
   *  moves) or the commit the stack starts from when it ends up empty. */
  tipFrom: string;
  /** That commit keeps its SHA. */
  sameSha: boolean;
  forcePush: RemoteBranch | null;
}

// --- Pull requests ---------------------------------------------------------------------------

/** The GitHub repository the open one pushes to. */
export interface GitHubRepo {
  owner: string;
  name: string;
  /** The remote that points at it. */
  remote: string;
}

/** A project on GitLab, the way a remote points at it. */
export interface GitLabProject {
  /** The remote that points at it. */
  remote: string;
  host: string;
  /** `group/subgroup/project`. */
  path: string;
  /** The project's page, e.g. `https://gitlab.com/group/project`. */
  webUrl: string;
}

/** What a push asks GitLab for: a merge request of the pushed branch into `target`. */
export interface MergeRequest {
  target: string;
  title: string;
  /** The title starts with `Draft:`. */
  draft: boolean;
  /** GitLab deletes the pushed branch there once it is merged. */
  removeSource: boolean;
}

export interface GitHubPerson {
  login: string;
  avatarUrl: string;
}

export type PullState = "open" | "closed" | "merged";

export interface PullSummary {
  number: number;
  title: string;
  state: PullState;
  draft: boolean;
  head: string;
  headSha: string;
  /** Another account for a pull request from a fork. */
  headOwner: string | null;
  base: string;
  htmlUrl: string;
  body: string;
  author: GitHubPerson;
  created: number;
  updated: number;
  nodeId: string;
  /** Reviews asked for and not given yet: logins, and teams as `org/team`. */
  requested: string[];
}

export interface PullCommit {
  sha: string;
  summary: string;
  message: string;
  author: string;
  time: number;
  additions: number | null;
  deletions: number | null;
}

export type ReviewState = "approved" | "changesRequested" | "commented" | "dismissed";

export interface PullReview {
  author: GitHubPerson;
  state: ReviewState;
  time: number;
}

export interface HunkLine {
  number: number | null;
  text: string;
  mark: boolean;
}

export interface PullComment {
  author: GitHubPerson;
  body: string;
  time: number;
}

export type PullEntry =
  | { kind: "comment"; author: GitHubPerson; body: string; time: number }
  | { kind: "review"; author: GitHubPerson; state: ReviewState; body: string; time: number }
  | {
      kind: "thread";
      /** The first comment's id, which replies go to. */
      id: number;
      /** GraphQL's id of the thread. */
      node: string | null;
      path: string;
      line: number | null;
      startLine: number | null;
      side: DiffSide;
      hunk: HunkLine[];
      comments: PullComment[];
      resolved: boolean | null;
      outdated: boolean;
      time: number;
    }
  | { kind: "event"; actor: string; text: string; icon: "push" | "ok" | "fail" | "review" | "draft" | "dot"; tone: "ok" | "bad" | null; time: number };

export interface PullCheck {
  name: string;
  detail: string;
  state: "ok" | "fail" | "run" | "skip";
  seconds: number | null;
  url: string | null;
}

/** What the repository allows when merging. */
export interface MergeSettings {
  mergeCommit: boolean;
  squash: boolean;
  rebase: boolean;
  autoMerge: boolean;
  deleteBranchOnMerge: boolean;
  defaultBranch: string;
  canPush: boolean;
}

export interface PullRequest extends PullSummary {
  additions: number;
  deletions: number;
  changedFiles: number;
  commitCount: number;
  /** `commits` has them all: GitHub lists at most 250. */
  commitsComplete: boolean;
  mergeable: boolean | null;
  mergeableState: string;
  mergedAt: number | null;
  mergeCommit: string | null;
  mergedBy: string | null;
  autoMerge: string | null;
  commits: PullCommit[];
  reviews: PullReview[];
  timeline: PullEntry[];
  checks: PullCheck[];
  requiredApprovals: number | null;
  settings: MergeSettings;
}

export type MergeMethodName = "merge" | "squash" | "rebase";

/** A change to a pull request; `number` 0 is the one a `create` earlier in the same run made. */
export type PullCall =
  | { kind: "merge"; number: number; method: MergeMethodName; title?: string | null; message?: string | null; sha: string }
  | { kind: "create"; head: string; base: string; title: string; body: string; draft: boolean }
  | { kind: "requestReviewers"; number: number; reviewers: string[] }
  | { kind: "update"; number: number; base?: string | null; body?: string | null }
  | { kind: "readyForReview"; number: number; id: string }
  | { kind: "updateBranch"; number: number; sha: string }
  | { kind: "autoMerge"; number: number; id: string; method?: MergeMethodName | null }
  | { kind: "viewed"; id: string; path: string; viewed: boolean }
  | { kind: "comment"; number: number; commit: string; spot: Spot; body: string }
  | { kind: "reply"; number: number; to: number; body: string }
  | { kind: "resolve"; thread: string; resolved: boolean }
  | { kind: "startReview"; id: string; commit: string }
  | { kind: "reviewComment"; review?: string | null; spot: Spot; body: string }
  | { kind: "reviewReply"; review?: string | null; thread: string; body: string }
  | { kind: "submitReview"; review?: string | null; verdict: Verdict; body: string }
  | { kind: "deleteReview"; review?: string | null };

/** `RIGHT`: a line of the new file; `LEFT`: a deleted line, by its old number. */
export type DiffSide = "RIGHT" | "LEFT";

/** The lines a review comment is on. */
export interface Spot {
  path: string;
  line: number;
  side: DiffSide;
  startLine?: number | null;
}

export type Verdict = "comment" | "approve" | "requestChanges";

/** GitHub's say on an open pull request, for Reviews. */
export interface PullStatus {
  number: number;
  decision: "approved" | "changesRequested" | "reviewRequired" | null;
  checks: "success" | "failure" | "pending" | "error" | "expected" | null;
}

/** Files this account ticked Viewed on GitHub; `dismissed` ones changed since. */
export interface Viewing {
  viewed: string[];
  dismissed: string[];
  pending: { id: string; commit: string | null; comments: number } | null;
}

export interface DraftComment {
  id: string;
  spot: Spot;
  body: string;
  time: number;
}

export interface DraftReply {
  id: string;
  /** The thread's first comment. */
  to: number;
  /** The thread's GraphQL id. */
  thread: string;
  body: string;
  time: number;
}

/** A review written here and not sent; its comments are on lines of `commit`. */
export interface ReviewDraft {
  commit: string;
  body: string;
  verdict: Verdict | null;
  comments: DraftComment[];
  replies: DraftReply[];
}

/** A commit of a series before or after a rewrite. */
export interface SeriesCommit {
  id: string;
  summary: string;
  authorName: string;
  authorEmail: string;
  time: number;
  parents: string[];
}

export type PairKind = "same" | "changed" | "added" | "dropped";

/** A line of the diff between two versions of a commit: `outer` says which version has it,
 *  `inner` what it is in that commit's own change. */
export interface InterLine {
  outer: "both" | "old" | "new";
  inner: "context" | "added" | "removed" | "hunk" | "text" | "gap";
  text: string;
}

export interface InterSection {
  kind: "metadata" | "message" | "file";
  /** The file's path, with git's note such as "(new)"; empty for the message and the author. */
  title: string;
  changed: boolean;
  lines: InterLine[];
}

/** An old commit and its new version, or a commit only one series has. */
export interface SeriesPair {
  kind: PairKind;
  old: SeriesCommit | null;
  new: SeriesCommit | null;
  /** Where the commit is in its series, 1 for the oldest. */
  oldIndex: number | null;
  newIndex: number | null;
  message: boolean;
  code: boolean;
  author: boolean;
  sections: InterSection[];
}

/** The old series of a rewrite beside the new one, from `git range-diff`. */
export interface Series {
  oldTip: string;
  newTip: string;
  oldBase: string | null;
  newBase: string | null;
  /** A branch, tag or remote branch at the new base, when it moved. */
  baseName: string | null;
  /** Commits the new base has that the old one didn't. */
  baseCommits: number;
  oldCount: number;
  newCount: number;
  /** Newest first; a dropped commit sits where it was. */
  pairs: SeriesPair[];
  command: string;
}

/** The latest rewrite of a branch as its remote branch knows it. */
export interface Rewrite {
  old: string;
  new: string;
  source: "notPushed" | "pushed" | "fetched";
  time: number | null;
  rewritten: string[];
}
