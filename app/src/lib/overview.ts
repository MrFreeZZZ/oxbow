import type { Row } from "./settingsRows.ts";
import type { OverviewRow, RepoOverview, RecentRepo } from "./types.ts";

/** Each screen/read owns an id. A late answer cannot update a replacement screen or refresh. */
export class OverviewReader {
  #current: string | null = null;
  private read: (id: string) => Promise<OverviewRow[]>;
  private cancel: (id: string) => Promise<void>;
  constructor(read: (id: string) => Promise<OverviewRow[]>, cancel: (id: string) => Promise<void>) {
    this.read = read;
    this.cancel = cancel;
  }

  async refresh(apply: (rows: OverviewRow[] | null, error: string | null) => void): Promise<void> {
    this.stop();
    const id = crypto.randomUUID();
    this.#current = id;
    try {
      const rows = await this.read(id);
      if (this.#current === id) apply(rows, null);
    } catch (error) {
      if (this.#current === id) apply(null, String(error));
    } finally {
      if (this.#current === id) this.#current = null;
    }
  }

  stop() {
    const id = this.#current;
    this.#current = null;
    if (id) void this.cancel(id).catch(() => {});
  }
}

export function trackingNote(summary: RepoOverview): string {
  if (!summary.commit) return "No commits";
  if (!summary.branch) return "Detached HEAD";
  if (!summary.upstream) return "No upstream";
  if (summary.ahead === null || summary.behind === null) return `${summary.upstream} · comparison unavailable`;
  return summary.upstream;
}

/** Selection survives a refresh/reorder and is always one of the visible, exact paths. */
export function selectedOverview(rows: OverviewRow[], path: string | null, current: string | null = null): OverviewRow | null {
  return rows.find(r => r.repository.path === path) ?? rows.find(r => r.repository.path === current) ?? rows[0] ?? null;
}

export function overviewFacts(summary: RepoOverview): Row[] {
  const commits = (count: number | null) => count === null ? "—" : `${count} commit${count === 1 ? "" : "s"}`;
  const rows: Row[] = [
    { id: "changed", label: "Uncommitted files", control: { type: "value", text: String(summary.changed) } },
    { id: "ahead", label: "Ahead of upstream", control: { type: "value", text: commits(summary.ahead) } },
    { id: "behind", label: "Behind local upstream", control: { type: "value", text: commits(summary.behind) } },
  ];
  if (summary.stashes) rows.push({ id: "stashes", label: "Stashes", control: { type: "value", text: String(summary.stashes) } });
  return rows;
}


/** Compact Welcome status, using local tracking information only. */
export function overviewStatus(repo: RecentRepo, summary: RepoOverview | null): { text: string; warn?: boolean } {
  if (repo.missing) {
    const why = repo.gone?.why;
    return { text: why === "disk" ? `Disk “${repo.gone?.disk}” not connected` : why === "trash" ? "In the Trash" : why === "moved" ? "Moved" : why === "noAccess" ? "Can’t read it" : "Folder not found" };
  }
  if (!summary) return { text: "Can’t read it" };
  const parts: string[] = [];
  if (summary.operation) parts.push(summary.operation);
  if (summary.conflicts) parts.push(`${summary.conflicts} conflict${summary.conflicts === 1 ? "" : "s"}`);
  if (summary.ahead) parts.push(`↑${summary.ahead}`);
  if (summary.behind) parts.push(`↓${summary.behind}`);
  if (summary.changed) parts.push(`${summary.changed} changed`);
  if (summary.stashes) parts.push(`${summary.stashes} stash${summary.stashes === 1 ? "" : "es"}`);
  if (!parts.length) parts.push(!summary.commit ? "No commits" : !summary.branch ? "Detached HEAD" : !summary.upstream ? "No upstream" : summary.ahead === null || summary.behind === null ? "Comparison unavailable" : "Up to date with local refs");
  return { text: parts.join(" · "), warn: !!summary.operation || summary.conflicts > 0 };
}
