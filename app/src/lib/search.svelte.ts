// History search: what is typed in the toolbar field, the filters of the bar under it, and the
// commits that matched. The search itself is `git log`, run by the backend; a SHA is looked up
// among the loaded commits.

import { api } from "./api";
import type { HistoryRow, SearchMode, SearchResult } from "./types";

export type Mode = SearchMode | "sha";
export type Since = "any" | "today" | "week" | "month";

export const MODES: { mode: Mode; label: string; menu: string; sub: string; flag: string; prefix: string }[] = [
  { mode: "message", label: "Message", menu: "Messages", sub: "commit title and description", flag: "--grep", prefix: "msg:" },
  { mode: "code", label: "Code", menu: "Changed code", sub: "commits that add or remove this text", flag: "-S", prefix: "code:" },
  { mode: "author", label: "Author", menu: "Authors", sub: "name or email", flag: "--author", prefix: "author:" },
  { mode: "file", label: "File", menu: "File paths", sub: "commits that touch a matching file", flag: "-- path", prefix: "path:" },
  { mode: "sha", label: "SHA", menu: "Commit id", sub: "the start of a SHA", flag: "show", prefix: "sha:" },
];

export const SINCE: { since: Since; label: string; git: string | null }[] = [
  { since: "any", label: "Any time", git: null },
  { since: "today", label: "Today", git: "midnight" },
  { since: "week", label: "Last 7 days", git: "1.week.ago" },
  { since: "month", label: "Last 30 days", git: "1.month.ago" },
];

const PREFIXES: Record<string, Mode> = { msg: "message", message: "message", code: "code", author: "author", path: "file", file: "file", sha: "sha" };
const RECENT_KEY = "oxbow.search.recent";
const HEX = /^[0-9a-f]{4,40}$/i;

export interface Recent {
  mode: Mode;
  text: string;
}

function loadRecent(): Recent[] {
  try {
    const list = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    if (!Array.isArray(list)) return [];
    return list.filter((r) => r && typeof r.text === "string" && MODES.some((m) => m.mode === r.mode)).slice(0, 6);
  } catch {
    return [];
  }
}

class Search {
  /** What is in the field, a `code:` style prefix included. */
  input = $state("");
  /** The mode picked with the token; a prefix in the field wins over it. */
  picked = $state<Mode>("message");
  branch = $state<string | null>(null);
  since = $state<Since>("any");
  author = $state<string | null>(null);
  /** Only Matches: a flat list of the matching commits, without the graph. */
  only = $state(false);
  result = $state<SearchResult | null>(null);
  /** The query and filters the result is for, as `key` read then. */
  resultKey = $state("");
  busy = $state(false);
  error = $state<string | null>(null);
  recent = $state<Recent[]>(loadRecent());
  /** Commits each mode would find for the text, for the mode menu; null while counting. */
  counts = $state<Partial<Record<Mode, number | null>>>({});
  #generation = 0;

  /** The mode and text, read from a prefix like `code:` when there is one. */
  get parsed(): { mode: Mode; text: string } {
    const m = /^(\w+):\s*(.*)$/s.exec(this.input);
    const mode = m && PREFIXES[m[1].toLowerCase()];
    return mode ? { mode, text: m![2].trim() } : { mode: this.picked, text: this.input.trim() };
  }

  /** The query and filters, to tell one search from another. */
  get key(): string {
    return JSON.stringify([this.parsed, this.branch, this.since, this.author]);
  }

  get active(): boolean {
    return this.parsed.text.length > 0;
  }

  /** Whether the SHA mode makes sense for the text. */
  get looksLikeSha(): boolean {
    return HEX.test(this.parsed.text);
  }

  clear() {
    this.input = "";
    this.result = null;
    this.error = null;
    this.busy = false;
    this.author = null;
    this.branch = null;
    this.since = "any";
    this.#generation++;
  }

  /** A typed `code:` style prefix turns into the mode token. */
  takePrefix() {
    const m = /^(\w+):\s*(.*)$/s.exec(this.input);
    const mode = m && PREFIXES[m[1].toLowerCase()];
    if (mode) {
      this.picked = mode;
      this.input = m![2];
    }
  }

  pick(mode: Mode) {
    this.picked = mode;
    // A prefix would override the token, so it goes.
    const m = /^(\w+):\s*(.*)$/s.exec(this.input);
    if (m && PREFIXES[m[1].toLowerCase()]) this.input = m[2];
  }

  /** Back to a recent search. */
  recall(entry: Recent) {
    this.picked = entry.mode;
    this.input = entry.text;
  }

  remember() {
    const { mode, text } = this.parsed;
    if (!text) return;
    const entry = { mode, text };
    this.recent = [entry, ...this.recent.filter((r) => r.mode !== mode || r.text !== text)].slice(0, 6);
    try {
      localStorage.setItem(RECENT_KEY, JSON.stringify(this.recent));
    } catch {
      // Private mode or full storage: recent searches just aren't kept.
    }
  }

  /** Count what every mode finds for the text, for the mode menu. */
  async countModes(rows: HistoryRow[]) {
    const { text } = this.parsed;
    const since = this.sinceArg;
    this.counts = {};
    if (!text) return;
    for (const { mode } of MODES) {
      if (mode === "sha") {
        if (HEX.test(text)) this.counts.sha = shaHits(rows, text).length;
        continue;
      }
      this.counts[mode] = null;
      api.search({ mode, text, branch: this.branch, since, author: mode === "author" ? null : this.author }).then(
        (r) => {
          if (this.parsed.text === text) this.counts[mode] = r.hits.length;
        },
        () => {
          if (this.parsed.text === text) delete this.counts[mode];
        },
      );
    }
  }

  get sinceArg(): string | null {
    return SINCE.find((s) => s.since === this.since)?.git ?? null;
  }

  /** Run the search for the current input and filters. Older answers that arrive late are dropped. */
  async run(rows: HistoryRow[]) {
    const mine = ++this.#generation;
    const { mode, text } = this.parsed;
    const key = this.key;
    if (!text) {
      this.result = null;
      this.busy = false;
      return;
    }
    const since = this.sinceArg;
    if (mode === "sha") {
      const hits = shaHits(rows, text);
      this.result = { hits, more: false, command: `git show ${text}` };
      this.resultKey = key;
      this.busy = false;
      this.error = null;
      return;
    }
    this.busy = true;
    try {
      // In Author mode the field is the author filter itself.
      const result = await api.search({ mode, text, branch: this.branch, since, author: mode === "author" ? null : this.author });
      if (mine !== this.#generation) return;
      this.result = result;
      this.resultKey = key;
      this.error = null;
    } catch (err) {
      if (mine !== this.#generation) return;
      this.result = null;
      this.error = String(err);
    } finally {
      if (mine === this.#generation) this.busy = false;
    }
  }
}

export const search = new Search();

function shaHits(rows: HistoryRow[], text: string) {
  const needle = text.toLowerCase();
  return rows.filter((r) => !r.worktree && r.id.startsWith(needle)).map((r) => ({ id: r.id, files: [] as string[] }));
}

/** Pieces of `text` with the matches of `find` (any case) flagged, for highlighting a summary. */
export function split(text: string, find: string): { text: string; hit: boolean }[] {
  if (!find) return [{ text, hit: false }];
  const lower = text.toLowerCase();
  const needle = find.toLowerCase();
  const out: { text: string; hit: boolean }[] = [];
  let at = 0;
  for (let i = lower.indexOf(needle); i >= 0; i = lower.indexOf(needle, i + needle.length)) {
    if (i > at) out.push({ text: text.slice(at, i), hit: false });
    out.push({ text: text.slice(i, i + needle.length), hit: true });
    at = i + needle.length;
  }
  if (at < text.length) out.push({ text: text.slice(at), hit: false });
  return out;
}

/** What the commit list shows of a search: the matching commits and how to mark them. */
export interface Found {
  /** Matching commit ids, with the files that matched for code and file searches. */
  hits: Map<string, string[]>;
  mode: Mode;
  text: string;
  /** Only the matches, without the graph. */
  only: boolean;
}
