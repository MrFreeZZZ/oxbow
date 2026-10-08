// What runs against the remotes in the background, as the toolbar's Activity popover shows it:
// the fetch that runs now with git's progress, a Push or Pull waiting for it, and what ran
// lately. A fetch that fails here never opens a sheet: the Fetch button gets an amber dot, and
// Fix… in Activity opens the way out.

import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { lineKind, type TermLine } from "./confirm.svelte";
import { hostName } from "./remote";
import type { ActionEvent, Failure } from "./types";

export type ActivityKind = "fetch" | "pull" | "push";

export interface ActivityItem {
  id: number;
  kind: ActivityKind;
  title: string;
  sub: string;
  state: "running" | "queued" | "done" | "failed" | "stopped";
  /** Milliseconds since the epoch. */
  time: number;
  /** Started by the timer of Settings › General, not by a click. */
  auto?: boolean;
  /** The remote it fetched, null for every remote. */
  remote?: string | null;
  failure?: Failure;
  /** What git printed, for the Fix… sheet. */
  lines?: TermLine[];
  /** A failure that a later fetch, or Fix…, put right. */
  fixed?: boolean;
}

/** How many finished items Activity keeps. */
const KEEP = 6;

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
  startQueued: ((kind: ActivityKind) => void) | null = null;
  #next = 1;

  constructor() {
    listen<ActionEvent>("fetch-event", (event) => this.#onEvent(event.payload)).catch(() => {});
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
    let ok = false;
    try {
      const { updated } = await api.fetchInBackground(remote);
      ok = true;
      this.#fixProblem();
      this.#finished({
        kind: "fetch",
        title: `Fetched ${label}`,
        sub: updated ? plural(updated, "update") : "Nothing new",
        state: "done",
        auto,
        remote,
      });
    } catch (err) {
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
      this.running = null;
      this.percent = null;
      const waiting = this.queued[0];
      this.queued = [];
      if (waiting) this.startQueued?.(waiting.kind);
    }
    return ok;
  }

  stop() {
    if (this.running) api.stopFetch().catch(() => {});
  }

  /** Push or Pull pressed while the fetch runs: it starts when the fetch is done. */
  queue(kind: ActivityKind, title: string) {
    this.queued = [{ id: this.#next++, kind, title, sub: "Starts when the fetch finishes", state: "queued", time: Date.now() }];
    this.open = true;
  }

  unqueue(id: number) {
    this.queued = this.queued.filter((item) => item.id !== id);
  }

  /** Note something that ran in the sheet, e.g. a push, among the recent items. */
  note(kind: ActivityKind, title: string, sub: string) {
    this.#finished({ kind, title, sub, state: "done" });
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
    const same = last && last.state === "done" && full.state === "done" && last.kind === "fetch" && full.kind === "fetch" && last.title === full.title && last.sub === full.sub;
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
