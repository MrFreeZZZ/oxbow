// Colors of a git command in the terminal blocks; they follow the app theme (--term-* in app.css).

const GIT = "var(--term-git)";
const SUB = "var(--term-sub)";
const FLAG = "var(--term-flag)";
const SHA = "var(--term-sha)";
const STR = "var(--term-str)";
const TEXT = "var(--term-text)";

/** Split a shell-quoted command into colored words. */
export function tokens(display: string) {
  const words = display.match(/'(?:[^']|'\\'')*'|"[^"]*"|\S+/g) ?? [];
  return words.map((word, i) => ({
    text: word,
    bold: i === 0,
    color:
      i === 0 ? GIT : i === 1 ? SUB : word.startsWith("-") ? FLAG : /^['"]/.test(word) ? STR : /^[0-9a-f]{7,40}$/.test(word) ? SHA : TEXT,
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
