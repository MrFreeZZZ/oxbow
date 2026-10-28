// Stashing part of the changes: only what is staged, or one hunk or a few lines of a file's
// unstaged changes, and the quoted lines the sheets of single lines show.

import type { Part, Request } from "./confirm.svelte";
import type { Hunk } from "./types";

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
const fileName = (path: string) => path.slice(path.lastIndexOf("/") + 1);

/** "“a”, “b” and 3 more" for the picked lines. */
export function quoted(texts: string[]): Part[] {
  const shown = texts.slice(0, 3);
  const parts: Part[] = [];
  shown.forEach((text, i) => {
    if (i) parts.push(i === shown.length - 1 && texts.length === shown.length ? " and " : ", ");
    const t = text.trim();
    parts.push({ quote: t.length > 48 ? `${t.slice(0, 47)}…` : t || " " });
  });
  if (texts.length > shown.length) parts.push(` and ${texts.length - shown.length} more`);
  return parts;
}

/** Stash only what is staged (`git stash push --staged`): `count` staged files. `again` makes
 *  the sheet anew as the message is typed, for a sheet that adds options of its own. */
export function stashStagedRequest(count: number, message: string, again: (message: string) => Request = (m) => stashStagedRequest(count, m)): Request {
  return {
    title: `Stash ${plural(count, "staged file")}?`,
    body: ["Saves what is staged as a new stash and takes it out of the staging area and your files. Changes that are not staged stay as they are. Bring them back from Stashes, staged again with Keep staged."],
    icon: "stash",
    button: "Stash Staged",
    fields: [
      {
        label: "Message",
        text: { value: message, placeholder: "What these changes are", edit: again },
      },
    ],
    invalid: count ? null : "Nothing is staged.",
    status: "Stashing the staged changes…",
    done: `Stashed the staged changes${message.trim() ? ` as “${message.trim()}”` : ""}. Unstaged ones stay.`,
    action: { kind: "stashPush", message: message.trim() || null, untracked: false, staged: true },
  };
}

/** Stash an unstaged hunk of `path`, or only its picked `lines`, and take it out of the file. */
export function stashLinesRequest(path: string, hunk: Hunk, lines: number[] | null, message: string): Request {
  const name = fileName(path);
  const changed = hunk.lines.map((l, i) => (l.kind !== "context" ? i : -1)).filter((i) => i >= 0);
  const picked = lines ? [...lines].sort((a, b) => a - b) : changed;
  const n = picked.length;
  const texts = picked.map((i) => hunk.lines[i]?.text ?? "");
  return {
    title: lines ? `Stash ${plural(n, "line")} of ${name}?` : `Stash this hunk of ${name}?`,
    body: [
      lines ? "Saves only the picked lines as a new stash and takes them out of your file: " : "Saves this hunk as a new stash and takes it out of your file: ",
      ...quoted(texts),
      ". The rest of the file, and what is staged, stay as they are. Bring them back from Stashes.",
    ],
    icon: "stash",
    button: "Stash",
    fields: [
      {
        label: "Message",
        text: { value: message, placeholder: "What these lines are", edit: (value) => stashLinesRequest(path, hunk, lines, value) },
      },
    ],
    status: "Stashing…",
    done: `Stashed ${lines ? plural(n, "line") : "the hunk"} of ${name}${message.trim() ? ` as “${message.trim()}”` : ""}.`,
    action: { kind: "stashHunk", path, header: hunk.header, check: hunk.check ?? "", lines: lines ? picked : null, message: message.trim() || null },
  };
}
