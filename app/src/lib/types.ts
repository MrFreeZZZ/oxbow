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
  conflicted: number;
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
  refs: RefInfo[];
  /** Tags the default remote doesn’t have yet, as far as Oxbow knows. */
  localTags: string[];
  remotes: string[];
  tracking: Tracking | null;
  defaultRemote: string | null;
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
  /** `lines`: only these changed lines of the hunk, as indexes into its lines. */
  | { kind: "stageHunk"; path: string; header: string; check: string; lines?: number[] | null }
  | { kind: "unstageHunk"; path: string; header: string; check: string; lines?: number[] | null }
  | { kind: "discardHunk"; path: string; header: string; check: string; lines?: number[] | null }
  | { kind: "ignore"; pattern: string }
  | { kind: "commit"; message: string; amend: boolean; noVerify?: boolean }
  | { kind: "fetch"; remote: string | null }
  | { kind: "pull"; remote: string; branch: string; fetchFirst?: boolean }
  | {
      kind: "push";
      remote: string;
      branch: string;
      upstream: string;
      setUpstream: boolean;
      force: boolean;
      lease: string | null;
      noVerify: boolean;
    }
  | { kind: "pullAndPush"; remote: string; branch: string; upstream: string; noVerify: boolean }
  | { kind: "abortRebase" }
  | { kind: "switch"; branch: string; stash: boolean; keep: string | null }
  | { kind: "detach"; commit: string; stash: boolean }
  | { kind: "track"; remote: string; branch: string; stash: boolean }
  | { kind: "createBranch"; name: string; start: string | null; switch: boolean; publish: string | null }
  | { kind: "renameBranch"; from: string; to: string; upstream: RemoteBranch | null }
  | { kind: "deleteBranch"; name: string; force: boolean; upstream: RemoteBranch | null }
  | { kind: "deleteRemoteBranch"; remote: string; branch: string }
  | { kind: "merge"; branch: string; method: MergeMethod; message: string | null }
  | { kind: "continue"; message: string | null }
  | { kind: "abort" }
  | { kind: "skip" }
  | { kind: "resolve"; path: string; picks: Pick[] }
  | { kind: "takeFile"; path: string; side: ConflictSide }
  | { kind: "cherryPick"; commit: string }
  | { kind: "revert"; commit: string }
  | { kind: "reset"; commit: string; mode: ResetMode }
  | { kind: "reword"; message: string }
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
  /** Back to before step `id` of the Operation Log, or right after it with `after`. */
  | { kind: "restore"; id: string; after: boolean }
  | { kind: "clearOperationLog" }
  | { kind: "editStack"; plan: StackPlan }
  | { kind: "addToCommit"; commit: string }
  | { kind: "pushBranches"; remote: string; branches: { branch: string; upstream: string }[] }
  | { kind: "pullRequestMerged"; remote: string; base: string; branch: string; deleteRemote: boolean; deleteLocal: boolean }
  | { kind: "lfsTrack"; pattern: string; paths: string[] }
  | { kind: "lfsUntrack"; pattern: string }
  | { kind: "lfsPull"; include: string | null }
  | { kind: "lfsPrune" }
  | { kind: "lfsInstall"; brew: string | null; pull: boolean };

export type ResetMode = "soft" | "mixed" | "hard";

export type MergeMethod = "merge" | "squash" | "rebase" | "fastForward";
export type OperationKind = "merge" | "squash" | "rebase" | "cherryPick" | "revert" | "stashApply";
/** A side of a conflict as git names the index stages: ours is stage 2, theirs stage 3. */
export type ConflictSide = "ours" | "theirs";
export type Pick = "ours" | "theirs" | "oursThenTheirs" | "theirsThenOurs";

export interface Operation {
  kind: OperationKind;
  /** The branch that changes: the checked-out one, or the one being rebased. */
  branch: string | null;
  /** The merged branch, the new base of a rebase, a short sha otherwise. */
  incoming: string | null;
  /** The commit being merged, replayed, picked or reverted. */
  commit: CommitBrief | null;
  incomingCount: number;
  /** For a rebase: [commit it stopped on, of how many], from 1. */
  step: [number, number] | null;
  conflicted: number;
  oursLabel: string;
  theirsLabel: string;
  /** The side with the user's own work: ours in a merge, theirs in a rebase. */
  yours: ConflictSide;
  message: string | null;
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
  chunks: Chunk[];
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
  | { kind: "line"; text: string; stderr: boolean; progress: boolean };

export type FailureKind =
  | "rejected"
  | "staleLease"
  | "auth"
  | "network"
  | "hook"
  | "conflict"
  | "localChanges"
  | "notMerged"
  | "cancelled"
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
  /** For a refused SSH connection: what Oxbow found out. */
  ssh?: SshCheck | null;
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
}

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
}

/** A stack of branches on top of the trunk, for Edit Stack. */
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
  branches: { name: string; commits: number; moves: boolean; forcePush: RemoteBranch | null }[];
}

// --- Pull requests ---------------------------------------------------------------------------

/** The GitHub repository the open one pushes to. */
export interface GitHubRepo {
  owner: string;
  name: string;
  /** The remote that points at it. */
  remote: string;
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
  | { kind: "thread"; path: string; line: number | null; hunk: HunkLine[]; comments: PullComment[]; resolved: boolean | null; outdated: boolean; time: number }
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
  mergeable: boolean | null;
  mergeableState: string;
  mergedAt: number | null;
  mergeCommit: string | null;
  mergedBy: string | null;
  autoMerge: string | null;
  commits: PullCommit[];
  reviews: PullReview[];
  requested: string[];
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
  | { kind: "autoMerge"; number: number; id: string; method?: MergeMethodName | null };
