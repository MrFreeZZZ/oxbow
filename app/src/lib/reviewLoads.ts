import type { PullList, PullRequest, PullSummary } from "./types";

/** A failed refresh keeps the previous snapshot, with the failure still visible. */
export function keepList(previous: PullSummary[], fresh: PullList): PullSummary[] {
  const unavailable = new Set(fresh.issues.filter((i) => i.state === "unavailable").map((i) => i.section));
  const old = previous.filter((p) => unavailable.has(p.state === "open" ? "Open" : p.state === "merged" ? "Merged" : "Closed") || (p.state === "merged" && unavailable.has("Closed")));
  const items = new Map(old.map((p) => [p.number, p]));
  for (const p of fresh.items) items.set(p.number, p);
  return [...items.values()];
}

/** Keep successful details on Retry only for the same request and head. Issues continue to
 *  block readiness, so cached information is never treated as a fresh provider verdict. */
export function keepDetails(previous: PullRequest | null, fresh: PullRequest): PullRequest {
  if (!previous || previous.number !== fresh.number || previous.headSha !== fresh.headSha) return fresh;
  const unavailable = fresh.loadIssues.filter((i) => i.state === "unavailable").map((i) => i.section);
  if (!unavailable.length) return fresh;
  const result = { ...fresh };
  const kept = previous.timeline.filter((e) => unavailable.includes("Discussions") || (e.kind === "thread" ? unavailable.includes("Comments") : unavailable.includes("Timeline")));
  if (kept.length) result.timeline = [...kept, ...fresh.timeline].sort((a, b) => a.time - b.time);
  if (unavailable.some((s) => ["Timeline", "Approvals", "Reviewers"].includes(s))) result.reviews = previous.reviews;
  if (unavailable.some((s) => ["Checks", "Statuses"].includes(s))) result.checks = [...new Map([...previous.checks, ...fresh.checks].map((c) => [c.name, c])).values()];
  if (unavailable.includes("Commits")) result.commits = previous.commits;
  result.loadIssues = fresh.loadIssues.map((i) => i.state === "unavailable" ? { ...i, message: `${i.message} Previously loaded data may still be shown.` } : i);
  return result;
}
