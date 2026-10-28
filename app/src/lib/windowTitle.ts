// The title of a repository window: the name of what it shows, and the line under it.

export type Screen = "history" | "stashes" | "oplog" | "compare" | "file" | "stack" | "pull" | "series" | "reviews";

export interface Shown {
  screen: Screen;
  repo: string;
  /** The path that led here is shown before the line. */
  steps: boolean;
  /** The Resolve Conflicts screen is open over the screen, for this operation; `noun`: "Merge". */
  conflicts: { kind: string; noun: string; conflicted: number } | null;
  file: { path: string; line: number | null } | null;
  github: { place: string | null; open: number };
  /** The branch of the Pull Request screen and its pull request, if it has one; `byNumber`: the
   *  screen was opened on the pull request, not the branch. */
  pull: { branch: string; pr: number | null; byNumber: boolean } | null;
  series: { branch: string; from: string } | null;
  compare: { base: string; target: string };
  head: string | null;
  /** The History screen's line: what runs, a fetch that failed, or the branches and the state of HEAD. */
  running: { title: string; percent: number | null } | null;
  failed: string | null;
  branches: number;
  bisect: { found: string | null } | null;
  detached: boolean;
}

export interface Title {
  name: string;
  sub: string;
  /** The steps that led here go before `sub`. */
  steps: boolean;
  /** `sub` says something went wrong. */
  amber: boolean;
}

/** A lowercase repository name, as a title: "oxbow" is "Oxbow". */
export const titled = (name: string) => (name === name.toLowerCase() ? name.charAt(0).toUpperCase() + name.slice(1) : name);

export function windowTitle(s: Shown): Title {
  const here = (name: string, sub: string): Title => ({ name, sub, steps: true, amber: false });
  // Where nothing led here, the line starts with the repository.
  const inRepo = s.steps ? "" : `${s.repo} · `;
  if (s.conflicts) {
    const op = s.conflicts;
    const doing = op.kind === "am" ? "Applying patches" : `${op.noun} in progress`;
    return { name: "Resolve Conflicts", sub: `${doing} · ${op.conflicted} ${op.conflicted === 1 ? "file" : "files"} left`, steps: false, amber: false };
  }
  if (s.screen === "file" && s.file) {
    const { path, line } = s.file;
    const sub = s.steps && line ? `Line ${line}` : `${inRepo}${path}${line ? ` · line ${line}` : ""}`;
    return here(path.slice(path.lastIndexOf("/") + 1), sub);
  }
  if (s.screen === "reviews") return here("Reviews", `${s.github.place ?? s.repo} · ${s.github.open} open`);
  if (s.screen === "pull" && s.pull) {
    const { branch, pr, byNumber } = s.pull;
    const place = s.github.place && !s.steps ? ` · ${s.github.place}` : "";
    return here(pr !== null && byNumber ? `PR #${pr}` : branch, `${pr !== null ? `Pull request #${pr}` : "No pull request yet"}${place}`);
  }
  if (s.screen === "stack") return here("Edit Stack", `Interactive rebase${s.steps ? "" : ` · ${s.repo}`}`);
  if (s.screen === "series" && s.series) return here("Compare Series", `${s.series.branch} · ${s.series.from}`);
  if (s.screen === "compare") return here("Compare", `${s.compare.target} with ${s.compare.base}`);
  if (s.screen === "stashes") return here("Stashes", `${inRepo}on ${s.head ?? "detached HEAD"}`);
  if (s.screen === "oplog") return here("Operation Log", `${inRepo}every step, to undo or restore`);
  const name = `${titled(s.repo)} history`;
  if (s.running) return { name, sub: `${s.running.title}${s.running.percent !== null ? ` · ${s.running.percent}%` : "…"}`, steps: false, amber: false };
  if (s.failed) return { name, sub: `Fetch failed · ${s.failed}`, steps: false, amber: true };
  const state = s.bisect ? (s.bisect.found ? " · Bisect done" : " · Bisecting") : s.detached ? " · detached HEAD" : "";
  return { name, sub: `${s.branches} ${s.branches === 1 ? "branch" : "branches"}${state}`, steps: false, amber: false };
}
