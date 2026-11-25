import type { RepoNotice, RepoSessionId, RepoTabOpen } from "./api";
import type { RepositoryTabTagId } from "./repositoryTabTags";
import type { History, RepoSummary, TrustFinding } from "./types";

type Answer = "trust" | "restricted";

export interface OpenTab {
  id: RepoSessionId;
  repo: RepoSummary;
  history: History;
}

/** A repository restored from the last session. Its screen opens the first time it is shown. */
export interface PendingTab {
  id: RepoSessionId;
  repo: RepoSummary;
  pending: true;
}

export type ShownTab = OpenTab | PendingTab;

export function isPending(tab: ShownTab): tab is PendingTab {
  return "pending" in tab;
}

interface TabLayout {
  paths: string[];
  active: string | null;
  canonicalPaths?: Record<string, string>;
}

const LAYOUT_KEY = "oxbow.repoTabs.layout";

export function readTabLayout(storage?: Pick<Storage, "getItem">): TabLayout | null {
  try {
    const layout = JSON.parse((storage ?? localStorage).getItem(LAYOUT_KEY) ?? "null");
    if (Array.isArray(layout?.paths) && layout.paths.every((path: unknown) => typeof path === "string" && path)
      && (layout.active === null || layout.paths.includes(layout.active))) return layout;
  } catch { /* An unavailable or invalid layout falls back to the last repository. */ }
  return null;
}

export function saveTabLayout(tabs: readonly ShownTab[], active: RepoSessionId | null, storage?: Pick<Storage, "setItem">) {
  const layout: TabLayout = {
    paths: tabs.map(tab => tab.repo.path),
    active: tabs.find(tab => tab.id === active)?.repo.path ?? null,
  };
  // Keep aliases usable for reopening while restored tabs already know their folder identity.
  const aliases = tabs.filter(({ repo }) => repo.canonicalPath !== repo.path);
  if (aliases.length) layout.canonicalPaths = Object.fromEntries(aliases.map(({ repo }) => [repo.path, repo.canonicalPath]));
  try {
    (storage ?? localStorage).setItem(LAYOUT_KEY, JSON.stringify(layout));
  } catch { /* Storage may be unavailable. */ }
}

export function repoTabActions(tab: ShownTab, count: number, actions: {
  copyPath(path: string): void;
  close(id: RepoSessionId): void;
  closeOthers(id: RepoSessionId): void;
  saveTag(path: string, id: RepositoryTabTagId | null): void;
}) {
  return {
    copy: { label: "Copy Repository Path", run: () => actions.copyPath(tab.repo.path) },
    tag: (id: RepositoryTabTagId | null) => actions.saveTag(tab.repo.canonicalPath, id),
    close: { label: "Close Tab", run: () => actions.close(tab.id) },
    others: { label: "Close Other Tabs", off: count < 2, run: () => actions.closeOthers(tab.id) },
  };
}

interface TabBackend {
  open(path: string, answer?: Answer): Promise<RepoTabOpen>;
  history(id: RepoSessionId): Promise<History>;
  activate(id: RepoSessionId): Promise<void>;
  close(id: RepoSessionId): Promise<void>;
  closeBatch(ids: RepoSessionId[], active: RepoSessionId | null, previous: RepoSessionId | null): Promise<void>;
}

/** Shown when a sheet is already open, so a new repository is not dropped without a word. */
export const DIALOG_IN_THE_WAY = "Close the open dialog first.";

interface WindowHooks {
  show(tabs: readonly ShownTab[], active: RepoSessionId | null, highlighted: RepoSessionId | null): void;
  repoSelected?(): void;
  canSwitch(): boolean;
  /** Why a switch is refused, when the user should hear it. Null stays quiet, as a mode change does. */
  switchProblem?(): string | null;
  beforeClose(id: RepoSessionId): Promise<boolean>;
  whileClosing(run: () => Promise<void>): Promise<void>;
  notice(notice: RepoNotice): void;
  /** Open a restored repository the first time its tab is shown. */
  loadTab?(path: string): Promise<void>;
}

export interface OpenFrom {
  live(): boolean;
  trust(name: string, findings: TrustFinding[]): Promise<Answer | null>;
  reopened(id: RepoSessionId, repo: RepoSummary): void | Promise<void>;
  fail(path: string, error: unknown): Promise<void>;
}

/** Owns this window's backend sessions. A restored tab stays in the strip and opens the first time it is shown. */
export class RepoTabs {
  #tabs: ShownTab[] = [];
  #active: RepoSessionId | null = null;
  /** The restored tab selected while its repository is still opening. */
  #armed: RepoSessionId | null = null;
  /** Bumped when the user picks another tab, so an open already in flight does not take focus. */
  #focusToken = 0;
  #claims = new Map<string, number>();
  #loading = new Map<string, Promise<void>>();
  #nextPending = 0;
  #queue = Promise.resolve();
  #opens = Promise.resolve();
  #closed = new Set<RepoSessionId>();
  #opening = false;
  #early = new Map<RepoSessionId, RepoNotice[]>();
  #disposed = false;
  #enabled = true;

  #backend: TabBackend;
  #hooks: WindowHooks;

  constructor(backend: TabBackend, hooks: WindowHooks) {
    this.#backend = backend;
    this.#hooks = hooks;
  }

  #serialize(run: () => Promise<void>): Promise<void> {
    const next = this.#queue.then(() => { if (!this.#disposed) return run(); });
    this.#queue = next.catch(() => {});
    return next;
  }

  get settled() { return this.#queue; }

  /** Home takes focus while the last repository remains active on the backend. */
  selectHome() {
    this.#focusToken += 1;
    this.#armed = null;
    this.#show();
  }

  setMode(enabled: boolean, closed: readonly RepoSessionId[] = []) {
    this.#enabled = enabled;
    this.#forget(closed);
    if (!enabled) this.#forget(this.#tabs.filter(isPending).map(tab => tab.id));
    if (this.#armed !== null && !this.#exists(this.#armed)) this.#armed = null;
    this.#show();
  }

  #forget(ids: readonly RepoSessionId[]) {
    for (const id of ids) this.#closed.add(id);
    this.#tabs = this.#tabs.filter(tab => !this.#closed.has(tab.id));
    if (this.#active !== null && this.#closed.has(this.#active)) this.#active = null;
    if (this.#armed !== null && this.#closed.has(this.#armed)) this.#armed = null;
  }

  #show() {
    if (!this.#disposed) this.#hooks.show(this.#tabs, this.#active, this.#armed ?? this.#active);
  }

  #exists(id: RepoSessionId) {
    return !this.#closed.has(id) && this.#tabs.some(tab => tab.id === id);
  }

  #pending(id: RepoSessionId): PendingTab | undefined {
    const tab = this.#tabs.find(tab => tab.id === id);
    return tab && isPending(tab) ? tab : undefined;
  }

  #placeholder(path: string, canonicalPath: unknown): PendingTab {
    this.#nextPending -= 1;
    const name = path.split(/[/\\]/).filter(Boolean).pop() ?? path;
    return {
      id: this.#nextPending,
      pending: true,
      repo: { name, path, canonicalPath: typeof canonicalPath === "string" && canonicalPath ? canonicalPath : path, restricted: [], untrusted: null },
    };
  }

  /** The open claims focus only while this token is still the latest choice. */
  #claim(path: string): number {
    const pending = this.#claims.get(path);
    if (pending !== undefined) {
      this.#claims.delete(path);
      return pending;
    }
    this.#focusToken += 1;
    return this.#focusToken;
  }

  #loadPending(path: string): Promise<void> {
    const current = this.#loading.get(path);
    if (current) return current;
    const tab = this.#tabs.find(item => isPending(item) && item.repo.path === path);
    if (!tab || this.#disposed) return Promise.resolve();
    const claim = ++this.#focusToken;
    this.#claims.set(path, claim);
    this.#armed = tab.id;
    this.#show();
    let run!: Promise<void>;
    run = (this.#hooks.loadTab?.(path) ?? Promise.resolve()).finally(() => {
      if (this.#loading.get(path) === run) this.#loading.delete(path);
      if (this.#claims.get(path) === claim) this.#claims.delete(path);
      if (this.#armed === tab.id) {
        this.#armed = null;
        this.#show();
      }
    });
    this.#loading.set(path, run);
    return run;
  }

  async #activate(id: RepoSessionId) {
    const tab = this.#tabs.find(item => item.id === id);
    if (!tab || isPending(tab) || !this.#hooks.canSwitch()) return;
    const claim = ++this.#focusToken;
    this.#armed = null;
    if (this.#active !== id) {
      await this.#backend.activate(id);
      if (!this.#exists(id)) return;
      this.#active = id;
    }
    if (this.#focusToken === claim) this.#hooks.repoSelected?.();
    this.#show();
  }

  activate(id: RepoSessionId) {
    const pending = this.#pending(id);
    if (!pending) return this.#serialize(() => this.#activate(id));
    if (this.#hooks.canSwitch()) return this.#loadPending(pending.repo.path);
    const problem = this.#hooks.switchProblem?.();
    if (problem) return Promise.reject(new Error(problem));
    return Promise.resolve();
  }

  async restore(layout: TabLayout, open: (path: string) => Promise<void>) {
    const active = layout.active && layout.paths.includes(layout.active) ? layout.active : layout.paths.at(-1) ?? null;
    this.#tabs = layout.paths.map(path => this.#placeholder(path, layout.canonicalPaths?.[path]));
    this.#active = null;
    this.#armed = null;
    this.#show();
    if (this.#disposed || !active) return;
    await open(active);
  }

  step(by: 1 | -1) {
    let follow: string | null = null;
    const done = this.#serialize(async () => {
      const tabs = this.#tabs.filter(tab => this.#exists(tab.id));
      const at = tabs.findIndex(tab => tab.id === this.#active);
      if (at < 0 || tabs.length < 2) return;
      const next = tabs[(at + by + tabs.length) % tabs.length];
      if (isPending(next)) {
        if (!this.#hooks.canSwitch()) {
          const problem = this.#hooks.switchProblem?.();
          if (problem) throw new Error(problem);
          return;
        }
        follow = next.repo.path;
      } else await this.#activate(next.id);
    });
    return done.then(() => (follow ? this.#loadPending(follow) : undefined));
  }

  /** `to` is the tab's index after the move. The active repository stays where it is. */
  move(id: RepoSessionId, to: number) {
    return this.#serialize(async () => {
      const from = this.#tabs.findIndex(tab => tab.id === id);
      if (from < 0 || !this.#exists(id) || to < 0 || to >= this.#tabs.length || to === from) return;
      const next = this.#tabs.slice();
      const [tab] = next.splice(from, 1);
      next.splice(to, 0, tab);
      this.#tabs = next;
      this.#show();
    });
  }

  open(path: string, from: OpenFrom) {
    const next = this.#opens.then(() => this.#open(path, from));
    this.#opens = next.catch(() => {});
    return next;
  }

  /** True when the open should stop. A dialog throws, so the new session is not dropped in silence. */
  #switchBlocked(): boolean {
    if (this.#hooks.canSwitch()) return false;
    const problem = this.#hooks.switchProblem?.();
    if (problem) throw new Error(problem);
    return true;
  }

  async #open(path: string, from: OpenFrom) {
    if (this.#disposed || !from.live()) return;
    const claim = this.#claim(path);
    let owned: RepoSessionId | null = null;
    this.#opening = true;
    try {
      if (this.#switchBlocked()) return;
      let result = await this.#backend.open(path);
      if (result.kind === "untrusted") {
        if (!from.live() || this.#disposed) return;
        const answer = await from.trust(result.repo.name, result.repo.untrusted ?? []);
        if (!answer || !from.live() || this.#disposed) return;
        result = await this.#backend.open(path, answer);
      }
      if (result.kind === "otherWindow" || result.kind === "untrusted") return;
      const { session: id, repo } = result;
      if (this.#exists(id)) {
        if (!from.live() || this.#disposed) return;
        await this.#serialize(async () => {
          this.#dropPlaceholder(path, repo.path);
          this.#show();
        });
        if (this.#focusToken === claim && this.#exists(id)) {
          await this.activate(id);
          if (this.#active === id) await from.reopened(id, repo);
        }
        return;
      }
      // Also reclaim an existing backend session if the WebView was reloaded.
      owned = id;
      if (!from.live() || this.#disposed || this.#closed.has(id)) return;
      const history = await this.#backend.history(id);
      await this.#serialize(async () => {
        if (!from.live() || this.#closed.has(id) || this.#switchBlocked()) return;
        const attach = (force = false) => {
          const real: OpenTab = { id, repo, history };
          const at = this.#tabs.findIndex(tab => isPending(tab) && (tab.repo.path === path || tab.repo.path === repo.path));
          if (at >= 0) {
            const next = this.#tabs.slice();
            next[at] = real;
            this.#tabs = next;
          } else this.#tabs = [...this.#tabs, real];
          owned = null;
          if (force || this.#focusToken === claim) {
            this.#active = id;
            this.#armed = null;
            this.#hooks.repoSelected?.();
          } else if (this.#armed !== null && !this.#tabs.some(tab => tab.id === this.#armed)) this.#armed = null;
          this.#show();
          for (const notice of this.#early.get(id) ?? []) this.#hooks.notice(notice);
        };
        if (!this.#enabled && this.#active !== null) {
          if (!await this.#closeBatch([this.#active], id, () => attach(true)) && !this.#exists(id)) {
            const problem = this.#hooks.switchProblem?.();
            if (problem) throw new Error(problem);
          }
        } else if (this.#focusToken === claim) {
          await this.#backend.activate(id);
          if (!this.#disposed && !this.#closed.has(id)) {
            const focused = this.#focusToken === claim;
            attach();
            // The user picked another tab while this one was reading History.
            if (!focused && this.#active !== null && this.#active !== id && !this.#pending(this.#active)) {
              await this.#backend.activate(this.#active);
            }
          }
        } else if (!this.#disposed && !this.#closed.has(id)) attach();
      });
    } catch (error) {
      if (from.live() && !this.#disposed) await from.fail(path, error);
    } finally {
      this.#opening = false;
      this.#early.clear();
      if (owned !== null && !this.#closed.has(owned)) await this.#backend.close(owned);
    }
  }

  #dropPlaceholder(path: string, canonical: string) {
    this.#tabs = this.#tabs.filter(tab => !(isPending(tab) && (tab.repo.path === path || tab.repo.path === canonical)));
    if (this.#armed !== null && !this.#tabs.some(tab => tab.id === this.#armed)) this.#armed = null;
  }

  async #remove(id: RepoSessionId): Promise<string | null> {
    const at = this.#tabs.findIndex(tab => tab.id === id);
    if (at < 0) return null;
    this.#tabs = this.#tabs.filter(tab => tab.id !== id);
    if (this.#armed === id) this.#armed = null;
    if (id !== this.#active) {
      this.#show();
      return null;
    }
    this.#active = null;
    let pending: string | null = null;
    try {
      for (const next of [...this.#tabs.slice(at), ...this.#tabs.slice(0, at).reverse()]) {
        if (!this.#exists(next.id)) continue;
        if (isPending(next)) {
          pending ??= next.repo.path;
          continue;
        }
        try {
          await this.#backend.activate(next.id);
          if (!this.#exists(next.id)) continue;
          this.#active = next.id;
          this.#armed = null;
          pending = null;
          break;
        } catch (error) {
          if (this.#exists(next.id)) throw error;
        }
      }
    } finally {
      this.#show();
    }
    return this.#active === null ? pending : null;
  }

  async #closeBatch(ids: RepoSessionId[], keep: RepoSessionId | null, after?: () => void): Promise<boolean> {
    let closed = false;
    await this.#hooks.whileClosing(async () => {
      for (const id of ids) if (!await this.#hooks.beforeClose(id)) return;
      const real = ids.filter(id => !this.#pending(id));
      if (real.length) {
        const previous = this.#active !== null && !this.#pending(this.#active) ? this.#active : null;
        await this.#backend.closeBatch(real, keep, previous);
      }
      this.#forget(ids);
      if (after) after();
      else {
        this.#active = keep;
        this.#show();
      }
      closed = true;
    });
    return closed;
  }

  async #dropPending(ids: RepoSessionId[]) {
    await this.#hooks.whileClosing(async () => {
      for (const id of ids) if (!await this.#hooks.beforeClose(id)) return;
      this.#forget(ids);
      this.#show();
    });
  }

  close(id: RepoSessionId) {
    let follow: string | null = null;
    const done = this.#serialize(async () => {
      if (!this.#exists(id) || !this.#hooks.canSwitch()) return;
      if (this.#pending(id)) return this.#dropPending([id]);
      const at = this.#tabs.findIndex(tab => tab.id === id);
      const next = this.#tabs.slice(at + 1).find(tab => this.#exists(tab.id))
        ?? this.#tabs.slice(0, at).reverse().find(tab => this.#exists(tab.id));
      if (id === this.#active && next && isPending(next)) {
        let allowed = false;
        await this.#hooks.whileClosing(async () => {
          allowed = await this.#hooks.beforeClose(id);
        });
        if (allowed) follow = next.repo.path;
        return;
      }
      const keep = id === this.#active ? (next?.id ?? null) : this.#active;
      await this.#closeBatch([id], keep);
    });
    return done.then(async () => {
      if (!follow) return;
      await this.#loadPending(follow);
      if (this.#active === id || !this.#exists(id)) return;
      await this.close(id);
    });
  }

  closeOthers(keep: RepoSessionId) {
    let follow: string | null = null;
    const done = this.#serialize(async () => {
      if (!this.#exists(keep) || !this.#hooks.canSwitch()) return;
      const kept = this.#tabs.find(tab => tab.id === keep);
      if (kept && isPending(kept)) return void (follow = kept.repo.path);
      const ids = this.#tabs.filter(tab => tab.id !== keep && this.#exists(tab.id)).map(tab => tab.id);
      if (ids.length) await this.#closeBatch(ids, keep);
    });
    return done.then(async () => {
      if (!follow) return;
      await this.#loadPending(follow);
      const real = this.#tabs.find(tab => !isPending(tab) && tab.repo.path === follow);
      if (real && this.#exists(real.id)) await this.closeOthers(real.id);
    });
  }

  lose(id: RepoSessionId) {
    return this.close(id);
  }

  removeRestored(path: string) {
    return this.#serialize(async () => {
      const removed = this.#tabs.filter(tab => isPending(tab) && tab.repo.canonicalPath === path);
      if (removed.length) {
        this.#forget(removed.map(tab => tab.id));
        this.#show();
      }
    });
  }

  receive(notice: RepoNotice) {
    const id = notice.session;
    if (notice.event === "repo-closed") {
      this.#closed.add(id);
      let follow: string | null = null;
      const done = this.#serialize(async () => {
        follow = await this.#remove(id);
      });
      return done.then(() => (follow ? this.#loadPending(follow) : undefined));
    }
    if (notice.event === "repo-activate-request") return this.activate(id);
    if (this.#exists(id)) this.#hooks.notice(notice);
    else if (this.#opening && !this.#closed.has(id)) this.#early.set(id, [...this.#early.get(id) ?? [], notice]);
  }

  dispose() {
    this.#disposed = true;
  }
}
