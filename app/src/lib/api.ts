import { invoke } from "@tauri-apps/api/core";
import type {
  Blame,
  Source,
  FileHistory,
  CompareMode,
  Comparison,
  Action,
  CommitBrief,
  CommitDetail,
  ConfigScope,
  GitSettings,
  RepoSettings,
  ConflictFile,
  DeletionCheck,
  FileDiff,
  History,
  MergePreview,
  Plan,
  RepoSummary,
  SearchQuery,
  SearchResult,
  Side,
  StashCheck,
  WorkingTree,
  RecentRepo,
  RepoGlance,
  WelcomeInfo,
  RemoteProbe,
  CloneOptions,
  NewRepoOptions,
  NewRepoPlan,
  GitCommand,
  OpEntry,
  Stack,
  StackPlan,
  StackPreview,
  GitHubAccount,
  SignInSetup,
  DeviceCode,
  GitHubOwners,
  Publish,
  GitHubRepo,
  PullCall,
  PullRequest,
  PullSummary,
} from "./types";
import type { PullSetup } from "./remote";

export const api = {
  initialRepo: () => invoke<string | null>("initial_repo"),
  openRepo: (path: string) => invoke<RepoSummary>("open_repo", { path }),
  recentRepos: () => invoke<RecentRepo[]>("recent_repos"),
  /** With `to`, the repository moved there. */
  forgetRepo: (path: string, to: string | null = null) => invoke<void>("forget_repo", { path, to }),
  repoGlance: (path: string) => invoke<RepoGlance>("repo_glance", { path }),
  welcomeInfo: () => invoke<WelcomeInfo>("welcome_info"),
  folderState: (path: string) => invoke<"missing" | "empty" | "files" | "file">("folder_state", { path }),
  probeRemote: (url: string) => invoke<RemoteProbe>("probe_remote", { url }),
  cloneCommand: (options: CloneOptions) => invoke<GitCommand>("clone_command", { options }),
  /** Sends `clone-line` events while it runs; Stop is `stopAction`. */
  cloneRepo: (options: CloneOptions) => invoke<string>("clone_repo", { options }),
  planNewRepo: (options: NewRepoOptions) => invoke<NewRepoPlan>("plan_new_repo", { options }),
  createRepo: (options: NewRepoOptions) => invoke<string>("create_repo", { options }),
  history: () => invoke<History>("history"),
  commitDetail: (id: string) => invoke<CommitDetail>("commit_detail", { id }),
  /** `full`: past the line limit in Settings (Show Diff Anyway). */
  commitDiff: (id: string, path: string | null, wholeFile: boolean, full = false) =>
    invoke<FileDiff[]>("commit_diff", { id, path, wholeFile, full }),
  compare: (base: string, target: string, mode: CompareMode) => invoke<Comparison>("compare", { base, target, mode }),
  compareDiff: (from: string, to: string, path: string | null, wholeFile: boolean, full = false) =>
    invoke<FileDiff[]>("compare_diff", { from, to, path, wholeFile, full }),
  fileHistory: (path: string, rev: string | null) => invoke<FileHistory>("file_history", { path, rev }),
  blame: (path: string, rev: string) => invoke<Blame>("blame", { path, rev }),
  lineHistory: (path: string, line: number, rev: string) => invoke<string[]>("line_history", { path, line, rev }),
  files: () => invoke<string[]>("files"),
  filePickaxe: (path: string, text: string, matchCase: boolean, rev: string) => invoke<string[]>("file_pickaxe", { path, text, matchCase, rev }),
  workingTree: () => invoke<WorkingTree>("working_tree"),
  workingDiff: (path: string, side: Side, wholeFile: boolean, full = false) =>
    invoke<FileDiff>("working_diff", { path, side, wholeFile, full }),
  sourceBytes: (source: Source) => invoke<ArrayBuffer>("source_bytes", { source }),
  /** `label` names the copy: `logo (old a3f9c21).png`. */
  openSource: (source: Source, path: string, label: string) => invoke<void>("open_source", { source, path, label }),
  deletionCheck: (branch: string) => invoke<DeletionCheck>("deletion_check", { branch }),
  remoteDeletionCheck: (branch: string) => invoke<CommitBrief[]>("remote_deletion_check", { branch }),
  mergePreview: (branch: string) => invoke<MergePreview>("merge_preview", { branch }),
  conflictFile: (path: string) => invoke<ConflictFile>("conflict_file", { path }),
  stashCheck: (index: number, id: string) => invoke<StashCheck>("stash_check", { index, id }),
  stack: (branch: string | null) => invoke<Stack>("stack", { branch }),
  stackPreview: (plan: StackPlan) => invoke<StackPreview>("stack_preview", { plan }),
  planAction: (action: Action) => invoke<Plan>("plan_action", { action }),
  /** Rejects with a `Failure`. */
  performAction: (action: Action) => invoke<string>("perform_action", { action }),
  stopAction: () => invoke<void>("stop_action"),
  getSetting: <T>(key: string) => invoke<T | null>("get_setting", { key }),
  allSettings: () => invoke<Record<string, unknown>>("all_settings"),
  openSettings: () => invoke<void>("open_settings"),
  setSetting: (key: string, value: unknown) => invoke<void>("set_setting", { key, value }),
  gitSettings: () => invoke<GitSettings>("git_settings"),
  repoSettings: () => invoke<RepoSettings | null>("repo_settings"),
  /** `null` removes the value. */
  setGitConfig: (scope: ConfigScope, key: string, value: string | null) =>
    invoke<void>("set_git_config", { scope, key, value }),
  /** Sends `fetch-event`s while it runs; rejects with a `Failure`. */
  fetchInBackground: (remote: string | null) => invoke<{ updated: number }>("fetch_in_background", { remote }),
  stopFetch: () => invoke<void>("stop_fetch"),
  publicKey: (key: string) => invoke<string>("public_key", { key }),
  pullSetup: () => invoke<PullSetup>("pull_setup"),
  openInEditor: (path: string, line: number | null) => invoke<void>("open_in_editor", { path, line }),
  revealFile: (path: string) => invoke<void>("reveal_file", { path }),
  openInTerminal: () => invoke<void>("open_in_terminal"),
  monospaceFonts: () => invoke<string[]>("monospace_fonts"),
  refreshRemoteTags: () => invoke<boolean>("refresh_remote_tags"),
  /** Newest first. */
  operationLog: () => invoke<OpEntry[]>("operation_log"),
  search: (query: SearchQuery) => invoke<SearchResult>("search", { query }),
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
  githubOwners: () => invoke<GitHubOwners>("github_owners"),
  /** Sends `action-event`s while it runs; Stop is `stopAction`. Resolves to the repository's page. */
  githubPublish: (path: string, publish: Publish) => invoke<string>("github_publish", { path, publish }),
  githubRepo: () => invoke<GitHubRepo | null>("github_repo"),
  /** Open and recently closed, the latest updated first. */
  githubPulls: () => invoke<PullSummary[]>("github_pulls"),
  githubPull: (number: number) => invoke<PullRequest>("github_pull", { number }),
  /** The requests as curl, for the sheet. */
  githubCallsPreview: (calls: PullCall[]) => invoke<string[]>("github_calls_preview", { calls }),
  /** Sends `action-event`s while it runs; resolves to what the last call (or a create) answered. */
  githubRun: (calls: PullCall[]) => invoke<{ number?: number; html_url?: string; sha?: string } | null>("github_run", { calls }),
  codeOwners: (base: string, branch: string) => invoke<string[]>("code_owners", { base, branch }),
};

/** Keys of `settings.json`. */
export const settings = {
  detailsWidth: "oxbow.history.detailsWidth",
};
