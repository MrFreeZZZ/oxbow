// The window's repository: opening it (asking first when it would run its own programs), its
// folder gone, trusting it, closing it. Its history loads in historyLoad.svelte.ts.

import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "./api";
import type { Gone, History, RepoSummary, TrustFinding } from "./types";
import { Tickets, type Phase, type Ticket } from "./loads";
import { HistoryLoader } from "./historyLoad.svelte";
import { moveMessage } from "./commitDrafts.svelte";
import { activity } from "./activity.svelte";
import { confirm } from "./confirm.svelte";
import { github } from "./github.svelte";
import { lfs } from "./lfs.svelte";
import { oplog } from "./oplog.svelte";
import { nav } from "./nav.svelte";

type TrustAnswer = "trust" | "restricted" | null;

/** What the window does as its repository comes and goes. */
export interface SessionHooks {
  /** Another repository is about to open: what the window shows of the old one goes. */
  leaving(): void;
  /** The repository opened, its first page of history in. */
  opened(next: History): void;
  /** A reload read something new: what stays selected. */
  reloaded(next: History): void;
  /** No repository any more: the window goes back to Welcome. */
  closed(): void;
}

export class RepoSession {
  repo = $state<RepoSummary | null>(null);
  phase = $state<Phase>("idle");
  /** What went wrong last, shown in the window. */
  error = $state<string | null>(null);
  /** The folder of the repository asked for, or of the one open, isn't there any more: its
   *  screen shows in place of Recent Repositories. */
  gone = $state<Gone | null>(null);
  /** What the last Retry or Find Folder… found, when it didn't work. */
  goneNote = $state<string | null>(null);
  reconnecting = $state(false);
  /** «Do you really want to let it run these programs?», while opening a repository or once it
   *  is open Restricted. */
  trustAsk = $state<{ name: string; findings: TrustFinding[]; opening: boolean; answer: (a: TrustAnswer) => void } | null>(null);

  readonly tickets = new Tickets();
  readonly loader: HistoryLoader;
  #hooks: SessionHooks;
  /** One open at a time: the backend keeps whichever repository was opened last. */
  #opening: Promise<void> = Promise.resolve();

  constructor(hooks: SessionHooks) {
    this.#hooks = hooks;
    this.loader = new HistoryLoader(this, (next) => hooks.reloaded(next));
  }

  /** A repository is opening. */
  get loading(): boolean {
    return this.phase === "loading";
  }

  /** Open the repository at `path`, in place of the one open. */
  load(path: string): Promise<void> {
    // Opening a repository switches the one the backend answers for: answers to anything asked
    // before are dropped from here on.
    this.trustAsk?.answer(null);
    const mine = this.tickets.switch();
    this.#opening = this.#opening.then(() => this.#loadNow(path, mine));
    return this.#opening;
  }

  async #loadNow(path: string, mine: Ticket) {
    if (!this.tickets.current(mine)) return;
    this.phase = "loading";
    this.error = null;
    this.#hooks.leaving();
    oplog.forget();
    lfs.forget();
    activity.reset();
    this.loader.stopPaging();
    this.repo = null;
    this.loader.clear();
    nav.repo = "";
    try {
      await api.closeRepo();
      this.loader.reset();
      let summary = await api.openRepo(path);
      if (!this.tickets.current(mine)) return;
      if (summary.untrusted) {
        const answer = await this.askTrust(summary.name, summary.untrusted, true);
        if (!this.tickets.current(mine) || !answer) return;
        summary = await api.openRepo(path, answer);
      }
      if (!this.tickets.current(mine)) return;
      const next = await this.loader.first();
      if (!this.tickets.current(mine)) return;
      this.gone = null;
      this.goneNote = null;
      this.repo = summary;
      nav.repo = summary.path;
      github.load(summary.path);
      this.loader.opened(next);
      this.#hooks.opened(next);
    } catch (err) {
      if (this.tickets.current(mine)) {
        await api.closeRepo();
        await this.failed(path, err, mine);
      }
    } finally {
      if (this.tickets.current(mine)) this.phase = this.repo ? "ready" : this.error || this.gone ? "failed" : "idle";
    }
  }

  askTrust(name: string, findings: TrustFinding[], opening: boolean): Promise<TrustAnswer> {
    return new Promise((answer) => {
      this.trustAsk = {
        name,
        findings,
        opening,
        answer: (a) => {
          this.trustAsk = null;
          answer(a);
        },
      };
    });
  }

  /** Trust This Repository…: git runs its settings from now on. */
  async trust() {
    if (!this.repo) return;
    const shown = this.repo;
    if ((await this.askTrust(shown.name, shown.restricted, false)) !== "trust") return;
    try {
      await api.trustRepo(shown.path);
    } catch (err) {
      confirm.say(String(err), "warn");
      return;
    }
    this.trusted(shown.path);
    await this.loader.reload();
  }

  /** The repository at `path` was trusted, here or in another window. */
  trusted(path: string) {
    if (this.repo?.path === path) this.repo = { ...this.repo, restricted: [] };
  }

  /** Opening or reading the repository at `path` failed: when its folder isn't there, the
   *  screen of a gone folder says why and what is safe, otherwise the error shows. */
  async failed(path: string, err: unknown, mine: Ticket) {
    const found = await api.goneFolder(path).catch(() => null);
    if (!this.tickets.current(mine)) return;
    if (!found) {
      this.error = String(err);
      return;
    }
    const again = this.gone?.path === found.path;
    if (this.repo) this.close();
    this.gone = found;
    this.goneNote = again ? `Still not there at ${new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}.` : null;
    this.phase = "failed";
  }

  /** Open Main Folder: the gone worktree's commit message and Conflicts draft stay with it. */
  goneMain() {
    if (this.gone?.worktree?.main) this.load(this.gone.worktree.main);
  }

  /** Close: the main window shows Recent Repositories again, another window closes. */
  goneClose() {
    const win = getCurrentWindow();
    if (win.label !== "main") {
      win.close();
      return;
    }
    this.gone = null;
    this.goneNote = null;
    this.error = null;
    this.phase = "idle";
  }

  /** Find Folder… and Locate…: choose where the folder that was at `old` is now, point git at it
   *  there and open it. It is the same folder, so its commit message and Conflicts draft go
   *  along, and the toast says they did. */
  async findFolder(old: string) {
    const name = old.split(/[/\\]/).filter(Boolean).pop() ?? old;
    const near = this.gone?.path === old ? this.gone.found : null;
    const picked = await open({ directory: true, multiple: false, title: `Find ${name}`, defaultPath: near ?? old.replace(/[/\\][^/\\]*[/\\]?$/, "") });
    if (typeof picked !== "string") return;
    this.reconnecting = true;
    this.goneNote = null;
    this.error = null;
    let moved: { path: string; conflictDraft: boolean };
    try {
      moved = await api.reconnectFolder(old, picked);
    } catch (err) {
      if (this.gone?.path === old) this.goneNote = String(err);
      else this.error = String(err);
      return;
    } finally {
      this.reconnecting = false;
    }
    const message = moveMessage(old, moved.path);
    await this.load(moved.path);
    if (this.repo?.path !== moved.path) return;
    const along = [message && "commit message", moved.conflictDraft && "Conflicts draft"].filter(Boolean);
    const newName = moved.path.split(/[/\\]/).filter(Boolean).pop() ?? moved.path;
    confirm.say(`Reconnected ${newName}.${along.length ? ` Its ${along.join(" and ")} came along.` : ""}`, "done");
  }

  /** Back to the Welcome window; the repository stays in Recent Repositories. */
  close() {
    this.trustAsk?.answer(null);
    this.tickets.switch();
    this.#opening = this.#opening.then(() => api.closeRepo()).catch((err) => { this.error = String(err); });
    this.loader.stopPaging();
    activity.reset();
    oplog.forget();
    this.repo = null;
    nav.repo = "";
    lfs.forget();
    this.loader.clear();
    this.error = null;
    this.phase = "idle";
    this.#hooks.closed();
    return this.#opening;
  }
}
