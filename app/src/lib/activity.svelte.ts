// What Oxbow runs, as the toolbar's Activity popover shows it: the fetch that runs now in the
// background with git's progress, a Push or Pull waiting for it, and under Done what ran lately
// with its git commands, also the actions that ran without asking. A fetch that fails here never
// opens a sheet: the Fetch button gets an amber dot, and Fix… in Activity opens the way out.

import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "./api";
import { lineKind, waitingWords, type Icon, type Ran, type TermLine } from "./confirm.svelte";
import { hostName } from "./remote";
import type { ActionEvent, Failure } from "./types";

export type ActivityKind = "fetch" | "pull" | "push" | "action" | "reload";

export interface ActivityItem {
  id: number;
  kind: ActivityKind;
  title: string;
  sub: string;
  state: "running" | "queued" | "done" | "failed" | "stopped";
  /** Milliseconds since the epoch. */
  time: number;
  /** Started by the timer of Settings › Git › Fetch, not by a click. */
  auto?: boolean;
  /** The remote it fetched, null for every remote. */
  remote?: string | null;
  failure?: Failure;
  /** What git printed, for the Fix… sheet. */
  lines?: TermLine[];
  /** A failure that a later fetch, or Fix…, put right. */
  fixed?: boolean;
  /** The icon of the action's sheet, for an action that isn't a fetch, pull or push. */
  icon?: Icon;
  /** The git commands it ran, as the sheet's terminal block shows them. */
  commands?: string[];
  /** A queued Pull: merge or rebase picked for this one pull. */
  mode?: "merge" | "rebase";
}

/** How many finished items Activity keeps. */
const KEEP = 30;

const PUSHES = new Set(["push", "pullAndPush", "replayAndPush", "pushBranches", "pushTags"]);
const FETCHES = new Set(["fetch", "fetchTags", "unshallow"]);

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

/** What went wrong, in a few words, e.g. "GitHub didn’t accept your SSH key". */
export function failureReason(failure: Failure): string {
  const ssh = failure.ssh;
  if (ssh) {
    const host = hostName(ssh.host);
    if (ssh.problem === "agentEmpty") return `${host} didn’t accept your SSH key`;
    if (ssh.problem === "keyNotOnHost") return `Your SSH key isn’t on ${host}`;
    if (ssh.problem === "noKey") return "No SSH key on this computer";
    if (ssh.problem === "unknownHost") return `ssh doesn’t trust ${ssh.host} yet`;
  }
  if (failure.kind === "auth") return "The remote did not accept your sign-in";
  if (failure.kind === "network") return "Can’t reach the remote";
  const line = failure.output.split("\n").find((l) => /^(fatal|error):/.test(l)) ?? failure.output.split("\n")[0] ?? "";
  return line.replace(/^(fatal|error):\s*/, "") || "Git stopped with an error";
}

/** The window subtitle's short form: "Fetch failed · SSH key". */
export function failureShort(failure: Failure): string {
  if (failure.ssh && failure.ssh.problem !== "works" && failure.ssh.problem !== "other") return "SSH key";
  if (failure.kind === "auth") return "sign-in";
  if (failure.kind === "network") return "offline";
  return "error";
}

/** "Receiving objects 64% · 4.1 MiB/s" from git's `Receiving objects:  64% (1203/1880), 3.20 MiB | 4.10 MiB/s`. */
export function phaseOf(text: string): { phase: string; percent: number | null } | null {
  const match = /^(?:remote: )?([A-Z][a-z]+(?: [a-z]+)*):\s+(\d{1,3})%/.exec(text);
  if (!match) return null;
  const speed = /\|\s*([\d.]+ [KMG]?i?B\/s)/.exec(text)?.[1];
  const percent = Math.min(100, Number(match[2]));
  return { phase: `${match[1]} ${percent}%${speed ? ` · ${speed}` : ""}`, percent };
}

class Activity {
  running = $state<ActivityItem | null>(null);
  /** Percent of the step git is on, or null before git says. */
  percent = $state<number | null>(null);
  lines = $state<TermLine[]>([]);
  queued = $state<ActivityItem[]>([]);
  /** Finished, newest first. */
  recent = $state<ActivityItem[]>([]);
  /** The last fetch failed and nothing put it right yet: the amber dot. */
  problem = $state<ActivityItem | null>(null);
  open = $state(false);
  showOutput = $state(false);
  /** Starts a Push or Pull that waited for the fetch; set by the app. */
  startQueued: ((item: ActivityItem) => void) | null = null;
  #next = 1;

  constructor() {
    // Only what the running fetch prints: one of the repository open before may still be printing.
    // This window's own: each window numbers its fetches.
    getCurrentWindow().listen<{ fetch: number | null; event: ActionEvent }>("fetch-event", ({ payload }) => {
      if (payload.fetch !== null && payload.fetch === this.running?.id) this.#onEvent(payload.event);
    }).catch(() => {});
  }

  /** A new repository: nothing of the old one's applies. */
  reset() {
    if (this.running) api.stopFetch().catch(() => {});
    this.running = null;
    this.queued = [];
    this.recent = [];
    this.problem = null;
    this.open = false;
  }

  /** Fetch `remote` (null: every remote) in the background; `label` names it, e.g. "origin". */
  async fetch(remote: string | null, label: string, auto = false): Promise<boolean> {
    if (this.running) return false;
    this.running = { id: this.#next++, kind: "fetch", title: `Fetching ${label}`, sub: "Connecting…", state: "running", time: Date.now(), auto, remote };
    this.percent = null;
    this.lines = [];
    this.showOutput = false;
    // After reset() a new repository's fetch may be running: this one's answer is the old one's.
    const id = this.running.id;
    const current = () => this.running?.id === id;
    let ok = false;
    try {
      const { updated, gaveWay } = await api.fetchInBackground(remote, id, auto);
      if (!current()) return false;
      // The timer's fetch stopped for an action: the next one fetches again, nothing to tell.
      if (gaveWay) return false;
      ok = true;
      this.#fixProblem();
      this.#finished({
        kind: "fetch",
        title: `Fetched ${label}`,
        sub: updated ? plural(updated, "update") : "Nothing new",
        state: "done",
        auto,
        remote,
        commands: this.#commands(),
      });
    } catch (err) {
      if (!current()) return false;
      const failure = toFailure(err);
      if (failure.kind === "cancelled") {
        this.#finished({ kind: "fetch", title: `Stopped fetching ${label}`, sub: "Nothing changed", state: "stopped", remote });
      } else {
        const item = this.#finished({
          kind: "fetch",
          title: auto ? "Auto-fetch failed" : `Fetching ${label} failed`,
          sub: failureReason(failure),
          state: "failed",
          auto,
          remote,
          failure,
          lines: this.lines.filter((line) => !line.progress),
        });
        if (this.problem) this.recent = this.recent.filter((i) => i.id !== this.problem!.id);
        this.problem = item;
        // A fetch someone asked for says what went wrong; one of the timer's only shows the dot.
        if (!auto) this.open = true;
      }
    } finally {
      if (current()) {
        this.running = null;
        this.percent = null;
        const waiting = this.queued[0];
        this.queued = [];
        if (waiting) this.startQueued?.(waiting);
      }
    }
    return ok;
  }

  stop() {
    if (this.running) api.stopFetch().catch(() => {});
  }

  /** Push or Pull pressed while the fetch runs: it starts when the fetch is done. */
  queue(kind: ActivityKind, title: string, mode?: "merge" | "rebase") {
    this.queued = [{ id: this.#next++, kind, title, sub: "Starts when the fetch finishes", state: "queued", time: Date.now(), mode }];
    this.open = true;
  }

  unqueue(id: number) {
    this.queued = this.queued.filter((item) => item.id !== id);
  }

  /** Note an action that ran, in the sheet or without asking, under Done with its commands; one
   *  that went over several things and failed on some, with those. */
  ran(ran: Ran) {
    const kind = PUSHES.has(ran.action.kind) ? "push" : ran.action.kind === "pull" ? "pull" : FETCHES.has(ran.action.kind) ? "fetch" : "action";
    this.#finished({ kind, title: ran.title, sub: ran.failed ?? "", state: ran.failed ? "failed" : "done", icon: ran.icon, commands: ran.commands });
  }

  /** Reload read the repository again: what changed, and the git commands it ran. */
  reloaded(sub: string, commands: string[]) {
    this.#finished({ kind: "reload", title: "Reloaded", sub, state: "done", commands });
  }

  /** A change outside Oxbow (a terminal, an editor) or in another window moved branches, HEAD or
   *  the stashes, or staged files: what the reload after it found. */
  outside(sub: string, commands: string[], window: string | null) {
    this.#finished({ kind: "reload", title: window ? `Changed in the ${window} window` : "Changed outside Oxbow", sub, state: "done", commands });
  }

  /** The commands the running fetch ran. */
  #commands(): string[] {
    return this.lines.filter((line) => line.kind === "cmd").map((line) => line.text);
  }

  /** Fix… put the failure right, e.g. the key is in the agent and the fetch went through. */
  fixed(label: string) {
    this.#fixProblem();
    this.#finished({ kind: "fetch", title: `Fetched ${label}`, sub: "After the fix", state: "done" });
  }

  #fixProblem() {
    const problem = this.problem;
    if (!problem) return;
    this.problem = null;
    this.recent = this.recent.map((item) =>
      item.id === problem.id ? { ...item, title: item.auto ? "Auto-fetch failed" : "Fetch failed", sub: `${item.sub} · fixed`, fixed: true } : item,
    );
  }

  #finished(item: Omit<ActivityItem, "id" | "time">): ActivityItem {
    const full: ActivityItem = { ...item, id: this.#next++, time: Date.now() };
    const last = this.recent[0];
    // The timer fetches every few minutes: "Nothing new" again just moves the time on.
    // So does Reload pressed again with nothing new.
    const same = last && last.state === "done" && full.state === "done" && last.kind === full.kind && (full.kind === "fetch" || full.kind === "reload") && last.title === full.title && last.sub === full.sub;
    this.recent = [full, ...(same ? this.recent.slice(1) : this.recent)].slice(0, KEEP);
    return full;
  }

  #onEvent(event: ActionEvent) {
    const running = this.running;
    if (!running) return;
    if (event.kind === "command") {
      this.lines.push({ kind: "cmd", text: event.display });
      return;
    }
    // A fetch moves branches on remotes, which Undo leaves alone: its log step isn't missed.
    // It goes over no parts either.
    if (event.kind === "notLogged" || event.kind === "parts" || event.kind === "part" || event.kind === "partProgress") return;
    if (event.kind === "waiting") {
      this.running = { ...running, sub: waitingWords(event) };
      return;
    }
    const phase = phaseOf(event.text);
    if (phase) {
      this.percent = phase.percent;
      this.running = { ...running, sub: phase.phase };
    }
    const line: TermLine = { kind: lineKind(event.text, event.stderr), text: event.text, progress: event.progress };
    const last = this.lines[this.lines.length - 1];
    if (last?.progress) this.lines[this.lines.length - 1] = line;
    else this.lines.push(line);
    if (this.lines.length > 200) this.lines.splice(1, this.lines.length - 200);
  }
}

function toFailure(err: unknown): Failure {
  if (err && typeof err === "object" && "kind" in err) return err as Failure;
  return { kind: "other", output: String(err), incoming: [], remoteTip: null, hook: null };
}

export const activity = new Activity();
