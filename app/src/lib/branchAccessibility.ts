import type { Tracking } from "./types";

interface BranchStatus {
  name: string;
  current: boolean;
  tracking: Tracking | null;
  worktree?: { name: string; missing: boolean } | null;
  pullNumber?: number | null;
  mergeNumber?: number | null;
  operation?: string | null;
}

/** The visible badges need words when the row has an explicit accessible name. */
export function branchAccessibleName(status: BranchStatus): string {
  const parts = [status.name];
  if (status.current) parts.push("current branch");
  const tracking = status.tracking;
  if (tracking?.gone) parts.push(`upstream ${tracking.remote}/${tracking.branch} deleted`);
  else if (tracking) {
    if (tracking.ahead) parts.push(`${tracking.ahead} commit${tracking.ahead === 1 ? "" : "s"} to push`);
    if (tracking.behind) parts.push(`${tracking.behind} commit${tracking.behind === 1 ? "" : "s"} to pull`);
  }
  if (status.worktree) parts.push(`checked out in ${status.worktree.name}${status.worktree.missing ? ", folder missing" : ""}`);
  if (status.pullNumber) parts.push(`pull request #${status.pullNumber}`);
  if (status.mergeNumber) parts.push(`merge request !${status.mergeNumber}`);
  if (status.operation) parts.push(`${status.operation.toLowerCase()} in progress`);
  return parts.join(", ");
}
