import { invoke } from "@tauri-apps/api/core";
import type {
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
} from "./types";
import type { PullSetup } from "./remote";

export const api = {
  initialRepo: () => invoke<string | null>("initial_repo"),
  openRepo: (path: string) => invoke<RepoSummary>("open_repo", { path }),
  history: () => invoke<History>("history"),
  commitDetail: (id: string) => invoke<CommitDetail>("commit_detail", { id }),
  commitDiff: (id: string, path: string | null, wholeFile: boolean) =>
    invoke<FileDiff[]>("commit_diff", { id, path, wholeFile }),
  workingTree: () => invoke<WorkingTree>("working_tree"),
  workingDiff: (path: string, side: Side, wholeFile: boolean) => invoke<FileDiff>("working_diff", { path, side, wholeFile }),
  deletionCheck: (branch: string) => invoke<DeletionCheck>("deletion_check", { branch }),
  remoteDeletionCheck: (branch: string) => invoke<CommitBrief[]>("remote_deletion_check", { branch }),
  mergePreview: (branch: string) => invoke<MergePreview>("merge_preview", { branch }),
  conflictFile: (path: string) => invoke<ConflictFile>("conflict_file", { path }),
  stashCheck: (index: number, id: string) => invoke<StashCheck>("stash_check", { index, id }),
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
  backgroundFetch: () => invoke<void>("background_fetch"),
  pullSetup: () => invoke<PullSetup>("pull_setup"),
  openInEditor: (path: string, line: number | null) => invoke<void>("open_in_editor", { path, line }),
  openInTerminal: () => invoke<void>("open_in_terminal"),
  monospaceFonts: () => invoke<string[]>("monospace_fonts"),
  refreshRemoteTags: () => invoke<boolean>("refresh_remote_tags"),
  search: (query: SearchQuery) => invoke<SearchResult>("search", { query }),
  settingsText: () => invoke<string>("settings_text"),
  saveSettingsText: (text: string) => invoke<void>("save_settings_text", { text }),
};

/** Keys of `settings.json`. */
export const settings = {
  detailsWidth: "oxbow.history.detailsWidth",
};
