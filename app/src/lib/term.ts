// Colors of a git command in the terminal blocks; they follow the app theme (--term-* in app.css).

import { words, type Word } from "./gitWords";

const COLORS: Record<Word["kind"], string> = {
  program: "var(--term-git)",
  sub: "var(--term-sub)",
  flag: "var(--term-flag)",
  string: "var(--term-str)",
  value: "var(--term-text)",
  arg: "var(--term-text)",
};
const SHA = "var(--term-sha)";

/** Split a shell-quoted command into colored words. */
export function tokens(display: string) {
  return words(display).map((word) => ({
    word,
    text: word.text,
    bold: word.kind === "program",
    color: word.kind === "arg" && /^[0-9a-f]{7,40}$/.test(word.text) ? SHA : COLORS[word.kind],
  }));
}

/** `path` with the home folder written as `~`, as a shell prompt shows it. */
export function tilde(path: string, home: string): string {
  if (!home) return path;
  const sep = home.includes("\\") ? "\\" : "/";
  const base = home.endsWith(sep) ? home.slice(0, -1) : home;
  if (path === base) return "~";
  return path.startsWith(base + sep) ? "~" + path.slice(base.length) : path;
}
