// Suggested changes in review comments: the ```suggestion block GitHub and GitLab read, written
// from the lines a comment is on, shown as a change of those lines, and applied to the file on
// disk by whoever has the branch checked out.
import type { Request } from "./confirm.svelte";
import type { Hunk, HunkLine } from "./types";

/** A piece of a comment: words, or lines suggested in place of the ones it is on. GitLab's
 *  `suggestion:-2+1` reaches lines above and below the commented one. */
export type BodyPart = { kind: "text"; text: string } | { kind: "suggestion"; lines: string[]; above: number; below: number };

const OPEN = /^ {0,3}(`{3,}|~{3,})[ \t]*suggestion(?::-(\d+)\+(\d+))?[ \t]*$/;
const CLOSE = /^ {0,3}(`{3,}|~{3,})[ \t]*$/;

/** A comment's words and suggestions, in order. A block left open runs to the end, as Markdown
 *  reads it. */
export function bodyParts(body: string): BodyPart[] {
  const out: BodyPart[] = [];
  let text: string[] = [];
  const flush = () => {
    const words = text.join("\n").replace(/^\n+|\s+$/g, "");
    if (words) out.push({ kind: "text", text: words });
    text = [];
  };
  const lines = body.replace(/\r\n?/g, "\n").split("\n");
  for (let i = 0; i < lines.length; i++) {
    const open = OPEN.exec(lines[i]);
    if (!open) {
      text.push(lines[i]);
      continue;
    }
    flush();
    const fence = open[1];
    const inside: string[] = [];
    for (i++; i < lines.length; i++) {
      const close = CLOSE.exec(lines[i]);
      if (close && close[1][0] === fence[0] && close[1].length >= fence.length) break;
      inside.push(lines[i]);
    }
    out.push({ kind: "suggestion", lines: inside, above: Number(open[2] ?? 0), below: Number(open[3] ?? 0) });
  }
  flush();
  return out;
}

/** The lines of the file a suggestion goes in place of: on GitHub all the lines its comment is
 *  on; on GitLab the commented line and as many above and below as it says. */
export function suggestedRange(part: Extract<BodyPart, { kind: "suggestion" }>, spot: { line: number | null; startLine?: number | null }, lab: boolean): { start: number; end: number } | null {
  if (spot.line === null) return null;
  if (lab) return { start: Math.max(1, spot.line - part.above), end: spot.line + part.below };
  return { start: Math.min(spot.startLine ?? spot.line, spot.line), end: spot.line };
}

/** The suggestion block to write: the lines in a fence longer than any run of backticks in them,
 *  for GitLab with how many lines above the commented one it covers. */
export function suggestionBlock(lines: string[], lab: boolean, span: number): string {
  const longest = Math.max(2, ...lines.map((l) => /^ {0,3}(`+)/.exec(l)?.[1].length ?? 0));
  const fence = "`".repeat(longest + 1);
  const reach = lab ? `:-${Math.max(0, span - 1)}+0` : "";
  return `${fence}suggestion${reach}\n${lines.join("\n")}\n${fence}`;
}

/** `block` put into `value` at `at`, on lines of its own, and where it starts. */
export function insertBlock(value: string, at: number, block: string): { value: string; start: number } {
  const before = value.slice(0, at);
  const after = value.slice(at);
  const lead = before && !before.endsWith("\n") ? "\n" : "";
  const tail = after.startsWith("\n") ? "" : "\n";
  return { value: before + lead + block + tail + after, start: before.length + lead.length };
}

/** The text of new lines `start` to `end` from numbered lines, or null when one is missing. */
function textOf(lines: { number: number | null; text: string }[], start: number, end: number): string[] | null {
  const byNumber = new Map(lines.filter((l) => l.number !== null).map((l) => [l.number, l.text]));
  const out: string[] = [];
  for (let n = start; n <= end; n++) {
    const text = byNumber.get(n);
    if (text === undefined) return null;
    out.push(text);
  }
  return out;
}

/** New lines `start` to `end` as a hunk of the diff has them, or null. */
export const hunkText = (hunk: Hunk, start: number, end: number) =>
  textOf(
    hunk.lines.map((l) => ({ number: l.kind === "removed" ? null : l.newLine, text: l.text })),
    start,
    end,
  );

/** New lines `start` to `end` as a thread's lines show them, or null. */
export const threadText = (lines: HunkLine[], start: number, end: number) => textOf(lines, start, end);

/** Apply Suggestion: the suggested lines go in place of the ones they were written on, in the
 *  file on disk, uncommitted. `commit` is the one whose lines the comment is on. */
export function applyRequest(path: string, commit: string, range: { start: number; end: number }, lines: string[], who: string): Request {
  const name = path.split("/").pop() ?? path;
  const where = range.start === range.end ? `line ${range.start}` : `lines ${range.start}–${range.end}`;
  return {
    title: `Apply ${who}’s suggestion to ${name}?`,
    body: ["Puts the suggested lines in place of ", where, " of ", { code: path }, " in your files, uncommitted. Commit and push it to send it."],
    icon: "edit",
    button: "Apply Suggestion",
    note: "Undo takes it back out, from the toast or the Operation Log.",
    status: `Applying the suggestion to ${name}…`,
    done: `Applied the suggestion to ${name}. Review it in Changes, then commit and push.`,
    action: { kind: "applySuggestion", commit, path, start: range.start, end: range.end, lines },
  };
}
