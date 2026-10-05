// Every change to the repository goes through `run`: it shows the confirmation sheet with the
// git commands first (the design's rule, so people learn the commands), then runs the action.

import { api, settings } from "./api";
import type { Action, GitCommand } from "./types";

/** A piece of the sheet's text: plain words, a branch capsule, a path or sha in mono, or a quoted message. */
export type Part = string | { branch: string; color: number } | { code: string } | { quote: string };

export type Icon = "stage" | "unstage" | "discard" | "commit";

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
  action: Action;
}

type Phase = "ask" | "running" | "failed";

class ConfirmState {
  request = $state<Request | null>(null);
  commands = $state<GitCommand[]>([]);
  phase = $state<Phase>("ask");
  output = $state("");
  #resolve: ((done: boolean) => void) | null = null;

  /** Ask, then run. Resolves to true once the action ran, false if it was cancelled or failed. */
  async run(request: Request): Promise<boolean> {
    // With confirmations turned off in settings.json the sheet only shows up when the action fails.
    const ask = (await api.getSetting<boolean>(settings.confirmActions).catch(() => null)) ?? true;
    if (!ask) {
      try {
        await api.performAction(request.action);
        return true;
      } catch (err) {
        this.#open(request, [], "failed", String(err));
        return this.#wait();
      }
    }
    let commands: GitCommand[];
    try {
      commands = (await api.planAction(request.action)).commands;
    } catch (err) {
      this.#open(request, [], "failed", String(err));
      return this.#wait();
    }
    this.#open(request, commands, "ask", "");
    return this.#wait();
  }

  async go() {
    if (!this.request) return;
    this.phase = "running";
    try {
      await api.performAction(this.request.action);
      this.#finish(true);
    } catch (err) {
      this.output = String(err);
      this.phase = "failed";
    }
  }

  cancel() {
    if (this.phase !== "running") this.#finish(false);
  }

  #open(request: Request, commands: GitCommand[], phase: Phase, output: string) {
    this.request = request;
    this.commands = commands;
    this.phase = phase;
    this.output = output;
  }

  #wait(): Promise<boolean> {
    return new Promise((resolve) => (this.#resolve = resolve));
  }

  #finish(done: boolean) {
    this.request = null;
    this.#resolve?.(done);
    this.#resolve = null;
  }
}

export const confirm = new ConfirmState();
