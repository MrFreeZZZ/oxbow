// Every change to the repository goes through `run`: it shows the confirmation sheet with the
// git commands first (the design's rule, so people learn the commands), then runs the action in
// the same sheet with git's live output. A failure turns the sheet into the way out of it.

import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { oplog, undoRequest } from "./oplog.svelte";
import { prefs } from "./prefs.svelte";
import type { Action, ActionEvent, Failure, GitCommand } from "./types";

/** A piece of the sheet's text: plain words, a branch capsule, a path or sha in mono, or a quoted message. */
export type Part = string | { branch: string; color: number } | { code: string } | { quote: string };

export type Icon =
  | "stage"
  | "unstage"
  | "discard"
  | "commit"
  | "fetch"
  | "pull"
  | "push"
  | "warn"
  | "key"
  | "offline"
  | "hook"
  | "checkout"
  | "branch"
  | "edit"
  | "drop"
  | "merge"
  | "rebase"
  | "cherry"
  | "revert"
  | "undo"
  | "stash"
  | "pop"
  | "reset"
  | "remote"
  | "box"
  | "tag"
  | "ignore"
  | "lines";

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
  /** Orange icon for a step worth a second look, e.g. leaving commits behind. */
  tone?: "warn";
  /** Another way to go, as a new confirmation, e.g. Create Branch First. */
  alt?: { label: string; request: () => Request };
  /** Extra line in the footer, e.g. how to undo a discard. */
  note?: string;
  /** Title while it runs, e.g. "Pushing main to origin…". */
  status?: string;
  /** Short message shown after it succeeded. */
  done?: string;
  /** Inputs above the commands, e.g. the name of a new branch. */
  fields?: Field[];
  /** Checkboxes that change the commands, e.g. the lease of a force push. */
  options?: Choice[];
  /** Why the action button is off, e.g. an invalid branch name. */
  invalid?: string | null;
  /** A picture of the result above the commands, e.g. the commits a merge brings in. */
  preview?: Preview;
  /** Stopping on conflicts is expected: the sheet closes so they can be resolved. */
  resolveConflicts?: boolean;
  /** The way out when the action fails. */
  recover?: (failure: Failure) => Recovery | null;
  /** Offered as Undo on the toast after it succeeded, e.g. putting a dropped stash back. */
  undo?: () => Request;
  action: Action;
}

/** A small graph of what the action makes, and a verdict under it. */
export interface Preview {
  rows: PreviewRow[];
  verdict: { tone: "ok" | "warn" | "info"; parts: Part[] } | null;
}

export interface PreviewRow {
  /** 0 is the checked-out branch's line, 1 the other branch's. */
  lane: 0 | 1;
  color: number;
  summary: string;
  /** A short sha; null for a commit the action makes. */
  sha: string | null;
  /** merge: two colors; base: where the branches split; more: "and N more" in a line. */
  node: "commit" | "new" | "merge" | "base" | "more";
  /** For a merge node: the other branch's color. */
  other?: number;
  /** Shown faded: stays as it is. */
  muted?: boolean;
  /** Rows this one connects down to. */
  links: number[];
}

/** A checkbox of the sheet. */
export interface Choice {
  label: string;
  sub?: string;
  on: boolean;
  toggle: () => Request;
}

/** A row of the sheet with a label: a text box, a choice of chips, or both, and a note under them. */
export interface Field {
  label: string;
  /** `multiline` for a commit message: Return makes a new line, ⌘Return runs the action. */
  text?: { value: string; placeholder?: string; multiline?: boolean; edit: (value: string) => Request };
  /** `off` says why a chip can't be picked. */
  chips?: { label: string; on: boolean; mono?: boolean; off?: string; pick: () => Request | Promise<Request> }[];
  note?: string;
  /** Shown instead of the note while the text can't be used, e.g. a name that is taken. */
  error?: string;
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

/** Actions that throw work away or rewrite history: with "Risky only" these still ask. */
const RISKY = new Set<Action["kind"]>(["discard", "discardHunk", "reset", "stashDrop", "deleteBranch", "deleteRemoteBranch", "deleteTag", "abort", "abortRebase", "restore", "clearOperationLog"]);

function isForcePush(action: Action): boolean {
  return action.kind === "push" && action.force;
}

/** Whether to show the sheet before running, from the Confirmations settings. A sheet that needs
 *  something typed or chosen, or can't run as it is, always shows. */
function shouldAsk(request: Request): boolean {
  if (request.fields?.length || request.invalid) return true;
  const action = request.action;
  if (isForcePush(action) && prefs.get("oxbow.push.confirmForce")) return true;
  if (!prefs.get("oxbow.confirm.enabled")) return false;
  if (prefs.get("oxbow.confirm.scope") === "all") return true;
  return !!request.danger || isForcePush(action) || RISKY.has(action.kind) || (action.kind === "merge" && action.method === "rebase");
}

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
  /** The Undo of the toast, when the action that showed it can be taken back. */
  toastUndo = $state<(() => Request) | null>(null);
  /** Runs a request the way the app does (with a reload after it); set by the app. */
  runner: ((request: Request) => void) | null = null;
  #resolve: ((done: boolean) => void) | null = null;
  /** Goes up with every plan asked for, so a slow answer can't replace a newer one. */
  #planned = 0;
  #editTimer: ReturnType<typeof setTimeout> | undefined;
  #toastTimer: ReturnType<typeof setTimeout> | undefined;

  constructor() {
    listen<ActionEvent>("action-event", (event) => this.#onEvent(event.payload)).catch(() => {});
  }

  /** Ask, then run. Resolves to true once the action ran, false if it was cancelled or failed. */
  async run(request: Request): Promise<boolean> {
    const done = new Promise<boolean>((resolve) => (this.#resolve = resolve));
    await this.#askOrRun(request);
    return done;
  }

  /** Show the sheet, or, when the settings say not to ask, run right away: then the sheet only
   *  shows up while it runs. */
  async #askOrRun(request: Request) {
    if (shouldAsk(request)) await this.#ask(request);
    else {
      this.request = request;
      this.#execute(request.action, request.status ?? request.title, request.done);
    }
  }

  go() {
    if (this.request && this.phase === "ask" && !this.request.invalid) this.#execute(this.request.action, this.request.status ?? this.request.title, this.request.done);
  }

  /** Show another version of the request, after a checkbox or chip changed it. */
  async change(request: Request | Promise<Request>) {
    if (this.phase !== "ask") return;
    try {
      const next = await request;
      if (this.phase === "ask") await this.#ask(next);
    } catch (err) {
      this.#fail({ kind: "other", output: String(err), incoming: [], remoteTip: null, hook: null });
    }
  }

  /** Show the request for the text just typed; its commands follow once typing pauses. */
  edit(request: Request) {
    if (this.phase !== "ask") return;
    this.request = request;
    clearTimeout(this.#editTimer);
    // `request` is the latest text: a newer keystroke clears this timer.
    this.#editTimer = setTimeout(() => {
      if (this.phase === "ask") this.#ask(request);
    }, 150);
  }

  /** Run the recovery's button, e.g. Pull and Push after a rejected push. */
  recover() {
    const button = this.recovery?.button;
    if (button) this.#execute(button.action, button.status, button.done);
  }

  /** Open the recovery's other choice, e.g. Force Push…, as a new confirmation. */
  async alternative() {
    const alt = this.recovery?.alt;
    if (alt) await this.#askOrRun(alt.request());
  }

  stop() {
    if (this.phase === "running") api.stopAction().catch(() => {});
  }

  cancel() {
    if (this.phase !== "running") this.#finish(false);
  }

  async #ask(request: Request) {
    const mine = ++this.#planned;
    this.request = request;
    this.lines = [];
    this.failure = null;
    this.recovery = null;
    try {
      const plan = await api.planAction(request.action);
      if (mine !== this.#planned) return;
      this.commands = plan.commands;
      this.phase = "ask";
    } catch (err) {
      if (mine !== this.#planned) return;
      this.commands = [];
      this.#fail({ kind: "other", output: String(err), incoming: [], remoteTip: null, hook: null });
    }
  }

  async #execute(action: Action, status: string, done?: string) {
    this.phase = "running";
    this.status = status;
    this.lines = [];
    this.progress = null;
    this.failure = null;
    this.recovery = null;
    // Only the request's own action can be undone, not a way out offered after it failed.
    const undo = this.request && action === this.request.action ? (this.request.undo ?? null) : null;
    const started = Math.floor(Date.now() / 1000);
    try {
      await api.performAction(action);
      this.#finish(true);
      if (done) this.#showToast(done, undo ?? (await this.#undoFromLog(action, started)));
    } catch (err) {
      const failure = toFailure(err);
      if (failure.kind === "conflict" && this.request?.resolveConflicts) {
        this.#finish(false);
        this.#showToast("Stopped on conflicts, as expected. Pick what goes into each file.");
        return;
      }
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

  /** Undo of the step the Operation Log just recorded for `action`, when there is one. */
  async #undoFromLog(action: Action, started: number): Promise<(() => Request) | null> {
    if (action.kind === "clearOperationLog") return null;
    await oplog.load();
    const entry = oplog.entries[0];
    return entry && entry.time >= started && !entry.failed ? () => undoRequest(entry) : null;
  }

  /** A short message at the bottom of the window, e.g. after copying a name. */
  say(text: string) {
    this.#showToast(text);
  }

  /** Take back what the toast reports. */
  undo() {
    const undo = this.toastUndo;
    this.toast = null;
    this.toastUndo = null;
    if (undo) this.runner?.(undo());
  }

  #showToast(text: string, undo: (() => Request) | null = null) {
    clearTimeout(this.#toastTimer);
    this.toast = text;
    this.toastUndo = undo;
    // An Undo stays a little longer, so there is time to reach it.
    this.#toastTimer = setTimeout(
      () => {
        this.toast = null;
        this.toastUndo = null;
      },
      undo ? 8000 : 4500,
    );
  }

  #finish(done: boolean) {
    clearTimeout(this.#editTimer);
    this.request = null;
    this.recovery = null;
    this.failure = null;
    this.#resolve?.(done);
    this.#resolve = null;
  }
}

function toFailure(err: unknown): Failure {
  if (err && typeof err === "object" && "kind" in err) return err as Failure;
  return { kind: "other", output: String(err), incoming: [], remoteTip: null, hook: null };
}

function lineKind(text: string, stderr: boolean): TermLine["kind"] {
  if (/^hint:/.test(text)) return "hint";
  if (/^(error|fatal):|^ ! |\[rejected\]|^CONFLICT/.test(text)) return "err";
  if (!stderr) return "out";
  return /^ [*+-] |^ {3}[0-9a-f]+\.\.[0-9a-f]+ |Successfully rebased|up to date/.test(text) ? "ok" : "out";
}

export const confirm = new ConfirmState();
