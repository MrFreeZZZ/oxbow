// ↑ and ↓ in the empty commit message box, as in VS Code: your latest commit messages, newest
// first. Once you type, the keys move the cursor again.

export interface Message {
  summary: string;
  description: string;
}

/** A commit message as the form shows it: the first line, and the rest after the blank line. */
export function splitMessage(message: string): Message {
  const [first = "", ...rest] = message.replace(/\r\n/g, "\n").split("\n");
  return { summary: first.trim(), description: rest.join("\n").replace(/^\n+/, "").trimEnd() };
}

const same = (a: Message, b: Message) => a.summary.trim() === b.summary.trim() && a.description.trim() === b.description.trim();

/** Whether the box is for the history: empty, or showing message `at` as it was put there. */
export function browsing(messages: string[], at: number, now: Message): boolean {
  const empty = !now.summary.trim() && !now.description.trim();
  return empty || (at >= 0 && at < messages.length && same(splitMessage(messages[at]), now));
}

/** The message ↑ (`by` 1) or ↓ (-1) shows next, from message `at` (-1: the empty box) while the
 *  box shows `now`: -1 is the empty box again, and null leaves the box as it is. */
export function stepMessage(messages: string[], at: number, by: 1 | -1, now: Message): number | null {
  if (!browsing(messages, at, now)) return null;
  const empty = !now.summary.trim() && !now.description.trim();
  const from = empty ? -1 : at;
  const next = from + by;
  if (next < -1 || next >= messages.length) return null;
  return next;
}
