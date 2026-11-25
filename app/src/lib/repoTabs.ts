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
  publish(id: RepoSessionId): Promise<void>;
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
  restored: OpenFrom;
}

export interface OpenFrom {
  live(): boolean;
  trust(name: string, findings: TrustFinding[]): Promise<Answer | null>;
  reopened(id: RepoSessionId, repo: RepoSummary): void | Promise<void>;
  fail(path: string, error: unknown): Promise<void>;
}

interface PendingOpen {
  claim: number;
  done: Promise<void>;
}

/** Owns this window's backend sessions. A restored tab stays in the strip and opens the first time it is shown. */
export class RepoTabs {
  #tabs: ShownTab[] = [];
  #active: RepoSessionId | null = null;
  /** The latest tab selection while its open or activation is in flight. */
  #armed: RepoSessionId | null = null;
  /** Bumped when the user picks another tab, so an open already in flight does not take focus. */
  #focusToken = 0;
  #loading = new Map<RepoSessionId, PendingOpen>();
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

  #pick(id: RepoSessionId | null) {
    const claim = ++this.#focusToken;
    this.#armed = id;
    this.#show();
    return claim;
  }

  #clearPick(claim: number) {
    if (claim !== this.#focusToken || this.#armed === null) return;
    this.#armed = null;
    this.#show();
  }

  #loadPending(tab: PendingTab, claim: number): Promise<void> {
    const current = this.#loading.get(tab.id);
    if (current) {
      current.claim = claim;
      return current.done;
    }
    const request: PendingOpen = {
      claim,
      done: this.#enqueueOpen(tab.repo.path, this.#hooks.restored, () => request.claim, tab.id).finally(() => {
        this.#loading.delete(tab.id);
        this.#clearPick(request.claim);
      }),
    };
    this.#loading.set(tab.id, request);
    return request.done;
  }

  async #activate(id: RepoSessionId, claim: number) {
    const tab = this.#tabs.find(item => item.id === id);
    if (claim !== this.#focusToken || !tab || isPending(tab) || !this.#hooks.canSwitch()) return;
    if (this.#active !== id) {
      await this.#backend.activate(id);
      if (!this.#exists(id)) return;
      if (claim !== this.#focusToken && this.#active !== null && this.#exists(this.#active)) {
        await this.#backend.activate(this.#active);
        return;
      }
      this.#active = id;
    }
    if (this.#focusToken === claim) this.#hooks.repoSelected?.();
    this.#show();
  }

  async activate(id: RepoSessionId) {
    if (!this.#exists(id) || this.#switchBlocked()) return;
    const claim = this.#pick(id);
    const pending = this.#pending(id);
    try {
      if (pending) await this.#loadPending(pending, claim);
      else await this.#serialize(() => this.#activate(id, claim));
    } finally {
      this.#clearPick(claim);
    }
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

  step(by: 1 | -1, fromHome = false) {
    const tabs = this.#tabs.filter(tab => this.#exists(tab.id));
    const at = tabs.findIndex(tab => tab.id === (this.#armed ?? (fromHome ? null : this.#active)));
    const next = at < 0 ? tabs.at(by === 1 ? 0 : -1) : tabs[(at + by + tabs.length) % tabs.length];
    return next ? this.activate(next.id) : Promise.resolve();
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
    const claim = this.#pick(null);
    return this.#enqueueOpen(path, from, () => claim);
  }

  #enqueueOpen(path: string, from: OpenFrom, claim: () => number, pending?: RepoSessionId) {
    const next = this.#opens.then(() => this.#open(path, from, claim, pending));
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

  async #open(path: string, from: OpenFrom, claim: () => number, pending?: RepoSessionId) {
    const live = () => !this.#disposed && from.live() && (pending === undefined || this.#exists(pending));
    // Open Recent may already be reading the same folder when its restored tab is picked.
    const placeholder = pending ?? this.#tabs.find(tab => isPending(tab) && tab.repo.path === path)?.id;
    const focused = () => this.#focusToken === claim() || (pending === undefined && placeholder !== undefined &&
      this.#loading.get(placeholder)?.claim === this.#focusToken);
    if (!live()) return;
    let owned: RepoSessionId | null = null;
    this.#opening = true;
    try {
      if (this.#switchBlocked()) return;
      let result = await this.#backend.open(path);
      if (result.kind === "untrusted") {
        const { name, untrusted } = result.repo;
        let asking: Promise<Answer | null> | undefined;
        await this.#serialize(async () => {
          if (live() && focused() && !this.#switchBlocked()) asking = from.trust(name, untrusted ?? []);
        });
        const answer = await asking;
        if (!answer || !live()) return;
        result = await this.#backend.open(path, answer);
      }
      if (result.kind === "otherWindow" || result.kind === "untrusted") return;
      const { session: id, repo } = result;
      if (this.#exists(id)) {
        let reopened = false;
        await this.#serialize(async () => {
          if (!live()) return;
          if (focused() && this.#exists(id)) {
            await this.#activate(id, claim());
            reopened = focused() && this.#active === id;
          }
          this.#dropPlaceholder(path, repo.path);
          this.#show();
        });
        if (reopened) await from.reopened(id, repo);
        return;
      }
      // Also reclaim an existing backend session if the WebView was reloaded.
      owned = id;
      if (!live() || this.#closed.has(id)) return;
      const history = await this.#backend.history(id);
      await this.#serialize(async () => {
        if (!live() || this.#closed.has(id) || this.#switchBlocked()) return;
        const attach = (force = false) => {
          const real: OpenTab = { id, repo, history };
          const at = this.#tabs.findIndex(tab => isPending(tab) && (tab.repo.path === path || tab.repo.path === repo.path));
          if (at >= 0) {
            const next = this.#tabs.slice();
            next[at] = real;
            this.#tabs = next;
          } else this.#tabs = [...this.#tabs, real];
          owned = null;
          if (force || focused()) {
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
          return;
        }
        if (this.#enabled && !focused()) await this.#backend.publish(id);
        if (!live() || this.#closed.has(id)) return;
        if (focused()) {
          await this.#backend.activate(id);
          if (live() && !this.#closed.has(id)) {
            // Publish the tab before restoring focus so a newer pick can queue its activation.
            const previous = !focused() && this.#active !== null && this.#exists(this.#active) ? this.#active : null;
            if (!focused() && previous === null) this.#active = id;
            attach();
            if (previous !== null) await this.#backend.activate(previous);
          }
        } else attach();
      });
    } catch (error) {
      if (live()) {
        await this.#queue;
        if (live()) await from.fail(path, error);
      }
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
    const claim = this.#focusToken;
    let follow: PendingTab | undefined;
    const done = this.#serialize(async () => {
      if (!this.#exists(id) || !this.#hooks.canSwitch()) return;
      if (this.#pending(id)) return this.#dropPending([id]);
      const at = this.#tabs.findIndex(tab => tab.id === id);
      const neighbors = [...this.#tabs.slice(at + 1), ...this.#tabs.slice(0, at).reverse()]
        .filter(tab => this.#exists(tab.id));
      const next = neighbors[0];
      const keep = id === this.#active ? neighbors.find(tab => !isPending(tab))?.id ?? null : this.#active;
      const closingActive = id === this.#active;
      if (await this.#closeBatch([id], keep) && closingActive && next && isPending(next) && this.#armed === null) {
        follow = next;
      }
    });
    return done.then(() => {
      if (follow && claim === this.#focusToken) return this.activate(follow.id);
    });
  }

  closeOthers(keep: RepoSessionId) {
    const close = () => {
      const ids = this.#tabs.filter(tab => tab.id !== keep && this.#exists(tab.id)).map(tab => tab.id);
      return ids.length ? this.#closeBatch(ids, keep) : Promise.resolve(false);
    };
    const pending = this.#pending(keep);
    if (!pending) return this.#serialize(async () => {
      if (this.#exists(keep) && this.#hooks.canSwitch()) await close();
    });
    if (!this.#hooks.canSwitch()) return Promise.resolve();
    const claim = this.#pick(keep);
    return this.#loadPending(pending, claim).then(() => this.#serialize(async () => {
      if (claim !== this.#focusToken || !this.#hooks.canSwitch()) return;
      const real = this.#tabs.find(tab => !isPending(tab) &&
        (tab.repo.path === pending.repo.path || tab.repo.canonicalPath === pending.repo.canonicalPath));
      if (!real || !this.#exists(real.id)) return;
      keep = real.id;
      await close();
    }));
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
      const claim = this.#focusToken;
      const done = this.#serialize(async () => {
        follow = await this.#remove(id);
      });
      return done.then(() => {
        const tab = this.#tabs.find(tab => isPending(tab) && tab.repo.path === follow);
        if (tab && this.#armed === null && claim === this.#focusToken) return this.activate(tab.id);
      });
    }
    if (notice.event === "repo-activate-request") return this.activate(id);
    if (this.#exists(id)) this.#hooks.notice(notice);
    else if (this.#opening && !this.#closed.has(id)) this.#early.set(id, [...this.#early.get(id) ?? [], notice]);
  }

  dispose() {
    this.#disposed = true;
  }
}
