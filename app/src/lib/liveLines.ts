// The output of a running command as the confirmation sheet shows it. A remote can print
// without end, so only the start and the end of each command's output are kept.

/** Lines of one command's output kept from its start. */
export const HEAD = 100;
/** Lines of one command's output kept from its end. */
export const TAIL = 400;

export interface LiveLine {
  kind: string;
  text: string;
  /** A progress line the next one replaces, like `\r` in a terminal. */
  progress?: boolean;
  /** The line that stands for this many lines left out. */
  cut?: number;
}

/** The line standing for `count` lines left out of a command's output. */
export function cutLine(count: number): LiveLine & { kind: "hint" } {
  return { kind: "hint", text: `⋯ ${count} more lines ⋯`, cut: count };
}

/** Add `line` under the last command in `lines`. Past `HEAD` + 2 × `TAIL` lines of its
 *  output, the lines between its first `HEAD` and last `TAIL` go, and one line says how
 *  many; they go in large steps, so a long output isn't moved for every line. */
export function addLine<T extends LiveLine>(lines: T[], line: T, cut: (count: number) => T): void {
  const last = lines[lines.length - 1];
  if (last?.progress) lines[lines.length - 1] = line;
  else lines.push(line);
  // Where the last command's output starts: at most a few hundred lines back.
  let start = lines.length;
  while (start > 0 && lines[start - 1].kind !== "cmd") start--;
  if (lines.length - start <= HEAD + 1 + 2 * TAIL) return;
  const at = start + HEAD;
  const had = lines[at].cut;
  const from = had === undefined ? at : at + 1;
  const drop = lines.length - TAIL - from;
  lines.splice(at, from - at + drop, cut((had ?? 0) + drop));
}
