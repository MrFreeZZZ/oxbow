// The open repository's place on GitHub and its pull requests, for the sidebar and the Pull
// Request screen. The list is asked for at most once a minute unless something changed it.

import { api } from "./api";
import type { GitHubRepo, PullSummary } from "./types";

const FRESH_FOR = 60_000;

class GitHubState {
  /** The GitHub repository the open one pushes to; null when it has none there. */
  repo = $state<GitHubRepo | null>(null);
  /** Open and recently closed pull requests, the latest updated first. */
  pulls = $state<PullSummary[]>([]);
  /** Why the list couldn't be loaded, e.g. nobody is signed in. */
  problem = $state<string | null>(null);
  #path = "";
  #loadedAt = 0;
  /** Counts loads: only the latest one's answer is kept, so an answer for the account signed
   *  out of, or for another repository, never comes back. */
  #load = 0;

  /** Load what the repository at `path` has on GitHub; `force` skips the one-minute wait. */
  async load(path: string, force = false) {
    if (path !== this.#path) {
      this.#path = path;
      this.#forget();
    }
    if (!force && Date.now() - this.#loadedAt < FRESH_FOR) return;
    this.#loadedAt = Date.now();
    const load = ++this.#load;
    const repo = await api.githubRepo().catch(() => null);
    if (load !== this.#load) return;
    this.repo = repo;
    if (!repo) {
      this.pulls = [];
      return;
    }
    try {
      const pulls = await api.githubPulls();
      if (load !== this.#load) return;
      this.pulls = pulls;
      this.problem = null;
    } catch (err) {
      if (load === this.#load) this.problem = String(err);
    }
  }

  /** Signed in as someone else, or out: what the old account could see goes at once. */
  accountChanged() {
    this.#forget();
    return this.refresh();
  }

  #forget() {
    this.#load += 1;
    this.repo = null;
    this.pulls = [];
    this.problem = null;
    this.#loadedAt = 0;
  }

  /** Load the list again now, e.g. after a pull request changed. */
  refresh() {
    return this.#path ? this.load(this.#path, true) : Promise.resolve();
  }

  /** The pull request of `branch`: its open one, else the latest one. Forks don't count. */
  pullOf(branch: string): PullSummary | null {
    const own = this.pulls.filter((p) => p.head === branch && (!p.headOwner || p.headOwner === this.repo?.owner));
    return own.find((p) => p.state === "open") ?? own[0] ?? null;
  }

  /** The open pull request whose head is `branch`. */
  openPullOf(branch: string): PullSummary | null {
    const pull = this.pullOf(branch);
    return pull?.state === "open" ? pull : null;
  }
}

export const github = new GitHubState();
