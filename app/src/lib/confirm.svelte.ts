// Every change to the repository goes through `run`: it shows the confirmation sheet with the
// git commands first (the design's rule, so people learn the commands), then runs the action in
// the same sheet with git's live output. A failure turns the sheet into the way out of it.

import { listen } from "@tauri-apps/api/event";
import { api, settings } from "./api";
import type { Action, ActionEvent, Failure, GitCommand } from "./types";

/** A piece of the sheet's text: plain words, a branch capsule, a path or sha in mono, or a quoted message. */
export type Part = string | { branch: string; color: number } | { code: string } | { quote: string };

export type Icon = "stage" | "unstage" | "discard" | "commit" | "fetch" | "pull" | "push" | "warn" | "key" | "offline" | "hook";

export interface Request {
  /** A question, e.g. "Discard changes in 2 files?" */
  title: string;
  /** One or two sentences with the details. */
  body: Part[];
  icon: Icon;
  /** Label of the action button. */
  button: string;
  /** Red button and icon for actions that throw work away. */
  danger?: boolean;
  /** Extra line in the footer, e.g. that discarding can't be undone. */
  note?: string;
  /** Title while it runs, e.g. "Pushing main to origin…". */
  status?: string;
  /** Short message shown after it succeeded. */
  done?: string;
  /** A checkbox that changes the command, e.g. the lease of a force push. */
  option?: { label: string; sub?: string; on: boolean; toggle: () => Request };
  /** The way out when the action fails. */
  recover?: (failure: Failure) => Recovery | null;
  action: Action;
}

/** What the sheet offers after a failure. */
export interface Recovery {
  title: string;
  body: Part[];
  icon: Icon;
  tone: "warn" | "err";
  /** Runs right away: the sheet already shows its commands. */
  button?: { label: string; action: Action; status: string; done?: string; danger?: boolean };
  /** Opens another confirmation, e.g. Force Push… */
  alt?: { label: string; danger?: boolean; request: () => Request };
  note?: string;
  /** Label of the button that closes the sheet. */
  close?: string;
}

export interface TermLine {
  kind: "cmd" | "out" | "err" | "hint" | "ok";
  text: string;
  /** A progress line the next one replaces, like `\r` in a terminal. */
  progress?: boolean;
}

type Phase = "ask" | "running" | "failed";

class ConfirmState {
  request = $state<Request | null>(null);
  commands = $state<GitCommand[]>([]);
  phase = $state<Phase>("ask");
  /** git's output while the action runs and after it failed. */
  lines = $state<TermLine[]>([]);
  status = $state("");
  /** Percent from git's progress lines, or null before any. */
  progress = $state<number | null>(null);
  failure = $state<Failure | null>(null);
  recovery = $state<Recovery | null>(null);
  /** Commands the recovery button will run. */
  recoveryCommands = $state<GitCommand[]>([]);
  toast = $state<string | null>(null);
  #resolve: ((done: boolean) => void) | null = null;
  #toastTimer: ReturnType<typeof setTimeout> | undefined;

  constructor() {
    listen<ActionEvent>("action-event", (event) => this.#onEvent(event.payload)).catch(() => {});
  }

  /** Ask, then run. Resolves to true once the action ran, false if it was cancelled or failed. */
  async run(request: Request): Promise<boolean> {
    const done = new Promise<boolean>((resolve) => (this.#resolve = resolve));
    // With confirmations turned off in settings.json the sheet only shows up while it runs.
    const ask = (await api.getSetting<boolean>(settings.confirmActions).catch(() => null)) ?? true;
    if (ask) await this.#ask(request);
    else {
      this.request = request;
      this.#execute(request.action, request.status ?? request.title, request.done);
    }
    return done;
  }

  go() {
    if (this.request && this.phase === "ask") this.#execute(this.request.action, this.request.status ?? this.request.title, this.request.done);
  }

  /** Flip the request's option, e.g. the lease of a force push. */
  async toggleOption() {
    if (this.request?.option && this.phase === "ask") await this.#ask(this.request.option.toggle());
  }

  /** Run the recovery's button, e.g. Pull and Push after a rejected push. */
  recover() {
    const button = this.recovery?.button;
    if (button) this.#execute(button.action, button.status, button.done);
  }

  /** Open the recovery's other choice, e.g. Force Push…, as a new confirmation. */
  async alternative() {
    const alt = this.recovery?.alt;
    if (alt) await this.#ask(alt.request());
  }

  stop() {
    if (this.phase === "running") api.stopAction().catch(() => {});
  }

  cancel() {
    if (this.phase !== "running") this.#finish(false);
  }

  async #ask(request: Request) {
    this.request = request;
    this.lines = [];
    this.failure = null;
    this.recovery = null;
    try {
      this.commands = (await api.planAction(request.action)).commands;
      this.phase = "ask";
    } catch (err) {
      this.commands = [];
      this.#fail({ kind: "other", output: String(err), incoming: [], remoteTip: null });
    }
  }

  async #execute(action: Action, status: string, done?: string) {
    this.phase = "running";
    this.status = status;
    this.lines = [];
    this.progress = null;
    this.failure = null;
    this.recovery = null;
    try {
      await api.performAction(action);
      this.#finish(true);
      if (done) this.#showToast(done);
    } catch (err) {
      const failure = toFailure(err);
      if (failure.kind === "cancelled") {
        this.#finish(false);
        this.#showToast("Stopped. Nothing else was run.");
        return;
      }
      this.#fail(failure);
    }
  }

  async #fail(failure: Failure) {
    this.failure = failure;
    if (!this.lines.length && failure.output) this.lines = failure.output.split("\n").map((text) => ({ kind: lineKind(text, true), text }));
    this.recovery = this.request?.recover?.(failure) ?? null;
    this.recoveryCommands = [];
    this.phase = "failed";
    const button = this.recovery?.button;
    if (button) this.recoveryCommands = (await api.planAction(button.action).catch(() => ({ commands: [] }))).commands;
  }

  #onEvent(event: ActionEvent) {
    if (this.phase !== "running") return;
    if (event.kind === "command") {
      this.lines.push({ kind: "cmd", text: event.display });
      this.progress = null;
      return;
    }
    const percent = /(\d{1,3})%/.exec(event.text);
    if (percent) this.progress = Math.min(100, Number(percent[1]));
    const line: TermLine = { kind: lineKind(event.text, event.stderr), text: event.text, progress: event.progress };
    const last = this.lines[this.lines.length - 1];
    if (last?.progress) this.lines[this.lines.length - 1] = line;
    else this.lines.push(line);
  }

  #showToast(text: string) {
    clearTimeout(this.#toastTimer);
    this.toast = text;
    this.#toastTimer = setTimeout(() => (this.toast = null), 4500);
  }

  #finish(done: boolean) {
    this.request = null;
    this.recovery = null;
    this.failure = null;
    this.#resolve?.(done);
    this.#resolve = null;
  }
}

function toFailure(err: unknown): Failure {
  if (err && typeof err === "object" && "kind" in err) return err as Failure;
  return { kind: "other", output: String(err), incoming: [], remoteTip: null };
}

function lineKind(text: string, stderr: boolean): TermLine["kind"] {
  if (/^hint:/.test(text)) return "hint";
  if (/^(error|fatal):|^ ! |\[rejected\]|^CONFLICT/.test(text)) return "err";
  if (!stderr) return "out";
  return /^ [*+-] |^ {3}[0-9a-f]+\.\.[0-9a-f]+ |Successfully rebased|up to date/.test(text) ? "ok" : "out";
}

export const confirm = new ConfirmState();
