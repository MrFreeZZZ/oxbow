// GitHub's and GitLab's words for the same things, so one Pull Request screen and its sheets
// serve both: pull request #12 with its checks, or merge request !12 with its pipeline.

export type ForgeKind = "github" | "gitlab";

export interface Words {
  kind: ForgeKind;
  /** "GitHub" or "GitLab". */
  name: string;
  /** What goes before the number: "#" or "!". */
  mark: string;
  /** "pull request" or "merge request". */
  noun: string;
  /** One of what runs on each push: "check" or "job". */
  check: string;
  /** All of them together: "checks" or "the pipeline". */
  checks: string;
  /** The command that does the same: "gh pr" or "glab mr". */
  cli: string;
}

export const GITHUB: Words = { kind: "github", name: "GitHub", mark: "#", noun: "pull request", check: "check", checks: "the checks", cli: "gh pr" };
export const GITLAB: Words = { kind: "gitlab", name: "GitLab", mark: "!", noun: "merge request", check: "job", checks: "the pipeline", cli: "glab mr" };

export const wordsOf = (kind: ForgeKind): Words => (kind === "gitlab" ? GITLAB : GITHUB);
