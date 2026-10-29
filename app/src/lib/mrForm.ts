// The form that opens a merge request on GitLab, as plain logic: the template it starts from,
// how it squashes, the labels and people the boxes suggest, and the `glab` command that does the
// same as the form.

import type { GitLabUser } from "./types";

export interface Template {
  name: string;
  text: string;
}

/** The template GitLab fills a new merge request with: `Default`, whatever its case. */
export function defaultTemplate(templates: Template[]): Template | null {
  return templates.find((t) => t.name.toLowerCase() === "default") ?? null;
}

/** Whether a new merge request starts squashed, by the project's `squash_option`. */
export const squashStarts = (option: string) => option === "always" || option === "default_on";

/** Whether the project leaves squashing to each merge request. */
export const squashChosen = (option: string) => option === "default_on" || option === "default_off";

/** Labels whose name has `text` in it, the not chosen ones, at most `most`; names that start
 *  with it first. */
export function matchLabels<L extends { name: string }>(labels: L[], chosen: string[], text: string, most = 8): L[] {
  const t = text.trim().toLowerCase();
  const free = labels.filter((l) => !chosen.includes(l.name) && l.name.toLowerCase().includes(t));
  const starts = (l: L) => (l.name.toLowerCase().startsWith(t) ? 0 : 1);
  return [...free].sort((a, b) => starts(a) - starts(b)).slice(0, most);
}

/** The one of `people` the typed text names: by username (with or without `@`), else the only
 *  one found. */
export function exactPerson(people: GitLabUser[], typed: string): GitLabUser | null {
  const name = typed.trim().replace(/^@/, "").toLowerCase();
  if (!name) return null;
  return people.find((p) => p.username.toLowerCase() === name) ?? (people.length === 1 ? people[0] : null);
}

/** What the form opens a merge request with, for `glab mr create`. */
export interface GlabCreate {
  branch: string;
  base: string;
  draft: boolean;
  reviewers: string[];
  assignee: string | null;
  labels: string[];
  removeSourceBranch: boolean;
  squash: boolean;
}

const word = (text: string) => (/^[\w.:/-]+$/.test(text) ? text : `'${text.replace(/'/g, "'\\''")}'`);

/** `glab mr create` with the flags of what the form asks for. */
export function glabCreate(c: GlabCreate): string {
  const flags = [
    `--source-branch ${word(c.branch)}`,
    `--target-branch ${word(c.base)}`,
    c.draft ? "--draft" : "",
    ...c.reviewers.map((r) => `--reviewer ${r}`),
    c.assignee ? `--assignee ${c.assignee}` : "",
    ...c.labels.map((l) => `--label ${word(l)}`),
    c.removeSourceBranch ? "--remove-source-branch" : "",
    c.squash ? "--squash-before-merge" : "",
  ];
  return `glab mr create ${flags.filter(Boolean).join(" ")}`;
}
