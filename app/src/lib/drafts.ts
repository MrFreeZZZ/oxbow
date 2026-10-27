// The Conflicts screen's draft on disk, apart from the page so it can be tested: the sides
// picked and the text typed in each conflicted file, saved as the user goes, so quitting Oxbow in
// the middle of a merge keeps them. A file's draft comes back by itself only for the same step of
// the same operation and the same versions of the file in the index; any other draft is kept as
// text, to compare with the file as it is now.

import type { Decision } from "./conflicts";

/** A file written as text: what it is now, what it started as, and its stray marker lines. */
export type DraftText = { text: string; start: string; stray: number };

/** What the screen had going for one file. */
export interface DraftFile {
  path: string;
  /** The versions of the file in the index that the decisions were made for. */
  stages: string;
  choices: Decision[];
  text: DraftText | null;
  /** The file as the decisions would write it, open conflicts between markers; empty when it is
   *  written as text, which is then the result, so a long file isn't saved twice. */
  result: string;
}

/** A draft made for other versions of the file, kept to compare with it. */
export interface OldDraft {
  path: string;
  result: string;
  /** When it was last saved, in milliseconds since the epoch. */
  savedAt: number;
}

export interface Draft {
  version: 1;
  /** The operation, its commit, and the commit checked out, as the screen's step key. */
  step: string;
  savedAt: number;
  /** Every file that was conflicted in the step, so resolved ones still count as done. */
  seen: string[];
  files: DraftFile[];
  old: OldDraft[];
}

/** What the screen keeps, by file; a Map, as a file may be called `constructor`. */
export interface DraftState {
  choices: Map<string, Decision[]>;
  texts: Map<string, DraftText>;
  stages: Map<string, string>;
  results: Map<string, string>;
  old: Map<string, OldDraft>;
  seen: string[];
}

/** Something was decided or typed: a file with nothing in it isn't worth a draft. */
export const hasWork = (choices: Decision[] | undefined, text: DraftText | null | undefined) =>
  !!text || !!choices?.some((c) => c !== null);

/** The draft of `state` for the files still conflicted; null when there is nothing to keep. */
export function makeDraft(step: string, state: DraftState, conflicted: string[], savedAt: number): Draft | null {
  const files: DraftFile[] = [];
  const old: OldDraft[] = [];
  for (const path of conflicted) {
    const choices = state.choices.get(path) ?? [];
    const text = state.texts.get(path) ?? null;
    const stages = state.stages.get(path);
    if (stages !== undefined && hasWork(choices, text)) {
      files.push({ path, stages, choices, text, result: text ? "" : (state.results.get(path) ?? "") });
    }
    const was = state.old.get(path);
    if (was) old.push(was);
  }
  if (!files.length && !old.length) return null;
  return { version: 1, step, savedAt, seen: state.seen, files, old };
}

const isDecision = (d: unknown): d is Decision =>
  d === null || d === "left" || d === "right" || d === "both" || (typeof d === "object" && Array.isArray((d as { lines?: unknown }).lines) && (d as { lines: unknown[] }).lines.every((l) => typeof l === "string"));
const isText = (t: unknown): t is DraftText =>
  typeof t === "object" && t !== null && typeof (t as DraftText).text === "string" && typeof (t as DraftText).start === "string" && typeof (t as DraftText).stray === "number";

/** A draft as the backend gives it back; null for none, or for one this version can't read. */
export function readDraft(value: unknown): Draft | null {
  if (typeof value !== "object" || value === null) return null;
  const d = value as Partial<Draft>;
  if (d.version !== 1 || typeof d.step !== "string" || typeof d.savedAt !== "number") return null;
  if (!Array.isArray(d.seen) || !Array.isArray(d.files) || !Array.isArray(d.old)) return null;
  const files = d.files.filter(
    (f): f is DraftFile =>
      typeof f?.path === "string" &&
      typeof f.stages === "string" &&
      typeof f.result === "string" &&
      Array.isArray(f.choices) &&
      f.choices.every(isDecision) &&
      (f.text === null || isText(f.text)),
  );
  const old = d.old.filter((o): o is OldDraft => typeof o?.path === "string" && typeof o.result === "string" && typeof o.savedAt === "number");
  return { version: 1, step: d.step, savedAt: d.savedAt, seen: d.seen.filter((p) => typeof p === "string"), files, old };
}

/** The file as a draft would write it: its text, or else what its decisions make. */
const resultIn = (f: DraftFile) => (f.text ? f.text.text : f.result);

/** What the screen starts the step `step` with from `draft`: its decisions when the draft is of
 *  this step (each still checked against the file's versions when it opens), otherwise only the
 *  results, to compare. */
export function takeDraft(draft: Draft | null, step: string, conflicted: string[]): DraftState {
  const state: DraftState = { choices: new Map(), texts: new Map(), stages: new Map(), results: new Map(), old: new Map(), seen: [...conflicted] };
  if (!draft) return state;
  for (const o of draft.old) state.old.set(o.path, o);
  if (draft.step === step) {
    for (const f of draft.files) {
      state.choices.set(f.path, f.choices);
      if (f.text) state.texts.set(f.path, f.text);
      state.stages.set(f.path, f.stages);
      state.results.set(f.path, resultIn(f));
    }
    state.seen = [...draft.seen, ...conflicted.filter((p) => !draft.seen.includes(p))];
  } else {
    for (const f of draft.files) state.old.set(f.path, { path: f.path, result: resultIn(f), savedAt: draft.savedAt });
  }
  return state;
}
