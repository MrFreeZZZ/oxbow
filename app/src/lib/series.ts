// Compare Series: a series of commits before and after a rebase, an amend or a force push, as
// `git range-diff` pairs them. The pieces here are pure, so they are tested without the app.

import type { HistoryRow, InterLine, InterSection, OpEntry, Series, SeriesCommit, SeriesPair } from "./types";

/** What Compare Series shows: the two tips, and the branches that moved with the rewrite. */
export interface SeriesQuery {
  /** The branch whose series it is, short name. */
  branch: string;
  old: string;
  new: string;
  /** Full ref names that moved with it: what they have is not the new base. */
  rewritten: string[];
  /** Other branches of the same step, to switch to. */
  branches: { branch: string; old: string; new: string }[];
  /** Where it came from, in the screen's subtitle: "Edit stack feature/b · 5 min ago". */
  from: string;
}

const short = (name: string) => name.replace(/^refs\/heads\//, "");

/** Compare Series for a step of the Operation Log, on `branch` or the one that was checked out. */
export function stepQuery(entry: OpEntry, from: string, branch?: string): SeriesQuery | null {
  const names = entry.rewrites ?? [];
  const branches = names.flatMap((name) => {
    const move = entry.refs.find((m) => m.name === name);
    return move?.before && move.after ? [{ branch: short(name), old: move.before, new: move.after }] : [];
  });
  if (!branches.length) return null;
  const head = entry.headAfter.branch ?? entry.headBefore.branch;
  const pick = branches.find((b) => b.branch === branch) ?? branches.find((b) => b.branch === head) ?? branches[0];
  return { ...pick, rewritten: entry.refs.map((m) => m.name), branches, from };
}

/** The short label of a pair, as the list shows it. */
export function pairLabel(pair: SeriesPair): string {
  switch (pair.kind) {
    case "same":
      return "Same";
    case "added":
      return "New";
    case "dropped":
      return "Dropped";
  }
  const parts = [pair.message && "message", pair.code && "code", pair.author && "author"].filter(Boolean) as string[];
  if (!parts.length) return "Changed";
  const text = parts.length === 1 ? parts[0] : `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
  return `${text[0].toUpperCase()}${text.slice(1)} changed`;
}

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

/** "3 commits → 4 commits": the headline of the screen. */
export function headline(s: Series): string {
  return s.oldCount === s.newCount ? `${plural(s.newCount, "commit")}, before and after` : `${plural(s.oldCount, "commit")} before, ${s.newCount} after`;
}

/** How many pairs of each kind, in the order the eye wants them: what changed first. */
export function tally(s: Series): { kind: SeriesPair["kind"]; text: string }[] {
  const count = (kind: SeriesPair["kind"]) => s.pairs.filter((p) => p.kind === kind).length;
  const rows: { kind: SeriesPair["kind"]; text: string }[] = [
    { kind: "changed", text: `${count("changed")} changed` },
    { kind: "added", text: `${count("added")} new` },
    { kind: "dropped", text: `${count("dropped")} dropped` },
    { kind: "same", text: `${count("same")} the same` },
  ];
  return rows.filter((r) => !r.text.startsWith("0 "));
}

/** Where the series sits now, against where it sat. */
export function baseLine(s: Series): string {
  if (s.oldBase && s.newBase && s.oldBase === s.newBase) return "Same base: only the commits themselves were rewritten.";
  if (s.baseCommits > 0) {
    const onto = s.baseName ? `onto ${s.baseName}` : `onto ${s.newBase!.slice(0, 7)}`;
    return `Moved ${onto}: ${plural(s.baseCommits, "newer commit")} under the series.`;
  }
  if (s.newBase && s.oldBase) return `Moved onto ${s.baseName ?? s.newBase.slice(0, 7)}.`;
  return "The commits don’t form one line, so both sides are compared in full.";
}

/** The pair to show first: the newest changed one, then the newest new or dropped, else the newest. */
export function firstPick(s: Series): number {
  const changed = s.pairs.findIndex((p) => p.kind === "changed");
  if (changed >= 0) return changed;
  const other = s.pairs.findIndex((p) => p.kind !== "same");
  return other < 0 ? 0 : other;
}

/** A row of the diff between versions: the old version's line beside the new one's, or a line
 *  across both columns. */
export type InterRow =
  | { kind: "pair"; left: InterLine | null; right: InterLine | null }
  | { kind: "across"; line: InterLine };

/** Rows of a section: unchanged lines face themselves; a run of the old version's lines faces
 *  the run of the new version's that follows it, line by line. */
export function interRows(lines: InterLine[]): InterRow[] {
  const rows: InterRow[] = [];
  let olds: InterLine[] = [];
  let news: InterLine[] = [];
  const flush = () => {
    for (let i = 0; i < Math.max(olds.length, news.length); i++) rows.push({ kind: "pair", left: olds[i] ?? null, right: news[i] ?? null });
    olds = [];
    news = [];
  };
  for (const line of lines) {
    if (line.outer === "old") {
      // An old line after new ones starts a run of its own.
      if (news.length) flush();
      olds.push(line);
    } else if (line.outer === "new") news.push(line);
    else {
      flush();
      if (line.inner === "hunk" || line.inner === "gap") rows.push({ kind: "across", line });
      else rows.push({ kind: "pair", left: line, right: line });
    }
  }
  flush();
  return rows;
}

export interface WordPiece {
  text: string;
  changed: boolean;
}

const TOKENS = /\w+|\s+|[^\w\s]/g;
/** Longer lines are marked as a whole: word by word costs too much there. */
const MAX_TOKENS = 400;

/** The words two versions of a line don't share, marked on each. */
export function wordDiff(a: string, b: string): [WordPiece[], WordPiece[]] {
  const x = a.match(TOKENS) ?? [];
  const y = b.match(TOKENS) ?? [];
  if (x.length > MAX_TOKENS || y.length > MAX_TOKENS) return [[{ text: a, changed: true }], [{ text: b, changed: true }]];
  // Longest common subsequence of tokens, from the end.
  const n = x.length;
  const m = y.length;
  const table = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) table[i][j] = x[i] === y[j] ? table[i + 1][j + 1] + 1 : Math.max(table[i + 1][j], table[i][j + 1]);
  const left: WordPiece[] = [];
  const right: WordPiece[] = [];
  const add = (into: WordPiece[], text: string, changed: boolean) => {
    const last = into[into.length - 1];
    if (last && last.changed === changed) last.text += text;
    else into.push({ text, changed });
  };
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && x[i] === y[j]) {
      add(left, x[i++], false);
      add(right, y[j++], false);
    } else if (j < m && (i === n || table[i][j + 1] >= table[i + 1][j])) add(right, y[j++], true);
    else add(left, x[i++], true);
  }
  // Only spaces between two changed words: part of the change, so it reads as one capsule.
  for (const side of [left, right])
    for (let k = 1; k < side.length - 1; k++) if (!side[k].changed && !side[k].text.trim() && side[k - 1].changed && side[k + 1].changed) side[k].changed = true;
  const merge = (pieces: WordPiece[]) => {
    const out: WordPiece[] = [];
    for (const piece of pieces) add(out, piece.text, piece.changed);
    return out;
  };
  return [merge(left), merge(right)];
}

/** Files whose change is the same in both versions, by name: the panel lists them in one line. */
export function sameFiles(sections: InterSection[]): string[] {
  return sections.filter((s) => s.kind === "file" && !s.changed).map((s) => s.title);
}

/** A commit of either series as a row CommitPanel can show; it is in no graph. */
export function seriesRow(commit: SeriesCommit, color: number): HistoryRow {
  return {
    id: commit.id,
    summary: commit.summary,
    authorName: commit.authorName,
    authorEmail: commit.authorEmail,
    time: commit.time,
    parents: commit.parents,
    labels: [],
    unpushed: false,
    noBranch: false,
    graph: { column: 0, color, forkColors: [], mergeColors: [], segments: [], width: 0, branch: null },
    worktree: null,
  };
}
