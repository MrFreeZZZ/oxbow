import { invoke } from "@tauri-apps/api/core";
import type { CommitDetail, FileDiff, History, RepoSummary } from "./types";

export const api = {
  initialRepo: () => invoke<string | null>("initial_repo"),
  openRepo: (path: string) => invoke<RepoSummary>("open_repo", { path }),
  history: () => invoke<History>("history"),
  commitDetail: (id: string) => invoke<CommitDetail>("commit_detail", { id }),
  commitDiff: (id: string, path: string | null, wholeFile: boolean) =>
    invoke<FileDiff[]>("commit_diff", { id, path, wholeFile }),
  getSetting: <T>(key: string) => invoke<T | null>("get_setting", { key }),
  setSetting: (key: string, value: unknown) => invoke<void>("set_setting", { key, value }),
};

/** Keys of `settings.json`. */
export const settings = {
  detailsWidth: "oxbow.history.detailsWidth",
};
