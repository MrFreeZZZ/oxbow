// The Reviews list, and reviewing a pull request: which pull requests wait for you, and which
// lines of a diff a comment can be on.

import type { PullSummary } from "./types";

export interface ReviewSection {
  title: string;
  pulls: PullSummary[];
  /** These ask for your review: the row says when, and offers Review. */
  asked: boolean;
}

/** The open pull requests waiting for `me`'s review first, then `me`'s own, then the others';
 *  with `closed`, the closed and merged ones instead. Each list keeps GitHub's order, the
 *  latest updated first. */
export function listFor(pulls: PullSummary[], me: string | null, closed: boolean): ReviewSection[] {
  if (closed) return [{ title: "Closed and merged", pulls: pulls.filter((p) => p.state !== "open"), asked: false }];
  const open = pulls.filter((p) => p.state === "open");
  const mine = (p: PullSummary) => !!me && p.author.login === me;
  const asked = (p: PullSummary) => !!me && !mine(p) && !p.draft && p.requested.includes(me);
  return [
    { title: "Waiting for your review", pulls: open.filter(asked), asked: true },
    { title: "Yours", pulls: open.filter(mine), asked: false },
    { title: "Others", pulls: open.filter((p) => !mine(p) && !asked(p)), asked: false },
  ];
}
