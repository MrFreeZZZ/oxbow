import { invoke } from "@tauri-apps/api/core";
import type { Action, CommitDetail, FileDiff, History, Plan, RepoSummary, Side, WorkingTree } from "./types";

export const api = {
  initialRepo: () => invoke<string | null>("initial_repo"),
  openRepo: (path: string) => invoke<RepoSummary>("open_repo", { path }),
  history: () => invoke<History>("history"),
  commitDetail: (id: string) => invoke<CommitDetail>("commit_detail", { id }),
  commitDiff: (id: string, path: string | null, wholeFile: boolean) =>
    invoke<FileDiff[]>("commit_diff", { id, path, wholeFile }),
  workingTree: () => invoke<WorkingTree>("working_tree"),
  workingDiff: (path: string, side: Side, wholeFile: boolean) => invoke<FileDiff>("working_diff", { path, side, wholeFile }),
  planAction: (action: Action) => invoke<Plan>("plan_action", { action }),
  /** Rejects with a `Failure`. */
  performAction: (action: Action) => invoke<string>("perform_action", { action }),
  stopAction: () => invoke<void>("stop_action"),
  getSetting: <T>(key: string) => invoke<T | null>("get_setting", { key }),
  setSetting: (key: string, value: unknown) => invoke<void>("set_setting", { key, value }),
};

/** Keys of `settings.json`. */
export const settings = {
  detailsWidth: "oxbow.history.detailsWidth",
  /** Ask before running a git command that changes the repository (default true). */
  confirmActions: "oxbow.confirm.enabled",
};
