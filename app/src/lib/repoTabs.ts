import type { RepoNotice, RepoSessionId, RepoTabOpen } from "./api";
import type { History, RepoSummary, TrustFinding } from "./types";

type Answer = "trust" | "restricted";

export interface OpenTab {
  id: RepoSessionId;
  repo: RepoSummary;
  history: History;
}

export function repoTabActions(tab: OpenTab, count: number, actions: {
  copyPath(path: string): void;
  close(id: RepoSessionId): void;
  closeOthers(id: RepoSessionId): void;
}) {
  return {
    copy: { label: "Copy Repository Path", run: () => actions.copyPath(tab.repo.path) },
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

interface WindowHooks {
  show(tabs: readonly OpenTab[], active: RepoSessionId | null): void;
  canSwitch(): boolean;
  beforeClose(id: RepoSessionId): Promise<boolean>;
  whileClosing(run: () => Promise<void>): Promise<void>;
  notice(notice: RepoNotice): void;
}

export interface OpenFrom {
  live(): boolean;
  trust(name: string, findings: TrustFinding[]): Promise<Answer | null>;
  reopened(id: RepoSessionId, repo: RepoSummary): void | Promise<void>;
  fail(path: string, error: unknown): Promise<void>;
}

/** Owns this window's backend sessions. Only a tab with its first History ready can take focus. */
export class RepoTabs {
  #tabs: OpenTab[] = [];
  #active: RepoSessionId | null = null;
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

  setMode(enabled: boolean, closed: readonly RepoSessionId[] = []) {
    this.#enabled = enabled;
    this.#forget(closed);
    this.#show();
  }

  #forget(ids: readonly RepoSessionId[]) {
    for (const id of ids) this.#closed.add(id);
    this.#tabs = this.#tabs.filter(tab => !this.#closed.has(tab.id));
    if (this.#active !== null && this.#closed.has(this.#active)) this.#active = null;
  }

  #show() {
    if (!this.#disposed) this.#hooks.show(this.#tabs, this.#active);
  }

  #exists(id: RepoSessionId) {
    return !this.#closed.has(id) && this.#tabs.some(tab => tab.id === id);
  }

  async #activate(id: RepoSessionId) {
    if (this.#active === id || !this.#exists(id) || !this.#hooks.canSwitch()) return;
    await this.#backend.activate(id);
    if (!this.#exists(id)) return;
    this.#active = id;
    this.#show();
  }

  activate(id: RepoSessionId) {
    return this.#serialize(() => this.#activate(id));
  }

  step(by: 1 | -1) {
    return this.#serialize(async () => {
      const tabs = this.#tabs.filter(tab => this.#exists(tab.id));
      const at = tabs.findIndex(tab => tab.id === this.#active);
      if (at >= 0 && tabs.length > 1) await this.#activate(tabs[(at + by + tabs.length) % tabs.length].id);
    });
  }

  open(path: string, from: OpenFrom) {
    const next = this.#opens.then(() => this.#open(path, from));
    this.#opens = next.catch(() => {});
    return next;
  }

  async #open(path: string, from: OpenFrom) {
    if (this.#disposed || !from.live() || !this.#hooks.canSwitch()) return;
    let owned: RepoSessionId | null = null;
    this.#opening = true;
    try {
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
        if (from.live() && !this.#disposed) {
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
        if (!from.live() || this.#closed.has(id) || !this.#hooks.canSwitch()) return;
        const attach = () => {
          this.#tabs = [...this.#tabs, { id, repo, history }];
          this.#active = id;
          owned = null;
          this.#show();
          for (const notice of this.#early.get(id) ?? []) this.#hooks.notice(notice);
        };
        if (!this.#enabled && this.#active !== null) {
          await this.#closeBatch([this.#active], id, attach);
        } else {
          await this.#backend.activate(id);
          if (!this.#disposed && !this.#closed.has(id)) attach();
        }
      });
    } catch (error) {
      if (from.live() && !this.#disposed) await from.fail(path, error);
    } finally {
      this.#opening = false;
      this.#early.clear();
      if (owned !== null && !this.#closed.has(owned)) await this.#backend.close(owned);
    }
  }

  async #remove(id: RepoSessionId) {
    const at = this.#tabs.findIndex(tab => tab.id === id);
    if (at < 0) return;
    this.#tabs = this.#tabs.filter(tab => tab.id !== id);
    if (id !== this.#active) return void this.#show();
    this.#active = null;
    try {
      for (const next of [...this.#tabs.slice(at), ...this.#tabs.slice(0, at).reverse()]) {
        if (!this.#exists(next.id)) continue;
        try {
          await this.#backend.activate(next.id);
          if (!this.#exists(next.id)) continue;
          this.#active = next.id;
          break;
        } catch (error) {
          if (this.#exists(next.id)) throw error;
        }
      }
    } finally {
      this.#show();
    }
  }

  async #closeBatch(ids: RepoSessionId[], keep: RepoSessionId | null, after?: () => void) {
    await this.#hooks.whileClosing(async () => {
      for (const id of ids) if (!await this.#hooks.beforeClose(id)) return;
      await this.#backend.closeBatch(ids, keep, this.#active);
      this.#forget(ids);
      if (after) after();
      else {
        this.#active = keep;
        this.#show();
      }
    });
  }

  close(id: RepoSessionId) {
    return this.#serialize(async () => {
      if (!this.#exists(id) || !this.#hooks.canSwitch()) return;
      const at = this.#tabs.findIndex(tab => tab.id === id);
      const next = this.#tabs.slice(at + 1).find(tab => this.#exists(tab.id))
        ?? this.#tabs.slice(0, at).reverse().find(tab => this.#exists(tab.id));
      await this.#closeBatch([id], id === this.#active ? next?.id ?? null : this.#active);
    });
  }

  closeOthers(keep: RepoSessionId) {
    return this.#serialize(async () => {
      if (!this.#exists(keep) || !this.#hooks.canSwitch()) return;
      const ids = this.#tabs.filter(tab => tab.id !== keep && this.#exists(tab.id)).map(tab => tab.id);
      if (ids.length) await this.#closeBatch(ids, keep);
    });
  }

  lose(id: RepoSessionId) {
    return this.close(id);
  }

  receive(notice: RepoNotice) {
    const id = notice.session;
    if (notice.event === "repo-closed") {
      this.#closed.add(id);
      return this.#serialize(() => this.#remove(id));
    }
    if (notice.event === "repo-activate-request") return this.activate(id);
    if (this.#exists(id)) this.#hooks.notice(notice);
    else if (this.#opening && !this.#closed.has(id)) this.#early.set(id, [...this.#early.get(id) ?? [], notice]);
  }

  dispose() {
    this.#disposed = true;
  }
}
