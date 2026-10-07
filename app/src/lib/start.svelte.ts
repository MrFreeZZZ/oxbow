// Starting work on a repository: the Welcome window's recent repositories and environment, and
// the Clone and New Repository sheets, which the repository's own window can open too.

import { api } from "./api";
import type { RecentRepo, RepoGlance, WelcomeInfo } from "./types";

export type Sheet = { kind: "clone"; url: string } | { kind: "new"; folder: string | null };

class Start {
  info = $state<WelcomeInfo | null>(null);
  recent = $state<RecentRepo[] | null>(null);
  /** Where each recent repository stands, by path, once looked at; `null` when that failed. */
  glances = $state<Record<string, RepoGlance | null>>({});
  sheet = $state<Sheet | null>(null);

  loadInfo() {
    return api.welcomeInfo().then(
      (info) => (this.info = info),
      () => {},
    );
  }

  /** Read the list; each repository is then looked at in the background, a few at a time. */
  async loadRecent() {
    const recent = await api.recentRepos().catch(() => [] as RecentRepo[]);
    this.recent = recent;
    const queue = recent.filter((r) => !r.missing).map((r) => r.path);
    const worker = async () => {
      for (let path = queue.shift(); path; path = queue.shift()) {
        const glance = await api.repoGlance(path).catch(() => null);
        this.glances = { ...this.glances, [path]: glance };
      }
    };
    void Promise.all([worker(), worker(), worker()]);
  }

  clone(url = "") {
    this.sheet = { kind: "clone", url };
  }

  newRepo(folder: string | null = null) {
    this.sheet = { kind: "new", folder };
  }
}

export const start = new Start();

/** The folder name a clone of `url` gets, as `git clone` would pick it. */
export function nameFromUrl(url: string): string {
  const trimmed = url.trim().replace(/[/\\]+$/, "");
  const last = trimmed.split(/[/\\:]/).pop() ?? "";
  return last.replace(/\.git$/i, "").replace(/^\.+/, "");
}

/** Looks like something `git clone` takes: a URL, `user@host:path`, or a path. */
export function looksLikeUrl(text: string): boolean {
  const t = text.trim();
  return /^(https?|ssh|git|file):\/\//i.test(t) || /^[\w.-]+@[\w.-]+:\S+/.test(t);
}

/** `parent` and `name` joined with the system's separator. */
export function join(parent: string, name: string): string {
  const sep = parent.includes("\\") && !parent.includes("/") ? "\\" : "/";
  return parent.endsWith(sep) ? parent + name : parent + sep + name;
}
