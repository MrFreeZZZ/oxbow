// The open repository's place on GitHub and its pull requests, for the sidebar, Reviews and the
// Pull Request screen; also its projects on GitLab, for Create Merge Request, and their merge
// requests once signed in there. The lists are asked for at most once a minute unless something
// changed them.

import { api } from "./api";
import type { ForgeKind } from "./forge";
import type { GitHubRepo, GitLabProject, PullStatus, PullSummary } from "./types";

const FRESH_FOR = 60_000;

class GitHubState {
  /** The GitHub repository the open one pushes to; null when it has none there. */
  repo = $state<GitHubRepo | null>(null);
  /** The GitLab projects the remotes point at, the default remote's first. */
  gitlab = $state<GitLabProject[]>([]);
  /** The GitLab project whose merge requests are listed: one on a server Oxbow is signed in to. */
  mrProject = $state<GitLabProject | null>(null);
  /** Its merge requests, open and recently closed, the latest updated first; `number` is `!N`. */
  mergeRequests = $state<PullSummary[]>([]);
  /** Why they couldn't be loaded, e.g. the token expired. */
  mrProblem = $state<string | null>(null);
  /** The account signed in to that project's server, by its username. */
  mrMe = $state<string | null>(null);
  /** GitLab's approvals and pipeline of the open merge requests, by number. */
  mrStatuses = $state<Record<number, PullStatus>>({});
  /** Open and recently closed pull requests, the latest updated first. */
  pulls = $state<PullSummary[]>([]);
  /** GitHub's verdict and checks of the open ones, by number. */
  statuses = $state<Record<number, PullStatus>>({});
  /** The account signed in, by its login. */
  me = $state<string | null>(null);
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
    const [repo, gitlab] = await Promise.all([api.githubRepo().catch(() => null), api.gitlabProjects().catch(() => [])]);
    if (load !== this.#load) return;
    this.repo = repo;
    this.gitlab = gitlab;
    if (gitlab.length) this.#loadMergeRequests(load);
    else this.mrProject = null;
    if (!repo) {
      this.pulls = [];
      return;
    }
    try {
      const [pulls, statuses, account] = await Promise.all([
        api.githubPulls(),
        // A verdict and checks are a nicety: the list stays without them.
        api.githubPullStatuses().catch(() => []),
        api.githubAccount().catch(() => null),
      ]);
      if (load !== this.#load) return;
      this.pulls = pulls;
      this.statuses = Object.fromEntries(statuses.map((s) => [s.number, s]));
      this.me = account?.login ?? null;
      this.problem = null;
    } catch (err) {
      if (load === this.#load) this.problem = String(err);
    }
  }

  async #loadMergeRequests(load: number) {
    try {
      const [list, accounts] = await Promise.all([api.gitlabMergeRequests(), api.gitlabAccounts().catch(() => [])]);
      if (load !== this.#load) return;
      this.mrProject = list?.project ?? null;
      this.mrMe = accounts.find((a) => a.host === list?.project.host)?.username ?? null;
      this.mergeRequests = list?.mergeRequests ?? [];
      this.mrProblem = null;
    } catch (err) {
      if (load === this.#load) this.mrProblem = String(err);
      return;
    }
    // Approvals and pipelines are a nicety, as on GitHub: the list stays without them.
    const open = this.mergeRequests.filter((p) => p.state === "open").map((p) => p.number);
    const statuses = open.length ? await api.gitlabStatuses(open).catch(() => []) : [];
    if (load === this.#load) this.mrStatuses = Object.fromEntries(statuses.map((s) => [s.number, s]));
  }

  /** Signed in as someone else, or out: what the old account could see goes at once. */
  accountChanged() {
    this.#forget();
    return this.refresh();
  }

  #forget() {
    this.#load += 1;
    this.repo = null;
    this.gitlab = [];
    this.mrProject = null;
    this.mergeRequests = [];
    this.mrProblem = null;
    this.mrMe = null;
    this.mrStatuses = {};
    this.pulls = [];
    this.statuses = {};
    this.me = null;
    this.problem = null;
    this.#loadedAt = 0;
  }

  /** Load the list again now, e.g. after a pull request changed. */
  refresh() {
    return this.#path ? this.load(this.#path, true) : Promise.resolve();
  }

  /** The pull request of `branch`: its open one, else the latest one. Forks don't count. On
   *  `gitlab`, its merge request. */
  pullOf(branch: string, forge: ForgeKind = "github"): PullSummary | null {
    if (forge === "gitlab") return this.mergeRequestOf(branch);
    const own = this.pulls.filter((p) => p.head === branch && (!p.headOwner || p.headOwner === this.repo?.owner));
    return own.find((p) => p.state === "open") ?? own[0] ?? null;
  }

  /** Pull request `number`, if the list has it; on `gitlab`, merge request `!number`. */
  byNumber(number: number, forge: ForgeKind = "github"): PullSummary | null {
    return (forge === "gitlab" ? this.mergeRequests : this.pulls).find((p) => p.number === number) ?? null;
  }

  /** The pull requests, or on `gitlab` the merge requests. */
  listOf(forge: ForgeKind): PullSummary[] {
    return forge === "gitlab" ? this.mergeRequests : this.pulls;
  }

  /** GitHub's verdict and checks by number, or GitLab's approvals and pipeline. */
  statusesOf(forge: ForgeKind): Record<number, PullStatus> {
    return forge === "gitlab" ? this.mrStatuses : this.statuses;
  }

  /** The account signed in to GitHub, or to the GitLab server of the merge requests. */
  meOf(forge: ForgeKind): string | null {
    return forge === "gitlab" ? this.mrMe : this.me;
  }

  /** Where Reviews and the sidebar look: GitHub when the repository is there, else GitLab
   *  once signed in to its server. */
  get forge(): ForgeKind {
    return !this.repo && this.mrProject ? "gitlab" : "github";
  }

  /** The merge request of `branch` on GitLab: its open one, else the latest one. Forks don't count. */
  mergeRequestOf(branch: string): PullSummary | null {
    const own = this.mergeRequests.filter((p) => p.head === branch && !p.headOwner);
    return own.find((p) => p.state === "open") ?? own[0] ?? null;
  }

  /** The open merge request whose source is `branch`. */
  openMergeRequestOf(branch: string): PullSummary | null {
    const mr = this.mergeRequestOf(branch);
    return mr?.state === "open" ? mr : null;
  }

  /** Open pull requests of others that ask this account for a review, and merge requests on
   *  GitLab that ask its account there. */
  get waiting(): PullSummary[] {
    const asks = (list: PullSummary[], me: string | null) => (me ? list.filter((p) => p.state === "open" && !p.draft && p.author.login !== me && p.requested.includes(me)) : []);
    return [...asks(this.pulls, this.me), ...asks(this.mergeRequests, this.mrMe)];
  }

  /** The open pull request whose head is `branch`; on `gitlab`, its open merge request. */
  openPullOf(branch: string, forge: ForgeKind = "github"): PullSummary | null {
    const pull = this.pullOf(branch, forge);
    return pull?.state === "open" ? pull : null;
  }
}

export const github = new GitHubState();
