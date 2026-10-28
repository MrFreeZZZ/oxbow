import { Channel, invoke } from "@tauri-apps/api/core";
import type { MenuPart } from "./appMenu";
import type {
  AllSettings,
  SubmoduleMove,
  UnpushedSubmodule,
  Submodule,
  Worktree,
  FolderState,
  Blame,
  BlameOptions,
  Source,
  SourceLines,
  FileHistory,
  CompareMode,
  Comparison,
  Action,
  CommitBrief,
  CommitDetail,
  DiffPage,
  FileChange,
  ConfigScope,
  GitSettings,
  RepoSettings,
  CommitCheck,
  ConflictFile,
  MergeTool,
  DeletionCheck,
  FileDiff,
  History,
  MergePreview,
  PatchPreview,
  Plan,
  RepoSummary,
  SearchQuery,
  SearchResult,
  Side,
  StashCheck,
  WorkingTree,
  RecentRepo,
  TrustedRepo,
  Gone,
  RepoGlance,
  WelcomeInfo,
  RemoteProbe,
  CloneOptions,
  NewRepoOptions,
  NewRepoPlan,
  GitCommand,
  OpEntry,
  Rewrite,
  Series,
  ReflogEntry,
  FixState,
  SnapshotComparison,
  Landing,
  Restack,
  RestackState,
  Stack,
  StackPlan,
  StackPreview,
  GitHubAccount,
  GitLabAccount,
  GitLabSignInSetup,
  SignInSetup,
  DeviceCode,
  GitHubOwners,
  Publish,
  GitHubRepo,
  GitLabProject,
  PullCall,
  PullRequest,
  PullStatus,
  ReviewDraft,
  Viewing,
  PullSummary,
  LfsStatus,
  SecretFinding,
} from "./types";
import type { PullSetup } from "./remote";
import type { ConsoleEntry, ConsoleRepos } from "./console";

/** What a read answers when a newer one in its lane, or `stopReads`, stopped it. */
export const stopped = (err: unknown) => String(err) === "stopped";

export const api = {
  /** Reduce Transparency and Increase Contrast from the system (always off outside macOS). */
  displayOptions: () => invoke<{ reduceTransparency: boolean; increaseContrast: boolean }>("display_options"),
  initialRepo: () => invoke<string | null>("initial_repo"),
  /** A repository whose settings run programs comes back `untrusted`, unopened, unless it is
   *  trusted or Settings says not to ask; `answer` is the user's reply to that. */
  openRepo: (path: string, answer: "trust" | "restricted" | null = null) => invoke<RepoSummary>("open_repo", { path, answer }),
  /** Trust This Repository…: git runs its settings from now on, in every window. */
  trustRepo: (path: string) => invoke<void>("trust_repo", { path }),
  trustedRepos: () => invoke<TrustedRepo[]>("trusted_repos"),
  untrustRepo: (path: string) => invoke<void>("untrust_repo", { path }),
  recentRepos: () => invoke<RecentRepo[]>("recent_repos"),
  /** With `to`, the repository moved there. */
  forgetRepo: (path: string, to: string | null = null) => invoke<void>("forget_repo", { path, to }),
  goneFolder: (path: string) => invoke<Gone | null>("gone_folder", { path }),
  reconnectFolder: (old: string, path: string) => invoke<{ path: string; conflictDraft: boolean }>("reconnect_folder", { old, new: path }),
  /** Empty Recent Repositories but for `keep`. */
  clearRecent: (keep: string[]) => invoke<void>("clear_recent", { keep }),
  repoGlance: (path: string) => invoke<RepoGlance>("repo_glance", { path }),
  welcomeInfo: () => invoke<WelcomeInfo>("welcome_info"),
  folderState: (path: string) => invoke<"missing" | "empty" | "files" | "file">("folder_state", { path }),
  probeRemote: (url: string) => invoke<RemoteProbe>("probe_remote", { url }),
  cloneCommand: (options: CloneOptions) => invoke<GitCommand>("clone_command", { options }),
  /** Sends `clone-line` events while it runs; Stop is `stopAction`. */
  cloneRepo: (options: CloneOptions) => invoke<string>("clone_repo", { options }),
  planNewRepo: (options: NewRepoOptions) => invoke<NewRepoPlan>("plan_new_repo", { options }),
  createRepo: (options: NewRepoOptions) => invoke<{ path: string; secrets: SecretFinding[] }>("create_repo", { options }),
  /** The latest `limit` commits, or more to reach `until`. */
  /** With `known`, the key of the history on screen, rows that came out the same are left out. */
  /** With `focus`, also the history of Current Branch Only: `History.focus`. With `quiet`, a
   *  refresh for no action of the window's own: what the watch saw no change in isn't asked of
   *  git again, and `History.unchanged` says when nothing changed. */
  history: (limit: number, until: string | null = null, known: string | null = null, focus = false, quiet = false) =>
    invoke<History>("history", { limit, until, known, focus, quiet, lane: "history" }),
  /** What the folders of the worktrees and submodules the last history listed have, each passed
   *  to `onState` as soon as it is read. One read before `since` (ms) is read again even when
   *  it shows no change. A newer call stops this one. */
  folderStates: (since: number | null, onState: (state: FolderState) => void) => {
    const onStateChannel = new Channel<FolderState>();
    onStateChannel.onmessage = onState;
    return invoke<void>("folder_states", { since, lane: "folders", onState: onStateChannel });
  },
  /** Which of `ids`, loaded commits the history could not tell, are unpushed: the next load
   *  marks them. Whether it found out anything. */
  settleUnpushed: (ids: string[], lane: string | null = null) => invoke<boolean>("settle_unpushed", { ids, lane }),
  /** `quick`: the files without their line counts, which needs no file read. A newer read in
   *  the same `lane` stops this one. */
  commitDetail: (id: string, quick = false, lane: string | null = null, countFrom = 0) => invoke<CommitDetail>("commit_detail", { id, quick, countFrom, lane }),
  /** `full`: past the line limit in Settings (Show Diff Anyway). */
  commitDiff: (id: string, path: string | null, wholeFile: boolean, full = false, lane: string | null = null) =>
    invoke<FileDiff[]>("commit_diff", { id, path, wholeFile, full, lane }),
  /** The diffs of the commit's files from the `from`th on, as many as fit `budget` lines. */
  commitDiffPage: (id: string, from: number, budget: number, wholeFile: boolean, lane: string | null = null) =>
    invoke<DiffPage>("commit_diff_page", { id, from, budget, wholeFile, lane }),
  /** Stop the reads running in these lanes: their answers are no longer wanted. */
  stopReads: (lanes: string[]) => invoke<void>("stop_reads", { lanes }),
  compare: (base: string, target: string, mode: CompareMode) => invoke<Comparison>("compare", { base, target, mode }),
  /** A file of `commit` beside the file on disk. */
  compareWithWorkingCopy: (commit: string, path: string) => invoke<Comparison>("compare_with_working_copy", { commit, path }),
  compareDiff: (from: string, to: string, path: string | null, wholeFile: boolean, full = false, lane: string | null = null) =>
    invoke<FileDiff[]>("compare_diff", { from, to, path, wholeFile, full, lane }),
  /** A page of a comparison's diffs, from the `start`th file until about `budget` lines. */
  compareDiffPage: (from: string, to: string, start: number, budget: number, wholeFile: boolean, lane: string | null = null) =>
    invoke<DiffPage>("compare_diff_page", { from, to, start, budget, wholeFile, lane }),
  /** A comparison's files with their line counts from the `countFrom`th on. */
  compareCounts: (from: string, to: string, countFrom: number, lane: string | null = null) =>
    invoke<FileChange[]>("compare_counts", { from, to, countFrom, lane }),
  /** The full id of the commit a SHA prefix names, anywhere in the repository. */
  resolveCommit: (prefix: string) => invoke<string | null>("resolve_commit", { prefix }),
  fileHistory: (path: string, rev: string | null, lane: string | null = null) => invoke<FileHistory>("file_history", { path, rev, lane }),
  blame: (path: string, rev: string, options: BlameOptions, lane: string | null = null) => invoke<Blame>("blame", { path, rev, options, lane }),
  lineHistory: (path: string, line: number, rev: string, lane: string | null = null) => invoke<string[]>("line_history", { path, line, rev, lane }),
  files: () => invoke<string[]>("files"),
  filePickaxe: (path: string, text: string, matchCase: boolean, rev: string, lane: string | null = null) =>
    invoke<string[]>("file_pickaxe", { path, text, matchCase, rev, lane }),
  workingTree: () => invoke<WorkingTree>("working_tree"),
  /** Your latest commit messages, newest first, for ↑ in the commit message box. */
  recentMessages: () => invoke<string[]>("recent_messages"),
  workingDiff: (path: string, side: Side, wholeFile: boolean, full = false) =>
    invoke<FileDiff>("working_diff", { path, side, wholeFile, full }),
  /** A page of the diffs of `paths` on one side, in that order, from the `from`th file with
   *  changes there until about `budget` lines; files without changes there are left out. A newer
   *  read in the same `lane` stops this one. */
  workingDiffPage: (paths: string[], side: Side, wholeFile: boolean, snapshot: number | null, from: number, budget: number, lane: string | null = null) =>
    invoke<DiffPage>("working_diff_page", { paths, side, wholeFile, snapshot, from, budget, lane }),
  /** The raw bytes arrive as an ArrayBuffer, or as a list of numbers when the IPC falls back to JSON. */
  sourceBytes: (source: Source) =>
    invoke<ArrayBuffer | ArrayBufferView | number[]>("source_bytes", { source }).then((data) =>
      data instanceof ArrayBuffer
        ? new Uint8Array(data)
        : ArrayBuffer.isView(data)
          ? new Uint8Array(data.buffer, data.byteOffset, data.byteLength)
          : Uint8Array.from(data),
    ),
  /** Lines `start` to `start + count - 1` (from 1) of one side of a file diff, and how many it has. */
  sourceLines: (source: Source, start: number, count: number) => invoke<SourceLines>("source_lines", { source, start, count }),
  /** `label` names the copy: `logo (old a3f9c21).png`. */
  openSource: (source: Source, path: string, label: string) => invoke<void>("open_source", { source, path, label }),
  deletionCheck: (branch: string) => invoke<DeletionCheck>("deletion_check", { branch }),
  remoteDeletionCheck: (branch: string) => invoke<CommitBrief[]>("remote_deletion_check", { branch }),
  mergePreview: (branch: string) => invoke<MergePreview>("merge_preview", { branch }),
  /** Patch files at full paths, before Apply Patches runs. */
  patchPreview: (paths: string[]) => invoke<PatchPreview>("patch_preview", { paths }),
  /** The commits Save as Patches writes, oldest first: those of `to` that `since` doesn't
   *  have, or the newest `count` up to `to`. */
  patchRange: (to: string, since: string | null, count: number) => invoke<CommitBrief[]>("patch_range", { to, since, count }),
  conflictFile: (path: string) => invoke<ConflictFile>("conflict_file", { path }),
  mergeTool: () => invoke<MergeTool | null>("merge_tool"),
  /** Waits until the tool quits; true when git staged the file. */
  openMergeTool: (path: string) => invoke<boolean>("open_merge_tool", { path }),
  /** The Conflicts screen's draft saved for the working folder `workdir`, as lib/drafts.ts
   *  writes it. The folder is named, not taken from the window: a save can come after the
   *  window opened another repository. */
  conflictDraft: (workdir: string) => invoke<unknown>("conflict_draft", { workdir }),
  saveConflictDraft: (workdir: string, draft: unknown) => invoke<void>("save_conflict_draft", { workdir, draft }),
  dropConflictDraft: (workdir: string) => invoke<void>("drop_conflict_draft", { workdir }),
  /** A window stayed open when Quit closed it: Oxbow doesn't quit after all. */
  cancelQuit: () => invoke<void>("cancel_quit"),
  stashCheck: (index: number, id: string) => invoke<StashCheck>("stash_check", { index, id }),
  stack: (branch: string | null) => invoke<Stack>("stack", { branch }),
  landedOn: (merged: string, candidates: string[]) => invoke<Landing | null>("landed_on", { merged, candidates }),
  restack: () => invoke<RestackState | null>("restack"),
  saveRestack: (restack: Restack | null) => invoke<void>("save_restack", { restack }),
  stackPreview: (plan: StackPlan) => invoke<StackPreview>("stack_preview", { plan }),
  splitFiles: (commit: string) => invoke<FileDiff[]>("split_files", { commit }),
  planAction: (action: Action) => invoke<Plan>("plan_action", { action }),
  /** Rejects with a `Failure`. */
  /** `withoutUndo`: run even when the state before can't be saved for Undo, once that was chosen. */
  performAction: (action: Action, withoutUndo = false) => invoke<string>("perform_action", { action, withoutUndo }),
  stopAction: () => invoke<void>("stop_action"),
  getSetting: <T>(key: string) => invoke<T | null>("get_setting", { key }),
  allSettings: () => invoke<AllSettings>("all_settings"),
  openSettings: () => invoke<void>("open_settings"),
  /** The key of the app menu's Settings…, "CmdOrCtrl+,", or null for none (macOS). */
  setSettingsKey: (accelerator: string | null) => invoke<void>("set_settings_key", { accelerator }),
  setMenu: (menus: MenuPart[]) => invoke<void>("set_menu", { menus }),
  openConsole: () => invoke<void>("open_console"),
  consoleRepos: () => invoke<ConsoleRepos>("console_repos"),
  /** The log of a repository (null: what ran outside any), oldest first. */
  consoleEntries: (path: string | null) => invoke<ConsoleEntry[]>("console_entries", { path }),
  consoleClear: (paths: (string | null)[]) => invoke<void>("console_clear", { paths }),
  /** Resolves to the number of the change, as `settings-changed` sends it. */
  setSetting: (key: string, value: unknown) => invoke<number>("set_setting", { key, value }),
  gitSettings: () => invoke<GitSettings>("git_settings"),
  repoSettings: () => invoke<RepoSettings | null>("repo_settings"),
  /** `null` removes the value. */
  setGitConfig: (scope: ConfigScope, key: string, value: string | null) =>
    invoke<void>("set_git_config", { scope, key, value }),
  /** The open repository's checks before commit, written whole in this order. */
  setCommitChecks: (checks: CommitCheck[]) => invoke<void>("set_commit_checks", { checks }),
  /** Sends `fetch-event`s marked with `fetch`, Activity's number for it, while it runs; rejects
   *  with a `Failure`. Without a number, Stop doesn't reach it. */
  fetchInBackground: (remote: string | null, fetch: number | null = null, auto = false) =>
    invoke<{ updated: number }>("fetch_in_background", { remote, fetch, auto }),
  stopFetch: () => invoke<void>("stop_fetch"),
  publicKey: (key: string) => invoke<string>("public_key", { key }),
  pullSetup: () => invoke<PullSetup>("pull_setup"),
  openInEditor: (path: string, line: number | null) => invoke<void>("open_in_editor", { path, line }),
  revealFile: (path: string) => invoke<void>("reveal_file", { path }),
  /** Quick Look on macOS, the default app elsewhere. */
  quickLook: (path: string) => invoke<void>("quick_look", { path }),
  lfsStatus: () => invoke<LfsStatus>("lfs_status"),
  /** e.g. "3 files would be pruned (1.4 GB)"; null when nothing would be. */
  lfsPrunePreview: () => invoke<string | null>("lfs_prune_preview"),
  openLfsDownload: () => invoke<void>("open_lfs_download"),
  /** With `worktree`, that working folder of the repository instead. */
  openInTerminal: (worktree: string | null = null) => invoke<void>("open_in_terminal", { worktree }),
  revealWorktree: (path: string) => invoke<void>("reveal_worktree", { path }),
  /** A window of its own for the worktree at `path`, or the one it is already open in. */
  openWindow: (path: string) => invoke<void>("open_window", { path }),
  submoduleMove: (path: string, old: string | null, newId: string | null, changed: number | null = 0) => invoke<SubmoduleMove>("submodule_move", { path, old, new: newId, changed }),
  unpushedSubmodules: () => invoke<UnpushedSubmodule[]>("unpushed_submodules"),
  /** The submodules alone, for Retry after History couldn't read them. */
  submodules: () => invoke<Submodule[]>("submodules"),
  /** The worktrees alone, with their counts of changed files, for Retry. */
  worktrees: () => invoke<Worktree[]>("worktrees"),
  /** After removing the worktree at `path`, close the window that showed it. */
  closeWindowOf: (path: string) => invoke<void>("close_window_of", { path }),
  monospaceFonts: () => invoke<string[]>("monospace_fonts"),
  refreshRemoteTags: () => invoke<boolean>("refresh_remote_tags"),
  /** Newest first. */
  operationLog: () => invoke<OpEntry[]>("operation_log"),
  series: (old: string, next: string, rewritten: string[]) => invoke<Series>("series", { old, new: next, rewritten }),
  lastRewrite: (branch: string) => invoke<Rewrite | null>("last_rewrite", { branch }),
  /** Newest first, each commit once. */
  reflogEntries: () => invoke<ReflogEntry[]>("reflog_entries"),
  /** What Fix a Mistake plans from. */
  fixState: () => invoke<FixState>("fix_state"),
  compareSnapshot: (id: string, after: boolean) => invoke<SnapshotComparison>("compare_snapshot", { id, after }),
  /** A newer search in the same `lane` stops this one. */
  search: (query: SearchQuery, lane: string | null = null) => invoke<SearchResult>("search", { query, lane }),
  settingsText: () => invoke<string>("settings_text"),
  saveSettingsText: (text: string) => invoke<void>("save_settings_text", { text }),
  /** settings.json and the copy Restore Defaults keeps, as typed in a shell. */
  settingsLocation: () => invoke<{ file: string; backup: string }>("settings_location"),
  restoreDefaultSettings: () => invoke<void>("restore_default_settings"),
  restoreSettingsBackup: () => invoke<void>("restore_settings_backup"),
  revealSettings: () => invoke<void>("reveal_settings"),
  githubAccount: () => invoke<GitHubAccount | null>("github_account"),
  githubSignInSetup: () => invoke<SignInSetup>("github_sign_in_setup"),
  githubDeviceStart: () => invoke<DeviceCode>("github_device_start"),
  /** Resolves once the code is approved in the browser; fails with "stopped" after Stop. */
  githubDeviceWait: (deviceCode: string, interval: number, expiresIn: number) => invoke<GitHubAccount>("github_device_wait", { deviceCode, interval, expiresIn }),
  githubDeviceStop: () => invoke<void>("github_device_stop"),
  githubSignInToken: (token: string) => invoke<GitHubAccount>("github_sign_in_token", { token }),
  githubSignOut: () => invoke<void>("github_sign_out"),
  /** Opens a github.com page in the browser. */
  openGitHub: (url: string) => invoke<void>("open_github", { url }),
  gitlabAccounts: () => invoke<GitLabAccount[]>("gitlab_accounts"),
  gitlabSignInSetup: (server: string) => invoke<GitLabSignInSetup>("gitlab_sign_in_setup", { server }),
  gitlabOpenNewToken: (server: string) => invoke<void>("gitlab_open_new_token", { server }),
  gitlabOpenProfile: (host: string) => invoke<void>("gitlab_open_profile", { host }),
  gitlabSignIn: (server: string, token: string) => invoke<GitLabAccount>("gitlab_sign_in", { server, token }),
  gitlabSignInStop: () => invoke<void>("gitlab_sign_in_stop"),
  gitlabSignOut: (host: string) => invoke<void>("gitlab_sign_out", { host }),
  githubOwners: () => invoke<GitHubOwners>("github_owners"),
  /** Sends `action-event`s while it runs; Stop is `stopAction`. Resolves to the repository's page. */
  /** `create: false`: the repository is on GitHub already, only origin and the push are left. */
  githubPublish: (path: string, publish: Publish, create = true) => invoke<string>("github_publish", { path, publish, create }),
  githubRepo: () => invoke<GitHubRepo | null>("github_repo"),
  /** The GitLab projects the remotes point at, the default remote's first. */
  gitlabProjects: () => invoke<GitLabProject[]>("gitlab_projects"),
  gitlabMergeRequests: () => invoke<{ project: GitLabProject; mergeRequests: PullSummary[] } | null>("gitlab_merge_requests"),
  /** Merge request `!number` in full, in the shape of a pull request. */
  gitlabMergeRequest: (number: number) => invoke<PullRequest>("gitlab_merge_request", { number }),
  /** The requests `calls` send to GitLab, as curl. */
  gitlabCallsPreview: (calls: PullCall[]) => invoke<string[]>("gitlab_calls_preview", { calls }),
  /** Sends them in order, each shown in the sheet; answers with the merge request as it is now. */
  gitlabRun: (calls: PullCall[]) => invoke<unknown>("gitlab_run", { calls }),
  /** Opens a merge request, pipeline or job page of one of them in the browser. */
  openGitLab: (url: string) => invoke<void>("open_gitlab", { url }),
  /** Open and recently closed, the latest updated first. */
  githubPulls: () => invoke<PullSummary[]>("github_pulls"),
  githubPull: (number: number) => invoke<PullRequest>("github_pull", { number }),
  /** The requests as curl, for the sheet. */
  githubCallsPreview: (calls: PullCall[]) => invoke<string[]>("github_calls_preview", { calls }),
  /** Sends `action-event`s while it runs; resolves to what the last call (or a create) answered. */
  githubRun: (calls: PullCall[]) => invoke<{ number?: number; html_url?: string; sha?: string } | null>("github_run", { calls }),
  codeOwners: (base: string, branch: string) => invoke<string[]>("code_owners", { base, branch }),
  githubPullStatuses: () => invoke<PullStatus[]>("github_pull_statuses"),
  /** Fetches the pull request's head and base into refs/oxbow/pull/<number>/, quietly. */
  pullFetch: (number: number, base: string) => invoke<void>("pull_fetch", { number, base }),
  githubViewing: (number: number) => invoke<Viewing>("github_viewing", { number }),
  /** One small call, sent quietly: no sheet, no Activity. */
  githubSend: (call: PullCall) => invoke<unknown>("github_send", { call }),
  reviewDraft: (number: number) => invoke<ReviewDraft | null>("review_draft", { number }),
  reviewDrafts: () => invoke<Record<string, ReviewDraft>>("review_drafts"),
  /** Keeps the draft; null puts it away, a copy kept for 30 days. */
  saveReviewDraft: (number: number, draft: ReviewDraft | null) => invoke<void>("save_review_draft", { number, draft }),
};

/** Keys of `settings.json`. */
export const settings = {
  detailsWidth: "oxbow.history.detailsWidth",
  sidebarHidden: "oxbow.sidebar.hidden",
  detailsHidden: "oxbow.history.detailsHidden",
  changesWidth: "oxbow.changes.width",
  changesExpanded: "oxbow.changes.expanded",
  zoomLevel: "oxbow.window.zoomLevel",
};
