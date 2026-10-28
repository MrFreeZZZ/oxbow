// GitLab's side of Create Merge Request, as plain text: the link GitLab prints after the push,
// and its New merge request page for when there is nothing to push. Also when a signed-in
// account's token runs out, for Settings.

import type { GitLabProject } from "./types";

/** The merge request GitLab printed the link of after the push: `!12` and its page. */
export function mergeRequestLink(output: string, project: GitLabProject): { url: string; number: number } | null {
  const start = `${project.webUrl}/-/merge_requests/`;
  for (const word of output.split(/\s+/)) {
    if (!word.toLowerCase().startsWith(start.toLowerCase())) continue;
    const number = /^(\d+)\/?$/.exec(word.slice(start.length))?.[1];
    if (number) return { url: start + number, number: Number(number) };
  }
  return null;
}

/** GitLab's New merge request page with the branches and the title filled in. */
export function newMergeRequestUrl(project: GitLabProject, source: string, target: string, title: string): string {
  const param = (name: string, value: string) => `${encodeURIComponent(`merge_request[${name}]`)}=${encodeURIComponent(value)}`;
  const params = [param("source_branch", source), param("target_branch", target)];
  if (title.trim()) params.push(param("title", title.trim()));
  return `${project.webUrl}/-/merge_requests/new?${params.join("&")}`;
}

/** The newest commit's summary says best where the branch ended up. */
export function mergeRequestTitle(summary: string | null | undefined, branch: string): string {
  return summary?.trim() || branch;
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** When a token stops working: GitLab ends it at the start of `expiresAt` (UTC). `soon` is
 *  within 14 days. */
export function tokenExpiry(expiresAt: string | null, now = new Date()): { text: string; soon: boolean; expired: boolean } | null {
  const day = /^(\d{4})-(\d{2})-(\d{2})$/.exec(expiresAt ?? "");
  if (!day) return null;
  const end = Date.UTC(Number(day[1]), Number(day[2]) - 1, Number(day[3]));
  const left = (end - now.getTime()) / 86_400_000;
  const date = `${Number(day[3])} ${MONTHS[Number(day[2]) - 1]} ${day[1]}`;
  if (left <= 0) return { text: `token expired on ${date}`, soon: true, expired: true };
  return { text: `token until ${date}`, soon: left < 14, expired: false };
}
